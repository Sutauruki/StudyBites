//! Phase 2 IPC: uploads, AI, study sessions and the update check.
//!
//! Kept apart from `commands.rs` (Phase 1's subjects and topics) because
//! everything here may touch the network or run for a while, and so is pushed
//! onto a worker rather than blocking the window.

use crate::keys;
use crate::state::{to_ipc, AppState, LOCK_FAILED};
use std::sync::MutexGuard;
use studybites_core::ai::{AiProvider, ProviderId, SchemeEntry, TopicMatch};
use studybites_core::embed::{Embedder, LexicalEmbedder};
use studybites_core::providers::{Attempt, Http, Provider, UreqGetHttp, UreqHttp};
use studybites_core::{
    files, ingest, jobs, library, marking, preview, quota, study, subjects, update, Db, Rating,
    Subject, Topic,
};
use tauri::{Emitter, State};

type Ipc<T> = Result<T, String>;

fn lock<'a>(state: &'a State<'_, AppState>) -> Result<MutexGuard<'a, Db>, String> {
    if !state.is_unlocked() {
        return Err(crate::state::NOT_ACTIVATED.to_string());
    }
    state.db.lock().map_err(|_| LOCK_FAILED.to_string())
}

/// Build the provider list in priority order, skipping any without a key.
/// The keys are read here and never leave Rust.
/// Settings key holding the model the student chose for a provider.
fn model_key(id: ProviderId) -> String {
    format!("ai.model.{}", id.as_str())
}

fn providers<'a>(db: &Db, http: &'a dyn Http) -> Ipc<Vec<Provider<'a>>> {
    let mut list = Vec::new();
    for id in ProviderId::default_order() {
        let Some(key) = keys::provider_key(db, id)? else {
            continue;
        };
        let chosen = db
            .setting(&model_key(id))
            .map_err(to_ipc)?
            .unwrap_or_default();
        let provider = Provider::new(id, key, http).with_model(chosen);
        // Without a model there is nothing to call, so skip rather than
        // spend a request discovering that.
        if !provider.model.trim().is_empty() {
            list.push(provider);
        }
    }
    Ok(list)
}

fn require_providers<'a>(
    state: &State<'_, AppState>,
    http: &'a dyn Http,
) -> Ipc<Vec<Provider<'a>>> {
    let list = {
        let db = lock(state)?;
        providers(&db, http)?
    };
    if list.is_empty() {
        return Err(
            "No AI provider is ready. Open Settings, add a key, then choose a model for it.".into(),
        );
    }
    Ok(list)
}

fn available_ids(state: &State<'_, AppState>, list: &[Provider<'_>]) -> Ipc<Vec<ProviderId>> {
    let db = lock(state)?;
    Ok(list
        .iter()
        .map(|p| p.id)
        .filter(|id| quota::is_available(&db, id.as_str()).unwrap_or(true))
        .collect())
}

/// Turn a round of failed attempts into one sentence a student can act on.
fn describe_attempts(attempts: Vec<Attempt>) -> String {
    if attempts.is_empty() {
        return "No AI provider is configured.".into();
    }
    if attempts
        .iter()
        .all(|a| matches!(a.error, studybites_core::AiError::QuotaExhausted { .. }))
    {
        return "Daily AI limit reached — this will work again after the limit resets.".into();
    }
    if attempts
        .iter()
        .all(|a| matches!(a.error, studybites_core::AiError::ModelUnavailable(_)))
    {
        return "The chosen model is no longer offered. Open Settings and pick a \
                current one from the list."
            .into();
    }
    attempts
        .iter()
        .map(|a| format!("{}: {}", a.provider.as_str(), a.error))
        .collect::<Vec<_>>()
        .join("; ")
}

// ---- uploads ------------------------------------------------------------

/// Accept a file the user picked, queue its work, and start the worker.
///
/// The file is read in Rust: the WebView never gets filesystem access.
#[tauri::command]
pub async fn material_upload(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    subject_id: i64,
    topic_id: Option<i64>,
    path: String,
) -> Ipc<ingest::Accepted> {
    let file = std::path::PathBuf::from(&path);
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .to_string();

    let bytes = files::read_checked(&file).map_err(to_ipc)?;
    let app_data = state.app_data.clone();

    let file_key = state.file_key();
    let accepted = {
        let mut db = lock(&state)?;
        ingest::accept_upload(
            &mut db, &app_data, &file_key, subject_id, topic_id, &filename, &bytes,
        )
        .map_err(to_ipc)?
    };

    if accepted.duplicate_of.is_none() {
        crate::worker::wake(&app);
    }
    Ok(accepted)
}

