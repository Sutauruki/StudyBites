//! Shared app state: one database handle behind a mutex.
//!
//! Every Tauri command is a thin wrapper that locks this, calls
//! `studybites-core` and maps the error to a string for the frontend.

use std::sync::Mutex;
use studybites_core::db::DEV_KEY;
use studybites_core::{CoreError, Db};
use tauri::{AppHandle, Manager};

pub struct AppState {
    pub db: Mutex<Db>,
    /// Where cold storage lives. Resolved once, so every caller agrees.
    pub app_data: std::path::PathBuf,
}

impl AppState {
    /// Open the database under the OS app-data directory.
    ///
    /// Phase 1 uses the fixed development key. Phase 3 swaps it for a key
    /// derived from the device fingerprint and the activation payload.
    pub fn load(app: &AppHandle) -> Result<Self, CoreError> {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| CoreError::Invalid(format!("no app data directory: {e}")))?;
        let db = Db::open(dir.join("studybites.db"), DEV_KEY)?;
        // Anything left mid-flight by a crash goes back in the queue.
        let _ = studybites_core::jobs::requeue_stuck(&db);
        Ok(Self {
            db: Mutex::new(db),
            app_data: dir,
        })
    }
}

/// Errors crossing the IPC boundary are plain strings the UI can show.
pub fn to_ipc(err: CoreError) -> String {
    err.to_string()
}

/// A poisoned mutex means another command panicked. Report it rather than
/// panicking again and taking the window down.
pub const LOCK_FAILED: &str =
    "The database is busy after an earlier error — please restart StudyBites.";
