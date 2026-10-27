//! The ingestion pipeline (Brief 3.4).
//!
//! extract → chunk → distil → cards → embed → store, one chunk per job so the
//! work is resumable. The rule that shapes everything: raw text is sent to a
//! model **once**, at upload time, and is never kept in the hot path. What
//! survives is the distilled note, its cards and its vector.

use crate::ai::{AiError, AiProvider, DistillRequest, ProviderId, SchemeEntry};
use crate::db::Db;
use crate::embed::Embedder;
use crate::error::{CoreError, Result};
use crate::library::{self, NewMaterial};
use crate::models::{NewTopic, Topic};
use crate::providers::{self, Provider};
use crate::{chunk, jobs, quota, topics};
use serde::{Deserialize, Serialize};

/// Job kind for one chunk of one upload.
pub const JOB_DISTILL_CHUNK: &str = "distill_chunk";

/// What a `distill_chunk` job carries.
///
/// The chunk text rides in the payload rather than being re-extracted: the
/// original is compressed in cold storage, decompressing it per chunk would
/// be slow, and re-extracting could drift if the parser changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistillJob {
    pub material_id: i64,
    pub subject_id: i64,
    pub topic_id: Option<i64>,
    pub subject_name: String,
    pub topic_title: Option<String>,
    pub heading: Option<String>,
    pub text: String,
    pub chunk_index: usize,
    pub chunk_total: usize,
}

/// Result of accepting an upload, before any AI work happens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    pub material: library::Material,
    pub chunk_count: usize,
    /// Set when this exact file was already uploaded to this subject.
    pub duplicate_of: Option<i64>,
}

/// Take an upload: store the original, chunk the text, queue the work.
///
/// No AI request is made here, so this stays fast and works offline; the
/// queue drains later when a provider is reachable.
pub fn accept_upload(
    db: &mut Db,
    app_data: &std::path::Path,
    file_key: &crate::crypto::SecretKey,
    subject_id: i64,
    topic_id: Option<i64>,
    filename: &str,
    bytes: &[u8],
) -> Result<Accepted> {
    let subject = crate::subjects::get(db, subject_id)?;
    let path = std::path::Path::new(filename);
    let extracted = crate::files::extract_from_bytes(path, bytes)?;

    // Re-uploading the same file must cost nothing (Brief 6).
    if let Some(existing) = library::material_by_hash(db, subject_id, &extracted.sha256)? {
        return Ok(Accepted {
            chunk_count: 0,
            duplicate_of: Some(existing.id),
            material: existing,
        });
    }

    let cold_path = crate::files::cold_store(app_data, file_key, &extracted.sha256, bytes)?;
    let material = library::material_create(
        db,
        &NewMaterial {
            subject_id,
            topic_id,
            original_filename: filename,
            mime: extracted.kind.mime(),
            size: extracted.size as i64,
            sha256: &extracted.sha256,
            cold_path: Some(&cold_path),
        },
    )?;

    let chunks = chunk::chunk_text(&extracted.text);
    if chunks.is_empty() {
        library::material_set_status(
            db,
            material.id,
            library::STATUS_FAILED,
            Some("no readable text could be found in that file"),
        )?;
        return Err(CoreError::Invalid(
            "no readable text could be found in that file".into(),
        ));
    }

    let topic_title = match topic_id {
        Some(id) => topics::get(db, id).ok().map(|t| t.title),
        None => None,
    };

    let total = chunks.len();
    for piece in chunks {
        jobs::enqueue(
            db,
            JOB_DISTILL_CHUNK,
            &DistillJob {
                material_id: material.id,
                subject_id,
                topic_id,
                subject_name: subject.name.clone(),
                topic_title: topic_title.clone(),
                heading: piece.heading,
                text: piece.text,
                chunk_index: piece.index,
                chunk_total: total,
            },
        )?;
    }

    // Real notes supersede any preview for this topic (Brief 3.7).
    if let Some(topic_id) = topic_id {
        crate::preview::retire_previews_for_topic(db, topic_id)?;
    }

    let material = library::material_set_status(db, material.id, library::STATUS_QUEUED, None)?;
    Ok(Accepted {
        material,
        chunk_count: total,
        duplicate_of: None,
    })
}

/// What happened to one job, so the worker loop knows whether to continue.
#[derive(Debug, Clone, PartialEq)]
pub enum Processed {
    /// Nothing was waiting.
    Idle,
    Done {
        material_id: i64,
        cards_added: usize,
    },
    /// Every provider is out of quota; the job is parked until `until`.
    Paused {
        until: Option<String>,
    },
    Failed {
        material_id: i64,
        error: String,
    },
}

