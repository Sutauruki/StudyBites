//! Versioned migrations, applied with SQLite's `user_version`.
//!
//! Rules: migrations are append-only. Never edit a migration that has shipped —
//! add a new one. `SCHEMA_VERSION` is simply `MIGRATIONS.len()`.

use crate::error::{CoreError, Result};
use rusqlite::Connection;

/// Each entry is one migration, applied in order inside a transaction.
pub const MIGRATIONS: &[&str] = &[
    // ---- 001: full Phase 1-4 schema (Brief Section 5) --------------------
    // The whole shape is created up front so later phases add rows, not tables.
    r#"
    CREATE TABLE subjects (
        id          INTEGER PRIMARY KEY,
        name        TEXT    NOT NULL,
        created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        archived    INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0,1)),
        exam_date   TEXT
    );
    CREATE UNIQUE INDEX idx_subjects_name ON subjects(name);

    CREATE TABLE scheme_topics (
        id                INTEGER PRIMARY KEY,
        subject_id        INTEGER NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
        parent_id         INTEGER REFERENCES scheme_topics(id) ON DELETE CASCADE,
        title             TEXT    NOT NULL,
        week              INTEGER,
        position          INTEGER NOT NULL DEFAULT 0,
        is_off_scheme     INTEGER NOT NULL DEFAULT 0 CHECK (is_off_scheme IN (0,1)),
        supports_topic_id INTEGER REFERENCES scheme_topics(id) ON DELETE SET NULL,
        is_preview        INTEGER NOT NULL DEFAULT 0 CHECK (is_preview IN (0,1)),
        created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX idx_topics_subject ON scheme_topics(subject_id, parent_id, position);

    CREATE TABLE materials (
        id                INTEGER PRIMARY KEY,
        subject_id        INTEGER NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
        topic_id          INTEGER REFERENCES scheme_topics(id) ON DELETE SET NULL,
        original_filename TEXT    NOT NULL,
        mime              TEXT    NOT NULL,
        size              INTEGER NOT NULL,
        sha256            TEXT    NOT NULL,
        cold_path         TEXT,
        uploaded_at       TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        status            TEXT    NOT NULL DEFAULT 'queued',
        error             TEXT
    );
    CREATE INDEX idx_materials_subject ON materials(subject_id, uploaded_at);
    CREATE INDEX idx_materials_sha ON materials(sha256);

    CREATE TABLE notes (
        id           INTEGER PRIMARY KEY,
        material_id  INTEGER REFERENCES materials(id) ON DELETE CASCADE,
        topic_id     INTEGER REFERENCES scheme_topics(id) ON DELETE SET NULL,
        content_json TEXT    NOT NULL,
        embedding    BLOB,
        is_preview   INTEGER NOT NULL DEFAULT 0 CHECK (is_preview IN (0,1)),
        created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX idx_notes_topic ON notes(topic_id);

    CREATE TABLE cards (
        id           INTEGER PRIMARY KEY,
        note_id      INTEGER REFERENCES notes(id) ON DELETE CASCADE,
        topic_id     INTEGER REFERENCES scheme_topics(id) ON DELETE SET NULL,
        kind         TEXT    NOT NULL CHECK (kind IN ('flash','mcq','short')),
        payload_json TEXT    NOT NULL,
        is_preview   INTEGER NOT NULL DEFAULT 0 CHECK (is_preview IN (0,1)),
        retired      INTEGER NOT NULL DEFAULT 0 CHECK (retired IN (0,1)),
        created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX idx_cards_topic ON cards(topic_id, retired);

    CREATE TABLE card_state (
        card_id     INTEGER PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
        stability   REAL    NOT NULL DEFAULT 0,
        difficulty  REAL    NOT NULL DEFAULT 0,
        due         TEXT    NOT NULL,
        last_review TEXT,
        reps        INTEGER NOT NULL DEFAULT 0,
        lapses      INTEGER NOT NULL DEFAULT 0,
        state       INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_card_state_due ON card_state(due);

    CREATE TABLE review_log (
        id          INTEGER PRIMARY KEY,
        card_id     INTEGER NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
        rating      INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 4),
        reviewed_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        elapsed_ms  INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_review_log_card ON review_log(card_id, reviewed_at);

    CREATE TABLE sessions (
        id              INTEGER PRIMARY KEY,
        subject_id      INTEGER REFERENCES subjects(id) ON DELETE SET NULL,
        started_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
        ended_at        TEXT,
        planned_minutes INTEGER NOT NULL DEFAULT 10,
        cards_done      INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE jobs (
        id           INTEGER PRIMARY KEY,
        kind         TEXT    NOT NULL,
        payload_json TEXT    NOT NULL,
        status       TEXT    NOT NULL DEFAULT 'queued',
        attempts     INTEGER NOT NULL DEFAULT 0,
        next_run_at  TEXT,
        last_error   TEXT,
        created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    CREATE INDEX idx_jobs_status ON jobs(status, next_run_at);

    CREATE TABLE quota_usage (
        provider TEXT    NOT NULL,
        date     TEXT    NOT NULL,
        requests INTEGER NOT NULL DEFAULT 0,
        tokens   INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (provider, date)
    );

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
    // ---- 002: EULA acceptance (Brief Section 13) -------------------------
    // Every acceptance is kept rather than overwritten, so that re-accepting
    // a revised agreement leaves a record of what was agreed and when.
    //
    // The licence itself is deliberately NOT here: the database key is derived
    // from it, so it has to be readable before the database can be opened.
    // It lives beside the database as a signed file (see `license::stored`).
    r#"
    CREATE TABLE eula_acceptance (
        id          INTEGER PRIMARY KEY,
        version     TEXT NOT NULL,
        accepted_at TEXT NOT NULL
    );
    "#,
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

fn user_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Bring `conn` up to `SCHEMA_VERSION`. Safe to call on every launch.
pub fn migrate(conn: &mut Connection) -> Result<i64> {
    let current = user_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(CoreError::SchemaTooNew {
            found: current,
            expected: SCHEMA_VERSION,
        });
    }
    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = idx as i64 + 1;
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        // PRAGMA does not accept bound parameters.
        tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
        tx.commit()?;
    }
    user_version(conn)
}
