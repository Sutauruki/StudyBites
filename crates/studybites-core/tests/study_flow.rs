//! Phase 2: scheduling, retrieval, extraction, the provider router, the
//! update check, and the end-to-end upload → cards → study → tomorrow path.

use studybites_core::ai::{AiError, DistillRequest, DistilledNote, GeneratedCard, ProviderId};
use studybites_core::crypto;
use studybites_core::db::DEV_KEY;
use studybites_core::embed::{self, Embedder, LexicalEmbedder};
use studybites_core::fsrs::{self, Memory, Rating, State};
use studybites_core::providers::{Attempt, Http, HttpError, Provider};
use studybites_core::{files, library, providers, study, subjects, topics, update, Db, NewTopic};

fn db() -> Db {
    Db::open_in_memory(DEV_KEY).expect("open in-memory db")
}

// ---- FSRS ---------------------------------------------------------------

#[test]
fn a_new_card_gets_a_schedule_that_reflects_the_grade() {
    let new = Memory::default();
    let again = fsrs::schedule(new, Rating::Again, 0.0);
    let hard = fsrs::schedule(new, Rating::Hard, 0.0);
    let good = fsrs::schedule(new, Rating::Good, 0.0);
    let easy = fsrs::schedule(new, Rating::Easy, 0.0);

    // "Again" keeps the card in the session rather than pushing it to tomorrow.
    assert_eq!(again.memory.state, State::Learning);
    assert!(again.interval_days < 1.0);

    for scheduled in [hard, good, easy] {
        assert_eq!(scheduled.memory.state, State::Review);
        assert!(scheduled.interval_days >= 1.0);
    }
    assert!(
        easy.interval_days > good.interval_days,
        "easy must wait longer than good ({} vs {})",
        easy.interval_days,
        good.interval_days
    );
    assert!(good.interval_days > hard.interval_days);
    assert!(easy.memory.difficulty < hard.memory.difficulty);
    assert_eq!(good.memory.reps, 1);
}

#[test]
fn a_lapse_shortens_the_interval_and_counts_itself() {
    let learned = Memory {
        stability: 40.0,
        difficulty: 5.0,
        reps: 6,
        lapses: 0,
        state: State::Review,
    };
    let lapsed = fsrs::schedule(learned, Rating::Again, 41.0);

    assert_eq!(lapsed.memory.state, State::Relearning);
    assert_eq!(lapsed.memory.lapses, 1);
    assert!(
        lapsed.memory.stability < learned.stability,
        "forgetting must never increase stability"
    );
    assert!(lapsed.interval_days < 1.0, "a lapse comes back soon");
}

#[test]
fn repeated_success_lengthens_intervals() {
    let mut memory = Memory::default();
    let mut last = 0.0;
    for round in 0..6 {
        let scheduled = fsrs::schedule(memory, Rating::Good, last);
        assert!(
            scheduled.interval_days >= last,
            "round {round}: interval shrank from {last} to {}",
            scheduled.interval_days
        );
        last = scheduled.interval_days;
        memory = scheduled.memory;
    }
    assert!(
        last > 5.0,
        "six good reviews should reach beyond a few days"
    );
    assert!(last <= fsrs::MAX_INTERVAL_DAYS);
}

#[test]
fn difficulty_and_stability_stay_in_range() {
    let mut memory = Memory::default();
    // Hammer it with the worst grade; nothing may drift out of bounds.
    for _ in 0..50 {
        let scheduled = fsrs::schedule(memory, Rating::Again, 1.0);
        memory = scheduled.memory;
        assert!((1.0..=10.0).contains(&memory.difficulty));
        assert!(memory.stability > 0.0 && memory.stability.is_finite());
        assert!(scheduled.interval_days.is_finite());
    }
}

#[test]
fn retrievability_decays_over_time() {
    let fresh = fsrs::retrievability(10.0, 0.0);
    let later = fsrs::retrievability(10.0, 10.0);
    let much_later = fsrs::retrievability(10.0, 60.0);
    assert!(fresh > 0.99);
    assert!(later < fresh && later > much_later);
    assert!(much_later > 0.0);
}

// ---- embeddings and retrieval -------------------------------------------

#[test]
fn similar_text_scores_higher_than_unrelated_text() {
    let embedder = LexicalEmbedder;
    let logs = embedder.embed("Logarithms turn multiplication into addition of exponents");
    let more_logs = embedder.embed("The logarithm of a product equals the sum of the logarithms");
    let cells = embedder.embed("Mitochondria generate energy inside animal cells");

    let related = embed::cosine_similarity(&logs, &more_logs);
    let unrelated = embed::cosine_similarity(&logs, &cells);
    assert!(
        related > unrelated,
        "related notes scored {related}, unrelated scored {unrelated}"
    );
    assert!(related > 0.0);
}