#[tauri::command]
pub async fn material_upload_bytes(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    subject_id: i64,
    topic_id: Option<i64>,
    filename: String,
    bytes: Vec<u8>,
) -> Ipc<ingest::Accepted> {
    if bytes.len() as u64 > studybites_core::files::MAX_FILE_BYTES {
        return Err("that file is larger than the 64 MB limit".into());
    }
    if bytes.is_empty() {
        return Err("that file is empty".into());
    }
    let app_data = state.app_data.clone();
    let file_key = state.file_key();
    let accepted = {
        let mut db = lock(&state)?;
        ingest::accept_upload(
            &mut db, &app_data, &file_key, subject_id, topic_id, &filename, &bytes,
        )
        .map_err(to_ipc)?
    };
    if accepted.duplicate_of.is_none() {
        crate::worker::wake(&app);
    }
    Ok(accepted)
}

#[tauri::command]
pub async fn material_upload_uri(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    subject_id: i64,
    topic_id: Option<i64>,
    uri: String,
    filename: String,
) -> Ipc<ingest::Accepted> {
    use tauri_plugin_fs::FsExt;
    let uri = tauri::Url::parse(&uri).map_err(|e| format!("invalid selected file URI: {e}"))?;
    let bytes = app
        .fs()
        .read(uri)
        .map_err(|e| format!("the selected file could not be read: {e}"))?;
    material_upload_bytes(app, state, subject_id, topic_id, filename, bytes).await
}

/// Read a file's text without storing anything, for the topic-match preview
/// and for scheme-of-work parsing.
#[tauri::command]
pub async fn file_preview_text(path: String) -> Ipc<String> {
    let file = std::path::PathBuf::from(&path);
    let extracted = files::extract(&file).map_err(to_ipc)?;
    // A few thousand characters is all the model is shown anyway.
    Ok(extracted.text.chars().take(4_000).collect())
}

#[tauri::command]
pub async fn file_preview_text_bytes(filename: String, bytes: Vec<u8>) -> Ipc<String> {
    if bytes.len() as u64 > studybites_core::files::MAX_FILE_BYTES {
        return Err("that file is larger than the 64 MB limit".into());
    }
    let extracted =
        files::extract_from_bytes(std::path::Path::new(&filename), &bytes).map_err(to_ipc)?;
    Ok(extracted.text.chars().take(4_000).collect())
}

#[tauri::command]
pub async fn file_preview_text_uri(
    app: tauri::AppHandle,
    uri: String,
    filename: String,
) -> Ipc<String> {
    use tauri_plugin_fs::FsExt;
    let uri = tauri::Url::parse(&uri).map_err(|e| format!("invalid selected file URI: {e}"))?;
    let bytes = app
        .fs()
        .read(uri)
        .map_err(|e| format!("the selected file could not be read: {e}"))?;
    if bytes.is_empty() {
        return Err("the selected file was empty".into());
    }
    let extracted =
        files::extract_from_bytes(std::path::Path::new(&filename), &bytes).map_err(to_ipc)?;
    Ok(extracted.text.chars().take(4_000).collect())
}

#[tauri::command]
pub fn materials_list(state: State<AppState>, subject_id: i64) -> Ipc<Vec<library::Material>> {
    let db = lock(&state)?;
    library::materials_for_subject(&db, subject_id).map_err(to_ipc)
}

#[tauri::command]
pub fn material_set_topic(
    state: State<AppState>,
    id: i64,
    topic_id: Option<i64>,
) -> Ipc<library::Material> {
    let db = lock(&state)?;
    library::material_set_topic(&db, id, topic_id).map_err(to_ipc)
}

#[tauri::command]
pub fn material_delete(state: State<AppState>, id: i64) -> Ipc<()> {
    let app_data = state.app_data.clone();
    let db = lock(&state)?;
    let cold = library::material_delete(&db, id).map_err(to_ipc)?;
    if let Some(cold) = cold {
        // A leftover file is untidy, not harmful, so this is not worth
        // failing the delete over.
        let _ = files::cold_delete(&app_data, &cold);
    }
    Ok(())
}

