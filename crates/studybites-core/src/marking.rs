//! Marking a typed answer against the one the card was made from.
//!
//! The student writes the answer in their own words instead of revealing it
//! and grading themselves. That is better practice — producing an answer from
//! memory is a far stronger test than recognising one — but it costs an AI
//! request per card, and the study loop has been offline and free until now.
//!
//! So marking happens in two stages:
//!
//! 1. **Locally, free, instantly.** A blank answer, an answer that matches the
//!    expected one almost word for word, and an answer with nothing whatever in
//!    common with it are all decided here, with no network.
//! 2. **By the AI, only when it is genuinely a judgement call.** That is where
//!    the value is anyway: deciding whether a differently-worded answer means
//!    the same thing, and saying how an examiner would want it written.
//!
//! The thresholds are deliberately lopsided. Marking something correct when it
//! is not teaches the student the wrong thing, so the local "correct" rule is
//! strict and anything doubtful is sent on rather than guessed at.

use serde::{Deserialize, Serialize};

use crate::ai::extract_json;
use crate::embed;
use crate::error::{CoreError, Result};
use crate::fsrs::Rating;

/// How close the answer was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Right, whatever words were used.
    Correct,
    /// The idea is there but incomplete, or it would lose marks as written.
    Close,
    Wrong,
}

impl Verdict {
    fn from_score(score: u8) -> Self {
        match score {
            80..=100 => Verdict::Correct,
            40..=79 => Verdict::Close,
            _ => Verdict::Wrong,
        }
    }
}

/// The result shown to the student after they submit an answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub verdict: Verdict,
    /// Out of 100, read as "what this would score in an exam".
    pub score: u8,
    /// What an examiner would say. One or two sentences, never a lecture.
    pub feedback: String,
    /// How to write it so it would earn full marks. Empty when the answer was
    /// already well put, so a correct student is not nagged.
    pub model_answer: String,
    /// False when this was decided locally and cost nothing.
    pub from_ai: bool,
}

impl Mark {
    /// What to tell the scheduler, unless the student overrides it.
    ///
    /// Note what this can and cannot know. FSRS wants *how hard was that to
    /// recall*; marking knows *how right was it*. They are related but not the
    /// same, so `Easy` is never suggested — only the student can say an answer
    /// came effortlessly. Suggesting it from a high score would inflate every
    /// interval for anyone who writes well.
    pub fn suggested_rating(&self) -> Rating {
        match self.verdict {
            Verdict::Correct => Rating::Good,
            Verdict::Close => Rating::Hard,
            Verdict::Wrong => Rating::Again,
        }
    }
}

/// Collapse to comparable words, so punctuation and capitals do not matter.
fn key_words(text: &str) -> Vec<String> {
    embed::tokenize(text)
}

/// Decide without the network, or return `None` to mean "ask the AI".
///
/// `None` is the normal path for a real attempt. This exists to stop the
/// obvious cases — nothing typed, the answer copied back verbatim, pure
/// noise — from costing a request each.
pub fn quick_mark(typed: &str, expected: &str) -> Option<Mark> {
    let typed_trimmed = typed.trim();
    if typed_trimmed.is_empty() {
        return Some(Mark {
            verdict: Verdict::Wrong,
            score: 0,
            feedback: "Nothing was written, so there is nothing to mark.".into(),
            model_answer: expected.trim().to_string(),
            from_ai: false,
        });
    }

    let typed_words = key_words(typed_trimmed);
    let expected_words = key_words(expected);
    if typed_words.is_empty() || expected_words.is_empty() {
        return None;
    }

    // Word-for-word, ignoring case, spacing and punctuation.
    if typed_words == expected_words {
        return Some(Mark {
            verdict: Verdict::Correct,
            score: 100,
            feedback: "Exactly right.".into(),
            model_answer: String::new(),
            from_ai: false,
        });
    }

    let overlap = shared_fraction(&typed_words, &expected_words);

    // Nothing in common at all. A real attempt in different words still shares
    // *some* vocabulary with its subject, so this only catches noise — a
    // keyboard mash, or an answer to a different question entirely.
    if overlap == 0.0 && typed_words.len() >= 3 {
        return Some(Mark {
            verdict: Verdict::Wrong,
            score: 0,
            feedback: "That does not answer this question.".into(),
            model_answer: expected.trim().to_string(),
            from_ai: false,
        });
    }

    // Everything else is a judgement about meaning, which is the AI's job.
    None
}

/// Fraction of the expected answer's words that appear in the typed one.
///
/// Deliberately one-directional: a student who writes more than was asked has
/// still covered the point, while one who writes less has not.
fn shared_fraction(typed: &[String], expected: &[String]) -> f32 {
    let present = expected.iter().filter(|w| typed.contains(w)).count();
    present as f32 / expected.len() as f32
}

/// Parse what the model sent back.
pub fn parse_mark(raw: &str) -> Result<Mark> {
    let value = extract_json(raw)?;

    let score = value
        .get("score")
        .and_then(|s| s.as_i64())
        .ok_or_else(|| CoreError::Invalid("the marking reply had no score".into()))?
        .clamp(0, 100) as u8;

    let text = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    let feedback = text("feedback");
    if feedback.is_empty() {
        return Err(CoreError::Invalid(
            "the marking reply had no feedback".into(),
        ));
    }

    Ok(Mark {
        verdict: Verdict::from_score(score),
        score,
        // A correct, well-expressed answer gets no model answer, so the
        // student is shown a better wording only when there is one.
        model_answer: text("modelAnswer"),
        feedback,
        from_ai: true,
    })
}
