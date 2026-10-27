//! Encrypted database handle.
//!
//! The key is never stored. On an activated install it is recomputed at every
//! launch by `crypto::derive` from the device identity and the licence secret,
//! so a database file copied to another PC derives a different key and will
//! not open — which is the whole anti-sharing mechanism (Brief Section 9).

use crate::crypto::SecretKey;
use crate::error::{CoreError, Result};
use crate::migrations;
use rusqlite::Connection;
use std::path::Path;
use zeroize::Zeroize;

/// Development key, used by tests and by builds made without a licensing key.
///
/// This is NOT a secret and provides no protection. It exists so the on-disk
/// format is encrypted from the first build onwards, making activation a key
/// change rather than a schema migration.
pub const DEV_KEY: Key<'static> = Key::Passphrase("studybites-dev-key-phase1");

/// How a key reaches SQLCipher.
///
/// The distinction matters: a passphrase is stretched by SQLCipher's own
/// PBKDF2, whereas a raw key is used as-is. Our derived keys are already the
/// output of HKDF, so stretching them again would only cost time.
#[derive(Clone, Copy)]
pub enum Key<'a> {
    Passphrase(&'a str),
    Raw(&'a SecretKey),
}

impl Key<'_> {
    /// The exact text that follows `PRAGMA key = `, quoting included.
    ///
    /// The caller zeroizes this; it is key material in a String.
    fn pragma_value(&self) -> String {
        match self {
            // Doubling quotes is the only escape a SQL string literal needs.
            Key::Passphrase(p) => format!("'{}'", p.replace('\'', "''")),
            // SQLCipher's raw-key form: exactly 64 hex digits, no derivation.
            Key::Raw(k) => format!("\"x'{}'\"", k.to_hex()),
        }
    }
}

/// Keeps SQLCipher's crypto provider registered for the life of the process.
///
/// SQLCipher reference-counts the provider: `sqlcipher_activate` registers it
/// on the first keyed connection and `sqlcipher_deactivate` frees it again
/// when the last one closes. A process that opens and closes short-lived
/// connections from several threads — which is exactly what the test suite
/// does — can therefore have one thread tear the provider down in the window
/// where another is attaching to it. The loser fails with
/// `sqlcipherCodecAttach: sqlcipher not initialized`, which surfaces here as
/// a spurious `Locked`.
///
/// Holding one keyed connection open forever keeps that count above zero, so
/// the provider is registered once and never freed. The connection is never
/// used for anything; its existence is the point.
///
/// Honest caveat: this failure was seen once in CI and does not reproduce on
/// the development machine, so this is reasoned from SQLCipher's own
/// lifecycle rather than from a reproduction. It cannot make matters worse —
/// the cost is one idle in-memory connection.
static SQLCIPHER_KEEPALIVE: std::sync::OnceLock<std::sync::Mutex<Connection>> =
    std::sync::OnceLock::new();

fn ensure_sqlcipher_ready() {
    SQLCIPHER_KEEPALIVE.get_or_init(|| {
        let conn = Connection::open_in_memory().expect("in-memory sqlite is always available");
        // The connection has to be *keyed* to count as an activation; an
        // unkeyed one never touches the codec at all.
        let _ = conn.execute_batch(
            "PRAGMA key = 'sqlcipher-keepalive'; SELECT count(*) FROM sqlite_master;",
        );
        std::sync::Mutex::new(conn)
    });
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (creating if needed) the database at `path` with `key`.
    pub fn open(path: impl AsRef<Path>, key: Key<'_>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::prepare(conn, key)
    }

    /// In-memory database, used by tests.
    pub fn open_in_memory(key: Key<'_>) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::prepare(conn, key)
    }

    fn prepare(conn: Connection, key: Key<'_>) -> Result<Self> {
        ensure_sqlcipher_ready();

        // The key must be set before any other statement touches the file, and
        // it has to go through a literal `PRAGMA key` statement: rusqlite's
        // `pragma_update` does not key an existing SQLCipher file correctly, so
        // the database would open once and then read as "not a database".
        let mut pragma = format!("PRAGMA key = {};", key.pragma_value());
        let keyed = conn.execute_batch(&pragma);
        pragma.zeroize();
        // A failure *here* is the cipher refusing to start at all, not a wrong
        // key. Keep the real message: debugging it from a bare "Locked" cost a
        // CI round once already.
        keyed?;

        // Fails with NotADatabase when the key is wrong — which is exactly what
        // a database copied to another device looks like.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|_| CoreError::Locked)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;

        let mut db = Self { conn };
        migrations::migrate(&mut db.conn)?;
        Ok(db)
    }

    /// Open with `key`, falling back to re-keying a database that is still on
    /// the development key.
    ///
    /// Builds before activation shipped with `DEV_KEY`. Turning licensing on
    /// must not strand the study history of anyone already using the app, so
    /// an existing database is re-keyed in place to the derived key the first
    /// time an activated build opens it. SQLCipher rewrites every page, which
    /// for a database this size is milliseconds.
    pub fn open_or_rekey(path: impl AsRef<Path>, key: Key<'_>) -> Result<Self> {
        let path = path.as_ref();
        match Self::open(path, key) {
            Ok(db) => Ok(db),
            Err(CoreError::Locked) if path.exists() => {
                let legacy = Self::open(path, DEV_KEY)?;
                legacy.rekey(key)?;
                drop(legacy);
                // Re-open so the handle is keyed the normal way from the start.
                Self::open(path, key)
            }
            Err(other) => Err(other),
        }
    }

    /// Change the key of an open database in place.
    pub fn rekey(&self, key: Key<'_>) -> Result<()> {
        let mut pragma = format!("PRAGMA rekey = {};", key.pragma_value());
        let result = self.conn.execute_batch(&pragma);
        pragma.zeroize();
        result?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Read a `settings` value.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    /// Write a `settings` value.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )?;
        Ok(())
    }
}
