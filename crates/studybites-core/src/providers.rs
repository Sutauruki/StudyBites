//! Provider implementations and the fallback router (Brief 7).
//!
//! All three providers speak JSON over HTTPS and differ mainly in URL shape
//! and where the key goes, so one `chat()` per provider is enough; everything
//! above it is shared.
//!
//! HTTP goes through the `Http` trait rather than calling `ureq` directly, so
//! the router, the retry logic and the error mapping are all testable without
//! a network or a key.

use crate::ai::{
    AiError, AiProvider, DistillRequest, DistilledNote, GeneratedCard, ProviderId, SchemeEntry,
    TopicMatch,
};
use crate::{ai, prompts};
use std::time::Duration;

/// Requests are given a generous ceiling: a slow connection is normal, and a
/// retry costs a request against the daily allowance.
const TIMEOUT: Duration = Duration::from_secs(90);

/// One HTTP exchange: a POST with JSON in and text out.
pub trait Http: Send + Sync {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
    ) -> std::result::Result<String, HttpError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum HttpError {
    /// A non-2xx response.
    Status { code: u16, body: String },
    /// Never reached the server, or the connection died mid-flight.
    Transport(String),
}

/// The real client, backed by `ureq` with rustls.
#[derive(Debug, Default, Clone)]
pub struct UreqHttp;

impl Http for UreqHttp {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
    ) -> std::result::Result<String, HttpError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();

        let mut request = agent.post(url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }

        match request.send_json(body) {
            Ok(mut response) => response
                .body_mut()
                .read_to_string()
                .map_err(|e| HttpError::Transport(e.to_string())),
            Err(ureq::Error::StatusCode(code)) => Err(HttpError::Status {
                code,
                body: String::new(),
            }),
            Err(e) => Err(HttpError::Transport(e.to_string())),
        }
    }
}

/// Like `UreqHttp`, but a null body means GET instead of POST.
///
/// The releases API used by the update check is a GET; keeping it here means
/// the Tauri crate needs no HTTP dependency of its own.
#[derive(Debug, Default, Clone)]
pub struct UreqGetHttp;

impl Http for UreqGetHttp {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &serde_json::Value,
    ) -> std::result::Result<String, HttpError> {
        if !body.is_null() {
            return UreqHttp.post_json(url, headers, body);
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .build()
            .into();
        let mut request = agent.get(url);
        for (name, value) in headers {
            request = request.header(name.as_str(), value.as_str());
        }
        match request.call() {
            Ok(mut response) => response
                .body_mut()
                .read_to_string()
                .map_err(|e| HttpError::Transport(e.to_string())),
            Err(ureq::Error::StatusCode(code)) => Err(HttpError::Status {
                code,
                body: String::new(),
            }),
            Err(e) => Err(HttpError::Transport(e.to_string())),
        }
    }
}

/// Turn an HTTP failure into the error kind the router acts on.
///
/// The distinction matters: 429 parks the provider until its quota resets,
/// 401 means the key is wrong and retrying is pointless, and a 5xx is worth
/// another attempt.
pub fn classify(error: HttpError) -> AiError {
    match error {
        HttpError::Transport(message) => AiError::Transient(message),
        HttpError::Status { code, body } => match code {
            429 => AiError::QuotaExhausted {
                until: retry_after_from_body(&body),
            },
            401 | 403 => AiError::Auth(format!("the provider rejected the key ({code})")),
            400 if body.contains("API key") || body.contains("api_key") => {
                AiError::Auth("the provider rejected the key (400)".into())
            }
            500..=599 => AiError::Transient(format!("the provider is having trouble ({code})")),
            other => AiError::Transient(format!("unexpected response {other}")),
        },
    }
}

/// Providers rarely give a machine-readable reset time, so this is best
/// effort; `None` leaves the caller to decide when to try again.
fn retry_after_from_body(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .pointer("/error/details/0/retryDelay")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Shared machinery: build the request, send it, pull out the text.
struct Chat<'a> {
    http: &'a dyn Http,
    api_key: &'a str,
    model: &'a str,
}

