//! Materials, notes and cards: everything an upload produces (Brief 3.4, 5).

use crate::ai::{DistilledNote, GeneratedCard};
use crate::db::Db;
use crate::embed;
use crate::error::{CoreError, Result};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

/// How far an upload has got through the pipeline.
pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_PROCESSING: &str = "processing";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_PAUSED: &str = "paused";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    pub id: i64,
    pub subject_id: i64,
    pub topic_id: Option<i64>,
    pub original_filename: String,
    pub mime: String,
    pub size: i64,
    pub sha256: String,
    pub cold_path: Option<String>,
    pub uploaded_at: String,
    pub status: String,
    pub error: Option<String>,
    /// Filled in for the materials list, so the UI can show progress.
    pub note_count: i64,
    pub card_count: i64,
}

fn row_to_material(row: &Row) -> rusqlite::Result<Material> {
    Ok(Material {
        id: row.get("id")?,
        subject_id: row.get("subject_id")?,
        topic_id: row.get("topic_id")?,
        original_filename: row.get("original_filename")?,
        mime: row.get("mime")?,
        size: row.get("size")?,
        sha256: row.get("sha256")?,
        cold_path: row.get("cold_path")?,
        uploaded_at: row.get("uploaded_at")?,
        status: row.get("status")?,
        error: row.get("error")?,
        note_count: row.get("note_count")?,
        card_count: row.get("card_count")?,
    })
}

const MATERIAL_SELECT: &str = "SELECT m.*,
        (SELECT count(*) FROM notes n WHERE n.material_id = m.id) AS note_count,
        (SELECT count(*) FROM cards c
           JOIN notes n2 ON n2.id = c.note_id
          WHERE n2.material_id = m.id) AS card_count
   FROM materials m";

pub fn material_get(db: &Db, id: i64) -> Result<Material> {
    let sql = format!("{MATERIAL_SELECT} WHERE m.id = ?1");
    let mut stmt = db.conn().prepare(&sql)?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => Ok(row_to_material(row)?),
        None => Err(CoreError::NotFound {
            kind: "material",
            id,
        }),
    }
}

/// Uploads for a subject, newest first.
pub fn materials_for_subject(db: &Db, subject_id: i64) -> Result<Vec<Material>> {
    let sql =
        format!("{MATERIAL_SELECT} WHERE m.subject_id = ?1 ORDER BY m.uploaded_at DESC, m.id DESC");
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map([subject_id], row_to_material)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Has this exact file been uploaded to this subject already?
///
/// Re-uploading the same notes should cost nothing (Brief 6), so the caller
/// can stop before spending a single request.
pub fn material_by_hash(db: &Db, subject_id: i64, sha256: &str) -> Result<Option<Material>> {
    let sql = format!("{MATERIAL_SELECT} WHERE m.subject_id = ?1 AND m.sha256 = ?2 LIMIT 1");
    let mut stmt = db.conn().prepare(&sql)?;
    let mut rows = stmt.query((subject_id, sha256))?;
    Ok(match rows.next()? {
        Some(row) => Some(row_to_material(row)?),
        None => None,
    })
}

#[derive(Debug, Clone)]
pub struct NewMaterial<'a> {
    pub subject_id: i64,
    pub topic_id: Option<i64>,
    pub original_filename: &'a str,
    pub mime: &'a str,
    pub size: i64,
    pub sha256: &'a str,
    pub cold_path: Option<&'a str>,
}

