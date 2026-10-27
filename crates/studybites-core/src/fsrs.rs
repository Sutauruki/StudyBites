//! FSRS scheduling (Brief 2, "Spaced repetition").
//!
//! This is a direct implementation of the FSRS-6 scheduling equations rather
//! than the `fsrs` crate. That crate carries `ndarray` and `rayon` — 66 crates
//! in all — because it also *trains* the parameters from a review history.
//! StudyBites only ever schedules, using the published defaults, so the
//! training half would be weight and heat on a dual-core laptop for nothing.
//! The brief allows "a port of ts-fsrs logic"; this is that port.
//!
//! The model: a card has **stability** (days until recall probability falls to
//! `REQUEST_RETENTION`) and **difficulty** (1–10). Each review updates both,
//! and the next interval follows from stability.

use serde::{Deserialize, Serialize};

/// FSRS-6 default parameters, as published by the FSRS project.
pub const DEFAULT_W: [f64; 21] = [
    0.2172, 1.1771, 3.2602, 16.1507, 7.0114, 0.57, 2.0966, 0.0069, 1.5261, 0.112, 1.0178, 1.849,
    0.1133, 0.3127, 2.2934, 0.2191, 3.0004, 0.7536, 0.3332, 0.1437, 0.2,
];

/// Target probability of recall at review time.
pub const REQUEST_RETENTION: f64 = 0.9;
/// Never schedule further out than this.
pub const MAX_INTERVAL_DAYS: f64 = 365.0 * 2.0;
const DECAY: f64 = -0.1542;

/// How well the student recalled a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Rating {
    pub fn from_i64(value: i64) -> Option<Self> {
        match value {
            1 => Some(Self::Again),
            2 => Some(Self::Hard),
            3 => Some(Self::Good),
            4 => Some(Self::Easy),
            _ => None,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }

    fn as_f64(self) -> f64 {
        self as i64 as f64
    }
}

/// Where a card is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Never studied.
    New = 0,
    /// Being learned; short, sub-day steps.
    Learning = 1,
    /// Scheduled in days.
    Review = 2,
    /// Lapsed and being relearned.
    Relearning = 3,
}

impl State {
    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Learning,
            2 => Self::Review,
            3 => Self::Relearning,
            _ => Self::New,
        }
    }

    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

/// A card's scheduling state, mirroring the `card_state` row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub stability: f64,
    pub difficulty: f64,
    pub reps: i64,
    pub lapses: i64,
    pub state: State,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            stability: 0.0,
            difficulty: 0.0,
            reps: 0,
            lapses: 0,
            state: State::New,
        }
    }
}

/// The result of grading a card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scheduled {
    pub memory: Memory,
    /// Days until this card is due again. Sub-day values are learning steps.
    pub interval_days: f64,
}

impl Scheduled {
    /// The interval as a SQLite date modifier, e.g. "+3456 seconds".
    pub fn due_modifier(&self) -> String {
        let seconds = (self.interval_days * 86_400.0).round().max(60.0) as i64;
        format!("+{seconds} seconds")
    }
}

fn clamp_difficulty(d: f64) -> f64 {
    d.clamp(1.0, 10.0)
}

fn clamp_stability(s: f64) -> f64 {
    s.max(0.01)
}

/// Stability for a card seen for the first time.
fn initial_stability(w: &[f64; 21], rating: Rating) -> f64 {
    clamp_stability(w[(rating.as_i64() - 1) as usize])
}

/// Difficulty for a card seen for the first time.
fn initial_difficulty(w: &[f64; 21], rating: Rating) -> f64 {
    clamp_difficulty(w[4] - (w[5] * (rating.as_f64() - 1.0)).exp() + 1.0)
}

/// Probability the card is still recalled after `elapsed_days`.
pub fn retrievability(stability: f64, elapsed_days: f64) -> f64 {
    if stability <= 0.0 {
        return 0.0;
    }
    let factor = 0.9_f64.powf(1.0 / DECAY) - 1.0;
    (1.0 + factor * elapsed_days / stability).powf(DECAY)
}

