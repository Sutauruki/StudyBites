use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The database could not be opened with the supplied key. With SQLCipher
    /// this is what a copied database file looks like on a different device.
    #[error("the database could not be unlocked with this key")]
    Locked,

    /// Input that failed validation in Rust (Section 10: validate every IPC input).
    #[error("{0}")]
    Invalid(String),

    #[error("{kind} {id} was not found")]
    NotFound { kind: &'static str, id: i64 },

    #[error("the database schema (v{found}) is newer than this app (v{expected})")]
    SchemaTooNew { found: i64, expected: i64 },
}

pub type Result<T> = std::result::Result<T, CoreError>;

/// Trim and validate a free-text name coming from the UI.
pub fn clean_name(raw: &str, field: &'static str, max: usize) -> Result<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(CoreError::Invalid(format!("{field} cannot be empty")));
    }
    if name.chars().count() > max {
        return Err(CoreError::Invalid(format!(
            "{field} cannot be longer than {max} characters"
        )));
    }
    Ok(name.to_string())
}
