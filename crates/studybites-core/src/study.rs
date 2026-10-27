//! Study sessions, grading and mastery (Brief 3.5, 3.6).
//!
//! The daily memory lives here, and it is all database state: FSRS card
//! states, review logs, session rows. Nothing is asked of an AI to remember
//! what happened yesterday, so opening the app costs zero tokens and is
//! instant.

use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::fsrs::{self, Memory, Rating, State};
use crate::library::{self, Card};
use serde::{Deserialize, Serialize};

/// Default cap on unseen cards per session, so a bite never feels heavy.
pub const DEFAULT_NEW_PER_SESSION: i64 = 10;
/// Roughly how long one card takes, used to size a session to its minutes.
const SECONDS_PER_CARD: f64 = 20.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCard {
    pub card: Card,
    /// True when this card has never been studied.
    pub is_new: bool,
    pub topic_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: i64,
    pub subject_id: Option<i64>,
    pub planned_minutes: i64,
    pub cards: Vec<SessionCard>,
    pub due_count: i64,
    pub new_count: i64,
}

/// How many cards are waiting, for the home screen's gentle "X due today".
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DueCounts {
    pub due: i64,
    pub new: i64,
}

/// Cards due now, plus unseen ones, for a subject or across all of them.
pub fn due_counts(db: &Db, subject_id: Option<i64>) -> Result<DueCounts> {
    let sql = "SELECT
            COALESCE(SUM(CASE WHEN s.state != 0
                               AND s.due <= strftime('%Y-%m-%dT%H:%M:%fZ','now')
                          THEN 1 ELSE 0 END), 0) AS due,
            COALESCE(SUM(CASE WHEN s.state = 0 THEN 1 ELSE 0 END), 0) AS new
          FROM cards c
          JOIN card_state s ON s.card_id = c.id
          LEFT JOIN notes n ON n.id = c.note_id
          LEFT JOIN materials m ON m.id = n.material_id
          LEFT JOIN scheme_topics t ON t.id = c.topic_id
         WHERE c.retired = 0
           AND (?1 IS NULL OR m.subject_id = ?1 OR t.subject_id = ?1)";

    let mut stmt = db.conn().prepare(sql)?;
    let counts = stmt.query_row([subject_id], |row| {
        Ok(DueCounts {
            due: row.get(0)?,
            new: row.get(1)?,
        })
    })?;
    Ok(counts)
}

/// Build a session: due reviews first, then a capped number of new cards
/// from the most recent material (Brief 3.5).
///
/// Preview cards sort last everywhere — they must never outrank real class
/// material in scheduling.
pub fn start_session(
    db: &mut Db,
    subject_id: Option<i64>,
    planned_minutes: i64,
    new_limit: i64,
) -> Result<Session> {
    if !(1..=120).contains(&planned_minutes) {
        return Err(CoreError::Invalid(
            "a session must be between 1 and 120 minutes".into(),
        ));
    }
    let capacity = ((planned_minutes as f64 * 60.0) / SECONDS_PER_CARD).ceil() as i64;
    let new_limit = new_limit.clamp(0, capacity);

    // As an exam approaches the mix shifts toward weak topics and coverage,
    // still inside the same bite-size limits (Brief 3.9).
    let exam_mode = crate::preview::exam_mode(db, subject_id)?;

    let due = select_cards(db, subject_id, true, capacity, exam_mode)?;
    let remaining = (capacity - due.len() as i64).max(0).min(new_limit);
    let fresh = select_cards(db, subject_id, false, remaining, exam_mode)?;

    let due_count = due.len() as i64;
    let new_count = fresh.len() as i64;

    db.conn().execute(
        "INSERT INTO sessions(subject_id, planned_minutes) VALUES (?1, ?2)",
        (subject_id, planned_minutes),
    )?;
    let id = db.conn().last_insert_rowid();

    let mut cards = due;
    cards.extend(fresh);

    Ok(Session {
        id,
        subject_id,
        planned_minutes,
        cards,
        due_count,
        new_count,
    })
}

