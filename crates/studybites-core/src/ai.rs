//! The AI provider abstraction and the strict parsing of what models return
//! (Brief 6 and 7).
//!
//! Two rules shape this module:
//!
//! 1. **Distil once.** Raw material is sent to a model exactly once, at upload
//!    time. Study sessions never re-send it. So the structures here are what
//!    gets stored, and they must be worth storing.
//! 2. **Never trust the output.** Models wrap JSON in prose or code fences,
//!    invent fields, and return four options with two marked correct. Every
//!    response is parsed and validated in Rust; invalid output is retried once
//!    and then skipped with a log, never written to the database.

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};

/// A compact study note distilled from one chunk (Brief 3.4 step 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DistilledNote {
    /// A short title for the note, used in lists and in "Explain" citations.
    pub title: String,
    #[serde(default)]
    pub key_ideas: Vec<String>,
    #[serde(default)]
    pub definitions: Vec<Definition>,
    #[serde(default)]
    pub formulas: Vec<String>,
    #[serde(default)]
    pub worked_examples: Vec<String>,
    #[serde(default)]
    pub common_mistakes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub term: String,
    pub meaning: String,
}

impl DistilledNote {
    /// The text that gets embedded and shown to the model during "Explain".
    pub fn as_plain_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.title);
        for idea in &self.key_ideas {
            out.push_str("\n- ");
            out.push_str(idea);
        }
        for def in &self.definitions {
            out.push_str(&format!("\n{}: {}", def.term, def.meaning));
        }
        for formula in &self.formulas {
            out.push('\n');
            out.push_str(formula);
        }
        out
    }

    fn validate(mut self) -> Result<Self> {
        self.title = self.title.trim().to_string();
        if self.title.is_empty() {
            return Err(CoreError::Invalid("the note has no title".into()));
        }
        fn tidy(items: Vec<String>) -> Vec<String> {
            items
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
        self.key_ideas = tidy(self.key_ideas);
        self.formulas = tidy(self.formulas);
        self.worked_examples = tidy(self.worked_examples);
        self.common_mistakes = tidy(self.common_mistakes);
        self.definitions.retain(|d| !d.term.trim().is_empty());

        // A note with nothing in it is not worth a row, and would otherwise
        // produce cards with no content behind them.
        if self.key_ideas.is_empty() && self.definitions.is_empty() && self.formulas.is_empty() {
            return Err(CoreError::Invalid(
                "the note has no key ideas, definitions or formulas".into(),
            ));
        }
        Ok(self)
    }
}

/// One generated card, in the three shapes the study session understands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GeneratedCard {
    Flash {
        front: String,
        back: String,
    },
    Mcq {
        question: String,
        options: Vec<String>,
        /// Index into `options`, zero-based.
        answer: usize,
        #[serde(default)]
        explanation: String,
    },
    Short {
        prompt: String,
        answer: String,
    },
}

impl GeneratedCard {
    /// The `cards.kind` column value.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Flash { .. } => "flash",
            Self::Mcq { .. } => "mcq",
            Self::Short { .. } => "short",
        }
    }

    fn validate(self) -> Result<Self> {
        fn blank(s: &str) -> bool {
            s.trim().is_empty()
        }
        match &self {
            Self::Flash { front, back } => {
                if blank(front) || blank(back) {
                    return Err(CoreError::Invalid("flashcard has an empty side".into()));
                }
            }
            Self::Short { prompt, answer } => {
                if blank(prompt) || blank(answer) {
                    return Err(CoreError::Invalid(
                        "short-answer card is missing its prompt or answer".into(),
                    ));
                }
            }
            Self::Mcq {
                question,
                options,
                answer,
                ..
            } => {
                if blank(question) {
                    return Err(CoreError::Invalid("MCQ has no question".into()));
                }
                // The brief specifies four options with exactly one correct.
                if options.len() != 4 {
                    return Err(CoreError::Invalid(format!(
                        "MCQ needs exactly 4 options, got {}",
                        options.len()
                    )));
                }
                if options.iter().any(|o| blank(o)) {
                    return Err(CoreError::Invalid("MCQ has an empty option".into()));
                }
                if *answer >= options.len() {
                    return Err(CoreError::Invalid(format!(
                        "MCQ answer index {answer} is out of range"
                    )));
                }
                let mut seen: Vec<String> =
                    options.iter().map(|o| o.trim().to_lowercase()).collect();
                seen.sort();
                seen.dedup();
                if seen.len() != options.len() {
                    return Err(CoreError::Invalid("MCQ has duplicate options".into()));
                }
            }
        }
        Ok(self)
    }
}

/// Pull a JSON value out of a model response.
///
/// Models routinely wrap JSON in ```json fences or add a sentence before it,
/// even when told not to, so the braces are located rather than assumed.
pub fn extract_json(raw: &str) -> Result<serde_json::Value> {
    let text = raw.trim();

    let body = if let Some(rest) = text.strip_prefix("```") {
        // ```json\n{...}\n```  — drop the language tag and the closing fence.
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        let rest = rest.trim_start_matches(['\n', '\r']);
        rest.rsplit_once("```").map(|(b, _)| b).unwrap_or(rest)
    } else {
        text
    };
    let body = body.trim();

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        return Ok(value);
    }

    // Fall back to the outermost {...} or [...] span.
    let start = body
        .find(['{', '['])
        .ok_or_else(|| CoreError::Invalid("the model returned no JSON".into()))?;
    let open = body.as_bytes()[start];
    let close = if open == b'{' { '}' } else { ']' };
    let end = body
        .rfind(close)
        .ok_or_else(|| CoreError::Invalid("the model returned unbalanced JSON".into()))?;
    if end <= start {
        return Err(CoreError::Invalid(
            "the model returned unbalanced JSON".into(),
        ));
    }
    Ok(serde_json::from_str(&body[start..=end])?)
}