/// Decompress and save the original file (Brief 3.10).
#[tauri::command]
pub async fn material_save_original(
    state: State<'_, AppState>,
    id: i64,
    destination: String,
) -> Ipc<String> {
    let app_data = state.app_data.clone();
    let material = {
        let db = lock(&state)?;
        library::material_get(&db, id).map_err(to_ipc)?
    };
    let cold = material
        .cold_path
        .ok_or("the original file was not kept for this upload")?;

    let bytes = files::cold_read(&app_data, &state.file_key(), &cold).map_err(to_ipc)?;
    std::fs::write(&destination, bytes).map_err(|e| format!("could not save the file: {e}"))?;
    Ok(destination)
}

// ---- AI-assisted steps --------------------------------------------------

/// Ask which outline topic an upload belongs to (Brief 3.3).
///
/// The answer is a suggestion: the student always confirms.
#[tauri::command]
pub async fn topic_suggest(
    state: State<'_, AppState>,
    subject_id: i64,
    text: String,
) -> Ipc<TopicMatch> {
    let titles: Vec<String> = {
        let db = lock(&state)?;
        studybites_core::topics::list_flat(&db, subject_id)
            .map_err(to_ipc)?
            .into_iter()
            .map(|t| t.title)
            .collect()
    };
    if titles.is_empty() {
        return Ok(TopicMatch {
            index: None,
            confidence: 0.0,
            reason: "This subject has no scheme of work yet.".into(),
        });
    }

    let http = UreqHttp;
    let list = require_providers(&state, &http)?;
    let available = available_ids(&state, &list)?;

    studybites_core::providers::route(&list, &|id| available.contains(&id), |p| {
        p.match_topic(&text, &titles)
    })
    .map(|(_, matched)| matched)
    .map_err(describe_attempts)
}

/// Parse a scheme-of-work document into a draft outline (Brief 3.2).
#[tauri::command]
pub async fn scheme_parse(state: State<'_, AppState>, text: String) -> Ipc<Vec<SchemeEntry>> {
    let http = UreqHttp;
    let list = require_providers(&state, &http)?;
    let available = available_ids(&state, &list)?;

    studybites_core::providers::route(&list, &|id| available.contains(&id), |p| {
        p.parse_scheme(&text)
    })
    .map(|(_, entries)| entries)
    .map_err(describe_attempts)
}

/// Save a scheme the student has reviewed and confirmed.
#[tauri::command]
pub fn scheme_save(
    state: State<AppState>,
    subject_id: i64,
    entries: Vec<SchemeEntry>,
) -> Ipc<Vec<Topic>> {
    let db = lock(&state)?;
    ingest::save_scheme(&db, subject_id, &entries).map_err(to_ipc)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainResult {
    pub answer: String,
    /// Titles of the notes the answer was drawn from, so the student can check.
    pub sources: Vec<String>,
}

/// Explain something, grounded in the student's own notes (Brief 3.8).
#[tauri::command]
pub async fn explain(
    state: State<'_, AppState>,
    subject_id: i64,
    question: String,
) -> Ipc<ExplainResult> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err("Type a question first.".into());
    }

    let embedder = LexicalEmbedder;
    let vector = embedder.embed(&question);

    let notes = {
        let db = lock(&state)?;
        study::retrieve_notes(&db, subject_id, &vector, 5).map_err(to_ipc)?
    };
    if notes.is_empty() {
        return Err("There are no notes in this subject yet to answer from.".into());
    }

    let contents: Vec<_> = notes.iter().map(|n| n.content.clone()).collect();
    let http = UreqHttp;
    let list = require_providers(&state, &http)?;
    let available = available_ids(&state, &list)?;

    let (_, answer) =
        studybites_core::providers::route(&list, &|id| available.contains(&id), |p| {
            p.explain(&question, &contents)
        })
        .map_err(describe_attempts)?;

    Ok(ExplainResult {
        answer,
        sources: notes.iter().map(|n| n.content.title.clone()).collect(),
    })
}

// ---- marking typed answers ---------------------------------------------

