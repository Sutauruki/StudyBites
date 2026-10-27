//! StudyBites core.
//!
//! All storage and domain logic lives here, free of any Tauri dependency, so
//! it builds and tests on Linux (Codespaces / CI) without a WebView.

pub mod db;
pub mod error;
pub mod migrations;
pub mod models;
pub mod subjects;
pub mod topics;

pub use db::Db;
pub use error::{CoreError, Result};
pub use models::{NewTopic, Subject, Topic};
