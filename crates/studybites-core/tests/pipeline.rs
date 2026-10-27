//! Phase 2 foundations: chunking, AI response parsing, the job queue, quota.

use studybites_core::ai::{self, AiError, GeneratedCard, ProviderId};
use studybites_core::chunk::{chunk_text, estimate_tokens, MAX_TOKENS};
use studybites_core::db::DEV_KEY;
use studybites_core::{jobs, quota, Db, JobStatus};

fn db() -> Db {
    Db::open_in_memory(DEV_KEY).expect("open in-memory db")
}

// ---- chunking -----------------------------------------------------------

/// Build a paragraph of roughly `tokens` estimated tokens.
fn paragraph(word: &str, tokens: usize) -> String {
    let per = estimate_tokens(&format!("{word} "));
    vec![word; tokens / per.max(1)].join(" ")
}

#[test]
fn chunking_keeps_short_notes_in_one_piece() {
    let text = "Logarithms turn multiplication into addition.\n\n\
                The base must be positive and not equal to one.";
    let chunks = chunk_text(text);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].index, 0);
    assert!(chunks[0].text.contains("multiplication"));
    assert!(chunks[0].text.contains("positive"));
}

#[test]
fn chunking_tags_each_piece_with_its_heading() {
    let text = "# Sets\n\nA set is a collection of distinct objects.\n\n\
                # Logarithms\n\nThe logarithm is the inverse of exponentiation.";
    let chunks = chunk_text(text);
    assert_eq!(chunks.len(), 2, "a new heading starts a new chunk");
    assert_eq!(chunks[0].heading.as_deref(), Some("Sets"));
    assert_eq!(chunks[1].heading.as_deref(), Some("Logarithms"));
    assert!(chunks[1].text.contains("inverse"));
    assert!(
        !chunks[0].text.contains("inverse"),
        "a chunk must not straddle two topics"
    );
}

#[test]
fn chunking_recognises_unmarked_headings() {
    // Lecture notes rarely use Markdown; all-caps and numbered lines are
    // what actually turns up in a PDF.
    let text = "INTRODUCTION\n\nBiology is the study of life.\n\n\
                2.1 Cell structure\n\nCells are the basic unit of life.";
    let chunks = chunk_text(text);
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].heading.as_deref(), Some("INTRODUCTION"));
    assert_eq!(chunks[1].heading.as_deref(), Some("2.1 Cell structure"));
}

#[test]
fn chunking_splits_long_text_and_respects_the_ceiling() {
    let text = (0..12)
        .map(|i| format!("Paragraph {i}. {}", paragraph("content", 120)))
        .collect::<Vec<_>>()
        .join("\n\n");

    let chunks = chunk_text(&text);
    assert!(chunks.len() > 1, "long material must be split");
    for chunk in &chunks {
        assert!(
            chunk.estimated_tokens <= MAX_TOKENS * 2,
            "chunk {} is {} tokens, far past the ceiling",
            chunk.index,
            chunk.estimated_tokens
        );
    }
    let indices: Vec<usize> = chunks.iter().map(|c| c.index).collect();
    assert_eq!(indices, (0..chunks.len()).collect::<Vec<_>>());
}

#[test]
fn chunking_breaks_up_a_single_giant_paragraph() {
    // A PDF with no blank lines is one enormous paragraph; it still has to
    // come apart or the AI request would be rejected for length.
    let text = paragraph("word", MAX_TOKENS * 4);
    let chunks = chunk_text(&text);
    assert!(chunks.len() > 1);
    assert!(chunks.iter().all(|c| c.estimated_tokens <= MAX_TOKENS * 2));
}

#[test]
fn chunking_handles_empty_and_whitespace_input() {
    assert!(chunk_text("").is_empty());
    assert!(chunk_text("   \n\n  \t ").is_empty());
}

#[test]
fn chunking_does_not_split_multibyte_characters() {
    // Degree signs, arrows and accents are common in science notes; slicing
    // mid-character would panic.
    let text = "Température → 25°C. Ωmega ≈ 3.14. ".repeat(400);
    let chunks = chunk_text(&text);
    assert!(!chunks.is_empty());
    for chunk in &chunks {
        assert!(chunk.text.chars().count() > 0);
    }
}

// ---- AI response parsing ------------------------------------------------

