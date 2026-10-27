//! Shared app state: the device identity, the licence, and one database
//! handle behind a mutex.
//!
//! Before activation there is no real database — its key does not exist yet,
//! because it is derived from the licence (Brief Section 9). So the handle
//! starts as an empty in-memory placeholder and `lock()` refuses to hand it
//! out until `status.unlocked` is true. Activation swaps the real, on-disk,
//! device-keyed database into the same slot, which keeps every command body
//! unchanged: there is exactly one gate, and it is `lock()`.

use std::sync::Mutex;
use studybites_core::crypto::{self, SecretKey};
use studybites_core::db::Key;
use studybites_core::device::{self, DeviceProfile};
use studybites_core::license;
use studybites_core::{CoreError, Db};
use tauri::{AppHandle, Manager};

/// Bumped whenever `resources/EULA.md` changes in a way users must re-accept.
pub const EULA_VERSION: &str = "1.0";

/// What the activation and EULA screens need to know. Nothing secret is in
/// here — it crosses to the WebView on every launch.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// False in builds made without `STUDYBITES_LICENSE_PUBKEY`, which run
    /// unlocked so development is not blocked. Shown plainly in Settings.
    pub licensing_enabled: bool,
    pub unlocked: bool,
    pub eula_version: String,
    pub eula_accepted: bool,
    /// Who the licence names, once activated.
    pub buyer_name: Option<String>,
    /// Shown on the activation screen for the owner to type into the tool.
    pub request_code: Option<String>,
    /// Components that differ from activation day. Advisory only — nothing
    /// refuses to run because of it.
    pub device_changed: Vec<String>,
    /// Set when the machine could not be identified at all.
    pub problem: Option<String>,
}

pub struct AppState {
    /// Placeholder until activation; the real database afterwards.
    pub db: Mutex<Db>,
    /// Cold-storage key. `None` until unlocked.
    pub file_key: Mutex<Option<SecretKey>>,
    pub app_data: std::path::PathBuf,
    pub profile: DeviceProfile,
    pub status: Mutex<Status>,
    /// The EULA is accepted before there is a database to record it in, so
    /// the timestamp waits here until one exists.
    pending_eula: Mutex<Option<String>>,
}