#[test]
fn embeddings_are_deterministic_and_normalised() {
    let embedder = LexicalEmbedder;
    let once = embedder.embed("Newton's second law");
    let twice = embedder.embed("Newton's second law");
    assert_eq!(once, twice, "the same text must always embed the same way");
    assert_eq!(once.len(), embed::DIMENSIONS);

    let norm: f32 = once.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-4, "vector should be unit length");
}

#[test]
fn embedding_blobs_round_trip() {
    let embedder = LexicalEmbedder;
    let vector = embedder.embed("Acid base titration");
    let blob = embed::to_blob(&vector);
    assert_eq!(blob.len(), vector.len() * 4);

    let restored = embed::from_blob(&blob).unwrap();
    assert_eq!(vector, restored);

    // A truncated blob is reported, not silently misread.
    assert!(embed::from_blob(&blob[..blob.len() - 1]).is_err());
}

#[test]
fn similarity_is_safe_with_mismatched_or_empty_vectors() {
    assert_eq!(embed::cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    assert_eq!(embed::cosine_similarity(&[], &[]), 0.0);
    assert_eq!(embed::cosine_similarity(&[0.0, 0.0], &[0.0, 0.0]), 0.0);
}

#[test]
fn top_k_ranks_by_score_and_respects_the_limit() {
    let embedder = LexicalEmbedder;
    let query = embedder.embed("photosynthesis chlorophyll light");
    let candidates = vec![
        (
            1,
            embedder.embed("Photosynthesis uses chlorophyll to capture light"),
        ),
        (2, embedder.embed("The French Revolution began in 1789")),
        (
            3,
            embedder.embed("Chlorophyll absorbs light during photosynthesis"),
        ),
    ];

    let matches = embed::top_k(&query, &candidates, 2);
    assert!(matches.len() <= 2);
    assert!(!matches.is_empty());
    assert!(
        matches.iter().all(|m| m.id != 2),
        "the unrelated note should not be retrieved"
    );
    if matches.len() == 2 {
        assert!(
            matches[0].score >= matches[1].score,
            "results must be ranked"
        );
    }
}

// ---- extraction ---------------------------------------------------------

#[test]
fn plain_text_extraction_tidies_whitespace_and_rejoins_words() {
    let path = std::path::Path::new("notes.txt");
    let raw = b"Logarithms\n\n\n\nThe loga-\nrithm is the inverse   of exponentiation.\n";
    let extracted = files::extract_from_bytes(path, raw).unwrap();

    assert_eq!(extracted.kind, files::FileKind::Text);
    assert!(
        extracted.text.contains("logarithm is the inverse"),
        "hyphenated line breaks should be rejoined, got: {}",
        extracted.text
    );
    assert!(!extracted.text.contains("   "), "runs of spaces collapse");
    assert_eq!(extracted.sha256.len(), 64);
}

#[test]
fn an_empty_or_unreadable_file_is_reported_clearly() {
    let path = std::path::Path::new("notes.txt");
    assert!(files::extract_from_bytes(path, b"   \n\n ").is_err());

    let exe = std::path::Path::new("setup.exe");
    assert!(files::extract_from_bytes(exe, b"MZ\x90\x00").is_err());
}

#[test]
fn file_type_comes_from_content_before_extension() {
    // A PDF saved as .txt must still be treated as a PDF.
    let mislabelled = std::path::Path::new("notes.txt");
    assert_eq!(
        files::FileKind::detect(mislabelled, b"%PDF-1.7\n...").unwrap(),
        files::FileKind::Pdf
    );
}

#[test]
fn docx_extraction_works_without_a_filename_extension() {
    use std::io::Write;

    let mut bytes = std::io::Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut bytes);
        archive
            .start_file(
                "word/document.xml",
                zip::write::FileOptions::<()>::default(),
            )
            .unwrap();
        write!(
            archive,
            r#"<w:document><w:body><w:p><w:r><w:t>Algebra &amp; functions</w:t></w:r></w:p><w:p><w:r><w:t>Second topic</w:t></w:r></w:p></w:body></w:document>"#
        )
        .unwrap();
        archive.finish().unwrap();
    }

    let extracted =
        files::extract_from_bytes(std::path::Path::new("picked-document"), &bytes.into_inner())
            .unwrap();
    assert_eq!(extracted.kind, files::FileKind::Docx);
    assert!(extracted.text.contains("Algebra & functions"));
    assert!(extracted.text.contains("Second topic"));
}

