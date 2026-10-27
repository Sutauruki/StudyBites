//! Encrypted database handle.
//!
//! Phase 1 uses a fixed development key (see `DEV_KEY`). Phase 3 replaces it
//! with a device-derived key: HKDF-SHA256(device_id || license_secret).

use crate::error::{CoreError, Result};
use crate::migrations;
use rusqlite::Connection;
use std::path::Path;

/// Temporary Phase 1 key. This is NOT a secret and provides no real protection;
/// it only keeps the on-disk format encrypted so Phase 3 is a key swap, not a
/// migration. Replaced by a device-derived key in Phase 3.
pub const DEV_KEY: &str = "studybites-dev-key-phase1";

/// Escape a key for use inside a single-quoted SQL literal.
///
/// Phase 3 will pass a 32-byte derived key as `x'<hex>'` instead, which needs
/// no escaping at all.
fn escape_literal(value: &str) -> String {
    value.replace('\'', "''")
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (creating if needed) the database at `path` with `key`.
    pub fn open(path: impl AsRef<Path>, key: &str) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::prepare(conn, key)
    }

    /// In-memory database, used by tests.
    pub fn open_in_memory(key: &str) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::prepare(conn, key)
    }

    fn prepare(conn: Connection, key: &str) -> Result<Self> {
        // The key must be set before any other statement touches the file, and
        // it has to go through a literal `PRAGMA key` statement: rusqlite's
        // `pragma_update` does not key an existing SQLCipher file correctly, so
        // the database would open once and then read as "not a database".
        conn.execute_batch(&format!("PRAGMA key = '{}';", escape_literal(key)))
            .map_err(|_| CoreError::Locked)?;

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