pub fn material_create(db: &Db, input: &NewMaterial<'_>) -> Result<Material> {
    crate::subjects::get(db, input.subject_id)?;
    let name = input.original_filename.trim();
    if name.is_empty() {
        return Err(CoreError::Invalid("the file has no name".into()));
    }

    db.conn().execute(
        "INSERT INTO materials
            (subject_id, topic_id, original_filename, mime, size, sha256, cold_path, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        (
            input.subject_id,
            input.topic_id,
            name,
            input.mime,
            input.size,
            input.sha256,
            input.cold_path,
            STATUS_QUEUED,
        ),
    )?;
    material_get(db, db.conn().last_insert_rowid())
}

pub fn material_set_status(
    db: &Db,
    id: i64,
    status: &str,
    error: Option<&str>,
) -> Result<Material> {
    db.conn().execute(
        "UPDATE materials SET status = ?2, error = ?3 WHERE id = ?1",
        (id, status, error),
    )?;
    material_get(db, id)
}

pub fn material_set_topic(db: &Db, id: i64, topic_id: Option<i64>) -> Result<Material> {
    let material = material_get(db, id)?;
    if let Some(topic_id) = topic_id {
        let topic = crate::topics::get(db, topic_id)?;
        if topic.subject_id != material.subject_id {
            return Err(CoreError::Invalid(
                "that topic belongs to a different subject".into(),
            ));
        }
    }
    db.conn().execute(
        "UPDATE materials SET topic_id = ?2 WHERE id = ?1",
        (id, topic_id),
    )?;
    // Keep everything produced from this upload on the same topic.
    db.conn().execute(
        "UPDATE notes SET topic_id = ?2 WHERE material_id = ?1",
        (id, topic_id),
    )?;
    db.conn().execute(
        "UPDATE cards SET topic_id = ?2
          WHERE note_id IN (SELECT id FROM notes WHERE material_id = ?1)",
        (id, topic_id),
    )?;
    material_get(db, id)
}

/// Delete an upload and everything derived from it.
///
/// Returns the cold-storage name, so removing the file stays with the caller
/// and this module touches no filesystem.
pub fn material_delete(db: &Db, id: i64) -> Result<Option<String>> {
    let material = material_get(db, id)?;
    db.conn()
        .execute("DELETE FROM materials WHERE id = ?1", [id])?;
    Ok(material.cold_path)
}

// ---- notes --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: i64,
    pub material_id: Option<i64>,
    pub topic_id: Option<i64>,
    pub content: DistilledNote,
    pub is_preview: bool,
    pub created_at: String,
}

fn row_to_note(row: &Row) -> rusqlite::Result<Note> {
    let json: String = row.get("content_json")?;
    Ok(Note {
        id: row.get("id")?,
        material_id: row.get("material_id")?,
        topic_id: row.get("topic_id")?,
        content: serde_json::from_str(&json).unwrap_or_default(),
        is_preview: row.get::<_, i64>("is_preview")? != 0,
        created_at: row.get("created_at")?,
    })
}

/// Store a distilled note together with its embedding.
pub fn note_create(
    db: &Db,
    material_id: Option<i64>,
    topic_id: Option<i64>,
    note: &DistilledNote,
    embedding: &[f32],
    is_preview: bool,
) -> Result<Note> {
    let content_json = serde_json::to_string(note)?;
    db.conn().execute(
        "INSERT INTO notes(material_id, topic_id, content_json, embedding, is_preview)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (
            material_id,
            topic_id,
            &content_json,
            embed::to_blob(embedding),
            i64::from(is_preview),
        ),
    )?;
    note_get(db, db.conn().last_insert_rowid())
}

pub fn note_get(db: &Db, id: i64) -> Result<Note> {
    let mut stmt = db.conn().prepare("SELECT * FROM notes WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => Ok(row_to_note(row)?),
        None => Err(CoreError::NotFound { kind: "note", id }),
    }
}

