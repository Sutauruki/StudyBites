//! Checking whether a newer build exists (notify only).
//!
//! The app never downloads or installs anything itself. It compares its own
//! version with the latest release and, if there is a newer one, says so and
//! offers to open the download page. The student stays in control of what
//! runs on their machine.
//!
//! Because the repository is private, reading its releases needs a token.
//! That token is the user's own, entered in Settings and kept in the OS
//! keychain — never compiled into the binary, where anyone could extract it
//! (Brief 7.2 makes the same point about AI keys). With no token configured,
//! update checking is simply switched off.

use crate::error::{CoreError, Result};
use crate::providers::{Http, HttpError};
use serde::{Deserialize, Serialize};

/// Settings key holding the releases API URL to check.
pub const SETTING_FEED: &str = "update.feed";
/// Settings key for the on/off switch.
pub const SETTING_ENABLED: &str = "update.enabled";

/// Default feed: the repository's own latest release.
pub const DEFAULT_FEED: &str = "https://api.github.com/repos/Sutauruki/StudyBites/releases/latest";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    /// Where to send the student to download it.
    pub download_url: Option<String>,
    pub notes: Option<String>,
    /// Why no check happened, when one did not.
    pub skipped_reason: Option<String>,
}

impl UpdateStatus {
    fn skipped(current: &str, reason: &str) -> Self {
        Self {
            current_version: current.to_string(),
            latest_version: None,
            update_available: false,
            download_url: None,
            notes: None,
            skipped_reason: Some(reason.to_string()),
        }
    }
}

/// Compare two dotted version strings.
///
/// True when `candidate` is strictly newer than `current`. Anything
/// unparseable sorts as older, so a malformed tag never triggers a prompt.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(value: &str) -> Vec<u64> {
        value
            .trim()
            .trim_start_matches(['v', 'V'])
            // Drop any pre-release or build suffix: "1.2.0-beta.1" -> "1.2.0".
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    }

    let a = parts(candidate);
    let b = parts(current);
    if a.is_empty() || a.iter().all(|p| *p == 0) {
        return false;
    }

    for i in 0..a.len().max(b.len()) {
        let left = a.get(i).copied().unwrap_or(0);
        let right = b.get(i).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

/// Pull the version, notes and installer link out of a GitHub release.
pub fn parse_release(body: &str) -> Result<(String, Option<String>, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| CoreError::Invalid(format!("the update feed was not JSON: {e}")))?;

    let tag = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CoreError::Invalid("the update feed had no version".into()))?
        .to_string();

    let notes = value
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Prefer the installer itself; fall back to the release page.
    let asset = value
        .get("assets")
        .and_then(|v| v.as_array())
        .and_then(|assets| {
            assets.iter().find(|a| {
                a.get("name")
                    .and_then(|n| n.as_str())
                    .is_some_and(|n| n.ends_with(".exe"))
            })
        })
        .and_then(|a| a.get("browser_download_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            value
                .get("html_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    Ok((tag, notes, asset))
}

/// A rolling "latest" tag carries no version of its own, so the release notes
/// are searched for one instead.
pub fn version_from_release(tag: &str, notes: Option<&str>) -> Option<String> {
    if tag
        .trim_start_matches(['v', 'V'])
        .starts_with(|c: char| c.is_ascii_digit())
    {
        return Some(tag.trim_start_matches(['v', 'V']).to_string());
    }
    // "Installer version: 0.2.0", written into the notes by the build.
    let notes = notes?;
    let marker = notes.to_lowercase().find("version")?;
    let after = &notes[marker + "version".len()..];
    let candidate: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    candidate.contains('.').then_some(candidate)
}

/// Ask the feed whether anything newer exists.
///
/// `token` is the user's own GitHub token, or `None`. With no token the check
/// is reported as skipped rather than failed — nothing is wrong, it is simply
/// switched off.
pub fn check(
    http: &dyn Http,
    feed_url: &str,
    token: Option<&str>,
    current_version: &str,
) -> Result<UpdateStatus> {
    let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(UpdateStatus::skipped(
            current_version,
            "Update checking is off. Add a GitHub token in Settings to switch it on.",
        ));
    };

    let headers = vec![
        (
            "Accept".to_string(),
            "application/vnd.github+json".to_string(),
        ),
        ("User-Agent".to_string(), "StudyBites".to_string()),
        ("Authorization".to_string(), format!("Bearer {token}")),
    ];

    // A null body means "GET" to the Http impl the app supplies.
    let body = match http.post_json(feed_url, &headers, &serde_json::Value::Null) {
        Ok(body) => body,
        Err(HttpError::Status { code: 404, .. }) => {
            return Ok(UpdateStatus::skipped(
                current_version,
                "No releases were found, or the token cannot see this repository.",
            ))
        }
        Err(HttpError::Status {
            code: 401 | 403, ..
        }) => {
            return Ok(UpdateStatus::skipped(
                current_version,
                "The GitHub token was rejected. Check it in Settings.",
            ))
        }
        Err(HttpError::Status { code, .. }) => {
            return Ok(UpdateStatus::skipped(
                current_version,
                &format!("The update check failed ({code})."),
            ))
        }
        Err(HttpError::Transport(message)) => {
            return Ok(UpdateStatus::skipped(
                current_version,
                &format!("Could not reach GitHub: {message}"),
            ))
        }
    };

    let (tag, notes, download_url) = parse_release(&body)?;
    let latest = version_from_release(&tag, notes.as_deref());

    let update_available = latest
        .as_deref()
        .is_some_and(|v| is_newer(v, current_version));

    Ok(UpdateStatus {
        current_version: current_version.to_string(),
        latest_version: latest,
        update_available,
        download_url: update_available.then_some(download_url).flatten(),
        notes: update_available.then_some(notes).flatten(),
        skipped_reason: None,
    })
}
