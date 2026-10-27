//! Per-provider daily quota tracking (Brief 7.3).
//!
//! Free tiers are counted per calendar day, so usage is keyed by provider and
//! `YYYY-MM-DD`. When a provider reports a rate limit it is marked exhausted
//! until a reset time, and the router skips it until then.

use crate::db::Db;
use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Settings key holding the exhaustion marker for one provider.
fn exhausted_key(provider: &str) -> String {
    format!("quota.exhausted.{provider}")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub provider: String,
    pub date: String,
    pub requests: i64,
    pub tokens: i64,
    /// RFC3339 time this provider becomes usable again, when it is exhausted.
    pub exhausted_until: Option<String>,
}

/// Today's date in UTC, as SQLite sees it.
pub fn today(db: &Db) -> Result<String> {
    Ok(db
        .conn()
        .query_row("SELECT strftime('%Y-%m-%d','now')", [], |r| r.get(0))?)
}

fn now_rfc3339(db: &Db) -> Result<String> {
    Ok(db
        .conn()
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
            r.get(0)
        })?)
}

/// Record one request against a provider's daily allowance.
pub fn record(db: &Db, provider: &str, tokens: i64) -> Result<Usage> {
    let date = today(db)?;
    db.conn().execute(
        "INSERT INTO quota_usage(provider, date, requests, tokens)
         VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(provider, date) DO UPDATE SET
            requests = requests + 1,
            tokens = tokens + excluded.tokens",
        (provider, &date, tokens.max(0)),
    )?;
    usage(db, provider)
}

/// Today's usage for one provider.
pub fn usage(db: &Db, provider: &str) -> Result<Usage> {
    let date = today(db)?;
    let mut stmt = db
        .conn()
        .prepare("SELECT requests, tokens FROM quota_usage WHERE provider = ?1 AND date = ?2")?;
    let mut rows = stmt.query((provider, &date))?;
    let (requests, tokens) = match rows.next()? {
        Some(row) => (row.get(0)?, row.get(1)?),
        None => (0, 0),
    };
    Ok(Usage {
        provider: provider.to_string(),
        date,
        requests,
        tokens,
        exhausted_until: exhausted_until(db, provider)?,
    })
}

/// Usage for every provider that has been used today.
pub fn usage_today(db: &Db) -> Result<Vec<Usage>> {
    let date = today(db)?;
    let mut stmt = db.conn().prepare(
        "SELECT provider, requests, tokens FROM quota_usage
          WHERE date = ?1 ORDER BY provider",
    )?;
    let rows = stmt.query_map([&date], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let collected = rows.collect::<rusqlite::Result<Vec<_>>>()?;

    let mut out = Vec::with_capacity(collected.len());
    for (provider, requests, tokens) in collected {
        let exhausted_until = exhausted_until(db, &provider)?;
        out.push(Usage {
            provider,
            date: date.clone(),
            requests,
            tokens,
            exhausted_until,
        });
    }
    Ok(out)
}

/// Mark a provider as out of quota until `until` (RFC3339).
pub fn mark_exhausted(db: &Db, provider: &str, until: &str) -> Result<()> {
    db.set_setting(&exhausted_key(provider), until)
}

/// Clear the exhaustion marker, e.g. after a successful request.
pub fn clear_exhausted(db: &Db, provider: &str) -> Result<()> {
    db.conn().execute(
        "DELETE FROM settings WHERE key = ?1",
        [exhausted_key(provider)],
    )?;
    Ok(())
}

/// The time a provider becomes usable again, or `None` if it is usable now.
/// A marker in the past is cleared as a side effect, so it self-heals.
pub fn exhausted_until(db: &Db, provider: &str) -> Result<Option<String>> {
    let Some(until) = db.setting(&exhausted_key(provider))? else {
        return Ok(None);
    };
    if until <= now_rfc3339(db)? {
        clear_exhausted(db, provider)?;
        return Ok(None);
    }
    Ok(Some(until))
}

/// Whether a provider can be used right now.
pub fn is_available(db: &Db, provider: &str) -> Result<bool> {
    Ok(exhausted_until(db, provider)?.is_none())
}

/// The earliest time any exhausted provider frees up, for the "resumes at"
/// message. `None` when at least one provider is available now.
pub fn next_reset(db: &Db, providers: &[String]) -> Result<Option<String>> {
    let mut earliest: Option<String> = None;
    for provider in providers {
        match exhausted_until(db, provider)? {
            // Something is usable now, so nothing is being waited on.
            None => return Ok(None),
            Some(until) => {
                earliest = Some(match earliest {
                    Some(current) if current <= until => current,
                    _ => until,
                });
            }
        }
    }
    Ok(earliest)
}
