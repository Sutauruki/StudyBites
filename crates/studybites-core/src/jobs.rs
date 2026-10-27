//! Persisted, resumable job queue (Brief 3.4).
//!
//! Ingestion is a sequence of AI calls that can fail halfway: the network
//! drops, the free tier runs out, the app is closed. Every unit of work is a
//! row, so closing the app mid-upload loses at most the job in flight, and
//! quota exhaustion pauses rather than fails.
//!
//! Workers `claim()` a job, do the work, then `complete()` or `fail()` it.

use crate::db::Db;
use crate::error::{CoreError, Result};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// Give up after this many attempts and surface the error to the user.
pub const MAX_ATTEMPTS: i64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
    /// Waiting on a provider's quota to reset. Not an error.
    Paused,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Paused => "paused",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "running" => Self::Running,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "paused" => Self::Paused,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: i64,
    pub kind: String,
    pub payload_json: String,
    pub status: JobStatus,
    pub attempts: i64,
    pub next_run_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

impl Job {
    /// Decode the payload into the type a worker expects.
    pub fn payload<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_str(&self.payload_json)?)
    }
}

fn row_to_job(row: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: row.get("id")?,
        kind: row.get("kind")?,
        payload_json: row.get("payload_json")?,
        status: JobStatus::parse(&row.get::<_, String>("status")?),
        attempts: row.get("attempts")?,
        next_run_at: row.get("next_run_at")?,
        last_error: row.get("last_error")?,
        created_at: row.get("created_at")?,
    })
}

/// Exponential backoff in seconds: 2, 4, 8, 16 … capped at five minutes.
///
/// The exponent is clamped only to keep the shift in range; the cap itself is
/// the `min`, so the documented five minutes is actually reachable.
pub fn backoff_seconds(attempts: i64) -> i64 {
    let exponent = attempts.clamp(1, 16) as u32;
    2_i64.saturating_pow(exponent).min(300)
}

/// Add a job to the queue.
pub fn enqueue<T: Serialize>(db: &Db, kind: &str, payload: &T) -> Result<Job> {
    let kind = kind.trim();
    if kind.is_empty() {
        return Err(CoreError::Invalid("Job kind cannot be empty".into()));
    }
    let payload_json = serde_json::to_string(payload)?;
    db.conn().execute(
        "INSERT INTO jobs(kind, payload_json, status) VALUES (?1, ?2, 'queued')",
        (kind, &payload_json),
    )?;
    get(db, db.conn().last_insert_rowid())
}

pub fn get(db: &Db, id: i64) -> Result<Job> {
    let mut stmt = db.conn().prepare("SELECT * FROM jobs WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => Ok(row_to_job(row)?),
        None => Err(CoreError::NotFound { kind: "job", id }),
    }
}

/// Take the next runnable job and mark it running.
///
/// Jobs waiting out a backoff or a quota pause are skipped until their
/// `next_run_at` passes. Returns `None` when there is nothing to do.
pub fn claim(db: &mut Db) -> Result<Option<Job>> {
    let tx = db.conn_mut().transaction()?;
    let found: Option<i64> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM jobs
              WHERE status IN ('queued','paused')
                AND (next_run_at IS NULL
                     OR next_run_at <= strftime('%Y-%m-%dT%H:%M:%fZ','now'))
              ORDER BY id
              LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        }
    };

    let Some(id) = found else {
        tx.rollback()?;
        return Ok(None);
    };

    tx.execute(
        "UPDATE jobs SET status = 'running', attempts = attempts + 1 WHERE id = ?1",
        [id],
    )?;
    tx.commit()?;
    Ok(Some(get(db, id)?))
}

/// Mark a job finished.
pub fn complete(db: &Db, id: i64) -> Result<Job> {
    db.conn().execute(
        "UPDATE jobs SET status = 'done', last_error = NULL, next_run_at = NULL
          WHERE id = ?1",
        [id],
    )?;
    get(db, id)
}

/// Record a failure. Retries with backoff until `MAX_ATTEMPTS`, then gives up
/// and leaves the error visible on the row.
pub fn fail(db: &Db, id: i64, error: &str) -> Result<Job> {
    let job = get(db, id)?;
    if job.attempts >= MAX_ATTEMPTS {
        db.conn().execute(
            "UPDATE jobs SET status = 'failed', last_error = ?2, next_run_at = NULL
              WHERE id = ?1",
            (id, error),
        )?;
    } else {
        let delay = backoff_seconds(job.attempts);
        db.conn().execute(
            "UPDATE jobs
                SET status = 'queued',
                    last_error = ?2,
                    next_run_at = strftime('%Y-%m-%dT%H:%M:%fZ','now', ?3)
              WHERE id = ?1",
            (id, error, format!("+{delay} seconds")),
        )?;
    }
    get(db, id)
}

/// Park a job until a provider's quota resets. This is not a failure, so the
/// attempt is given back and no error is recorded.
pub fn pause_until(db: &Db, id: i64, until: &str, reason: &str) -> Result<Job> {
    db.conn().execute(
        "UPDATE jobs
            SET status = 'paused',
                attempts = MAX(attempts - 1, 0),
                next_run_at = ?2,
                last_error = ?3
          WHERE id = ?1",
        (id, until, reason),
    )?;
    get(db, id)
}

/// Put a failed job back in the queue, e.g. from a "Try again" button.
pub fn retry(db: &Db, id: i64) -> Result<Job> {
    db.conn().execute(
        "UPDATE jobs
            SET status = 'queued', attempts = 0, next_run_at = NULL, last_error = NULL
          WHERE id = ?1",
        [id],
    )?;
    get(db, id)
}

/// Jobs that are not finished, oldest first.
pub fn pending(db: &Db) -> Result<Vec<Job>> {
    let mut stmt = db
        .conn()
        .prepare("SELECT * FROM jobs WHERE status != 'done' ORDER BY id")?;
    let rows = stmt.query_map([], row_to_job)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueCounts {
    pub queued: i64,
    pub running: i64,
    pub done: i64,
    pub failed: i64,
    pub paused: i64,
}

/// Counts per status, for the per-upload progress display.
pub fn counts(db: &Db) -> Result<QueueCounts> {
    let mut stmt = db
        .conn()
        .prepare("SELECT status, count(*) FROM jobs GROUP BY status")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;

    let mut counts = QueueCounts::default();
    for entry in rows {
        let (status, n) = entry?;
        match JobStatus::parse(&status) {
            JobStatus::Queued => counts.queued = n,
            JobStatus::Running => counts.running = n,
            JobStatus::Done => counts.done = n,
            JobStatus::Failed => counts.failed = n,
            JobStatus::Paused => counts.paused = n,
        }
    }
    Ok(counts)
}

/// Clear finished jobs so the table does not grow without bound.
pub fn purge_done(db: &Db) -> Result<usize> {
    Ok(db
        .conn()
        .execute("DELETE FROM jobs WHERE status = 'done'", [])?)
}

/// Recover jobs left `running` by a crash or a forced quit, so work resumes on
/// the next launch instead of hanging.
pub fn requeue_stuck(db: &Db) -> Result<usize> {
    Ok(db.conn().execute(
        "UPDATE jobs SET status = 'queued', next_run_at = NULL WHERE status = 'running'",
        [],
    )?)
}