pub fn notes_for_material(db: &Db, material_id: i64) -> Result<Vec<Note>> {
    let mut stmt = db
        .conn()
        .prepare("SELECT * FROM notes WHERE material_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([material_id], row_to_note)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every note belonging to one subject, with its vector, for retrieval.
///
/// Retrieval is scoped to a single subject by the brief, so this join is the
/// boundary that enforces it.
pub fn note_vectors_for_subject(db: &Db, subject_id: i64) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut stmt = db.conn().prepare(
        "SELECT n.id, n.embedding
           FROM notes n
           LEFT JOIN materials m ON m.id = n.material_id
           LEFT JOIN scheme_topics t ON t.id = n.topic_id
          WHERE n.embedding IS NOT NULL
            AND (m.subject_id = ?1 OR t.subject_id = ?1)",
    )?;
    let rows = stmt.query_map([subject_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;

    let mut out = Vec::new();
    for entry in rows {
        let (id, blob) = entry?;
        out.push((id, embed::from_blob(&blob)?));
    }
    Ok(out)
}

/// Fetch several notes by id, keeping the order given.
pub fn notes_by_ids(db: &Db, ids: &[i64]) -> Result<Vec<Note>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Ok(note) = note_get(db, *id) {
            out.push(note);
        }
    }
    Ok(out)
}

// ---- cards --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: i64,
    pub note_id: Option<i64>,
    pub topic_id: Option<i64>,
    pub payload: GeneratedCard,
    pub is_preview: bool,
    pub retired: bool,
    pub created_at: String,
}

fn row_to_card(row: &Row) -> rusqlite::Result<Option<Card>> {
    let json: String = row.get("payload_json")?;
    // A card whose payload no longer parses is skipped rather than fatal: one
    // bad row must not break a whole study session.
    let Ok(payload) = serde_json::from_str::<GeneratedCard>(&json) else {
        return Ok(None);
    };
    Ok(Some(Card {
        id: row.get("id")?,
        note_id: row.get("note_id")?,
        topic_id: row.get("topic_id")?,
        payload,
        is_preview: row.get::<_, i64>("is_preview")? != 0,
        retired: row.get::<_, i64>("retired")? != 0,
        created_at: row.get("created_at")?,
    }))
}

/// Store generated cards and give each a fresh scheduling state.
pub fn cards_create(
    db: &mut Db,
    note_id: i64,
    topic_id: Option<i64>,
    cards: &[GeneratedCard],
    is_preview: bool,
) -> Result<Vec<i64>> {
    let tx = db.conn_mut().transaction()?;
    let mut ids = Vec::with_capacity(cards.len());

    for card in cards {
        let payload_json = serde_json::to_string(card)?;
        tx.execute(
            "INSERT INTO cards(note_id, topic_id, kind, payload_json, is_preview)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                note_id,
                topic_id,
                card.kind(),
                &payload_json,
                i64::from(is_preview),
            ),
        )?;
        let id = tx.last_insert_rowid();
        // Due immediately: a brand-new card is waiting to be learned.
        tx.execute(
            "INSERT INTO card_state(card_id, due, state)
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'), 0)",
            [id],
        )?;
        ids.push(id);
    }

    tx.commit()?;
    Ok(ids)
}

pub fn card_get(db: &Db, id: i64) -> Result<Card> {
    let mut stmt = db.conn().prepare("SELECT * FROM cards WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => row_to_card(row)?.ok_or(CoreError::Invalid(
            "that card is stored in a format this version cannot read".into(),
        )),
        None => Err(CoreError::NotFound { kind: "card", id }),
    }
}

/// Cards belonging to a subject, newest first, excluding retired ones.
pub fn cards_for_subject(db: &Db, subject_id: i64, limit: i64) -> Result<Vec<Card>> {
    let mut stmt = db.conn().prepare(
        "SELECT c.* FROM cards c
           JOIN notes n ON n.id = c.note_id
           LEFT JOIN materials m ON m.id = n.material_id
           LEFT JOIN scheme_topics t ON t.id = c.topic_id
          WHERE c.retired = 0 AND (m.subject_id = ?1 OR t.subject_id = ?1)
          ORDER BY c.id DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map((subject_id, limit), row_to_card)?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

pub fn card_set_retired(db: &Db, id: i64, retired: bool) -> Result<()> {
    db.conn().execute(
        "UPDATE cards SET retired = ?2 WHERE id = ?1",
        (id, i64::from(retired)),
    )?;
    Ok(())
}
