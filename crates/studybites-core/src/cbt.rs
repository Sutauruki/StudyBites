//! CBT practice tests (Phase 5).
//!
//! A timed, multiple-choice mock in the shape Nigerian students already
//! practise on: pick your subjects, get a fixed number of questions, a clock
//! running down, a numbered grid to jump around, and a score at the end.
//!
//! Three decisions shape this module:
//!
//! * **A test is not a review.** Nothing here touches `card_state` or
//!   `review_log`. Letting a 40-question mock drive FSRS would quietly
//!   reschedule half the deck on the strength of guesses made against a clock.
//!   Practice is recorded in its own tables and leaves the schedule alone.
//!
//! * **The answers do not go to the WebView until the test is submitted.**
//!   An exam whose answer key is already sitting in the page is not an exam.
//!   `start` returns questions only; `submit` returns the marking.
//!
//! * **Questions come from cards the student already has**, so a test costs
//!   nothing and works offline. AI preview cards are excluded: they are
//!   labelled "not from your class" precisely because they might not be.

use serde::{Deserialize, Serialize};

use crate::ai::GeneratedCard;
use crate::error::{CoreError, Result};
use crate::Db;

/// Smallest test worth sitting, and a ceiling that keeps one session sane.
pub const MIN_QUESTIONS: i64 = 5;
pub const MAX_QUESTIONS: i64 = 100;

/// One question as the student sees it: no answer, no explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub card_id: i64,
    pub position: i64,
    pub question: String,
    pub options: Vec<String>,
    pub subject_name: Option<String>,
    pub topic_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Test {
    pub id: i64,
    pub minutes: i64,
    pub questions: Vec<Question>,
}

/// How one question went, revealed only after submitting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Marked {
    pub position: i64,
    pub question: String,
    pub options: Vec<String>,
    pub correct: usize,
    /// `None` means it was left blank.
    pub chosen: Option<usize>,
    pub explanation: String,
    pub topic_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub test_id: i64,
    pub score: i64,
    pub total: i64,
    /// Whole percent, rounded down — the number students actually quote.
    pub percent: i64,
    pub answers: Vec<Marked>,
}

/// A past attempt, for the history list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSummary {
    pub id: i64,
    pub started_at: String,
    pub score: Option<i64>,
    pub total: i64,
    pub minutes: i64,
    pub subject_names: Vec<String>,
}

/// How many questions the chosen subjects can actually supply.
///
/// The setup screen asks first, so a student is offered a length that exists
/// rather than being refused after choosing one.
pub fn available(db: &Db, subject_ids: &[i64]) -> Result<i64> {
    if subject_ids.is_empty() {
        return Ok(0);
    }
    Ok(multiple_choice_cards(db, subject_ids)?.len() as i64)
}

