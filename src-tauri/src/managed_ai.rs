use std::path::Path;
use studybites_core::license;
use studybites_core::providers::{Http, HttpError, Provider};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub photo_pages: u32,
    pub tutor_messages: u32,
    pub smart_model_messages: u32,
    pub marked_answers: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub plan: Option<String>,
    pub date: Option<String>,
    pub reset_at: Option<String>,
    pub used: Option<Usage>,
    pub limits: Option<Usage>,
}

impl Status {
    fn disabled() -> Self {
        Self {
            enabled: false,
            plan: None,
            date: None,
            reset_at: None,
            used: None,
            limits: None,
        }
    }
}

fn base_url() -> Result<Option<String>, String> {
    let Some(value) = option_env!("STUDYBITES_AI_URL") else {
        return Ok(None);
    };
    let value = value.trim().trim_end_matches('/');
    if value.is_empty() {
        return Ok(None);
    }
    if !value.starts_with("https://") || value.chars().any(char::is_whitespace) {
        return Err("Managed AI requires a valid HTTPS Worker URL.".into());
    }
    Ok(Some(value.to_string()))
}

pub fn provider<'a>(app_data: &Path, http: &'a dyn Http) -> Result<Option<Provider<'a>>, String> {
    let Some(base_url) = base_url()? else {
        return Ok(None);
    };
    let Some(code) = license::stored(app_data) else {
        return Ok(None);
    };
    Ok(Some(Provider::managed(
        format!("{base_url}/v1/ai"),
        code,
        http,
    )))
}

pub fn status(http: &dyn Http, app_data: &Path) -> Result<Status, String> {
    let Some(base_url) = base_url()? else {
        return Ok(Status::disabled());
    };
    let Some(code) = license::stored(app_data) else {
        return Ok(Status::disabled());
    };
    let headers = vec![
        ("Authorization".to_string(), format!("Bearer {code}")),
        ("Accept".to_string(), "application/json".to_string()),
    ];
    let raw = http
        .post_json(
            &format!("{base_url}/v1/status"),
            &headers,
            &serde_json::Value::Null,
        )
        .map_err(|error| match error {
            HttpError::Status { code: 401, .. } => {
                "Managed AI rejected this activation code. Confirm the Worker uses the app's license public key.".to_string()
            }
            HttpError::Status { code, .. } => {
                format!("Managed AI status returned HTTP {code}.")
            }
            HttpError::Transport(message) => {
                format!("Managed AI status could not be reached: {message}")
            }
        })?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("Managed AI returned an invalid status: {error}"))
}