/// Take one job off the queue and run it to completion.
///
/// Returns after a single job so the caller can show progress and stay
/// responsive, rather than disappearing into a long loop.
pub fn process_one(
    db: &mut Db,
    providers: &[Provider<'_>],
    embedder: &dyn Embedder,
) -> Result<Processed> {
    let Some(job) = jobs::claim(db)? else {
        return Ok(Processed::Idle);
    };
    if job.kind != JOB_DISTILL_CHUNK {
        jobs::fail(db, job.id, "unknown job kind")?;
        return Ok(Processed::Idle);
    }

    let payload: DistillJob = job.payload()?;
    library::material_set_status(db, payload.material_id, library::STATUS_PROCESSING, None)?;

    let request = DistillRequest {
        subject: payload.subject_name.clone(),
        topic: payload.topic_title.clone(),
        heading: payload.heading.clone(),
        text: payload.text.clone(),
    };

    // Skip providers already known to be out of quota, so an exhausted one
    // does not cost a request to rediscover.
    let available = available_set(db, providers);
    let availability = |id: ProviderId| -> bool { available.contains(&id) };

    let distilled = match providers::route(providers, &availability, |p| p.distill(&request)) {
        Ok((id, note)) => {
            record_success(db, id)?;
            note
        }
        Err(attempts) => return handle_failure(db, &job, payload.material_id, attempts),
    };

    // Cards are a second call; if it fails the note is still worth keeping,
    // so the chunk's work is not thrown away.
    let cards = match providers::route(providers, &availability, |p| p.generate_cards(&distilled)) {
        Ok((id, cards)) => {
            record_success(db, id)?;
            cards
        }
        Err(_) => Vec::new(),
    };

    let vector = embedder.embed(&distilled.as_plain_text());
    let note = library::note_create(
        db,
        Some(payload.material_id),
        payload.topic_id,
        &distilled,
        &vector,
        false,
    )?;
    let added = library::cards_create(db, note.id, payload.topic_id, &cards, false)?;

    jobs::complete(db, job.id)?;
    finish_material_if_last(db, payload.material_id)?;

    Ok(Processed::Done {
        material_id: payload.material_id,
        cards_added: added.len(),
    })
}

/// Which providers are usable right now.
fn available_set(db: &Db, providers: &[Provider<'_>]) -> Vec<ProviderId> {
    providers
        .iter()
        .map(|p| p.id)
        .filter(|id| quota::is_available(db, id.as_str()).unwrap_or(true))
        .collect()
}

fn record_success(db: &Db, id: ProviderId) -> Result<()> {
    quota::clear_exhausted(db, id.as_str())?;
    // Token counts are not reported uniformly across providers, so the
    // request count is what the daily allowance is actually tracked on.
    quota::record(db, id.as_str(), 0)?;
    Ok(())
}

/// Midnight UTC tonight, the usual reset point for a free daily tier.
fn next_utc_midnight(db: &Db) -> String {
    db.conn()
        .query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now','+1 day','start of day')",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "2099-01-01T00:00:00.000Z".to_string())
}

/// Decide what a round of failed attempts means for the job.
fn handle_failure(
    db: &mut Db,
    job: &jobs::Job,
    material_id: i64,
    attempts: Vec<providers::Attempt>,
) -> Result<Processed> {
    let all_quota = !attempts.is_empty()
        && attempts
            .iter()
            .all(|a| matches!(a.error, AiError::QuotaExhausted { .. }));

    if all_quota {
        // Park until the soonest reset; studying keeps working offline.
        let reported = attempts
            .iter()
            .filter_map(|a| match &a.error {
                AiError::QuotaExhausted { until } => until.clone(),
                _ => None,
            })
            .min();
        let marker = reported.clone().unwrap_or_else(|| next_utc_midnight(db));

        for attempt in &attempts {
            quota::mark_exhausted(db, attempt.provider.as_str(), &marker)?;
        }
        const MESSAGE: &str =
            "Daily AI limit reached — processing resumes automatically after reset.";
        jobs::pause_until(db, job.id, &marker, MESSAGE)?;
        library::material_set_status(db, material_id, library::STATUS_PAUSED, Some(MESSAGE))?;
        return Ok(Processed::Paused { until: reported });
    }

    let message = attempts
        .iter()
        .map(|a| format!("{}: {}", a.provider.as_str(), a.error))
        .collect::<Vec<_>>()
        .join("; ");

    let after = jobs::fail(db, job.id, &message)?;
    if after.status == jobs::JobStatus::Failed {
        library::material_set_status(db, material_id, library::STATUS_FAILED, Some(&message))?;
    }
    Ok(Processed::Failed {
        material_id,
        error: message,
    })
}

/// Mark an upload done once none of its jobs remain.
fn finish_material_if_last(db: &Db, material_id: i64) -> Result<()> {
    let outstanding: i64 = db.conn().query_row(
        "SELECT count(*) FROM jobs
          WHERE status != 'done'
            AND json_extract(payload_json, '$.materialId') = ?1",
        [material_id],
        |r| r.get(0),
    )?;
    if outstanding == 0 {
        library::material_set_status(db, material_id, library::STATUS_DONE, None)?;
    }
    Ok(())
}

/// Turn a confirmed scheme of work into topics (Brief 3.2).
///
/// The student edits and approves the outline before this is called, so the
/// entries arrive already confirmed.
pub fn save_scheme(db: &Db, subject_id: i64, entries: &[SchemeEntry]) -> Result<Vec<Topic>> {
    crate::subjects::get(db, subject_id)?;
    for entry in entries {
        if entry.title.trim().is_empty() {
            continue;
        }
        let parent = topics::create(
            db,
            &NewTopic {
                subject_id,
                parent_id: None,
                title: entry.title.clone(),
                week: entry.week,
                is_off_scheme: false,
            },
        )?;
        for sub in &entry.subtopics {
            if sub.trim().is_empty() {
                continue;
            }
            topics::create(
                db,
                &NewTopic {
                    subject_id,
                    parent_id: Some(parent.id),
                    title: sub.clone(),
                    week: None,
                    is_off_scheme: false,
                },
            )?;
        }
    }
    topics::list_tree(db, subject_id)
}