/// Whether a subject is studied by typing answers out.
///
/// On by default: writing the answer from memory is the stronger test. It is
/// off for subjects where typing fights the material — formulas, derivations —
/// and the student flips it on the subject page.
#[tauri::command]
pub fn typed_answers_get(state: State<AppState>, subject_id: i64) -> Ipc<bool> {
    let db = lock(&state)?;
    Ok(db
        .setting(&typed_key(subject_id))
        .map_err(to_ipc)?
        .map(|v| v != "off")
        .unwrap_or(true))
}

#[tauri::command]
pub fn typed_answers_set(state: State<AppState>, subject_id: i64, enabled: bool) -> Ipc<bool> {
    let db = lock(&state)?;
    db.set_setting(&typed_key(subject_id), if enabled { "on" } else { "off" })
        .map_err(to_ipc)?;
    Ok(enabled)
}

fn typed_key(subject_id: i64) -> String {
    format!("study.typed.{subject_id}")
}

/// Mark what the student wrote.
///
/// Tries locally first and only reaches for a provider when the answer is a
/// genuine judgement call. If no provider can be reached — no key, no network,
/// quota gone — this does NOT fail the card: it says so and lets the student
/// grade themselves, exactly as before. Studying must never stop because the
/// internet did (D21).
#[tauri::command]
pub async fn answer_mark(
    state: State<'_, AppState>,
    question: String,
    expected: String,
    typed: String,
) -> Ipc<MarkOutcome> {
    if let Some(mark) = marking::quick_mark(&typed, &expected) {
        return Ok(MarkOutcome {
            mark: Some(mark),
            unavailable: None,
        });
    }

    let http = UreqHttp;
    let list = match require_providers(&state, &http) {
        Ok(list) => list,
        Err(why) => return Ok(MarkOutcome::unavailable(why)),
    };
    let available = match available_ids(&state, &list) {
        Ok(ids) => ids,
        Err(why) => return Ok(MarkOutcome::unavailable(why)),
    };

    match studybites_core::providers::route(&list, &|id| available.contains(&id), |p| {
        p.mark_answer(&question, &expected, &typed)
    }) {
        Ok((_, mark)) => Ok(MarkOutcome {
            mark: Some(mark),
            unavailable: None,
        }),
        Err(attempts) => Ok(MarkOutcome::unavailable(describe_attempts(attempts))),
    }
}

/// Either a mark, or the reason there is none and a prompt to self-grade.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkOutcome {
    pub mark: Option<marking::Mark>,
    /// Set when marking could not run. The card still counts; the student
    /// grades it themselves.
    pub unavailable: Option<String>,
}

impl MarkOutcome {
    fn unavailable(why: String) -> Self {
        Self {
            mark: None,
            unavailable: Some(why),
        }
    }
}

// ---- studying -----------------------------------------------------------

#[tauri::command]
pub fn due_counts(state: State<AppState>, subject_id: Option<i64>) -> Ipc<study::DueCounts> {
    let db = lock(&state)?;
    study::due_counts(&db, subject_id).map_err(to_ipc)
}

#[tauri::command]
pub fn session_start(
    state: State<AppState>,
    subject_id: Option<i64>,
    minutes: i64,
    new_limit: Option<i64>,
) -> Ipc<study::Session> {
    let mut db = lock(&state)?;
    let limit = new_limit.unwrap_or(study::DEFAULT_NEW_PER_SESSION);
    study::start_session(&mut db, subject_id, minutes, limit).map_err(to_ipc)
}

#[tauri::command]
pub fn session_grade(
    state: State<AppState>,
    session_id: Option<i64>,
    card_id: i64,
    rating: i64,
    elapsed_ms: i64,
) -> Ipc<study::GradeResult> {
    let rating = Rating::from_i64(rating).ok_or("that grade is not one of 1-4")?;
    let mut db = lock(&state)?;
    study::grade(&mut db, session_id, card_id, rating, elapsed_ms).map_err(to_ipc)
}

#[tauri::command]
pub fn session_end(state: State<AppState>, session_id: i64) -> Ipc<study::SessionSummary> {
    let db = lock(&state)?;
    study::end_session(&db, session_id).map_err(to_ipc)
}