#[test]
fn notes_parse_from_a_clean_response() {
    let raw = r#"{
        "title": "Laws of logarithms",
        "keyIdeas": ["log(ab) = log a + log b"],
        "definitions": [{"term": "base", "meaning": "the number being raised"}],
        "formulas": ["log_b(x) = ln x / ln b"],
        "workedExamples": [],
        "commonMistakes": ["Assuming log(a+b) = log a + log b"]
    }"#;
    let note = ai::parse_note(raw).unwrap();
    assert_eq!(note.title, "Laws of logarithms");
    assert_eq!(note.key_ideas.len(), 1);
    assert_eq!(note.definitions[0].term, "base");
    assert!(note.as_plain_text().contains("Laws of logarithms"));
}

#[test]
fn notes_parse_when_the_model_wraps_json_in_prose_or_fences() {
    let fenced = "```json\n{\"title\":\"Sets\",\"keyIdeas\":[\"A set has no duplicates\"]}\n```";
    assert_eq!(ai::parse_note(fenced).unwrap().title, "Sets");

    let chatty = "Sure! Here is the JSON you asked for:\n\
                  {\"title\":\"Sets\",\"keyIdeas\":[\"A set has no duplicates\"]}\n\
                  Let me know if you need anything else.";
    assert_eq!(ai::parse_note(chatty).unwrap().title, "Sets");
}