fn next_difficulty(w: &[f64; 21], difficulty: f64, rating: Rating) -> f64 {
    let delta = -w[6] * (rating.as_f64() - 3.0);
    let damped = difficulty + delta * ((10.0 - difficulty) / 9.0);
    // Pull slowly back toward the "easy" anchor so difficulty cannot ratchet
    // upward forever.
    let anchor = initial_difficulty(w, Rating::Easy);
    clamp_difficulty(w[7] * anchor + (1.0 - w[7]) * damped)
}

fn stability_after_recall(
    w: &[f64; 21],
    difficulty: f64,
    stability: f64,
    retrievability: f64,
    rating: Rating,
) -> f64 {
    let hard_penalty = if rating == Rating::Hard { w[15] } else { 1.0 };
    let easy_bonus = if rating == Rating::Easy { w[16] } else { 1.0 };
    let growth = (w[8].exp())
        * (11.0 - difficulty)
        * stability.powf(-w[9])
        * ((1.0 - retrievability) * w[10]).exp_m1()
        * hard_penalty
        * easy_bonus;
    clamp_stability(stability * (1.0 + growth))
}

fn stability_after_lapse(
    w: &[f64; 21],
    difficulty: f64,
    stability: f64,
    retrievability: f64,
) -> f64 {
    let long_term = w[11]
        * difficulty.powf(-w[12])
        * ((stability + 1.0).powf(w[13]) - 1.0)
        * ((1.0 - retrievability) * w[14]).exp();
    // A lapse must never increase stability.
    clamp_stability(long_term.min(stability))
}

/// Interval, in days, that puts recall probability at `REQUEST_RETENTION`.
pub fn interval_for(stability: f64) -> f64 {
    let factor = 0.9_f64.powf(1.0 / DECAY) - 1.0;
    let days = (stability / factor) * (REQUEST_RETENTION.powf(1.0 / DECAY) - 1.0);
    days.clamp(1.0, MAX_INTERVAL_DAYS)
}

/// Grade a card and produce its next state and interval.
///
/// `elapsed_days` is the time since the previous review; it is ignored for a
/// card being seen for the first time.
pub fn schedule(memory: Memory, rating: Rating, elapsed_days: f64) -> Scheduled {
    schedule_with(&DEFAULT_W, memory, rating, elapsed_days)
}

pub fn schedule_with(
    w: &[f64; 21],
    memory: Memory,
    rating: Rating,
    elapsed_days: f64,
) -> Scheduled {
    let mut next = memory;
    next.reps += 1;

    match memory.state {
        State::New => {
            next.stability = initial_stability(w, rating);
            next.difficulty = initial_difficulty(w, rating);
            next.state = if rating == Rating::Again {
                State::Learning
            } else {
                State::Review
            };
        }
        State::Learning | State::Relearning => {
            next.difficulty = next_difficulty(w, memory.difficulty, rating);
            if rating == Rating::Again {
                // Still not sticking: keep it in the short loop.
                next.state = memory.state;
            } else {
                next.stability = clamp_stability(memory.stability);
                next.state = State::Review;
            }
        }
        State::Review => {
            let r = retrievability(memory.stability, elapsed_days.max(0.0));
            next.difficulty = next_difficulty(w, memory.difficulty, rating);
            if rating == Rating::Again {
                next.lapses += 1;
                next.stability = stability_after_lapse(w, memory.difficulty, memory.stability, r);
                next.state = State::Relearning;
            } else {
                next.stability =
                    stability_after_recall(w, memory.difficulty, memory.stability, r, rating);
                next.state = State::Review;
            }
        }
    }

    // Learning and relearning steps are minutes, not days, so a card the
    // student just failed comes back inside the same session.
    let interval_days = match next.state {
        State::Learning => 1.0 / 144.0,  // 10 minutes
        State::Relearning => 1.0 / 96.0, // 15 minutes
        State::New => 1.0,
        State::Review => interval_for(next.stability),
    };

    Scheduled {
        memory: next,
        interval_days,
    }
}