#[tauri::command]
pub fn mastery(state: State<AppState>, subject_id: i64) -> Ipc<Vec<study::TopicMastery>> {
    let db = lock(&state)?;
    study::mastery_for_subject(&db, subject_id).map_err(to_ipc)
}

// ---- previews and exam mode (Brief 3.7, 3.9) ----------------------------

/// Is there a next topic worth offering a head start on?
///
/// Costs nothing: with a scheme of work this is a lookup, and with none it
/// simply declines rather than spending a request to guess.
#[tauri::command]
pub fn preview_offer(
    state: State<AppState>,
    subject_id: i64,
) -> Ipc<Option<preview::PreviewOffer>> {
    let db = lock(&state)?;
    preview::next_topic(&db, subject_id).map_err(to_ipc)
}

/// Generate and store a labelled preview of the next topic.
///
/// Only called when the student accepts the offer — previews are offered,
/// never forced.
#[tauri::command]
pub async fn preview_generate(
    state: State<'_, AppState>,
    subject_id: i64,
    topic_id: Option<i64>,
    topic_title: Option<String>,
) -> Ipc<usize> {
    let (subject, recent) = {
        let db = lock(&state)?;
        (
            subjects::get(&db, subject_id).map_err(to_ipc)?,
            preview::recent_topics(&db, subject_id).map_err(to_ipc)?,
        )
    };

    let http = UreqHttp;
    let list = require_providers(&state, &http)?;
    let available = available_ids(&state, &list)?;

    let (_, generated) =
        studybites_core::providers::route(&list, &|id| available.contains(&id), |p| {
            p.preview_next(&subject.name, &recent, topic_title.as_deref(), false)
        })
        .map_err(describe_attempts)?;

    // Only pay for a web search when the model says it is guessing
    // (Brief 3.7), and only when there was no outline to go on.
    let generated = if generated.confidence < 0.5 && topic_title.is_none() {
        studybites_core::providers::route(&list, &|id| available.contains(&id), |p| {
            p.preview_next(&subject.name, &recent, topic_title.as_deref(), true)
        })
        .map(|(_, better)| better)
        .unwrap_or(generated)
    } else {
        generated
    };

    let embedder = LexicalEmbedder;
    let mut db = lock(&state)?;
    preview::save_preview(&mut db, topic_id, &generated, &embedder).map_err(to_ipc)
}

/// Set or unset a subject's exam date (Brief 3.9).
#[tauri::command]
pub fn exam_date_set(
    state: State<AppState>,
    subject_id: i64,
    exam_date: Option<String>,
) -> Ipc<Subject> {
    let db = lock(&state)?;
    subjects::set_exam_date(&db, subject_id, exam_date.as_deref()).map_err(to_ipc)
}