#[test]
fn empty_or_titleless_notes_are_rejected() {
    assert!(ai::parse_note(r#"{"title":"  ","keyIdeas":["x"]}"#).is_err());
    // Nothing worth storing: no ideas, definitions or formulas.
    assert!(ai::parse_note(r#"{"title":"Sets","keyIdeas":[]}"#).is_err());
    assert!(ai::parse_note("I could not do that").is_err());
}

#[test]
fn cards_parse_and_invalid_ones_are_dropped_not_fatal() {
    let raw = r#"[
        {"kind":"flash","front":"What is a logarithm?","back":"The inverse of exponentiation"},
        {"kind":"mcq","question":"log(ab) = ?","options":["log a + log b","log a - log b","log a * log b","ab"],"answer":0,"explanation":"Products become sums."},
        {"kind":"mcq","question":"Too few options","options":["a","b"],"answer":0},
        {"kind":"mcq","question":"Out of range","options":["a","b","c","d"],"answer":9},
        {"kind":"mcq","question":"Duplicates","options":["a","a","c","d"],"answer":0},
        {"kind":"flash","front":"","back":"empty front"},
        {"kind":"short","prompt":"State the product rule","answer":"log(ab) = log a + log b"}
    ]"#;

    let (cards, rejected) = ai::parse_cards(raw).unwrap();
    assert_eq!(cards.len(), 3, "the three valid cards survive");
    assert_eq!(rejected.len(), 4, "the four bad ones are reported");
    assert_eq!(cards[0].kind(), "flash");
    assert_eq!(cards[1].kind(), "mcq");
    assert_eq!(cards[2].kind(), "short");

    match &cards[1] {
        GeneratedCard::Mcq {
            options, answer, ..
        } => {
            assert_eq!(options.len(), 4);
            assert_eq!(*answer, 0);
        }
        other => panic!("expected an MCQ, got {other:?}"),
    }
}

#[test]
fn cards_accept_the_wrapped_object_form() {
    let raw = r#"{"cards":[{"kind":"flash","front":"Q","back":"A"}]}"#;
    let (cards, rejected) = ai::parse_cards(raw).unwrap();
    assert_eq!(cards.len(), 1);
    assert!(rejected.is_empty());

    assert!(ai::parse_cards(r#"{"notcards":[]}"#).is_err());
}

#[test]
fn ai_errors_route_correctly() {
    let quota = AiError::QuotaExhausted {
        until: Some("2026-10-28T00:00:00Z".into()),
    };
    assert!(quota.should_fall_through(), "try the next provider");
    assert!(!quota.is_retryable(), "the same provider will not work");

    let transient = AiError::Transient("connection reset".into());
    assert!(transient.is_retryable());
    assert!(transient.should_fall_through());

    let auth = AiError::Auth("401".into());
    assert!(!auth.is_retryable(), "a bad key stays bad");
    assert!(auth.should_fall_through());

    // A malformed reply is worth one retry, but another provider would be
    // answering a different question, so it does not fall through.
    let bad = AiError::BadResponse("no json".into());
    assert!(bad.is_retryable());
    assert!(!bad.should_fall_through());

    assert!(quota.to_string().contains("2026-10-28"));
}

#[test]
fn provider_ids_round_trip() {
    for id in ProviderId::default_order() {
        assert_eq!(ProviderId::parse(id.as_str()), Some(id));
        assert!(id.key_help_url().starts_with("https://"));
    }
    assert_eq!(ProviderId::parse(" GEMINI "), Some(ProviderId::Gemini));
    assert_eq!(ProviderId::parse("chatgpt"), None);
    assert_eq!(ProviderId::default_order()[0], ProviderId::Gemini);
}

// ---- job queue ----------------------------------------------------------

#[test]
fn jobs_run_in_order_and_complete() {
    let mut db = db();
    jobs::enqueue(&db, "distill", &serde_json::json!({"chunk": 1})).unwrap();
    jobs::enqueue(&db, "distill", &serde_json::json!({"chunk": 2})).unwrap();

    let first = jobs::claim(&mut db).unwrap().expect("a job is waiting");
    assert_eq!(first.status, JobStatus::Running);
    assert_eq!(first.attempts, 1);
    let payload: serde_json::Value = first.payload().unwrap();
    assert_eq!(payload["chunk"], 1);

    // The running job is not handed out twice.
    let second = jobs::claim(&mut db).unwrap().expect("second job");
    assert_eq!(second.id, first.id + 1);

    jobs::complete(&db, first.id).unwrap();
    assert_eq!(jobs::get(&db, first.id).unwrap().status, JobStatus::Done);

    jobs::complete(&db, second.id).unwrap();
    assert!(jobs::claim(&mut db).unwrap().is_none(), "queue is empty");
    assert_eq!(jobs::pending(&db).unwrap().len(), 0);
}

#[test]
fn a_failed_job_backs_off_then_gives_up() {
    let mut db = db();
    let job = jobs::enqueue(&db, "distill", &serde_json::json!({})).unwrap();

    for _ in 0..jobs::MAX_ATTEMPTS {
        let claimed = jobs::claim(&mut db)
            .unwrap()
            .expect("a retryable job comes back");
        let after = jobs::fail(&db, claimed.id, "connection reset").unwrap();
        if after.status == JobStatus::Failed {
            break;
        }
        assert_eq!(after.status, JobStatus::Queued);
        assert!(after.next_run_at.is_some(), "a retry waits out a backoff");
        // Clear the backoff so the test does not sleep.
        db.conn()
            .execute("UPDATE jobs SET next_run_at = NULL WHERE id = ?1", [job.id])
            .unwrap();
    }

    let final_state = jobs::get(&db, job.id).unwrap();
    assert_eq!(final_state.status, JobStatus::Failed);
    assert_eq!(final_state.attempts, jobs::MAX_ATTEMPTS);
    assert_eq!(final_state.last_error.as_deref(), Some("connection reset"));

    // The user can send it round again from the UI.
    let retried = jobs::retry(&db, job.id).unwrap();
    assert_eq!(retried.status, JobStatus::Queued);
    assert_eq!(retried.attempts, 0);
    assert!(retried.last_error.is_none());
}

#[test]
fn backoff_grows_and_is_capped() {
    assert_eq!(jobs::backoff_seconds(1), 2);
    assert_eq!(jobs::backoff_seconds(2), 4);
    assert_eq!(jobs::backoff_seconds(3), 8);
    assert_eq!(jobs::backoff_seconds(99), 300, "capped at five minutes");
}

#[test]
fn a_quota_pause_is_not_a_failure() {
    let mut db = db();
    let job = jobs::enqueue(&db, "distill", &serde_json::json!({})).unwrap();
    let claimed = jobs::claim(&mut db).unwrap().unwrap();
    assert_eq!(claimed.attempts, 1);

    let paused = jobs::pause_until(
        &db,
        job.id,
        "2099-01-01T00:00:00.000Z",
        "Daily AI limit reached",
    )
    .unwrap();
    assert_eq!(paused.status, JobStatus::Paused);
    assert_eq!(
        paused.attempts, 0,
        "waiting on quota must not burn a retry attempt"
    );

    // It stays parked until its time comes.
    assert!(
        jobs::claim(&mut db).unwrap().is_none(),
        "a paused job is not runnable yet"
    );

    db.conn()
        .execute(
            "UPDATE jobs SET next_run_at = '2000-01-01T00:00:00.000Z' WHERE id = ?1",
            [job.id],
        )
        .unwrap();
    assert!(
        jobs::claim(&mut db).unwrap().is_some(),
        "once the quota resets the job resumes on its own"
    );
}

#[test]
fn stuck_jobs_are_recovered_after_a_crash() {
    let mut db = db();
    jobs::enqueue(&db, "distill", &serde_json::json!({})).unwrap();
    let claimed = jobs::claim(&mut db).unwrap().unwrap();
    assert_eq!(claimed.status, JobStatus::Running);

    // Simulate the app being killed mid-job, then relaunched.
    assert_eq!(jobs::requeue_stuck(&db).unwrap(), 1);
    assert_eq!(
        jobs::get(&db, claimed.id).unwrap().status,
        JobStatus::Queued
    );
    assert!(jobs::claim(&mut db).unwrap().is_some());
}

#[test]
fn queue_counts_and_purge() {
    let mut db = db();
    for _ in 0..3 {
        jobs::enqueue(&db, "distill", &serde_json::json!({})).unwrap();
    }
    let done = jobs::claim(&mut db).unwrap().unwrap();
    jobs::complete(&db, done.id).unwrap();

    let counts = jobs::counts(&db).unwrap();
    assert_eq!(counts.done, 1);
    assert_eq!(counts.queued, 2);

    assert_eq!(jobs::purge_done(&db).unwrap(), 1);
    assert_eq!(jobs::counts(&db).unwrap().done, 0);
    assert_eq!(jobs::counts(&db).unwrap().queued, 2);
}

#[test]
fn an_empty_job_kind_is_rejected() {
    let db = db();
    assert!(jobs::enqueue(&db, "   ", &serde_json::json!({})).is_err());
}

// ---- quota --------------------------------------------------------------

#[test]
fn quota_accumulates_per_provider_per_day() {
    let db = db();
    quota::record(&db, "gemini", 1200).unwrap();
    let usage = quota::record(&db, "gemini", 800).unwrap();
    assert_eq!(usage.requests, 2);
    assert_eq!(usage.tokens, 2000);

    // Providers are counted separately.
    let groq = quota::record(&db, "groq", 50).unwrap();
    assert_eq!(groq.requests, 1);
    assert_eq!(quota::usage(&db, "gemini").unwrap().requests, 2);

    let all = quota::usage_today(&db).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].provider, "gemini");

    // A provider never used today reads as zero, not as an error.
    let unused = quota::usage(&db, "openrouter").unwrap();
    assert_eq!(unused.requests, 0);
    assert_eq!(unused.tokens, 0);
}

#[test]
fn an_exhausted_provider_is_skipped_until_it_resets() {
    let db = db();
    assert!(quota::is_available(&db, "gemini").unwrap());

    quota::mark_exhausted(&db, "gemini", "2099-01-01T00:00:00.000Z").unwrap();
    assert!(!quota::is_available(&db, "gemini").unwrap());
    assert!(quota::usage(&db, "gemini")
        .unwrap()
        .exhausted_until
        .is_some());
    assert!(
        quota::is_available(&db, "groq").unwrap(),
        "one provider running out must not block the others"
    );

    quota::clear_exhausted(&db, "gemini").unwrap();
    assert!(quota::is_available(&db, "gemini").unwrap());
}

#[test]
fn a_past_reset_time_heals_itself() {
    let db = db();
    // Yesterday's marker must not keep the provider switched off forever.
    quota::mark_exhausted(&db, "gemini", "2000-01-01T00:00:00.000Z").unwrap();
    assert!(quota::is_available(&db, "gemini").unwrap());
    assert!(quota::exhausted_until(&db, "gemini").unwrap().is_none());
}

#[test]
fn next_reset_reports_when_work_can_resume() {
    let db = db();
    let providers = vec!["gemini".to_string(), "groq".to_string()];

    assert_eq!(
        quota::next_reset(&db, &providers).unwrap(),
        None,
        "nothing to wait for while a provider is usable"
    );

    quota::mark_exhausted(&db, "gemini", "2099-06-01T00:00:00.000Z").unwrap();
    assert_eq!(
        quota::next_reset(&db, &providers).unwrap(),
        None,
        "groq is still available"
    );

    quota::mark_exhausted(&db, "groq", "2099-03-01T00:00:00.000Z").unwrap();
    assert_eq!(
        quota::next_reset(&db, &providers).unwrap().as_deref(),
        Some("2099-03-01T00:00:00.000Z"),
        "report the earliest moment work can resume"
    );
}
