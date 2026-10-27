//! IPC for CBT practice tests (Phase 5).
//!
//! Thin wrappers, as everywhere: the rules live in `studybites_core::cbt` so
//! they are testable on Linux without a WebView.

use crate::state::{to_ipc, AppState, LOCK_FAILED, NOT_ACTIVATED};
use std::sync::MutexGuard;
use studybites_core::{cbt, Db};
use tauri::State;

type Ipc<T> = Result<T, String>;

fn lock<'a>(state: &'a State<'_, AppState>) -> Result<MutexGuard<'a, Db>, String> {
    if !state.is_unlocked() {
        return Err(NOT_ACTIVATED.to_string());
    }
    state.db.lock().map_err(|_| LOCK_FAILED.to_string())
}

/// How many questions are available, so the setup screen can offer a sensible
/// length instead of letting the student pick one that will be refused.
#[tauri::command]
pub fn cbt_available(state: State<AppState>, subject_ids: Vec<i64>) -> Ipc<i64> {
    let db = lock(&state)?;
    cbt::available(&db, &subject_ids).map_err(to_ipc)
}

#[tauri::command]
pub fn cbt_start(
    state: State<AppState>,
    subject_ids: Vec<i64>,
    count: i64,
    minutes: i64,
) -> Ipc<cbt::Test> {
    let db = lock(&state)?;
    cbt::start(&db, &subject_ids, count, minutes).map_err(to_ipc)
}

/// Mark the paper. The answer key crosses to the WebView only here.
#[tauri::command]
pub fn cbt_submit(
    state: State<AppState>,
    test_id: i64,
    chosen: Vec<Option<usize>>,
) -> Ipc<cbt::TestResult> {
    let mut db = lock(&state)?;
    cbt::submit(&mut db, test_id, &chosen).map_err(to_ipc)
}

#[tauri::command]
pub fn cbt_history(state: State<AppState>, limit: i64) -> Ipc<Vec<cbt::TestSummary>> {
    let db = lock(&state)?;
    cbt::history(&db, limit).map_err(to_ipc)
}