impl Chat<'_> {
    /// OpenAI-compatible chat completions, used by Groq and OpenRouter.
    fn openai_style(
        &self,
        url: &str,
        system: &str,
        user: &str,
    ) -> std::result::Result<String, AiError> {
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0.2,
            "response_format": {"type": "json_object"},
        });
        let headers = vec![
            (
                "Authorization".to_string(),
                format!("Bearer {}", self.api_key),
            ),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];

        let raw = self
            .http
            .post_json(url, &headers, &body)
            .map_err(classify)?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| AiError::BadResponse(format!("response was not JSON: {e}")))?;

        parsed
            .pointer("/choices/0/message/content")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AiError::BadResponse("response had no message content".into()))
    }

    /// Gemini's generateContent, which takes the key in a header and wraps the
    /// system prompt in its own field.
    fn gemini(&self, system: &str, user: &str) -> std::result::Result<String, AiError> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );
        let body = serde_json::json!({
            "systemInstruction": {"parts": [{"text": system}]},
            "contents": [{"role": "user", "parts": [{"text": user}]}],
            "generationConfig": {
                "temperature": 0.2,
                "responseMimeType": "application/json",
            },
        });
        let headers = vec![
            ("x-goog-api-key".to_string(), self.api_key.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];

        let raw = self
            .http
            .post_json(&url, &headers, &body)
            .map_err(classify)?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| AiError::BadResponse(format!("response was not JSON: {e}")))?;

        // A safety block returns a candidate with no parts rather than an
        // error, so say something useful instead of "no content".
        if let Some(reason) = parsed
            .pointer("/candidates/0/finishReason")
            .and_then(|v| v.as_str())
        {
            if reason == "SAFETY" || reason == "PROHIBITED_CONTENT" {
                return Err(AiError::BadResponse(
                    "the provider declined to answer for this material".into(),
                ));
            }
        }

        parsed
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| AiError::BadResponse("response had no text".into()))
    }
}

/// A configured provider: which service, which model, whose key.
pub struct Provider<'a> {
    pub id: ProviderId,
    pub api_key: String,
    pub model: String,
    pub http: &'a dyn Http,
}

impl<'a> Provider<'a> {
    /// Free-tier models that are good enough for distilling and card writing.
    pub fn default_model(id: ProviderId) -> &'static str {
        match id {
            ProviderId::Gemini => "gemini-2.0-flash",
            ProviderId::Groq => "llama-3.3-70b-versatile",
            ProviderId::OpenRouter => "meta-llama/llama-3.3-70b-instruct:free",
        }
    }

    pub fn new(id: ProviderId, api_key: String, http: &'a dyn Http) -> Self {
        Self {
            id,
            api_key,
            model: Self::default_model(id).to_string(),
            http,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let model = model.into();
        if !model.trim().is_empty() {
            self.model = model;
        }
        self
    }

    fn chat(&self, system: &str, user: &str) -> std::result::Result<String, AiError> {
        if self.api_key.trim().is_empty() {
            return Err(AiError::Auth("no API key has been set".into()));
        }
        let chat = Chat {
            http: self.http,
            api_key: &self.api_key,
            model: &self.model,
        };
        match self.id {
            ProviderId::Gemini => chat.gemini(system, user),
            ProviderId::Groq => chat.openai_style(
                "https://api.groq.com/openai/v1/chat/completions",
                system,
                user,
            ),
            ProviderId::OpenRouter => chat.openai_style(
                "https://openrouter.ai/api/v1/chat/completions",
                system,
                user,
            ),
        }
    }
}

