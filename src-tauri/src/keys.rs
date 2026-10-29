//! API keys in platform-secure storage.
//!
//! Desktop and iOS use the native keychain. Android stores them in private
//! SQLCipher settings because Android has no keyring backend in `keyring`.
//! Secrets are never returned to the WebView.

#[cfg(not(target_os = "android"))]
use keyring::Entry;
use studybites_core::ai::ProviderId;
use studybites_core::Db;

#[cfg(not(target_os = "android"))]
const SERVICE: &str = "com.sutauruki.studybites";

#[cfg(target_os = "android")]
const PRIVATE_SETTING_PREFIX: &str = "__secret.";

/// Keychain account name for a provider's key.
#[cfg(not(target_os = "android"))]
fn account_for(provider: ProviderId) -> String {
    format!("ai.{}", provider.as_str())
}

/// Keychain account for the GitHub token used by the update check.
#[cfg(not(target_os = "android"))]
const GITHUB_ACCOUNT: &str = "github.token";

#[cfg(target_os = "android")]
fn setting_for(account: &str) -> String {
    format!("{PRIVATE_SETTING_PREFIX}{account}")
}

#[cfg(not(target_os = "android"))]
fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| format!("the key store is unavailable: {e}"))
}

/// Store a secret, or clear it when the text is empty.
#[cfg(not(target_os = "android"))]
pub fn set(account: &str, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        return clear(account);
    }
    entry(account)?
        .set_password(secret)
        .map_err(|e| format!("the key could not be saved: {e}"))
}

#[cfg(not(target_os = "android"))]
fn get(account: &str) -> Result<Option<String>, String> {
    match entry(account)?.get_password() {
        Ok(secret) => Ok((!secret.trim().is_empty()).then_some(secret.trim().to_string())),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("the key store could not be read: {e}")),
    }
}

#[cfg(not(target_os = "android"))]
pub fn clear(account: &str) -> Result<(), String> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        // Deleting something that was never there is not a failure.
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("the key could not be removed: {e}")),
    }
}

pub fn set_provider_key(db: &Db, provider: ProviderId, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    #[cfg(target_os = "android")]
    {
        db.set_setting(&setting_for(&format!("ai.{}", provider.as_str())), secret)
            .map_err(|e| format!("the key could not be saved in encrypted storage: {e}"))?;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = db;
        set(&account_for(provider), secret)?;
    }

    match provider_key(db, provider)? {
        Some(stored) if stored == secret => Ok(()),
        None if secret.is_empty() => Ok(()),
        _ => Err("the key was not available after saving; please try again".into()),
    }
}

pub fn provider_key(db: &Db, provider: ProviderId) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        db.setting(&setting_for(&format!("ai.{}", provider.as_str())))
            .map(|value| value.filter(|secret| !secret.trim().is_empty()))
            .map_err(|e| format!("the encrypted key store could not be read: {e}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = db;
        get(&account_for(provider))
    }
}

pub fn set_github_token(db: &Db, token: &str) -> Result<(), String> {
    let token = token.trim();
    #[cfg(target_os = "android")]
    {
        db.set_setting(&setting_for("github.token"), token)
            .map_err(|e| format!("the token could not be saved in encrypted storage: {e}"))?;
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = db;
        set(GITHUB_ACCOUNT, token)?;
    }

    match github_token(db)? {
        Some(stored) if stored == token => Ok(()),
        None if token.is_empty() => Ok(()),
        _ => Err("the token was not available after saving; please try again".into()),
    }
}

pub fn github_token(db: &Db) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        db.setting(&setting_for("github.token"))
            .map(|value| value.filter(|secret| !secret.trim().is_empty()))
            .map_err(|e| format!("the encrypted key store could not be read: {e}"))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = db;
        get(GITHUB_ACCOUNT)
    }
}

/// Which providers currently have a key, for the Settings screen.
///
/// Only the fact that a key exists crosses the IPC boundary, never the key.
pub fn configured(db: &Db) -> Result<Vec<ProviderId>, String> {
    ProviderId::default_order()
        .into_iter()
        .filter_map(|id| match provider_key(db, id) {
            Ok(Some(_)) => Some(Ok(id)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}