/// Build a test from the student's own multiple-choice cards.
pub fn start(db: &Db, subject_ids: &[i64], count: i64, minutes: i64) -> Result<Test> {
    if subject_ids.is_empty() {
        return Err(CoreError::Invalid("choose at least one subject".into()));
    }
    if !(MIN_QUESTIONS..=MAX_QUESTIONS).contains(&count) {
        return Err(CoreError::Invalid(format!(
            "a test must have between {MIN_QUESTIONS} and {MAX_QUESTIONS} questions"
        )));
    }
    if !(1..=300).contains(&minutes) {
        return Err(CoreError::Invalid(
            "a test must run between 1 and 300 minutes".into(),
        ));
    }

    let candidates = multiple_choice_cards(db, subject_ids)?;
    if (candidates.len() as i64) < MIN_QUESTIONS {
        return Err(CoreError::Invalid(format!(
            "there are only {} multiple-choice questions in the subjects you picked. Upload more \
             material, or pick another subject.",
            candidates.len()
        )));
    }

    let mut picked = candidates;
    shuffle(&mut picked);
    picked.truncate(count as usize);

    db.conn().execute(
        "INSERT INTO cbt_tests(minutes, subject_ids, total) VALUES (?1, ?2, ?3)",
        (
            minutes,
            serde_json::to_string(subject_ids)?,
            picked.len() as i64,
        ),
    )?;
    let id = db.conn().last_insert_rowid();

    // Rows are written now, so a test survives the app being closed mid-way
    // and cannot be re-rolled by restarting until the questions look easier.
    let questions = picked
        .into_iter()
        .enumerate()
        .map(|(i, card)| {
            let position = i as i64;
            db.conn().execute(
                "INSERT INTO cbt_answers(test_id, card_id, position, chosen, correct)
                 VALUES (?1, ?2, ?3, NULL, 0)",
                (id, card.card_id, position),
            )?;
            Ok(Question {
                card_id: card.card_id,
                position,
                question: card.question,
                options: card.options,
                subject_name: card.subject_name,
                topic_title: card.topic_title,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Test {
        id,
        minutes,
        questions,
    })
}

/// Mark a submitted test. `chosen` is one entry per question, by position.
pub fn submit(db: &mut Db, test_id: i64, chosen: &[Option<usize>]) -> Result<TestResult> {
    let total: i64 = db
        .conn()
        .query_row(
            "SELECT total FROM cbt_tests WHERE id = ?1",
            [test_id],
            |r| r.get(0),
        )
        .map_err(|_| CoreError::NotFound {
            kind: "test",
            id: test_id,
        })?;

    // Read everything first and let the statement drop, so the transaction
    // below can take the connection mutably.
    let loaded = {
        let mut stmt = db.conn().prepare(
            "SELECT a.position, a.card_id, c.payload_json, t.title AS topic_title
               FROM cbt_answers a
               LEFT JOIN cards c ON c.id = a.card_id
               LEFT JOIN scheme_topics t ON t.id = c.topic_id
              WHERE a.test_id = ?1
              ORDER BY a.position",
        )?;
        let rows = stmt
            .query_map([test_id], |row| {
                Ok((
                    row.get::<_, i64>("position")?,
                    row.get::<_, Option<String>>("payload_json")?,
                    row.get::<_, Option<String>>("topic_title")?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let tx = db.conn_mut().transaction()?;
    let mut answers = Vec::with_capacity(loaded.len());
    let mut score = 0;

    for (position, payload_json, topic_title) in loaded {
        // A card deleted between starting and submitting is skipped rather
        // than failing the whole test.
        let Some(payload) = payload_json else {
            continue;
        };
        let Ok(GeneratedCard::Mcq {
            question,
            options,
            answer,
            explanation,
        }) = serde_json::from_str::<GeneratedCard>(&payload)
        else {
            continue;
        };

        let picked = chosen.get(position as usize).copied().flatten();
        let right = picked == Some(answer);
        if right {
            score += 1;
        }

        tx.execute(
            "UPDATE cbt_answers SET chosen = ?1, correct = ?2
              WHERE test_id = ?3 AND position = ?4",
            (
                picked.map(|c| c as i64),
                i64::from(right),
                test_id,
                position,
            ),
        )?;

        answers.push(Marked {
            position,
            question,
            options,
            correct: answer,
            chosen: picked,
            explanation,
            topic_title,
        });
    }

    tx.execute(
        "UPDATE cbt_tests
            SET score = ?1, ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
          WHERE id = ?2",
        (score, test_id),
    )?;
    tx.commit()?;

    Ok(TestResult {
        test_id,
        score,
        total,
        percent: if total > 0 { score * 100 / total } else { 0 },
        answers,
    })
}

/// Past attempts, newest first.
pub fn history(db: &Db, limit: i64) -> Result<Vec<TestSummary>> {
    let mut stmt = db.conn().prepare(
        "SELECT id, started_at, score, total, minutes, subject_ids
           FROM cbt_tests
          WHERE ended_at IS NOT NULL
          ORDER BY started_at DESC
          LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit.clamp(1, 100)], |row| {
            Ok((
                row.get::<_, i64>("id")?,
                row.get::<_, String>("started_at")?,
                row.get::<_, Option<i64>>("score")?,
                row.get::<_, i64>("total")?,
                row.get::<_, i64>("minutes")?,
                row.get::<_, String>("subject_ids")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    rows.into_iter()
        .map(|(id, started_at, score, total, minutes, ids_json)| {
            let ids: Vec<i64> = serde_json::from_str(&ids_json).unwrap_or_default();
            Ok(TestSummary {
                id,
                started_at,
                score,
                total,
                minutes,
                subject_names: subject_names(db, &ids)?,
            })
        })
        .collect()
}

fn subject_names(db: &Db, ids: &[i64]) -> Result<Vec<String>> {
    ids.iter()
        .filter_map(|id| {
            db.conn()
                .query_row("SELECT name FROM subjects WHERE id = ?1", [id], |r| {
                    r.get::<_, String>(0)
                })
                .ok()
        })
        .map(Ok)
        .collect()
}

struct Candidate {
    card_id: i64,
    question: String,
    options: Vec<String>,
    subject_name: Option<String>,
    topic_title: Option<String>,
}

/// Every usable multiple-choice card in the chosen subjects.
fn multiple_choice_cards(db: &Db, subject_ids: &[i64]) -> Result<Vec<Candidate>> {
    // Subject ids come from our own database, but they arrive here by way of
    // the WebView, so they are bound as parameters rather than interpolated.
    let placeholders = (1..=subject_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");

    let sql = format!(
        "SELECT c.id, c.payload_json, t.title AS topic_title, sub.name AS subject_name
           FROM cards c
           LEFT JOIN notes n ON n.id = c.note_id
           LEFT JOIN materials m ON m.id = n.material_id
           LEFT JOIN scheme_topics t ON t.id = c.topic_id
           LEFT JOIN subjects sub ON sub.id = COALESCE(m.subject_id, t.subject_id)
          WHERE c.kind = 'mcq'
            AND c.retired = 0
            AND c.is_preview = 0
            AND COALESCE(m.subject_id, t.subject_id) IN ({placeholders})"
    );

    let mut stmt = db.conn().prepare(&sql)?;
    let params = rusqlite::params_from_iter(subject_ids.iter());
    let rows = stmt
        .query_map(params, |row| {
            Ok((
                row.get::<_, i64>("id")?,
                row.get::<_, String>("payload_json")?,
                row.get::<_, Option<String>>("topic_title")?,
                row.get::<_, Option<String>>("subject_name")?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(rows
        .into_iter()
        .filter_map(|(id, payload, topic_title, subject_name)| {
            match serde_json::from_str::<GeneratedCard>(&payload) {
                Ok(GeneratedCard::Mcq {
                    question, options, ..
                }) => Some(Candidate {
                    card_id: id,
                    question,
                    options,
                    subject_name,
                    topic_title,
                }),
                _ => None,
            }
        })
        .collect())
}

/// Fisher-Yates, so every card has an equal chance of appearing and the same
/// subjects do not arrive in blocks.
fn shuffle<T>(items: &mut [T]) {
    if items.len() < 2 {
        return;
    }
    let mut seed = [0u8; 8];
    if getrandom::getrandom(&mut seed).is_err() {
        // Without randomness the order is simply left alone; a predictable
        // test is better than no test.
        return;
    }
    let mut state = u64::from_le_bytes(seed) | 1;

    for i in (1..items.len()).rev() {
        // xorshift64*: small, no dependency, and far better than it needs to
        // be for ordering questions.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let j = (state.wrapping_mul(0x2545_F491_4F6C_DD1D) % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}