/// Parse and validate a distilled note from a raw model response.
pub fn parse_note(raw: &str) -> Result<DistilledNote> {
    let value = extract_json(raw)?;
    let note: DistilledNote = serde_json::from_value(value)?;
    note.validate()
}

/// Parse a batch of cards, dropping the invalid ones.
///
/// One malformed card should not cost the whole chunk's work, so this returns
/// what was usable alongside the reasons the rest were rejected. The caller
/// logs the reasons; nothing invalid reaches the database.
pub fn parse_cards(raw: &str) -> Result<(Vec<GeneratedCard>, Vec<String>)> {
    let value = extract_json(raw)?;
    // Accept either a bare array or {"cards": [...]}.
    let array = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(ref map) => match map.get("cards") {
            Some(serde_json::Value::Array(items)) => items.clone(),
            _ => return Err(CoreError::Invalid("expected a list of cards".into())),
        },
        _ => return Err(CoreError::Invalid("expected a list of cards".into())),
    };

    let mut cards = Vec::new();
    let mut rejected = Vec::new();
    for (index, item) in array.into_iter().enumerate() {
        match serde_json::from_value::<GeneratedCard>(item).map_err(CoreError::from) {
            Ok(card) => match card.validate() {
                Ok(card) => cards.push(card),
                Err(err) => rejected.push(format!("card {index}: {err}")),
            },
            Err(err) => rejected.push(format!("card {index}: {err}")),
        }
    }
    Ok((cards, rejected))
}

/// Which provider a request should go to, in the user's priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Gemini,
    Groq,
    OpenRouter,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Groq => "groq",
            Self::OpenRouter => "openrouter",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "gemini" => Some(Self::Gemini),
            "groq" => Some(Self::Groq),
            "openrouter" => Some(Self::OpenRouter),
            _ => None,
        }
    }

    /// Default priority: Gemini first, per the brief.
    pub fn default_order() -> Vec<Self> {
        vec![Self::Gemini, Self::Groq, Self::OpenRouter]
    }

    /// Where the user gets a free key, shown in Settings.
    pub fn key_help_url(self) -> &'static str {
        match self {
            Self::Gemini => "https://aistudio.google.com/apikey",
            Self::Groq => "https://console.groq.com/keys",
            Self::OpenRouter => "https://openrouter.ai/keys",
        }
    }
}

/// What went wrong with an AI call, which decides whether the router falls
/// through to the next provider, backs off, or gives up.
#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    /// Out of quota until the given RFC3339 time, if the provider said when.
    QuotaExhausted { until: Option<String> },
    /// Transient: network trouble, a 5xx, a timeout. Worth retrying.
    Transient(String),
    /// The key is missing, wrong, or rejected. Retrying will not help.
    Auth(String),
    /// The model replied, but not with usable JSON.
    BadResponse(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QuotaExhausted { until: Some(t) } => {
                write!(f, "daily limit reached, resumes after {t}")
            }
            Self::QuotaExhausted { until: None } => write!(f, "daily limit reached"),
            Self::Transient(m) => write!(f, "temporary problem: {m}"),
            Self::Auth(m) => write!(f, "API key problem: {m}"),
            Self::BadResponse(m) => write!(f, "unusable response: {m}"),
        }
    }
}

impl AiError {
    /// Whether trying the same provider again could succeed.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient(_) | Self::BadResponse(_))
    }

    /// Whether the router should move on to the next provider.
    pub fn should_fall_through(&self) -> bool {
        matches!(
            self,
            Self::QuotaExhausted { .. } | Self::Auth(_) | Self::Transient(_)
        )
    }
}

/// A request to distil one chunk into a note.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistillRequest {
    pub subject: String,
    pub topic: Option<String>,
    pub heading: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicMatch {
    /// Index into the topic list that was offered, if the model picked one.
    pub index: Option<usize>,
    /// 0.0–1.0. Low confidence is what triggers the web-search fallback later.
    pub confidence: f32,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemeEntry {
    pub title: String,
    pub week: Option<i64>,
    #[serde(default)]
    pub subtopics: Vec<String>,
}

/// The operations a provider must support (Brief 7.1). Implementations are
/// added in the next slice; the trait is here so the router, the job workers
/// and the tests can be written against it.
pub trait AiProvider {
    fn id(&self) -> ProviderId;

    fn distill(&self, request: &DistillRequest) -> std::result::Result<DistilledNote, AiError>;

    fn generate_cards(
        &self,
        note: &DistilledNote,
    ) -> std::result::Result<Vec<GeneratedCard>, AiError>;

    /// Pick the scheme topic a piece of material belongs to, returning the
    /// index into `topics` and the model's confidence (Brief 3.3).
    fn match_topic(
        &self,
        text: &str,
        topics: &[String],
    ) -> std::result::Result<TopicMatch, AiError>;

    /// Turn a scheme-of-work document into an ordered outline (Brief 3.2).
    fn parse_scheme(&self, text: &str) -> std::result::Result<Vec<SchemeEntry>, AiError>;

    /// A short explanation grounded in the supplied notes (Brief 3.8).
    fn explain(
        &self,
        question: &str,
        notes: &[DistilledNote],
    ) -> std::result::Result<String, AiError>;
}