impl AiProvider for Provider<'_> {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn distill(&self, request: &DistillRequest) -> std::result::Result<DistilledNote, AiError> {
        let user = prompts::distill_user(
            &request.subject,
            request.topic.as_deref(),
            request.heading.as_deref(),
            &request.text,
        );
        let raw = self.chat(prompts::DISTILL_SYSTEM, &user)?;
        ai::parse_note(&raw).map_err(|e| AiError::BadResponse(e.to_string()))
    }

    fn generate_cards(
        &self,
        note: &DistilledNote,
    ) -> std::result::Result<Vec<GeneratedCard>, AiError> {
        let raw = self.chat(prompts::CARDS_SYSTEM, &prompts::cards_user(note))?;
        let (cards, _rejected) =
            ai::parse_cards(&raw).map_err(|e| AiError::BadResponse(e.to_string()))?;
        if cards.is_empty() {
            return Err(AiError::BadResponse(
                "no usable cards came back for this note".into(),
            ));
        }
        Ok(cards)
    }

    fn match_topic(
        &self,
        text: &str,
        topics: &[String],
    ) -> std::result::Result<TopicMatch, AiError> {
        if topics.is_empty() {
            return Ok(TopicMatch {
                index: None,
                confidence: 0.0,
                reason: "there is no outline to match against".into(),
            });
        }
        let raw = self.chat(
            prompts::TOPIC_MATCH_SYSTEM,
            &prompts::topic_match_user(text, topics),
        )?;
        let value = ai::extract_json(&raw).map_err(|e| AiError::BadResponse(e.to_string()))?;
        let mut matched: TopicMatch = serde_json::from_value(value)
            .map_err(|e| AiError::BadResponse(format!("unexpected shape: {e}")))?;

        // A model that points past the end of the list is treated as unsure,
        // not as a crash.
        if matched.index.is_some_and(|i| i >= topics.len()) {
            matched.index = None;
            matched.confidence = 0.0;
        }
        matched.confidence = matched.confidence.clamp(0.0, 1.0);
        Ok(matched)
    }

    fn parse_scheme(&self, text: &str) -> std::result::Result<Vec<SchemeEntry>, AiError> {
        let raw = self.chat(prompts::SCHEME_SYSTEM, &prompts::scheme_user(text))?;
        let value = ai::extract_json(&raw).map_err(|e| AiError::BadResponse(e.to_string()))?;
        let array = match value {
            serde_json::Value::Array(items) => items,
            serde_json::Value::Object(ref map) => map
                .get("topics")
                .and_then(|v| v.as_array())
                .cloned()
                .ok_or_else(|| AiError::BadResponse("expected a list of topics".into()))?,
            _ => return Err(AiError::BadResponse("expected a list of topics".into())),
        };

        let entries: Vec<SchemeEntry> = array
            .into_iter()
            .filter_map(|item| serde_json::from_value::<SchemeEntry>(item).ok())
            .filter(|e| !e.title.trim().is_empty())
            .collect();

        if entries.is_empty() {
            return Err(AiError::BadResponse(
                "no topics could be read from that document".into(),
            ));
        }
        Ok(entries)
    }

    fn explain(
        &self,
        question: &str,
        notes: &[DistilledNote],
    ) -> std::result::Result<String, AiError> {
        let raw = self.chat(
            prompts::EXPLAIN_SYSTEM,
            &prompts::explain_user(question, notes),
        )?;
        let text = raw.trim();
        if text.is_empty() {
            return Err(AiError::BadResponse(
                "the explanation came back empty".into(),
            ));
        }
        Ok(text.to_string())
    }
}

/// One failed try, so the caller can record quota and report what happened.
#[derive(Debug, Clone, PartialEq)]
pub struct Attempt {
    pub provider: ProviderId,
    pub error: AiError,
}

/// Run `call` against each provider in turn until one succeeds (Brief 7.1).
///
/// `available` lets the caller skip providers already known to be out of
/// quota, so an exhausted one does not cost a request to rediscover.
pub fn route<T>(
    providers: &[Provider<'_>],
    available: &dyn Fn(ProviderId) -> bool,
    call: impl Fn(&Provider<'_>) -> std::result::Result<T, AiError>,
) -> std::result::Result<(ProviderId, T), Vec<Attempt>> {
    let mut attempts = Vec::new();

    for provider in providers {
        if !available(provider.id) {
            attempts.push(Attempt {
                provider: provider.id,
                error: AiError::QuotaExhausted { until: None },
            });
            continue;
        }

        match call(provider) {
            Ok(value) => return Ok((provider.id, value)),
            Err(error) => {
                let fall_through = error.should_fall_through();
                let retryable = error.is_retryable();
                attempts.push(Attempt {
                    provider: provider.id,
                    error,
                });

                // A garbled reply is worth one more go at the same provider:
                // another one would be answering a different question.
                if retryable && !fall_through {
                    match call(provider) {
                        Ok(value) => return Ok((provider.id, value)),
                        Err(second) => {
                            attempts.push(Attempt {
                                provider: provider.id,
                                error: second,
                            });
                            continue;
                        }
                    }
                }
                if !fall_through {
                    break;
                }
            }
        }
    }

    Err(attempts)
}
