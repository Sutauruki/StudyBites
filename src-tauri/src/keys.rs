//! API keys in the OS keychain (Brief 7.2, 9).
//!
//! On Windows this is DPAPI-backed credential storage. Keys never go into the
//! database, never into a config file, and are never returned to the WebView —
//! the frontend can only ask whether one is *set*, and Rust is what uses it.

use keyring::Entry;
use studybites_core::ai::ProviderId;

const SERVICE: &str = "com.sutauruki.studybites";

/// Keychain account name for a provider's key.
fn account_for(provider: ProviderId) -> String {
    format!("ai.{}", provider.as_str())
}

/// Keychain account for the GitHub token used by the update check.
const GITHUB_ACCOUNT: &str = "github.token";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| format!("the key store is unavailable: {e}"))
}

/// Store a secret, or clear it when the text is empty.
pub fn set(account: &str, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return clear(account);
    }
    entry(account)?
        .set_password(secret)
        .map_err(|e| format!("the key could not be saved: {e}"))
}

pub fn get(account: &str) -> Option<String> {
    entry(account)
        .ok()?
        .get_password()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn clear(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        // Deleting something that was never there is not a failure.
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("the key could not be removed: {e}")),
    }
}

pub fn set_provider_key(provider: ProviderId, secret: &str) -> Result<(), String> {
    set(&account_for(provider), secret)
}

pub fn provider_key(provider: ProviderId) -> Option<String> {
    get(&account_for(provider))
}

pub fn set_github_token(token: &str) -> Result<(), String> {
    set(GITHUB_ACCOUNT, token)
}

pub fn github_token() -> Option<String> {
    get(GITHUB_ACCOUNT)
}

/// Which providers currently have a key, for the Settings screen.
///
/// Only the fact that a key exists crosses the IPC boundary, never the key.
pub fn configured() -> Vec<ProviderId> {
    ProviderId::default_order()
        .into_iter()
        .filter(|id| provider_key(*id).is_some())
        .collect()
}