#[test]
fn cold_storage_round_trips_and_refuses_path_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let original = b"the original lecture notes, byte for byte";
    let hash = files::sha256_hex(original);

    let keys = crypto::derive(&[7u8; 32], b"licence-secret");
    let key = &keys.file;

    let stored = files::cold_store(dir.path(), key, &hash, original).unwrap();
    let read_back = files::cold_read(dir.path(), key, &stored).unwrap();
    assert_eq!(read_back, original, "the original must survive exactly");

    // The stored file must not be the plain original lying on disk.
    let on_disk = std::fs::read(files::cold_dir(dir.path()).join(&stored)).unwrap();
    assert_ne!(on_disk, original);

    // Another device derives another key, so the folder is worth nothing there.
    let other = crypto::derive(&[8u8; 32], b"licence-secret");
    assert!(
        files::cold_read(dir.path(), &other.file, &stored).is_err(),
        "a copied originals folder must not open on another machine"
    );

    assert!(files::cold_read(dir.path(), key, "../../etc/passwd").is_err());
    assert!(files::cold_read(dir.path(), key, "sub/dir.sbc").is_err());

    files::cold_delete(dir.path(), &stored).unwrap();
    assert!(files::cold_read(dir.path(), key, &stored).is_err());
}

// ---- provider router ----------------------------------------------------

/// Canned HTTP responses, so routing is tested without a network or a key.
struct FakeHttp {
    responses: std::sync::Mutex<Vec<std::result::Result<String, HttpError>>>,
    calls: std::sync::Mutex<usize>,
}

impl FakeHttp {
    fn new(responses: Vec<std::result::Result<String, HttpError>>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
            calls: std::sync::Mutex::new(0),
        }
    }
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl Http for FakeHttp {
    fn post_json(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        _body: &serde_json::Value,
    ) -> std::result::Result<String, HttpError> {
        *self.calls.lock().unwrap() += 1;
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(HttpError::Transport("no more canned responses".into()));
        }
        responses.remove(0)
    }
}

fn gemini_body(text: &str) -> String {
    serde_json::json!({"candidates": [{"content": {"parts": [{"text": text}]}}]}).to_string()
}

fn openai_body(text: &str) -> String {
    serde_json::json!({"choices": [{"message": {"content": text}}]}).to_string()
}

const NOTE_JSON: &str = r#"{"title":"Logarithms","keyIdeas":["log(ab)=log a+log b"]}"#;

fn a_request() -> DistillRequest {
    DistillRequest {
        subject: "MTH 101".into(),
        topic: None,
        heading: None,
        text: "material".into(),
    }
}

#[test]
fn the_router_falls_through_to_the_next_provider_on_quota() {
    let exhausted = FakeHttp::new(vec![Err(HttpError::Status {
        code: 429,
        body: String::new(),
    })]);
    let working = FakeHttp::new(vec![Ok(openai_body(NOTE_JSON))]);

    let list = vec![
        Provider::new(ProviderId::Gemini, "key-a".into(), &exhausted),
        Provider::new(ProviderId::Groq, "key-b".into(), &working),
    ];

    let (used, note) = providers::route(&list, &|_| true, |p| {
        studybites_core::AiProvider::distill(p, &a_request())
    })
    .expect("the second provider should answer");

    assert_eq!(used, ProviderId::Groq);
    assert_eq!(note.title, "Logarithms");
    assert_eq!(exhausted.calls(), 1);
    assert_eq!(working.calls(), 1);
}

#[test]
fn a_provider_known_to_be_exhausted_is_not_called_at_all() {
    let never = FakeHttp::new(vec![Ok(gemini_body(NOTE_JSON))]);
    let working = FakeHttp::new(vec![Ok(openai_body("a short explanation"))]);

    let list = vec![
        Provider::new(ProviderId::Gemini, "key-a".into(), &never),
        Provider::new(ProviderId::Groq, "key-b".into(), &working),
    ];

    let result = providers::route(&list, &|id| id != ProviderId::Gemini, |p| {
        studybites_core::AiProvider::explain(p, "why?", &[DistilledNote::default()])
    });

    assert!(result.is_ok());
    assert_eq!(
        never.calls(),
        0,
        "an exhausted provider must not cost a request to rediscover"
    );
}

#[test]
fn a_garbled_reply_is_retried_once_on_the_same_provider() {
    // Another provider would be answering a different question, so a bad
    // response retries in place rather than falling through.
    let flaky = FakeHttp::new(vec![
        Ok(gemini_body("I'm sorry, I can't do that")),
        Ok(gemini_body(NOTE_JSON)),
    ]);
    let list = vec![Provider::new(ProviderId::Gemini, "key".into(), &flaky)];

    let result = providers::route(&list, &|_| true, |p| {
        studybites_core::AiProvider::distill(p, &a_request())
    });

    assert!(result.is_ok());
    assert_eq!(flaky.calls(), 2, "exactly one retry");
}

#[test]
fn every_provider_failing_reports_each_attempt() {
    let a = FakeHttp::new(vec![Err(HttpError::Status {
        code: 429,
        body: String::new(),
    })]);
    let b = FakeHttp::new(vec![Err(HttpError::Status {
        code: 401,
        body: String::new(),
    })]);

    let list = vec![
        Provider::new(ProviderId::Gemini, "key-a".into(), &a),
        Provider::new(ProviderId::Groq, "key-b".into(), &b),
    ];

    let attempts: Vec<Attempt> = providers::route(&list, &|_| true, |p| {
        studybites_core::AiProvider::explain(p, "why?", &[DistilledNote::default()])
    })
    .expect_err("all providers failed");

    assert_eq!(attempts.len(), 2);
    assert!(matches!(attempts[0].error, AiError::QuotaExhausted { .. }));
    assert!(matches!(attempts[1].error, AiError::Auth(_)));
}