/// Pick either due cards or new ones.
fn select_cards(
    db: &Db,
    subject_id: Option<i64>,
    due_only: bool,
    limit: i64,
    exam_mode: bool,
) -> Result<Vec<SessionCard>> {
    if limit <= 0 {
        return Ok(Vec::new());
    }

    // Normally: due cards most-overdue first, new cards newest-material
    // first, which is what the student just uploaded and most wants.
    //
    // In exam mode: weakest first instead. Low stability means the card has
    // not stuck, and that is what is worth the remaining time.
    let (filter, order) = if due_only {
        (
            "s.state != 0 AND s.due <= strftime('%Y-%m-%dT%H:%M:%fZ','now')",
            if exam_mode {
                "c.is_preview ASC, s.stability ASC, s.due ASC"
            } else {
                "c.is_preview ASC, s.due ASC"
            },
        )
    } else {
        (
            "s.state = 0",
            if exam_mode {
                // Spread across topics rather than finishing one document.
                "c.is_preview ASC, c.topic_id ASC, c.id DESC"
            } else {
                "c.is_preview ASC, c.id DESC"
            },
        )
    };

    let sql = format!(
        "SELECT c.*, t.title AS topic_title, s.state AS fsrs_state
           FROM cards c
           JOIN card_state s ON s.card_id = c.id
           LEFT JOIN notes n ON n.id = c.note_id
           LEFT JOIN materials m ON m.id = n.material_id
           LEFT JOIN scheme_topics t ON t.id = c.topic_id
          WHERE c.retired = 0 AND {filter}
            AND (?1 IS NULL OR m.subject_id = ?1 OR t.subject_id = ?1)
          ORDER BY {order}
          LIMIT ?2"
    );

    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map((subject_id, limit), |row| {
        let payload_json: String = row.get("payload_json")?;
        let Ok(payload) = serde_json::from_str(&payload_json) else {
            return Ok(None);
        };
        Ok(Some(SessionCard {
            is_new: row.get::<_, i64>("fsrs_state")? == 0,
            topic_title: row.get("topic_title")?,
            card: Card {
                id: row.get("id")?,
                note_id: row.get("note_id")?,
                topic_id: row.get("topic_id")?,
                payload,
                is_preview: row.get::<_, i64>("is_preview")? != 0,
                retired: row.get::<_, i64>("retired")? != 0,
                created_at: row.get("created_at")?,
            },
        }))
    })?;

    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// Read a card's current scheduling state.
fn memory_of(db: &Db, card_id: i64) -> Result<(Memory, Option<String>)> {
    let mut stmt = db.conn().prepare(
        "SELECT stability, difficulty, reps, lapses, state, last_review
           FROM card_state WHERE card_id = ?1",
    )?;
    let mut rows = stmt.query([card_id])?;
    match rows.next()? {
        Some(row) => Ok((
            Memory {
                stability: row.get(0)?,
                difficulty: row.get(1)?,
                reps: row.get(2)?,
                lapses: row.get(3)?,
                state: State::from_i64(row.get(4)?),
            },
            row.get(5)?,
        )),
        None => Err(CoreError::NotFound {
            kind: "card state",
            id: card_id,
        }),
    }
}

/// Days since a card's last review, as SQLite measures it.
fn elapsed_days(db: &Db, last_review: Option<&str>) -> Result<f64> {
    let Some(last) = last_review else {
        return Ok(0.0);
    };
    let days: f64 =
        db.conn()
            .query_row("SELECT (julianday('now') - julianday(?1))", [last], |r| {
                r.get(0)
            })?;
    Ok(days.max(0.0))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradeResult {
    pub card_id: i64,
    /// When this card comes back, ISO-8601.
    pub due: String,
    pub interval_days: f64,
    pub state: State,
}

/// Grade a card and write the new schedule immediately (Brief 3.5).
pub fn grade(
    db: &mut Db,
    session_id: Option<i64>,
    card_id: i64,
    rating: Rating,
    elapsed_ms: i64,
) -> Result<GradeResult> {
    let (memory, last_review) = memory_of(db, card_id)?;
    let elapsed = elapsed_days(db, last_review.as_deref())?;
    let scheduled = fsrs::schedule(memory, rating, elapsed);
    let modifier = scheduled.due_modifier();

    let tx = db.conn_mut().transaction()?;
    tx.execute(
        "UPDATE card_state
            SET stability = ?2, difficulty = ?3, reps = ?4, lapses = ?5, state = ?6,
                last_review = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                due = strftime('%Y-%m-%dT%H:%M:%fZ','now', ?7)
          WHERE card_id = ?1",
        (
            card_id,
            scheduled.memory.stability,
            scheduled.memory.difficulty,
            scheduled.memory.reps,
            scheduled.memory.lapses,
            scheduled.memory.state.as_i64(),
            &modifier,
        ),
    )?;
    tx.execute(
        "INSERT INTO review_log(card_id, rating, elapsed_ms) VALUES (?1, ?2, ?3)",
        (card_id, rating.as_i64(), elapsed_ms.max(0)),
    )?;
    if let Some(session_id) = session_id {
        tx.execute(
            "UPDATE sessions SET cards_done = cards_done + 1 WHERE id = ?1",
            [session_id],
        )?;
    }
    tx.commit()?;

    let due: String = db.conn().query_row(
        "SELECT due FROM card_state WHERE card_id = ?1",
        [card_id],
        |r| r.get(0),
    )?;

    Ok(GradeResult {
        card_id,
        due,
        interval_days: scheduled.interval_days,
        state: scheduled.memory.state,
    })
}

/// The calm end-of-session summary (Brief 3.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub session_id: i64,
    pub cards_done: i64,
    pub topics_touched: Vec<String>,
    /// Cards that will be waiting tomorrow.
    pub due_tomorrow: i64,
    pub minutes: i64,
}

pub fn end_session(db: &Db, session_id: i64) -> Result<SessionSummary> {
    db.conn().execute(
        "UPDATE sessions SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
          WHERE id = ?1 AND ended_at IS NULL",
        [session_id],
    )?;

    let (subject_id, cards_done, planned): (Option<i64>, i64, i64) = db.conn().query_row(
        "SELECT subject_id, cards_done, planned_minutes FROM sessions WHERE id = ?1",
        [session_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    let mut stmt = db.conn().prepare(
        "SELECT DISTINCT t.title
           FROM review_log r
           JOIN cards c ON c.id = r.card_id
           JOIN scheme_topics t ON t.id = c.topic_id
          WHERE r.reviewed_at >= (SELECT started_at FROM sessions WHERE id = ?1)
          ORDER BY t.title",
    )?;
    let topics = stmt
        .query_map([session_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let due_tomorrow: i64 = db.conn().query_row(
        "SELECT count(*)
           FROM card_state s
           JOIN cards c ON c.id = s.card_id
           LEFT JOIN notes n ON n.id = c.note_id
           LEFT JOIN materials m ON m.id = n.material_id
           LEFT JOIN scheme_topics t ON t.id = c.topic_id
          WHERE c.retired = 0
            AND s.due <= strftime('%Y-%m-%dT%H:%M:%fZ','now','+1 day')
            AND (?1 IS NULL OR m.subject_id = ?1 OR t.subject_id = ?1)",
        [subject_id],
        |r| r.get(0),
    )?;

    Ok(SessionSummary {
        session_id,
        cards_done,
        topics_touched: topics,
        due_tomorrow,
        minutes: planned,
    })
}

/// Per-topic mastery for the subject view (Brief 3.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicMastery {
    pub topic_id: i64,
    pub title: String,
    pub total_cards: i64,
    pub studied_cards: i64,
    /// 0.0–1.0, from average card stability.
    pub mastery: f32,
    pub due_now: i64,
}

/// Mastery per topic, as a simple fraction the UI draws as a bar.
///
/// Stability is measured against three weeks: a card holding that long has
/// genuinely stuck rather than merely been seen.
pub fn mastery_for_subject(db: &Db, subject_id: i64) -> Result<Vec<TopicMastery>> {
    let mut stmt = db.conn().prepare(
        "SELECT t.id, t.title,
                count(c.id) AS total,
                COALESCE(SUM(CASE WHEN s.state != 0 THEN 1 ELSE 0 END), 0) AS studied,
                COALESCE(AVG(MIN(s.stability / 21.0, 1.0)), 0.0) AS mastery,
                COALESCE(SUM(CASE WHEN s.due <= strftime('%Y-%m-%dT%H:%M:%fZ','now')
                                   AND s.state != 0
                              THEN 1 ELSE 0 END), 0) AS due_now
           FROM scheme_topics t
           LEFT JOIN cards c ON c.topic_id = t.id AND c.retired = 0
           LEFT JOIN card_state s ON s.card_id = c.id
          WHERE t.subject_id = ?1
          GROUP BY t.id, t.title
          ORDER BY t.parent_id IS NOT NULL, t.position, t.id",
    )?;

    let rows = stmt.query_map([subject_id], |row| {
        Ok(TopicMastery {
            topic_id: row.get(0)?,
            title: row.get(1)?,
            total_cards: row.get(2)?,
            studied_cards: row.get(3)?,
            mastery: row.get::<_, f64>(4)? as f32,
            due_now: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Notes most similar to a question, within one subject (Brief 3.8).
pub fn retrieve_notes(
    db: &Db,
    subject_id: i64,
    query_vector: &[f32],
    k: usize,
) -> Result<Vec<library::Note>> {
    let candidates = library::note_vectors_for_subject(db, subject_id)?;
    let matches = crate::embed::top_k(query_vector, &candidates, k);
    let ids: Vec<i64> = matches.iter().map(|m| m.id).collect();
    library::notes_by_ids(db, &ids)
}
