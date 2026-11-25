//! The ingestion pipeline (Brief 3.4).
//!
//! extract → chunk → distil → cards → embed → store, one chunk per job so the
//! work is resumable. The rule that shapes everything: raw text is sent to a
//! model **once**, at upload time, and is never kept in the hot path. What
//! survives is the distilled note, its cards and its vector.

use crate::ai::{AiError, AiProvider, DistillRequest, DistillSource, ProviderId, SchemeEntry};
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
pub const JOB_TRANSCRIBE_PAGE: &str = "transcribe_page";
const PHOTO_LIMIT: usize = 20;
const PHOTO_MAX_BYTES: usize = 4 * 1024 * 1024;
const NO_IMAGE_PROVIDER: &str = "None of your AI keys can read images. Add a Gemini key.";

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
    #[serde(default)]
    pub source: DistillSource,
    /// Pages this chunk came from. Defaulted rather than required: a job queued
    /// by an older build is still on the device after an update and must run.
    #[serde(default)]
    pub page_start: Option<u32>,
    #[serde(default)]
    pub page_end: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribePageJob {
    pub material_id: i64,
    pub page_id: i64,
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
    // Pages first: the chunks carry the page they came from, and that is what
    // the answer shows the student later.
    let kind = crate::files::FileKind::detect(path, bytes)?;
    let pages = crate::files::extract_pages(bytes, kind)?;
    let sha256 = crate::files::sha256_hex(bytes);

    // Re-uploading the same file must cost nothing (Brief 6).
    if let Some(existing) = library::material_by_hash(db, subject_id, &sha256)? {
        return Ok(Accepted {
            chunk_count: 0,
            duplicate_of: Some(existing.id),
            material: existing,
        });
    }

    let cold_path = crate::files::cold_store(app_data, file_key, &sha256, bytes)?;
    let material = library::material_create(
        db,
        &NewMaterial {
            subject_id,
            topic_id,
            original_filename: filename,
            mime: kind.mime(),
            size: bytes.len() as i64,
            sha256: &sha256,
            cold_path: Some(&cold_path),
        },
    )?;

    let chunks = chunk::chunk_pages(&pages);
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
                source: DistillSource::Document,
                page_start: piece.page_start,
                page_end: piece.page_end,
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

/// Store resized photo pages and queue one transcription job per page.
pub fn accept_photo_pages(
    db: &mut Db,
    app_data: &std::path::Path,
    file_key: &crate::crypto::SecretKey,
    subject_id: i64,
    topic_id: Option<i64>,
    pages: &[Vec<u8>],
) -> Result<Accepted> {
    if pages.is_empty() || pages.len() > PHOTO_LIMIT {
        return Err(CoreError::Invalid("Choose between 1 and 20 photos.".into()));
    }
    crate::subjects::get(db, subject_id)?;
    if let Some(topic_id) = topic_id {
        if topics::get(db, topic_id)?.subject_id != subject_id {
            return Err(CoreError::Invalid(
                "that topic belongs to a different subject".into(),
            ));
        }
    }
    let mut page_hashes = Vec::with_capacity(pages.len());
    let mut total_size = 0_i64;
    for image in pages {
        if image.is_empty() {
            return Err(CoreError::Invalid("a selected photo is empty".into()));
        }
        if image.len() > PHOTO_MAX_BYTES {
            return Err(CoreError::Invalid(
                "Each photo must be 4 MB or smaller. Choose fewer or smaller photos.".into(),
            ));
        }
        crate::files::image_mime(image)?;
        page_hashes.push(crate::files::sha256_hex(image));
        total_size += image.len() as i64;
    }

    let material_hash = crate::files::sha256_hex(page_hashes.join("").as_bytes());
    if let Some(existing) = library::material_by_hash(db, subject_id, &material_hash)? {
        return Ok(Accepted {
            chunk_count: 0,
            duplicate_of: Some(existing.id),
            material: existing,
        });
    }

    let filename = format!("Notes photos ({} pages)", pages.len());
    let material = library::material_create(
        db,
        &NewMaterial {
            subject_id,
            topic_id,
            original_filename: &filename,
            mime: "image/jpeg",
            size: total_size,
            sha256: &material_hash,
            cold_path: None,
        },
    )?;

    for (index, (image, hash)) in pages.iter().zip(page_hashes.iter()).enumerate() {
        let cold_path = crate::files::cold_store(app_data, file_key, hash, image)?;
        let page = library::page_create(db, material.id, index as i64, hash, &cold_path)?;
        jobs::enqueue(
            db,
            JOB_TRANSCRIBE_PAGE,
            &TranscribePageJob {
                material_id: material.id,
                page_id: page.id,
            },
        )?;
    }

    let material = library::material_set_status(db, material.id, library::STATUS_QUEUED, None)?;
    Ok(Accepted {
        material,
        chunk_count: pages.len(),
        duplicate_of: None,
    })
}

/// A student-approved transcript becomes ordinary distillation work.
pub fn confirm_transcript(db: &mut Db, material_id: i64) -> Result<usize> {
    let material = library::material_get(db, material_id)?;
    if material.mime != "image/jpeg" || material.status != library::STATUS_REVIEW {
        return Err(CoreError::Invalid(
            "this photo upload is not ready for transcript review".into(),
        ));
    }
    let pages = library::pages_for_material(db, material_id)?;
    if pages.is_empty()
        || pages
            .iter()
            .any(|page| page.status != "done" || page.transcript.is_none())
    {
        return Err(CoreError::Invalid(
            "Wait until every page has a transcript before confirming.".into(),
        ));
    }
    // One page per photo, numbered from 1. The "Page N" line stays in the text
    // because the model has always seen it and it marks where a page starts;
    // the number is also carried beside the text so the answer can cite it.
    let mut pages_text: Vec<crate::files::PageText> = Vec::with_capacity(pages.len());
    for page in &pages {
        let mut text = format!("Page {}\n", page.page_index + 1);
        if let Some(transcript) = &page.transcript {
            text.push_str(transcript);
        }
        pages_text.push(crate::files::PageText {
            page: Some(page.page_index as u32 + 1),
            text,
        });
    }
    let chunks = chunk::chunk_pages(&pages_text);
    if chunks.is_empty() {
        return Err(CoreError::Invalid(
            "Add readable text to at least one page before confirming.".into(),
        ));
    }
    let subject = crate::subjects::get(db, material.subject_id)?;
    let topic_title = match material.topic_id {
        Some(id) => topics::get(db, id).ok().map(|topic| topic.title),
        None => None,
    };
    let total = chunks.len();
    for piece in chunks {
        jobs::enqueue(
            db,
            JOB_DISTILL_CHUNK,
            &DistillJob {
                material_id,
                subject_id: material.subject_id,
                topic_id: material.topic_id,
                subject_name: subject.name.clone(),
                topic_title: topic_title.clone(),
                heading: piece.heading,
                text: piece.text,
                chunk_index: piece.index,
                chunk_total: total,
                source: DistillSource::StudentNotes,
                page_start: piece.page_start,
                page_end: piece.page_end,
            },
        )?;
    }
    if let Some(topic_id) = material.topic_id {
        crate::preview::retire_previews_for_topic(db, topic_id)?;
    }
    library::material_set_status(db, material_id, library::STATUS_QUEUED, None)?;
    Ok(total)
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
    app_data: &std::path::Path,
    file_key: &crate::crypto::SecretKey,
) -> Result<Processed> {
    let Some(job) = jobs::claim(db)? else {
        return Ok(Processed::Idle);
    };
    if job.kind == JOB_TRANSCRIBE_PAGE {
        return process_transcribe_page(db, providers, app_data, file_key, &job);
    }
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
        source: payload.source,
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
    let note = library::note_create_with_pages(
        db,
        Some(payload.material_id),
        payload.topic_id,
        &distilled,
        &vector,
        false,
        payload
            .page_start
            .map(|start| (start, payload.page_end.unwrap_or(start))),
    )?;
    let added = library::cards_create(db, note.id, payload.topic_id, &cards, false)?;

    jobs::complete(db, job.id)?;
    finish_material_if_last(db, payload.material_id)?;

    Ok(Processed::Done {
        material_id: payload.material_id,
        cards_added: added.len(),
    })
}

fn process_transcribe_page(
    db: &mut Db,
    providers: &[Provider<'_>],
    app_data: &std::path::Path,
    file_key: &crate::crypto::SecretKey,
    job: &jobs::Job,
) -> Result<Processed> {
    let payload: TranscribePageJob = job.payload()?;
    let page = library::page_get(db, payload.page_id)?;
    library::page_set_status(db, page.id, "processing", None)?;
    library::material_set_status(db, payload.material_id, library::STATUS_PROCESSING, None)?;

    let Some(cold_path) = page.cold_path.as_deref() else {
        return fail_transcription(
            db,
            job,
            &page,
            payload.material_id,
            "the stored page is missing",
        );
    };
    let image = match crate::files::cold_read(app_data, file_key, cold_path) {
        Ok(image) => image,
        Err(error) => {
            return fail_transcription(
                db,
                job,
                &page,
                payload.material_id,
                &format!("the stored page could not be read: {error}"),
            )
        }
    };
    let mime = match crate::files::image_mime(&image) {
        Ok(mime) => mime,
        Err(error) => {
            return fail_transcription(
                db,
                job,
                &page,
                payload.material_id,
                &format!("the stored page is not a supported image: {error}"),
            )
        }
    };
    let available = available_set(db, providers);
    let availability = |id: ProviderId| {
        providers.iter().any(|provider| {
            provider.id == id && (!provider.supports_images() || available.contains(&id))
        })
    };

    match providers::route(providers, &availability, |provider| {
        provider.transcribe_page(&image, mime)
    }) {
        Ok((provider, transcript)) => {
            record_success(db, provider)?;
            library::page_set_transcription(db, page.id, &transcript.text, &transcript.uncertain)?;
            library::page_set_status(db, page.id, "done", None)?;
            jobs::complete(db, job.id)?;
            let pages = library::pages_for_material(db, payload.material_id)?;
            if pages.iter().all(|item| item.status == "done") {
                library::material_set_status(
                    db,
                    payload.material_id,
                    library::STATUS_REVIEW,
                    None,
                )?;
            }
            Ok(Processed::Done {
                material_id: payload.material_id,
                cards_added: 0,
            })
        }
        Err(attempts)
            if attempts.is_empty()
                || attempts
                    .iter()
                    .all(|a| matches!(a.error, AiError::Unsupported(_))) =>
        {
            jobs::pause_for_configuration(db, job.id, NO_IMAGE_PROVIDER)?;
            library::page_set_status(db, page.id, "paused", Some(NO_IMAGE_PROVIDER))?;
            library::material_set_status(
                db,
                payload.material_id,
                library::STATUS_PAUSED,
                Some(NO_IMAGE_PROVIDER),
            )?;
            Ok(Processed::Paused { until: None })
        }
        Err(attempts)
            if attempts
                .iter()
                .any(|attempt| matches!(attempt.error, AiError::QuotaExhausted { .. }))
                && attempts.iter().all(|attempt| {
                    matches!(
                        attempt.error,
                        AiError::QuotaExhausted { .. } | AiError::Unsupported(_)
                    )
                }) =>
        {
            let reported = attempts
                .iter()
                .filter_map(|attempt| match &attempt.error {
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
            library::page_set_status(db, page.id, "paused", Some(MESSAGE))?;
            library::material_set_status(
                db,
                payload.material_id,
                library::STATUS_PAUSED,
                Some(MESSAGE),
            )?;
            Ok(Processed::Paused { until: reported })
        }
        Err(attempts) => {
            let message = attempts
                .iter()
                .map(|attempt| format!("{}: {}", attempt.provider.as_str(), attempt.error))
                .collect::<Vec<_>>()
                .join("; ");
            let after = jobs::fail(db, job.id, &message)?;
            let status = if after.status == jobs::JobStatus::Failed {
                library::STATUS_FAILED
            } else {
                library::STATUS_QUEUED
            };
            library::page_set_status(db, page.id, status, Some(&message))?;
            library::material_set_status(db, payload.material_id, status, Some(&message))?;
            Ok(Processed::Failed {
                material_id: payload.material_id,
                error: message,
            })
        }
    }
}

fn fail_transcription(
    db: &mut Db,
    job: &jobs::Job,
    page: &library::MaterialPage,
    material_id: i64,
    message: &str,
) -> Result<Processed> {
    let after = jobs::fail(db, job.id, message)?;
    let status = if after.status == jobs::JobStatus::Failed {
        library::STATUS_FAILED
    } else {
        library::STATUS_QUEUED
    };
    library::page_set_status(db, page.id, status, Some(message))?;
    library::material_set_status(db, material_id, status, Some(message))?;
    Ok(Processed::Failed {
        material_id,
        error: message.to_string(),
    })
}

/// Park photo jobs when the worker has no configured provider at all.
pub fn pause_waiting_photo_jobs(db: &Db) -> Result<usize> {
    let pending = jobs::pending(db)?;
    let mut paused = 0;
    for job in pending.into_iter().filter(|job| {
        job.kind == JOB_TRANSCRIBE_PAGE
            && matches!(
                job.status,
                jobs::JobStatus::Queued | jobs::JobStatus::Paused
            )
    }) {
        let payload: TranscribePageJob = job.payload()?;
        jobs::pause_for_configuration(db, job.id, NO_IMAGE_PROVIDER)?;
        library::page_set_status(db, payload.page_id, "paused", Some(NO_IMAGE_PROVIDER))?;
        library::material_set_status(
            db,
            payload.material_id,
            library::STATUS_PAUSED,
            Some(NO_IMAGE_PROVIDER),
        )?;
        paused += 1;
    }
    Ok(paused)
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
