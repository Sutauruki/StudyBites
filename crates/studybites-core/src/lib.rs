//! StudyBites core.
//!
//! All storage and domain logic lives here, free of any Tauri dependency, so
//! it builds and tests on Linux (Codespaces / CI) without a WebView.

pub mod ai;
pub mod base32;
pub mod cbt;
pub mod chunk;
pub mod crypto;
pub mod db;
pub mod device;
pub mod embed;
pub mod error;
pub mod files;
pub mod fsrs;
pub mod ingest;
pub mod jobs;
pub mod library;
pub mod license;
pub mod marking;
pub mod migrations;
pub mod models;
pub mod preview;
pub mod prompts;
pub mod providers;
pub mod quota;
pub mod study;
pub mod subjects;
pub mod topics;
pub mod update;

pub use ai::{AiError, AiProvider, DistilledNote, GeneratedCard, ProviderId};
pub use chunk::{chunk_text, Chunk};
pub use crypto::{DeviceKeys, SecretKey};
pub use db::{Db, Key};
pub use device::DeviceProfile;
pub use embed::{Embedder, LexicalEmbedder};
pub use error::{CoreError, Result};
pub use fsrs::{Memory, Rating, State};
pub use jobs::{Job, JobStatus};
pub use license::{License, VerifiedLicense};
pub use marking::{Mark, Verdict};
pub use models::{NewTopic, Subject, Topic};
