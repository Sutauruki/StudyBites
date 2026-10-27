//! The background worker that drains the ingestion queue (Brief 3.4).
//!
//! Heavy work never runs on the UI thread: the window stays responsive while
//! uploads are processed, and progress is pushed to the frontend as events.
//!
//! One worker thread, woken on demand. A single worker is deliberate — the
//! free tiers are rate-limited per minute, so running several in parallel
//! would only trip the limits sooner.

use crate::keys;
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use studybites_core::ai::ProviderId;
use studybites_core::embed::LexicalEmbedder;
use studybites_core::providers::{Provider, UreqHttp};
use studybites_core::{ingest, jobs};
use tauri::{AppHandle, Manager};

/// Shared "there is work to do" flag.
#[derive(Default)]
pub struct Signal {
    woken: Mutex<bool>,
    condvar: Condvar,
    running: AtomicBool,
}

impl Signal {
    /// Tell the worker to look again.
    pub fn wake(&self) {
        let mut woken = self.woken.lock().unwrap_or_else(|e| e.into_inner());
        *woken = true;
        self.condvar.notify_all();
    }

    /// Block until someone calls `wake`.
    fn wait(&self) {
        let mut woken = self.woken.lock().unwrap_or_else(|e| e.into_inner());
        while !*woken {
            woken = self.condvar.wait(woken).unwrap_or_else(|e| e.into_inner());
        }
        *woken = false;
    }
}

/// Nudge the worker from a command handler.
pub fn wake(app: &AppHandle) {
    if let Some(signal) = app.try_state::<Arc<Signal>>() {
        signal.wake();
    }
}

/// Start the worker thread. Called once, during setup.
pub fn spawn(app: AppHandle, signal: Arc<Signal>) {
    if signal.running.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::Builder::new()
        .name("studybites-ingest".into())
        .spawn(move || loop {
            signal.wait();
            drain(&app);
        })
        .expect("the ingestion worker could not be started");
}

/// Push the current queue counts to the UI.
fn report(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(db) = state.db.lock() {
            if let Ok(counts) = jobs::counts(&db) {
                crate::phase2::emit_progress(app, &counts);
            }
        }
    }
}

/// Work through the queue until it is empty, paused, or has nothing runnable.
fn drain(app: &AppHandle) {
    let embedder = LexicalEmbedder;

    loop {
        // The keys are re-read each pass, so adding one in Settings takes
        // effect without a restart.
        let http = UreqHttp;
        let providers: Vec<Provider<'_>> = ProviderId::default_order()
            .into_iter()
            .filter_map(|id| keys::provider_key(id).map(|key| Provider::new(id, key, &http)))
            .collect();

        if providers.is_empty() {
            // Nothing can be processed without a key; the queue simply waits.
            break;
        }

        let Some(state) = app.try_state::<AppState>() else {
            break;
        };

        let outcome = {
            let Ok(mut db) = state.db.lock() else {
                break;
            };
            ingest::process_one(&mut db, &providers, &embedder)
        };

        match outcome {
            Ok(ingest::Processed::Idle) => break,
            // A pause means every provider is out of quota; stop rather than
            // spin, and let the next wake-up try again.
            Ok(ingest::Processed::Paused { .. }) => break,
            Ok(_) => report(app),
            Err(_) => break,
        }
    }

    report(app);
}