#[test]
fn a_missing_key_is_an_auth_error_not_a_request() {
    let http = FakeHttp::new(vec![Ok(gemini_body(NOTE_JSON))]);
    let provider = Provider::new(ProviderId::Gemini, "  ".into(), &http);

    let error = studybites_core::AiProvider::explain(&provider, "q", &[])
        .expect_err("an empty key cannot work");
    assert!(matches!(error, AiError::Auth(_)));
    assert_eq!(http.calls(), 0, "no request is wasted on a missing key");
}

#[test]
fn a_safety_block_is_reported_as_a_bad_response() {
    let http = FakeHttp::new(vec![Ok(
        serde_json::json!({"candidates": [{"finishReason": "SAFETY"}]}).to_string(),
    )]);
    let provider = Provider::new(ProviderId::Gemini, "key".into(), &http);

    let error = studybites_core::AiProvider::explain(&provider, "q", &[]).unwrap_err();
    assert!(matches!(error, AiError::BadResponse(_)));
}

#[test]
fn topic_match_treats_an_out_of_range_index_as_unsure() {
    let http = FakeHttp::new(vec![Ok(openai_body(
        r#"{"index":99,"confidence":0.9,"reason":"guessed"}"#,
    ))]);
    let provider = Provider::new(ProviderId::Groq, "key".into(), &http);

    let matched = studybites_core::AiProvider::match_topic(
        &provider,
        "material",
        &["Sets".to_string(), "Logarithms".to_string()],
    )
    .unwrap();

    assert_eq!(matched.index, None);
    assert_eq!(matched.confidence, 0.0);
}

// ---- update check -------------------------------------------------------

#[test]
fn version_comparison_only_prompts_for_genuinely_newer_builds() {
    assert!(update::is_newer("0.2.0", "0.1.0"));
    assert!(update::is_newer("v1.0.0", "0.9.9"));
    assert!(update::is_newer("0.1.10", "0.1.9"));
    assert!(!update::is_newer("0.1.0", "0.1.0"));
    assert!(!update::is_newer("0.1.0", "0.2.0"));
    // A malformed tag must never trigger a prompt.
    assert!(!update::is_newer("latest", "0.1.0"));
    assert!(!update::is_newer("", "0.1.0"));
}

#[test]
fn a_release_yields_its_installer_link() {
    let body = serde_json::json!({
        "tag_name": "v0.2.0",
        "body": "Phase 2",
        "html_url": "https://github.com/x/y/releases/tag/v0.2.0",
        "assets": [
            {"name": "notes.txt", "browser_download_url": "https://example.com/notes.txt"},
            {"name": "StudyBites_0.2.0_x64-setup.exe",
             "browser_download_url": "https://example.com/setup.exe"}
        ]
    })
    .to_string();

    let (tag, notes, url) = update::parse_release(&body).unwrap();
    assert_eq!(tag, "v0.2.0");
    assert_eq!(notes.as_deref(), Some("Phase 2"));
    assert_eq!(url.as_deref(), Some("https://example.com/setup.exe"));
}

#[test]
fn the_rolling_latest_tag_takes_its_version_from_the_notes() {
    assert_eq!(
        update::version_from_release("latest", Some("Installer version: 0.3.1\nBuilt today")),
        Some("0.3.1".to_string())
    );
    assert_eq!(
        update::version_from_release("v0.2.0", None),
        Some("0.2.0".to_string())
    );
    assert_eq!(
        update::version_from_release("latest", Some("no number")),
        None
    );
}

#[test]
fn without_a_token_the_check_is_skipped_not_failed() {
    let http = FakeHttp::new(vec![]);
    let status = update::check(&http, update::DEFAULT_FEED, None, "0.1.0").unwrap();

    assert!(!status.update_available);
    assert!(status.skipped_reason.is_some());
    assert_eq!(http.calls(), 0, "no request without a token");
}

#[test]
fn a_rejected_token_explains_itself_rather_than_erroring() {
    let http = FakeHttp::new(vec![Err(HttpError::Status {
        code: 401,
        body: String::new(),
    })]);
    let status = update::check(&http, update::DEFAULT_FEED, Some("bad"), "0.1.0").unwrap();
    assert!(status.skipped_reason.unwrap().contains("token"));
}