fn now() -> String {
    // RFC 3339 in UTC, matching what the schema stores elsewhere.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's days-to-civil algorithm, so a timestamp needs no date crate.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<Self, CoreError> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|e| CoreError::Invalid(format!("no app data directory: {e}")))?;
        std::fs::create_dir_all(&app_data)?;

        let profile = device::collect();
        let request_code = profile.device_tag().ok().map(|tag| {
            license::RequestCode {
                device_tag: tag,
                app_major: major(),
                app_minor: minor(),
            }
            .encode()
        });
        let problem = profile.device_id().err().map(|e| e.to_string());

        let state = Self {
            // Placeholder. `lock()` will not hand it out while `unlocked` is
            // false, and it never touches the disk.
            db: Mutex::new(Db::open_in_memory(studybites_core::db::DEV_KEY)?),
            file_key: Mutex::new(None),
            status: Mutex::new(Status {
                licensing_enabled: license::licensing_enabled(),
                unlocked: false,
                eula_version: EULA_VERSION.to_string(),
                eula_accepted: false,
                buyer_name: None,
                request_code,
                device_changed: Vec::new(),
                problem,
            }),
            app_data,
            profile,
            pending_eula: Mutex::new(None),
        };

        // A build with no public key compiled in cannot verify anything, so
        // it runs on the development key and says so rather than pretending.
        if !license::licensing_enabled() {
            state.unlock_development()?;
        } else if let Some(code) = license::stored(&state.app_data) {
            // A stored licence is re-verified on every launch (Brief 10), not
            // trusted because it is on disk.
            let _ = state.unlock_with(&code);
        }
        Ok(state)
    }

    fn unlock_development(&self) -> Result<(), CoreError> {
        let db = Db::open_or_rekey(
            self.app_data.join("studybites.db"),
            studybites_core::db::DEV_KEY,
        )?;
        self.finish_unlock(db, None, None)
    }

    /// Verify a code, bind it to this machine, derive the keys and open up.
    ///
    /// Used both at startup with the stored code and when the student types a
    /// new one, so there is only one path that can ever unlock the app.
    pub fn unlock_with(&self, code: &str) -> Result<(), CoreError> {
        let public = license::embedded_public_key().ok_or_else(|| {
            CoreError::Invalid(
                "this build of StudyBites has no licensing key, so it cannot be activated.".into(),
            )
        })?;

        let verified = license::verify(code, &public)?;
        let tag = self.profile.device_tag()?;
        if !verified.matches_device(&tag) {
            return Err(CoreError::Invalid(
                "that activation code belongs to a different computer. Each code works on the \
                 one device it was issued for."
                    .into(),
            ));
        }

        let device_id = self.profile.device_id()?;
        let license = verified.into_inner();
        let keys = crypto::derive(&device_id, &license.secret);

        let db = Db::open_or_rekey(self.app_data.join("studybites.db"), Key::Raw(&keys.db))?;
        let buyer = license.buyer_name.clone();
        self.finish_unlock(db, Some(keys.file.clone()), Some(buyer))
    }

    /// Store a newly accepted code, then unlock with it.
    pub fn activate(&self, code: &str) -> Result<(), CoreError> {
        let code = code.trim();
        self.unlock_with(code)?;
        license::save(
            &self.app_data,
            code,
            &license::StoredProfile {
                components: self.profile.components.clone(),
                activated_at: now(),
            },
        )
    }

    fn finish_unlock(
        &self,
        db: Db,
        file_key: Option<SecretKey>,
        buyer: Option<String>,
    ) -> Result<(), CoreError> {
        // Anything left mid-flight by a crash goes back in the queue.
        let _ = studybites_core::jobs::requeue_stuck(&db);

        // The EULA may have been accepted minutes ago, before any database
        // existed to record it in.
        let pending = self.pending_eula.lock().ok().and_then(|mut p| p.take());
        if let Some(at) = pending {
            if !license::eula_accepted(&db, EULA_VERSION).unwrap_or(false) {
                let _ = license::record_eula(&db, EULA_VERSION, &at);
            }
        }
        let eula_accepted = license::eula_accepted(&db, EULA_VERSION).unwrap_or(false);

        let changed = license::stored_profile(&self.app_data)
            .map(|stored| device::drift(&stored.components, &self.profile.components).changed)
            .unwrap_or_default();

        if let Ok(mut slot) = self.db.lock() {
            *slot = db;
        }
        if let Ok(mut slot) = self.file_key.lock() {
            *slot = file_key;
        }
        if let Ok(mut status) = self.status.lock() {
            status.unlocked = true;
            status.eula_accepted = eula_accepted;
            status.buyer_name = buyer;
            status.device_changed = changed;
        }
        Ok(())
    }

    /// Record that the agreement was accepted, now or once a database exists.
    pub fn accept_eula(&self) -> Result<(), CoreError> {
        let at = now();
        let unlocked = self.status.lock().map(|s| s.unlocked).unwrap_or(false);
        if unlocked {
            let db = self
                .db
                .lock()
                .map_err(|_| CoreError::Invalid(LOCK_FAILED.to_string()))?;
            if !license::eula_accepted(&db, EULA_VERSION)? {
                license::record_eula(&db, EULA_VERSION, &at)?;
            }
        } else if let Ok(mut pending) = self.pending_eula.lock() {
            *pending = Some(at);
        }
        if let Ok(mut status) = self.status.lock() {
            status.eula_accepted = true;
        }
        Ok(())
    }

    pub fn status(&self) -> Status {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|p| p.into_inner().clone())
    }

    pub fn is_unlocked(&self) -> bool {
        self.status.lock().map(|s| s.unlocked).unwrap_or(false)
    }

    /// The cold-storage key, when there is one. Development builds have none,
    /// so originals are stored under the development file key instead.
    pub fn file_key(&self) -> SecretKey {
        self.file_key
            .lock()
            .ok()
            .and_then(|k| k.clone())
            .unwrap_or_else(|| {
                // Matches the development database key: no protection, but a
                // consistent one, so files written now are readable later.
                crypto::derive(&[0u8; 32], b"studybites-development")
                    .file
                    .clone()
            })
    }
}

fn major() -> u8 {
    env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0)
}

fn minor() -> u8 {
    env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0)
}

/// Errors crossing the IPC boundary are plain strings the UI can show.
pub fn to_ipc(err: CoreError) -> String {
    err.to_string()
}

/// A poisoned mutex means another command panicked. Report it rather than
/// panicking again and taking the window down.
pub const LOCK_FAILED: &str =
    "The database is busy after an earlier error — please restart StudyBites.";

/// Shown when a command that needs data runs before activation.
pub const NOT_ACTIVATED: &str =
    "StudyBites is not activated on this computer yet. Enter your activation code to continue.";