/// Days until the exam, and whether the session mix has shifted for it.
#[tauri::command]
pub fn exam_status(state: State<AppState>, subject_id: i64) -> Ipc<ExamStatus> {
    let db = lock(&state)?;
    Ok(ExamStatus {
        days_until: preview::days_until_exam(&db, subject_id).map_err(to_ipc)?,
        exam_mode: preview::exam_mode(&db, Some(subject_id)).map_err(to_ipc)?,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExamStatus {
    pub days_until: Option<f64>,
    pub exam_mode: bool,
}

// ---- queue and quota ----------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    pub counts: jobs::QueueCounts,
    pub quota: Vec<quota::Usage>,
    /// When paused work resumes, if it is waiting on a quota reset.
    pub resumes_at: Option<String>,
}

#[tauri::command]
pub fn queue_status(state: State<AppState>) -> Ipc<QueueStatus> {
    let db = lock(&state)?;
    let names: Vec<String> = ProviderId::default_order()
        .into_iter()
        .map(|id| id.as_str().to_string())
        .collect();
    Ok(QueueStatus {
        counts: jobs::counts(&db).map_err(to_ipc)?,
        quota: quota::usage_today(&db).map_err(to_ipc)?,
        resumes_at: quota::next_reset(&db, &names).map_err(to_ipc)?,
    })
}

/// Put failed uploads back in the queue.
#[tauri::command]
pub fn queue_retry(app: tauri::AppHandle, state: State<AppState>) -> Ipc<jobs::QueueCounts> {
    {
        let db = lock(&state)?;
        for job in jobs::pending(&db).map_err(to_ipc)? {
            if job.status == jobs::JobStatus::Failed {
                jobs::retry(&db, job.id).map_err(to_ipc)?;
            }
        }
    }
    crate::worker::wake(&app);
    let db = lock(&state)?;
    jobs::counts(&db).map_err(to_ipc)
}

// ---- settings: keys and updates -----------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub label: String,
    pub configured: bool,
    pub help_url: String,
    pub default_model: String,
    pub selected_model: String,
}

/// The model in effect for a provider: the chosen one, else the default.
fn chosen_model(db: &Db, id: ProviderId) -> Ipc<String> {
    db.setting(&model_key(id)).map_err(to_ipc).map(|model| {
        model
            .filter(|model| !model.trim().is_empty())
            .map(|model| model.trim().to_string())
            .unwrap_or_else(|| Provider::default_model(id).to_string())
    })
}

#[tauri::command]
pub fn providers_list(state: State<AppState>) -> Ipc<Vec<ProviderInfo>> {
    let db = lock(&state)?;
    let configured = keys::configured(&db)?;
    ProviderId::default_order()
        .into_iter()
        .map(|id| {
            Ok(ProviderInfo {
                id: id.as_str().to_string(),
                label: match id {
                    ProviderId::Gemini => "Google Gemini".into(),
                    ProviderId::Groq => "Groq".into(),
                    ProviderId::OpenRouter => "OpenRouter".into(),
                },
                configured: configured.contains(&id),
                help_url: id.key_help_url().to_string(),
                default_model: Provider::default_model(id).to_string(),
                selected_model: chosen_model(&db, id)?,
            })
        })
        .collect()
}

/// The models a provider currently offers, asked of the provider itself.
#[tauri::command]
pub async fn provider_models(state: State<'_, AppState>, provider: String) -> Ipc<Vec<String>> {
    let id = ProviderId::parse(&provider).ok_or("that is not a provider StudyBites knows")?;
    // OpenRouter publishes its list without a key; the others need one.
    let key = {
        let db = lock(&state)?;
        keys::provider_key(&db, id)?.unwrap_or_default()
    };
    if key.is_empty() && id != ProviderId::OpenRouter {
        return Err("Add this provider's key first, then its models can be listed.".into());
    }
    studybites_core::providers::list_models(&UreqGetHttp, id, &key).map_err(|e| e.to_string())
}

/// Choose which model a provider should use.
#[tauri::command]
pub fn provider_model_set(state: State<AppState>, provider: String, model: String) -> Ipc<()> {
    let id = ProviderId::parse(&provider).ok_or("that is not a provider StudyBites knows")?;
    let db = lock(&state)?;
    db.set_setting(&model_key(id), model.trim()).map_err(to_ipc)
}

/// Save a provider key. An empty value removes it.
#[tauri::command]
pub fn provider_key_set(state: State<AppState>, provider: String, key: String) -> Ipc<()> {
    let id = ProviderId::parse(&provider).ok_or("that is not a provider StudyBites knows")?;
    let db = lock(&state)?;
    keys::set_provider_key(&db, id, &key)
}

#[tauri::command]
pub fn github_token_set(state: State<AppState>, token: String) -> Ipc<()> {
    let db = lock(&state)?;
    keys::set_github_token(&db, &token)
}

#[tauri::command]
pub fn github_token_present(state: State<AppState>) -> Ipc<bool> {
    let db = lock(&state)?;
    Ok(keys::github_token(&db)?.is_some())
}

/// Check whether a newer build exists. Never downloads or installs anything.
#[tauri::command]
pub async fn update_check(state: State<'_, AppState>) -> Ipc<update::UpdateStatus> {
    let (feed, token) = {
        let db = lock(&state)?;
        (
            db.setting(update::SETTING_FEED)
                .map_err(to_ipc)?
                .unwrap_or_else(|| update::DEFAULT_FEED.to_string()),
            keys::github_token(&db)?,
        )
    };
    update::check(
        &UreqGetHttp,
        &feed,
        token.as_deref(),
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(to_ipc)
}

/// Emitted so the UI can refresh while the queue drains.
pub fn emit_progress(app: &tauri::AppHandle, counts: &jobs::QueueCounts) {
    let _ = app.emit("queue-progress", counts);
}
