//! StudyBites core.
//!
//! All storage and domain logic lives here, free of any Tauri dependency, so
//! it builds and tests on Linux (Codespaces / CI) without a WebView.

pub mod ai;
pub mod chunk;
pub mod db;
pub mod error;
pub mod jobs;
pub mod migrations;
pub mod models;
pub mod quota;
pub mod subjects;
pub mod topics;

pub use ai::{AiError, AiProvider, DistilledNote, GeneratedCard, ProviderId};
pub use chunk::{chunk_text, Chunk};
pub use db::Db;
pub use error::{CoreError, Result};
pub use jobs::{Job, JobStatus};
pub use models::{NewTopic, Subject, Topic};