#[test]
fn a_newer_release_is_reported_with_its_download_link() {
    let body = serde_json::json!({
        "tag_name": "v0.5.0",
        "body": "New stuff",
        "assets": [{"name": "s.exe", "browser_download_url": "https://example.com/s.exe"}]
    })
    .to_string();
    let http = FakeHttp::new(vec![Ok(body)]);

    let status = update::check(&http, update::DEFAULT_FEED, Some("tok"), "0.1.0").unwrap();
    assert!(status.update_available);
    assert_eq!(status.latest_version.as_deref(), Some("0.5.0"));
    assert_eq!(
        status.download_url.as_deref(),
        Some("https://example.com/s.exe")
    );
}

// ---- the whole path -----------------------------------------------------

/// Upload → notes → cards → study → tomorrow, which is Phase 2's acceptance
/// criterion from the brief, with the AI calls stubbed out.
#[test]
fn a_studied_card_comes_back_when_it_is_due() {
    let mut db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let topic = topics::create(
        &db,
        &NewTopic {
            subject_id: subject.id,
            title: "Logarithms".into(),
            ..Default::default()
        },
    )
    .unwrap();

    let embedder = LexicalEmbedder;
    let note_content = DistilledNote {
        title: "Laws of logarithms".into(),
        key_ideas: vec!["log(ab) = log a + log b".into()],
        ..Default::default()
    };
    let vector = embedder.embed(&note_content.as_plain_text());
    let note =
        library::note_create(&db, None, Some(topic.id), &note_content, &vector, false).unwrap();

    let cards = vec![
        GeneratedCard::Flash {
            front: "What is log(ab)?".into(),
            back: "log a + log b".into(),
        },
        GeneratedCard::Short {
            prompt: "State the product rule".into(),
            answer: "log(ab) = log a + log b".into(),
        },
    ];
    let ids = library::cards_create(&mut db, note.id, Some(topic.id), &cards, false).unwrap();
    assert_eq!(ids.len(), 2);

    // Both cards are new and waiting.
    let counts = study::due_counts(&db, Some(subject.id)).unwrap();
    assert_eq!(counts.new, 2);
    assert_eq!(counts.due, 0);

    let session = study::start_session(&mut db, Some(subject.id), 10, 10).unwrap();
    assert_eq!(session.cards.len(), 2);
    assert!(session.cards.iter().all(|c| c.is_new));
    assert_eq!(
        session.cards[0].topic_title.as_deref(),
        Some("Logarithms"),
        "cards carry their topic into the session"
    );

    // Grade one well and one badly.
    let good = study::grade(&mut db, Some(session.id), ids[0], Rating::Good, 4_200).unwrap();
    let again = study::grade(&mut db, Some(session.id), ids[1], Rating::Again, 9_000).unwrap();
    assert_eq!(good.state, State::Review);
    assert!(
        good.interval_days >= 1.0,
        "a good card waits until tomorrow"
    );
    assert_eq!(again.state, State::Learning);
    assert!(again.interval_days < 1.0, "a failed card comes back today");

    let summary = study::end_session(&db, session.id).unwrap();
    assert_eq!(summary.cards_done, 2);
    assert_eq!(summary.topics_touched, vec!["Logarithms"]);
    assert!(summary.due_tomorrow >= 1, "the graded cards return");

    // Nothing is new any more.
    let after = study::due_counts(&db, Some(subject.id)).unwrap();
    assert_eq!(after.new, 0);

    // Simulate tomorrow: both cards become due again.
    db.conn()
        .execute(
            "UPDATE card_state SET due = strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 hour')",
            [],
        )
        .unwrap();
    let tomorrow = study::start_session(&mut db, Some(subject.id), 10, 10).unwrap();
    assert_eq!(
        tomorrow.due_count, 2,
        "the next day brings back exactly what was studied"
    );
    assert!(tomorrow.cards.iter().all(|c| !c.is_new));

    let mastery = study::mastery_for_subject(&db, subject.id).unwrap();
    let logs = mastery.iter().find(|m| m.topic_id == topic.id).unwrap();
    assert_eq!(logs.total_cards, 2);
    assert_eq!(logs.studied_cards, 2);
}

