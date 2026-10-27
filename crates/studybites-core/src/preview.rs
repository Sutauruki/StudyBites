//! "Prepare for next class" previews and exam mode (Brief 3.7, 3.9).
//!
//! Previews are the one place StudyBites shows the student something that did
//! not come from their own class, so three rules hold throughout:
//!
//! 1. **Offered, never forced.** Nothing is generated until asked for.
//! 2. **Always labelled.** Every note and card is stored with `is_preview`,
//!    and the UI badges them wherever they appear.
//! 3. **Never outranks real material.** Preview cards sort last in sessions,
//!    and are retired the moment real notes for that topic arrive.

use crate::ai::TopicPreview;
use crate::db::Db;
use crate::embed::Embedder;
use crate::error::Result;
use crate::library;
use crate::models::Topic;
use serde::{Deserialize, Serialize};

/// Only offer a preview when the exam is not imminent and the student has
/// actually covered something — a preview of topic 1 before any class is noise.
pub const MIN_TOPICS_COVERED: i64 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewOffer {
    /// The scheme topic this preview is for, when the outline named one.
    pub topic_id: Option<i64>,
    pub title: String,
    /// Shown to the student, e.g. "next in your scheme of work".
    pub reason: String,
    /// True when the outline gave the answer, so no web search is needed.
    pub from_scheme: bool,
    /// Already has a preview waiting, so there is nothing to generate.
    pub already_prepared: bool,
}

/// Topics the student has genuinely covered, most recent first.
fn covered_titles(db: &Db, subject_id: i64, limit: i64) -> Result<Vec<String>> {
    let mut stmt = db.conn().prepare(
        "SELECT DISTINCT t.title
           FROM scheme_topics t
           JOIN notes n ON n.topic_id = t.id AND n.is_preview = 0
          WHERE t.subject_id = ?1
          ORDER BY n.id DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map((subject_id, limit), |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Does this topic already hold real (non-preview) material?
fn is_covered(db: &Db, topic_id: i64) -> Result<bool> {
    let count: i64 = db.conn().query_row(
        "SELECT count(*) FROM notes WHERE topic_id = ?1 AND is_preview = 0",
        [topic_id],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

fn has_preview(db: &Db, topic_id: i64) -> Result<bool> {
    let count: i64 = db.conn().query_row(
        "SELECT count(*) FROM notes WHERE topic_id = ?1 AND is_preview = 1",
        [topic_id],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// Work out what is probably next (Brief 3.7).
///
/// With a scheme of work this is just the next uncovered topic in order — no
/// AI call and no web search. Without one, the caller asks the model instead.
pub fn next_topic(db: &Db, subject_id: i64) -> Result<Option<PreviewOffer>> {
    let covered = covered_titles(db, subject_id, 50)?;
    if (covered.len() as i64) < MIN_TOPICS_COVERED {
        return Ok(None);
    }

    let outline: Vec<Topic> = crate::topics::list_flat(db, subject_id)?
        .into_iter()
        // Off-scheme and preview topics are not part of the teaching order.
        .filter(|t| !t.is_off_scheme && !t.is_preview)
        .collect();

    for topic in outline {
        if is_covered(db, topic.id)? {
            continue;
        }
        return Ok(Some(PreviewOffer {
            already_prepared: has_preview(db, topic.id)?,
            topic_id: Some(topic.id),
            title: topic.title,
            reason: "next in your scheme of work".into(),
            from_scheme: true,
        }));
    }

    // No outline, or every topic covered: the model has to infer it, and the
    // caller decides whether that is worth a request.
    Ok(None)
}

/// Recent topics, for the prompt that infers what comes next.
pub fn recent_topics(db: &Db, subject_id: i64) -> Result<Vec<String>> {
    covered_titles(db, subject_id, 5)
}

/// Store a generated preview as a labelled note plus its cards.
///
/// Returns how many cards were added. Everything written here carries
/// `is_preview = 1`, which is what the badge and the scheduling order read.
pub fn save_preview(
    db: &mut Db,
    topic_id: Option<i64>,
    preview: &TopicPreview,
    embedder: &dyn Embedder,
) -> Result<usize> {
    let note = preview.as_note();
    let vector = embedder.embed(&note.as_plain_text());
    let stored = library::note_create(db, None, topic_id, &note, &vector, true)?;
    let ids = library::cards_create(db, stored.id, topic_id, &preview.cards, true)?;
    Ok(ids.len())
}

/// Retire the previews for a topic once real material lands on it (Brief 3.7).
///
/// Cards are retired rather than deleted, so a review log entry never points
/// at a row that has vanished. The note is kept too — it is cheap, and it
/// records what the preview claimed if the student wants to compare.
pub fn retire_previews_for_topic(db: &Db, topic_id: i64) -> Result<usize> {
    let retired = db.conn().execute(
        "UPDATE cards SET retired = 1
          WHERE is_preview = 1 AND retired = 0 AND topic_id = ?1",
        [topic_id],
    )?;
    Ok(retired)
}

/// How close the exam is, if one is set (Brief 3.9).
pub fn days_until_exam(db: &Db, subject_id: i64) -> Result<Option<f64>> {
    let exam: Option<String> = db.conn().query_row(
        "SELECT exam_date FROM subjects WHERE id = ?1",
        [subject_id],
        |r| r.get(0),
    )?;
    let Some(exam) = exam else { return Ok(None) };
    let days: f64 =
        db.conn()
            .query_row("SELECT julianday(?1) - julianday('now')", [&exam], |r| {
                r.get(0)
            })?;
    Ok(Some(days))
}

/// Inside this many days the session mix shifts toward weak topics.
pub const EXAM_WINDOW_DAYS: f64 = 21.0;

/// Whether exam mode applies right now.
pub fn exam_mode(db: &Db, subject_id: Option<i64>) -> Result<bool> {
    let Some(subject_id) = subject_id else {
        return Ok(false);
    };
    Ok(match days_until_exam(db, subject_id)? {
        // Past the exam date it stops mattering.
        Some(days) => (0.0..=EXAM_WINDOW_DAYS).contains(&days),
        None => false,
    })
}
