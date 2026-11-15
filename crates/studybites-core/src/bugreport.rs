use crate::providers::{Http, HttpError};
use crate::{CoreError, Db, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_ATTACHMENT_BYTES: usize = 1_258_291;
const MAX_DIAGNOSTICS_BYTES: usize = 4 * 1024;
const MAX_AGE_DAYS: i64 = 14;
const MAX_ATTEMPTS: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Bug,
    Confusing,
    Idea,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub filename: String,
    pub mime: String,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BugReport {
    pub id: String,
    pub device_id: String,
    pub title: String,
    pub details: String,
    #[serde(default)]
    pub steps: Option<String>,
    #[serde(default)]
    pub contact: Option<String>,
    pub category: Category,
    #[serde(default)]
    pub diag: BTreeMap<String, String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

impl BugReport {
    pub fn validate(mut self) -> Result<Self> {
        if !is_uuid(&self.id) || !is_uuid(&self.device_id) {
            return Err(invalid("The report identifier is not valid."));
        }
        self.title = clean_text(&self.title);
        self.details = clean_text(&self.details);
        if !in_range(&self.title, 1, 120) {
            return Err(invalid("Add a summary of 1 to 120 characters."));
        }
        if !in_range(&self.details, 1, 2_000) {
            return Err(invalid("Describe what happened in 1 to 2,000 characters."));
        }
        if let Some(steps) = self.steps.take() {
            let steps = clean_text(&steps);
            if !steps.is_empty() && !in_range(&steps, 1, 1_500) {
                return Err(invalid("Steps must be 1 to 1,500 characters."));
            }
            self.steps = (!steps.is_empty()).then_some(steps);
        }
        if let Some(contact) = self.contact.take() {
            let contact = clean_text(&contact);
            if !contact.is_empty() && (!in_range(&contact, 1, 120) || !valid_email(&contact)) {
                return Err(invalid("Enter a valid contact email address."));
            }
            self.contact = (!contact.is_empty()).then_some(contact);
        }
        if self.diag.len() > 32 {
            return Err(invalid("Technical details are too long."));
        }
        let mut diagnostic_bytes = 0;
        let mut clean_diag = BTreeMap::new();
        for (key, value) in self.diag {
            let key = clean_text(&key);
            let value = redact(&clean_text(&value));
            if !allowed_diagnostic_key(&key)
                || key.encode_utf16().count() > 80
                || value.encode_utf16().count() > 512
            {
                return Err(invalid("Technical details must contain short text values."));
            }
            diagnostic_bytes += key.len() + value.len();
            if diagnostic_bytes > MAX_DIAGNOSTICS_BYTES {
                return Err(invalid("Technical details exceed the 4 KB limit."));
            }
            clean_diag.insert(key, value);
        }
        self.diag = clean_diag;
        if self.attachments.len() > 3 {
            return Err(invalid("Choose no more than 3 screenshots."));
        }
        for attachment in &mut self.attachments {
            attachment.filename = clean_text(&attachment.filename)
                .chars()
                .map(|character| {
                    if matches!(character, '/' | '\\') {
                        '_'
                    } else {
                        character
                    }
                })
                .take(120)
                .collect();
            if attachment.filename.is_empty() {
                return Err(invalid("A screenshot filename is required."));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&attachment.data)
                .map_err(|_| invalid("A screenshot is invalid or larger than 1.2 MB."))?;
            if bytes.len() > MAX_ATTACHMENT_BYTES
                || base64::engine::general_purpose::STANDARD.encode(&bytes) != attachment.data
            {
                return Err(invalid("A screenshot is invalid or larger than 1.2 MB."));
            }
            if !matches_magic(&attachment.mime, &bytes) {
                return Err(invalid(
                    "A screenshot's file type does not match its contents.",
                ));
            }
        }
        if serde_json::to_vec(&self)?.len() > MAX_BODY_BYTES {
            return Err(invalid("The report is larger than 4 MB."));
        }
        Ok(self)
    }
}

fn allowed_diagnostic_key(key: &str) -> bool {
    matches!(
        key,
        "appVersion"
            | "platform"
            | "webview"
            | "screenSize"
            | "language"
            | "timeZone"
            | "online"
            | "view"
            | "licenseState"
            | "recentErrors"
    )
}

fn invalid(message: &str) -> CoreError {
    CoreError::Invalid(message.to_string())
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23].iter().all(|index| bytes[*index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
        && matches!(bytes[14], b'1'..=b'8')
        && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
}

fn clean_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect::<String>()
        .trim()
        .to_string()
}

fn in_range(value: &str, min: usize, max: usize) -> bool {
    let length = value.encode_utf16().count();
    (min..=max).contains(&length)
}

fn valid_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !value.chars().any(char::is_whitespace)
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn matches_magic(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

pub fn build_request_json(report: &BugReport) -> Result<serde_json::Value> {
    let report = report.clone().validate()?;
    Ok(serde_json::to_value(report)?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    RateLimited,
    Retryable,
    Rejected(String),
}

pub fn send(http: &dyn Http, url: &str, report: &BugReport) -> SendOutcome {
    let body = match build_request_json(report) {
        Ok(body) => body,
        Err(error) => return SendOutcome::Rejected(error.to_string()),
    };
    match http.post_json_with_timeout(
        url,
        &[("Content-Type".to_string(), "application/json".to_string())],
        &body,
        Duration::from_secs(15),
    ) {
        Ok(response) => match serde_json::from_str::<serde_json::Value>(&response) {
            Ok(value) if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) => {
                SendOutcome::Sent
            }
            Ok(value) => SendOutcome::Rejected(
                value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("The bug-report service rejected this report.")
                    .to_string(),
            ),
            Err(_) => SendOutcome::Retryable,
        },
        Err(HttpError::Status { code: 429, .. }) => SendOutcome::RateLimited,
        Err(HttpError::Status {
            code: 400..=499, ..
        }) => SendOutcome::Rejected("The bug-report service rejected this report.".into()),
        Err(HttpError::Status { .. } | HttpError::Transport(_)) => SendOutcome::Retryable,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReport {
    pub id: i64,
    pub created_at: String,
    pub report: BugReport,
    pub attempts: i64,
}

pub fn outbox_add(db: &Db, report: &BugReport) -> Result<()> {
    let report = report.clone().validate()?;
    let payload = serde_json::to_string(&report)?;
    db.conn().execute(
        "INSERT INTO bug_outbox(report_id, payload_json) VALUES (?1, ?2)
         ON CONFLICT(report_id) DO NOTHING",
        (&report.id, payload),
    )?;
    Ok(())
}

pub fn outbox_pending(db: &Db) -> Result<Vec<PendingReport>> {
    let mut statement = db
        .conn()
        .prepare("SELECT id, created_at, payload_json, attempts FROM bug_outbox ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (id, created_at, payload, attempts) = row?;
        Ok(PendingReport {
            id,
            created_at,
            report: serde_json::from_str(&payload)?,
            attempts,
        })
    })
    .collect()
}

pub fn outbox_remove(db: &Db, id: i64) -> Result<()> {
    db.conn()
        .execute("DELETE FROM bug_outbox WHERE id = ?1", [id])?;
    Ok(())
}

pub fn outbox_bump(db: &Db, id: i64) -> Result<i64> {
    let changed = db.conn().execute(
        "UPDATE bug_outbox SET attempts = attempts + 1 WHERE id = ?1",
        [id],
    )?;
    if changed == 0 {
        return Err(CoreError::NotFound {
            kind: "bug report",
            id,
        });
    }
    Ok(db.conn().query_row(
        "SELECT attempts FROM bug_outbox WHERE id = ?1",
        [id],
        |row| row.get(0),
    )?)
}

pub fn flush(db: &Db, http: &dyn Http, url: &str) -> Result<usize> {
    let pending = outbox_pending(db)?;
    let mut sent = 0;
    for report in pending {
        let age_limit = format!("-{MAX_AGE_DAYS} days");
        let expired: bool = db.conn().query_row(
            "SELECT COALESCE(julianday(?1) <= julianday('now', ?2), 0)",
            (&report.created_at, age_limit),
            |row| row.get(0),
        )?;
        if expired || report.attempts >= MAX_ATTEMPTS {
            outbox_remove(db, report.id)?;
            continue;
        }
        if send(http, url, &report.report) == SendOutcome::Sent {
            outbox_remove(db, report.id)?;
            sent += 1;
            continue;
        }
        if outbox_bump(db, report.id)? >= MAX_ATTEMPTS {
            outbox_remove(db, report.id)?;
        }
        break;
    }
    Ok(sent)
}

pub fn redact(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if starts_ascii_case_insensitive(bytes, index, b"bearer ") {
            output.push_str("Bearer [redacted]");
            index += 7;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
        } else if starts_ascii_case_insensitive(bytes, index, b"AIza")
            && token_run(bytes, index + 4) >= 20
        {
            output.push_str("[redacted]");
            index += 4 + token_run(bytes, index + 4);
        } else if bytes.get(index..index + 3) == Some(b"sk-") && token_run(bytes, index + 3) >= 12 {
            output.push_str("[redacted]");
            index += 3 + token_run(bytes, index + 3);
        } else if let Some(end) = secret_assignment_end(bytes, index) {
            output.push_str("[redacted]");
            index = end;
        } else if is_path_start(bytes, index) {
            output.push_str("[path]");
            break;
        } else {
            let run = base64_run(bytes, index);
            if run >= 80 {
                output.push_str("[redacted]");
                index += run;
            } else {
                let character = input[index..].chars().next().expect("valid UTF-8 boundary");
                output.push(character);
                index += character.len_utf8();
            }
        }
    }
    output
}

fn secret_assignment_end(value: &[u8], index: usize) -> Option<usize> {
    let at_boundary = index == 0
        || value[index - 1].is_ascii_whitespace()
        || matches!(value[index - 1], b'(' | b'[' | b'"' | b'\'' | b'=' | b':');
    if !at_boundary {
        return None;
    }

    for label in [
        b"api_key".as_slice(),
        b"api-key".as_slice(),
        b"apikey".as_slice(),
        b"token".as_slice(),
        b"password".as_slice(),
        b"secret".as_slice(),
        b"authorization".as_slice(),
    ] {
        if !starts_ascii_case_insensitive(value, index, label) {
            continue;
        }
        let mut cursor = index + label.len();
        while value.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if !value
            .get(cursor)
            .is_some_and(|byte| matches!(*byte, b'=' | b':'))
        {
            continue;
        }
        cursor += 1;
        while value.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if value
            .get(cursor)
            .is_some_and(|byte| matches!(*byte, b'\'' | b'"'))
        {
            cursor += 1;
        }
        if label.eq_ignore_ascii_case(b"authorization")
            && starts_ascii_case_insensitive(value, cursor, b"Bearer ")
        {
            cursor += 7;
        }
        let value_start = cursor;
        while value
            .get(cursor)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(*byte, b',' | b';'))
        {
            cursor += 1;
        }
        if cursor > value_start {
            return Some(cursor);
        }
    }
    None
}

fn starts_ascii_case_insensitive(value: &[u8], index: usize, needle: &[u8]) -> bool {
    value
        .get(index..index + needle.len())
        .is_some_and(|slice| slice.eq_ignore_ascii_case(needle))
}

fn token_run(value: &[u8], start: usize) -> usize {
    value
        .get(start..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(**byte, b'_' | b'-'))
        .count()
}

fn base64_run(value: &[u8], start: usize) -> usize {
    value
        .get(start..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| {
            byte.is_ascii_alphanumeric() || matches!(**byte, b'+' | b'/' | b'=' | b'_' | b'-')
        })
        .count()
}

fn is_path_start(value: &[u8], index: usize) -> bool {
    let at_boundary = index == 0
        || value[index - 1].is_ascii_whitespace()
        || matches!(value[index - 1], b'(' | b'[' | b'"' | b'\'' | b'=' | b':');
    if !at_boundary {
        return false;
    }
    let rest = &value[index..];
    (rest.len() >= 2 && rest[0] == b'/' && rest[1] != b'/')
        || (rest.len() >= 3
            && rest[0].is_ascii_alphabetic()
            && rest[1] == b':'
            && matches!(rest[2], b'\\' | b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DEV_KEY;
    use crate::providers::HttpError;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct FakeHttp {
        responses: Mutex<VecDeque<std::result::Result<String, HttpError>>>,
    }

    impl FakeHttp {
        fn new(responses: Vec<std::result::Result<String, HttpError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
            }
        }
    }

    impl Http for FakeHttp {
        fn post_json(
            &self,
            _url: &str,
            _headers: &[(String, String)],
            _body: &serde_json::Value,
        ) -> std::result::Result<String, HttpError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(HttpError::Transport("no canned response".into())))
        }
    }

    fn report() -> BugReport {
        BugReport {
            id: "00000000-0000-4000-8000-000000000001".into(),
            device_id: "00000000-0000-4000-8000-000000000002".into(),
            title: "A problem".into(),
            details: "The button did not respond.".into(),
            steps: None,
            contact: None,
            category: Category::Bug,
            diag: BTreeMap::new(),
            attachments: vec![],
        }
    }

    fn db() -> Db {
        Db::open_in_memory(DEV_KEY).unwrap()
    }

    #[test]
    fn validates_report_limits_and_attachment_magic() {
        assert!(report().validate().is_ok());
        let mut oversized = report();
        oversized.details = "x".repeat(2_001);
        assert!(oversized.validate().is_err());
        let mut bad_image = report();
        bad_image.attachments.push(Attachment {
            filename: "screen.png".into(),
            mime: "image/png".into(),
            data: base64::engine::general_purpose::STANDARD.encode(b"not png"),
        });
        assert!(bad_image.validate().is_err());
    }

    #[test]
    fn redacts_keys_tokens_base64_and_local_paths() {
        let input = "AIza123456789012345678901234567890123456 Bearer abcdef123 sk-abcdefghijklmnop /home/student/notes.pdf";
        let redacted = redact(input);
        assert!(!redacted.contains("AIza"));
        assert!(!redacted.contains("abcdef123"));
        assert!(!redacted.contains("sk-abcdefghijklmnop"));
        assert!(!redacted.contains("/home/student"));
        assert!(redacted.contains("[path]"));
    }

    #[test]
    fn validates_and_redacts_only_known_diagnostic_fields() {
        let mut valid = report();
        valid.diag.insert(
            "webview".into(),
            "Bearer abcdef123 api_key=very-secret authorization='Bearer auth-secret' /home/student/notes.pdf".into(),
        );
        let clean = valid.validate().unwrap();
        let webview = &clean.diag["webview"];
        assert!(!webview.contains("abcdef123"));
        assert!(!webview.contains("very-secret"));
        assert!(!webview.contains("auth-secret"));
        assert!(!webview.contains("/home/student"));

        let mut invalid = report();
        invalid
            .diag
            .insert("notes".into(), "student material".into());
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn maps_worker_outcomes_to_send_statuses() {
        let valid = serde_json::json!({"ok": true}).to_string();
        assert_eq!(
            send(
                &FakeHttp::new(vec![Ok(valid)]),
                "https://example.test/",
                &report()
            ),
            SendOutcome::Sent
        );
        assert_eq!(
            send(
                &FakeHttp::new(vec![Err(HttpError::Status {
                    code: 429,
                    body: String::new()
                })]),
                "https://example.test/",
                &report()
            ),
            SendOutcome::RateLimited
        );
        assert_eq!(
            send(
                &FakeHttp::new(vec![Err(HttpError::Status {
                    code: 500,
                    body: String::new()
                })]),
                "https://example.test/",
                &report()
            ),
            SendOutcome::Retryable
        );
        assert!(matches!(
            send(
                &FakeHttp::new(vec![Err(HttpError::Status {
                    code: 400,
                    body: String::new()
                })]),
                "https://example.test/",
                &report()
            ),
            SendOutcome::Rejected(_)
        ));
    }

    #[test]
    fn outbox_flushes_sent_reports_and_drops_expired_or_exhausted_rows() {
        let db = db();
        let mut first = report();
        outbox_add(&db, &first).unwrap();
        first.id = "00000000-0000-4000-8000-000000000003".into();
        outbox_add(&db, &first).unwrap();
        let pending = outbox_pending(&db).unwrap();
        db.conn()
            .execute(
                "UPDATE bug_outbox SET created_at = datetime('now', '-15 days') WHERE id = ?1",
                [pending[0].id],
            )
            .unwrap();
        for _ in 0..MAX_ATTEMPTS {
            outbox_bump(&db, pending[1].id).unwrap();
        }

        assert_eq!(
            flush(&db, &FakeHttp::new(vec![]), "https://example.test/").unwrap(),
            0
        );
        assert!(outbox_pending(&db).unwrap().is_empty());
    }

    #[test]
    fn outbox_removes_a_successfully_sent_report() {
        let db = db();
        outbox_add(&db, &report()).unwrap();
        let http = FakeHttp::new(vec![Ok(r#"{"ok":true}"#.into())]);
        assert_eq!(flush(&db, &http, "https://example.test/").unwrap(), 1);
        assert!(outbox_pending(&db).unwrap().is_empty());
    }
}