#[test]
fn retrieval_stays_inside_one_subject() {
    let db = db();
    let maths = subjects::create(&db, "MTH 101").unwrap();
    let biology = subjects::create(&db, "BIO 101").unwrap();
    let embedder = LexicalEmbedder;

    let maths_topic = topics::create(
        &db,
        &NewTopic {
            subject_id: maths.id,
            title: "Logarithms".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let bio_topic = topics::create(
        &db,
        &NewTopic {
            subject_id: biology.id,
            title: "Cells".into(),
            ..Default::default()
        },
    )
    .unwrap();

    for (topic_id, title, idea) in [
        (maths_topic.id, "Logarithms", "log(ab) = log a + log b"),
        (bio_topic.id, "Cells", "Mitochondria make energy in cells"),
    ] {
        let content = DistilledNote {
            title: title.into(),
            key_ideas: vec![idea.into()],
            ..Default::default()
        };
        let vector = embedder.embed(&content.as_plain_text());
        library::note_create(&db, None, Some(topic_id), &content, &vector, false).unwrap();
    }

    // Ask a biology question, scoped to maths: it must not reach across.
    let query = embedder.embed("mitochondria energy cells");
    let found = study::retrieve_notes(&db, maths.id, &query, 5).unwrap();
    assert!(
        found.iter().all(|n| n.topic_id == Some(maths_topic.id)),
        "retrieval must never cross subjects"
    );

    let bio_found = study::retrieve_notes(&db, biology.id, &query, 5).unwrap();
    assert_eq!(bio_found.len(), 1);
    assert_eq!(bio_found[0].content.title, "Cells");
}

#[test]
fn a_session_respects_its_new_card_limit() {
    let mut db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let topic = topics::create(
        &db,
        &NewTopic {
            subject_id: subject.id,
            title: "Sets".into(),
            ..Default::default()
        },
    )
    .unwrap();

    let embedder = LexicalEmbedder;
    let content = DistilledNote {
        title: "Sets".into(),
        key_ideas: vec!["A set has no duplicates".into()],
        ..Default::default()
    };
    let note = library::note_create(
        &db,
        None,
        Some(topic.id),
        &content,
        &embedder.embed("sets"),
        false,
    )
    .unwrap();

    let many: Vec<GeneratedCard> = (0..30)
        .map(|i| GeneratedCard::Flash {
            front: format!("Question {i}"),
            back: format!("Answer {i}"),
        })
        .collect();
    library::cards_create(&mut db, note.id, Some(topic.id), &many, false).unwrap();

    let session = study::start_session(&mut db, Some(subject.id), 10, 5).unwrap();
    assert_eq!(session.new_count, 5, "the new-card cap is honoured");
    assert_eq!(session.cards.len(), 5);

    assert!(
        study::start_session(&mut db, Some(subject.id), 0, 5).is_err(),
        "a zero-minute session makes no sense"
    );
}

#[test]
fn material_status_and_duplicate_detection() {
    let db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();

    let material = library::material_create(
        &db,
        &library::NewMaterial {
            subject_id: subject.id,
            topic_id: None,
            original_filename: "week1.pdf",
            mime: "application/pdf",
            size: 1024,
            sha256: "abc123",
            cold_path: Some("abc123.sbc"),
        },
    )
    .unwrap();
    assert_eq!(material.status, library::STATUS_QUEUED);
    assert_eq!(material.note_count, 0);

    // The same file in the same subject is recognised, so it costs nothing.
    let duplicate = library::material_by_hash(&db, subject.id, "abc123").unwrap();
    assert_eq!(duplicate.map(|m| m.id), Some(material.id));
    assert!(library::material_by_hash(&db, subject.id, "different")
        .unwrap()
        .is_none());

    let failed = library::material_set_status(
        &db,
        material.id,
        library::STATUS_FAILED,
        Some("the network went away"),
    )
    .unwrap();
    assert_eq!(failed.status, library::STATUS_FAILED);
    assert_eq!(failed.error.as_deref(), Some("the network went away"));

    let cold = library::material_delete(&db, material.id).unwrap();
    assert_eq!(cold.as_deref(), Some("abc123.sbc"));
    assert!(library::material_get(&db, material.id).is_err());
}

// ---- Phase 4: previews and exam mode ------------------------------------

use studybites_core::ai::TopicPreview;
use studybites_core::preview;

#[test]
fn the_next_topic_comes_from_the_outline_without_an_ai_call() {
    let mut db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let mut ids = Vec::new();
    for title in ["Sets", "Logarithms", "Matrices"] {
        ids.push(
            topics::create(
                &db,
                &NewTopic {
                    subject_id: subject.id,
                    title: title.into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id,
        );
    }
    let embedder = LexicalEmbedder;

    // Nothing covered yet: no offer, because a preview of topic 1 is noise.
    assert!(preview::next_topic(&db, subject.id).unwrap().is_none());

    // Cover "Sets" with real material.
    let content = DistilledNote {
        title: "Sets".into(),
        key_ideas: ["A set has no duplicates".to_string()].into(),
        ..Default::default()
    };
    library::note_create(
        &db,
        None,
        Some(ids[0]),
        &content,
        &embedder.embed("sets"),
        false,
    )
    .unwrap();

    let offer = preview::next_topic(&db, subject.id)
        .unwrap()
        .expect("an offer");
    assert_eq!(
        offer.title, "Logarithms",
        "the next uncovered topic in order"
    );
    assert!(offer.from_scheme, "no web search is needed with an outline");
    assert!(!offer.already_prepared);

    // Store a preview for it, then the offer says so rather than repeating.
    let generated = TopicPreview {
        title: "Logarithms".into(),
        key_ideas: ["log(ab) = log a + log b".to_string()].into(),
        cards: vec![GeneratedCard::Flash {
            front: "What is log(ab)?".into(),
            back: "log a + log b".into(),
        }],
        confidence: 0.8,
    };
    let added = preview::save_preview(&mut db, Some(ids[1]), &generated, &embedder).unwrap();
    assert_eq!(added, 1);

    let again = preview::next_topic(&db, subject.id).unwrap().unwrap();
    assert!(
        again.already_prepared,
        "do not offer the same preview twice"
    );
}

#[test]
fn preview_material_is_labelled_and_never_outranks_real_cards() {
    let mut db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let topic = topics::create(
        &db,
        &NewTopic {
            subject_id: subject.id,
            title: "Logarithms".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let embedder = LexicalEmbedder;

    // A preview first, then real material for the same topic.
    let generated = TopicPreview {
        title: "Logarithms".into(),
        key_ideas: ["Logs invert exponentiation".to_string()].into(),
        cards: vec![GeneratedCard::Flash {
            front: "Preview card".into(),
            back: "From the AI".into(),
        }],
        confidence: 0.9,
    };
    preview::save_preview(&mut db, Some(topic.id), &generated, &embedder).unwrap();

    // The stored note carries the label in its own title.
    let note = library::note_get(&db, 1).unwrap();
    assert!(note.is_preview);
    assert!(note.content.title.contains("AI preview"));

    let real = DistilledNote {
        title: "Laws of logarithms".into(),
        key_ideas: ["log(ab) = log a + log b".to_string()].into(),
        ..Default::default()
    };
    let real_note = library::note_create(
        &db,
        None,
        Some(topic.id),
        &real,
        &embedder.embed("logs"),
        false,
    )
    .unwrap();
    library::cards_create(
        &mut db,
        real_note.id,
        Some(topic.id),
        &[GeneratedCard::Flash {
            front: "Real card".into(),
            back: "From class".into(),
        }],
        false,
    )
    .unwrap();

    // Before retirement, real cards still sort ahead of preview ones.
    let session = study::start_session(&mut db, Some(subject.id), 10, 10).unwrap();
    assert!(
        !session.cards[0].card.is_preview,
        "a preview card must never come before real material"
    );
}

#[test]
fn uploading_real_notes_retires_the_preview_for_that_topic() {
    let mut db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let topic = topics::create(
        &db,
        &NewTopic {
            subject_id: subject.id,
            title: "Logarithms".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let embedder = LexicalEmbedder;

    let generated = TopicPreview {
        title: "Logarithms".into(),
        key_ideas: ["Logs invert exponentiation".to_string()].into(),
        cards: vec![
            GeneratedCard::Flash {
                front: "Preview one".into(),
                back: "A".into(),
            },
            GeneratedCard::Flash {
                front: "Preview two".into(),
                back: "B".into(),
            },
        ],
        confidence: 0.9,
    };
    preview::save_preview(&mut db, Some(topic.id), &generated, &embedder).unwrap();
    assert_eq!(study::due_counts(&db, Some(subject.id)).unwrap().new, 2);

    // Real material lands on the topic: the preview cards step aside.
    let retired = preview::retire_previews_for_topic(&db, topic.id).unwrap();
    assert_eq!(retired, 2);
    assert_eq!(
        study::due_counts(&db, Some(subject.id)).unwrap().new,
        0,
        "retired previews leave the study rotation"
    );

    // Retired, not deleted — the review history stays intact.
    let still_there: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM cards WHERE retired = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(still_there, 2);
}

#[test]
fn exam_mode_turns_on_only_inside_the_window() {
    let db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();

    assert!(!preview::exam_mode(&db, Some(subject.id)).unwrap());
    assert!(preview::days_until_exam(&db, subject.id).unwrap().is_none());

    // An exam far away does not change how sessions are built.
    let far: String = db
        .conn()
        .query_row("SELECT date('now','+90 days')", [], |r| r.get(0))
        .unwrap();
    subjects::set_exam_date(&db, subject.id, Some(&far)).unwrap();
    assert!(!preview::exam_mode(&db, Some(subject.id)).unwrap());

    // One week out, it does.
    let soon: String = db
        .conn()
        .query_row("SELECT date('now','+7 days')", [], |r| r.get(0))
        .unwrap();
    subjects::set_exam_date(&db, subject.id, Some(&soon)).unwrap();
    assert!(preview::exam_mode(&db, Some(subject.id)).unwrap());
    let days = preview::days_until_exam(&db, subject.id).unwrap().unwrap();
    assert!((6.0..=8.0).contains(&days), "about a week, got {days}");

    // A date already past stops mattering.
    let gone: String = db
        .conn()
        .query_row("SELECT date('now','-2 days')", [], |r| r.get(0))
        .unwrap();
    subjects::set_exam_date(&db, subject.id, Some(&gone)).unwrap();
    assert!(!preview::exam_mode(&db, Some(subject.id)).unwrap());

    // Unsetting works, and nonsense is refused.
    let cleared = subjects::set_exam_date(&db, subject.id, None).unwrap();
    assert!(cleared.exam_date.is_none());
    assert!(subjects::set_exam_date(&db, subject.id, Some("next tuesday")).is_err());
}

#[test]
fn a_preview_is_kept_small_and_validated() {
    // Six ideas and five cards come back; the cap holds.
    let raw = r#"{
        "title": "  Matrices  ",
        "keyIdeas": ["one","two","three","four","five","six"," "],
        "cards": [
            {"kind":"flash","front":"a","back":"b"},
            {"kind":"flash","front":"c","back":"d"},
            {"kind":"flash","front":"e","back":"f"},
            {"kind":"flash","front":"g","back":"h"},
            {"kind":"mcq","question":"bad","options":["a","b"],"answer":0}
        ],
        "confidence": 1.7
    }"#;
    let parsed = studybites_core::ai::parse_preview(raw).unwrap();
    assert_eq!(parsed.title, "Matrices");
    assert_eq!(parsed.key_ideas.len(), 5, "at most five ideas");
    assert_eq!(parsed.cards.len(), 3, "at most three cards");
    assert_eq!(parsed.confidence, 1.0, "confidence is clamped");
    assert!(parsed.as_note().title.contains("AI preview"));

    // Nothing usable is an error, not an empty preview.
    assert!(studybites_core::ai::parse_preview(r#"{"title":"X","keyIdeas":[]}"#).is_err());
}

// ---- marking typed answers (Phase 5) ------------------------------------

#[test]
fn obvious_answers_are_marked_without_spending_a_request() {
    use studybites_core::marking::{self, Verdict};

    let expected = "A compiler translates source code into machine code before the program runs.";

    // Nothing typed.
    let blank = marking::quick_mark("   ", expected).expect("blank is decided locally");
    assert_eq!(blank.verdict, Verdict::Wrong);
    assert!(!blank.from_ai);
    assert!(
        !blank.model_answer.is_empty(),
        "a student who wrote nothing should still be shown the answer"
    );

    // The same answer, differently punctuated and capitalised.
    let same = marking::quick_mark(
        "a compiler TRANSLATES source code, into machine code before the program runs!!",
        expected,
    )
    .expect("a word-for-word answer is decided locally");
    assert_eq!(same.verdict, Verdict::Correct);
    assert_eq!(same.score, 100);
    assert!(!same.from_ai);
    assert!(
        same.model_answer.is_empty(),
        "a correct, well-put answer must not be rewritten at the student"
    );

    // Noise, or an answer to some other question entirely.
    let noise = marking::quick_mark("asdf qwer zxcv", expected).expect("noise is decided locally");
    assert_eq!(noise.verdict, Verdict::Wrong);
    assert!(!noise.from_ai);
}

#[test]
fn a_real_attempt_in_different_words_goes_to_the_ai() {
    use studybites_core::marking;

    let expected = "A compiler translates source code into machine code before the program runs.";

    // This is right, but shares little vocabulary. Deciding it locally would
    // mean guessing, so it must be passed on rather than marked wrong.
    assert!(
        marking::quick_mark(
            "It changes what you wrote into something the processor can run, ahead of time.",
            expected,
        )
        .is_none(),
        "a differently-worded answer is a judgement call, not a local decision"
    );

    // Half an answer is also a judgement call.
    assert!(marking::quick_mark("it translates source code", expected).is_none());
}

#[test]
fn a_mark_parses_and_never_suggests_easy() {
    use studybites_core::fsrs::Rating;
    use studybites_core::marking::{self, Verdict};

    let mark = marking::parse_mark(
        r#"```json
        {"score": 90, "feedback": "Right idea, well put.", "modelAnswer": ""}
        ```"#,
    )
    .unwrap();
    assert_eq!(mark.verdict, Verdict::Correct);
    assert_eq!(mark.score, 90);
    assert!(mark.from_ai);
    // Correctness is not effortlessness: only the student can claim Easy.
    assert_eq!(mark.suggested_rating(), Rating::Good);

    let partial =
        marking::parse_mark(r#"{"score": 55, "feedback": "You missed the timing.", "modelAnswer": "Say it happens before the program runs."}"#)
            .unwrap();
    assert_eq!(partial.verdict, Verdict::Close);
    assert_eq!(partial.suggested_rating(), Rating::Hard);
    assert!(!partial.model_answer.is_empty());

    let wrong =
        marking::parse_mark(r#"{"score": 5, "feedback": "That is an interpreter."}"#).unwrap();
    assert_eq!(wrong.verdict, Verdict::Wrong);
    assert_eq!(wrong.suggested_rating(), Rating::Again);

    // A reply with no score or no feedback is unusable, not a zero.
    assert!(marking::parse_mark(r#"{"feedback": "ok"}"#).is_err());
    assert!(marking::parse_mark(r#"{"score": 70}"#).is_err());
    assert!(marking::parse_mark("not json at all").is_err());
}
