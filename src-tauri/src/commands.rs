//! IPC surface. Inputs are validated in `studybites-core`, never trusted here.

use crate::state::{to_ipc, AppState, LOCK_FAILED};
use std::sync::MutexGuard;
use studybites_core::{subjects, topics, Db, NewTopic, Subject, Topic};
use tauri::State;

type Ipc<T> = Result<T, String>;

/// Take the database lock, or report that an earlier command poisoned it.
fn lock<'a>(state: &'a State<'_, AppState>) -> Result<MutexGuard<'a, Db>, String> {
    state.db.lock().map_err(|_| LOCK_FAILED.to_string())
}

#[tauri::command]
pub fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "phase": 1,
    })
}

// ---- subjects -----------------------------------------------------------

#[tauri::command]
pub fn subjects_list(state: State<AppState>, include_archived: bool) -> Ipc<Vec<Subject>> {
    let db = lock(&state)?;
    subjects::list(&db, include_archived).map_err(to_ipc)
}

#[tauri::command]
pub fn subject_get(state: State<AppState>, id: i64) -> Ipc<Subject> {
    let db = lock(&state)?;
    subjects::get(&db, id).map_err(to_ipc)
}

#[tauri::command]
pub fn subject_create(state: State<AppState>, name: String) -> Ipc<Subject> {
    let db = lock(&state)?;
    subjects::create(&db, &name).map_err(to_ipc)
}

#[tauri::command]
pub fn subject_rename(state: State<AppState>, id: i64, name: String) -> Ipc<Subject> {
    let db = lock(&state)?;
    subjects::rename(&db, id, &name).map_err(to_ipc)
}

#[tauri::command]
pub fn subject_set_archived(state: State<AppState>, id: i64, archived: bool) -> Ipc<Subject> {
    let db = lock(&state)?;
    subjects::set_archived(&db, id, archived).map_err(to_ipc)
}

#[tauri::command]
pub fn subject_delete(state: State<AppState>, id: i64) -> Ipc<()> {
    let db = lock(&state)?;
    subjects::delete(&db, id).map_err(to_ipc)
}

// ---- scheme of work -----------------------------------------------------

#[tauri::command]
pub fn topics_list(state: State<AppState>, subject_id: i64) -> Ipc<Vec<Topic>> {
    let db = lock(&state)?;
    topics::list_tree(&db, subject_id).map_err(to_ipc)
}

#[tauri::command]
pub fn topic_create(state: State<AppState>, input: NewTopic) -> Ipc<Topic> {
    let db = lock(&state)?;
    topics::create(&db, &input).map_err(to_ipc)
}

/// `title`/`week` left out mean "leave as is". `clear_week` wins over `week`
/// and is how the UI empties the field: `Option<Option<i64>>` has no honest
/// JSON representation, so the flag is explicit instead.
#[tauri::command]
pub fn topic_update(
    state: State<AppState>,
    id: i64,
    title: Option<String>,
    week: Option<i64>,
    clear_week: Option<bool>,
) -> Ipc<Topic> {
    let week = if clear_week.unwrap_or(false) {
        Some(None)
    } else {
        week.map(Some)
    };
    let db = lock(&state)?;
    topics::update(&db, id, title.as_deref(), week).map_err(to_ipc)
}

#[tauri::command]
pub fn topic_delete(state: State<AppState>, id: i64) -> Ipc<()> {
    let db = lock(&state)?;
    topics::delete(&db, id).map_err(to_ipc)
}

#[tauri::command]
pub fn topic_reorder(
    state: State<AppState>,
    subject_id: i64,
    parent_id: Option<i64>,
    ordered_ids: Vec<i64>,
) -> Ipc<Vec<Topic>> {
    let mut db = lock(&state)?;
    topics::reorder(&mut db, subject_id, parent_id, &ordered_ids).map_err(to_ipc)
}

#[tauri::command]
pub fn topic_nudge(state: State<AppState>, id: i64, delta: i64) -> Ipc<Vec<Topic>> {
    let mut db = lock(&state)?;
    topics::nudge(&mut db, id, delta).map_err(to_ipc)
}

// ---- settings -----------------------------------------------------------

#[tauri::command]
pub fn setting_get(state: State<AppState>, key: String) -> Ipc<Option<String>> {
    lock(&state)?.setting(&key).map_err(to_ipc)
}

#[tauri::command]
pub fn setting_set(state: State<AppState>, key: String, value: String) -> Ipc<()> {
    lock(&state)?.set_setting(&key, &value).map_err(to_ipc)
}
