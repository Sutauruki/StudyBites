//! Scheme-of-work topics (Brief 3.2). Phase 1 is the manual outline editor;
//! Phase 2 fills the same tables from an AI-parsed scheme.

use crate::db::Db;
use crate::error::{clean_name, CoreError, Result};
use crate::models::{NewTopic, Topic};
use rusqlite::Row;

const MAX_TITLE: usize = 200;

/// The values `set_coverage` accepts. Kept next to the migration's CHECK.
pub const COVERAGES: [&str; 3] = ["unknown", "taught", "self"];

fn row_to_topic(row: &Row) -> rusqlite::Result<Topic> {
    Ok(Topic {
        id: row.get("id")?,
        subject_id: row.get("subject_id")?,
        parent_id: row.get("parent_id")?,
        title: row.get("title")?,
        week: row.get("week")?,
        position: row.get("position")?,
        is_off_scheme: row.get::<_, i64>("is_off_scheme")? != 0,
        supports_topic_id: row.get("supports_topic_id")?,
        is_preview: row.get::<_, i64>("is_preview")? != 0,
        archived_at: row.get("archived_at")?,
        coverage: row.get("coverage")?,
        children: Vec::new(),
    })
}

/// Every topic of a subject that is not archived, ordered by parent then
/// position.
pub fn list_flat(db: &Db, subject_id: i64) -> Result<Vec<Topic>> {
    let mut stmt = db.conn().prepare(
        "SELECT * FROM scheme_topics
          WHERE subject_id = ?1 AND archived_at IS NULL
          ORDER BY (parent_id IS NOT NULL), parent_id, position, id",
    )?;
    let rows = stmt.query_map([subject_id], row_to_topic)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Every topic of a subject, archived ones included.
///
/// Used by the outline diff: a topic archived by an earlier update must be
/// matchable again, so that re-importing a scheme restores it rather than
/// creating a second copy.
pub fn list_all(db: &Db, subject_id: i64) -> Result<Vec<Topic>> {
    let mut stmt = db.conn().prepare(
        "SELECT * FROM scheme_topics
          WHERE subject_id = ?1
          ORDER BY (parent_id IS NOT NULL), parent_id, position, id",
    )?;
    let rows = stmt.query_map([subject_id], row_to_topic)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The archived topics of a subject, oldest first.
pub fn list_archived(db: &Db, subject_id: i64) -> Result<Vec<Topic>> {
    let mut stmt = db.conn().prepare(
        "SELECT * FROM scheme_topics
          WHERE subject_id = ?1 AND archived_at IS NOT NULL
          ORDER BY archived_at, parent_id IS NOT NULL, parent_id, position, id",
    )?;
    let rows = stmt.query_map([subject_id], row_to_topic)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The outline as a tree: top-level topics in order, each with its children.
pub fn list_tree(db: &Db, subject_id: i64) -> Result<Vec<Topic>> {
    let flat = list_flat(db, subject_id)?;
    let (mut roots, children): (Vec<Topic>, Vec<Topic>) =
        flat.into_iter().partition(|t| t.parent_id.is_none());
    for child in children {
        if let Some(parent) = roots.iter_mut().find(|r| Some(r.id) == child.parent_id) {
            parent.children.push(child);
        } else {
            // Orphan (its parent is nested deeper than one level): show it at
            // the top rather than hiding it.
            roots.push(child);
        }
    }
    Ok(roots)
}

pub fn get(db: &Db, id: i64) -> Result<Topic> {
    let mut stmt = db
        .conn()
        .prepare("SELECT * FROM scheme_topics WHERE id = ?1")?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => Ok(row_to_topic(row)?),
        None => Err(CoreError::NotFound { kind: "topic", id }),
    }
}

/// Append a topic to the end of its parent's list.
pub fn create(db: &Db, input: &NewTopic) -> Result<Topic> {
    let title = clean_name(&input.title, "Topic title", MAX_TITLE)?;
    if let Some(week) = input.week {
        if !(1..=104).contains(&week) {
            return Err(CoreError::Invalid("Week must be between 1 and 104".into()));
        }
    }
    // Make sure the subject exists, and that the parent belongs to it.
    crate::subjects::get(db, input.subject_id)?;
    if let Some(parent_id) = input.parent_id {
        let parent = get(db, parent_id)?;
        if parent.subject_id != input.subject_id {
            return Err(CoreError::Invalid(
                "The parent topic belongs to a different subject".into(),
            ));
        }
        if parent.parent_id.is_some() {
            return Err(CoreError::Invalid(
                "Topics can only be nested one level deep".into(),
            ));
        }
    }

    let position: i64 = db.conn().query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM scheme_topics
          WHERE subject_id = ?1 AND parent_id IS ?2",
        (input.subject_id, input.parent_id),
        |r| r.get(0),
    )?;

    db.conn().execute(
        "INSERT INTO scheme_topics
            (subject_id, parent_id, title, week, position, is_off_scheme)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (
            input.subject_id,
            input.parent_id,
            &title,
            input.week,
            position,
            i64::from(input.is_off_scheme),
        ),
    )?;
    get(db, db.conn().last_insert_rowid())
}

/// Edit a topic's title and/or week. `None` leaves the field alone; to clear a
/// week, pass `Some(None)`.
pub fn update(db: &Db, id: i64, title: Option<&str>, week: Option<Option<i64>>) -> Result<Topic> {
    let existing = get(db, id)?;
    let title = match title {
        Some(t) => clean_name(t, "Topic title", MAX_TITLE)?,
        None => existing.title.clone(),
    };
    let week = match week {
        Some(w) => {
            if let Some(w) = w {
                if !(1..=104).contains(&w) {
                    return Err(CoreError::Invalid("Week must be between 1 and 104".into()));
                }
            }
            w
        }
        None => existing.week,
    };
    db.conn().execute(
        "UPDATE scheme_topics SET title = ?2, week = ?3 WHERE id = ?1",
        (id, &title, week),
    )?;
    get(db, id)
}

/// Delete a topic. Its children go with it (ON DELETE CASCADE); materials,
/// notes and cards keep existing with `topic_id` set to NULL.
pub fn delete(db: &Db, id: i64) -> Result<()> {
    let topic = get(db, id)?;
    db.conn()
        .execute("DELETE FROM scheme_topics WHERE id = ?1", [id])?;
    compact_positions(db, topic.subject_id, topic.parent_id)?;
    Ok(())
}

/// Hide a topic and its subtopics. Nothing is deleted: materials, notes and
/// cards keep pointing at the same topic id, they just stop appearing while it
/// is archived.
pub fn archive(db: &Db, id: i64) -> Result<Topic> {
    let topic = get(db, id)?;
    db.conn().execute(
        "UPDATE scheme_topics
            SET archived_at = COALESCE(archived_at, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
          WHERE id = ?1 OR parent_id = ?1",
        [id],
    )?;
    get(db, topic.id)
}

/// Bring an archived topic and its subtopics back.
pub fn restore(db: &Db, id: i64) -> Result<Topic> {
    let topic = get(db, id)?;
    db.conn().execute(
        "UPDATE scheme_topics SET archived_at = NULL
          WHERE id = ?1 OR parent_id = ?1",
        [id],
    )?;
    get(db, topic.id)
}

/// Record whether the student met this topic in class or studied it alone.
pub fn set_coverage(db: &Db, id: i64, coverage: &str) -> Result<Topic> {
    if !COVERAGES.contains(&coverage) {
        return Err(CoreError::Invalid(format!(
            "Coverage must be unknown, taught or self, not \"{coverage}\""
        )));
    }
    let changed = db.conn().execute(
        "UPDATE scheme_topics SET coverage = ?2 WHERE id = ?1",
        (id, coverage),
    )?;
    if changed == 0 {
        return Err(CoreError::NotFound {
            kind: "topic",
            id,
        });
    }
    get(db, id)
}

/// Rewrite the order of one sibling group. `ordered_ids` must be exactly the
/// ids currently in that group — anything else is rejected rather than
/// silently reordering part of the outline.
pub fn reorder(
    db: &mut Db,
    subject_id: i64,
    parent_id: Option<i64>,
    ordered_ids: &[i64],
) -> Result<Vec<Topic>> {
    let mut current: Vec<i64> = {
        let mut stmt = db.conn().prepare(
            "SELECT id FROM scheme_topics
              WHERE subject_id = ?1 AND parent_id IS ?2",
        )?;
        let rows = stmt.query_map((subject_id, parent_id), |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut wanted = ordered_ids.to_vec();
    current.sort_unstable();
    wanted.sort_unstable();
    if current != wanted {
        return Err(CoreError::Invalid(
            "The new order must list exactly the topics in that group".into(),
        ));
    }

    let tx = db.conn_mut().transaction()?;
    for (position, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE scheme_topics SET position = ?2 WHERE id = ?1",
            (id, position as i64),
        )?;
    }
    tx.commit()?;
    list_tree(db, subject_id)
}

/// Move a topic one step up or down among its siblings.
pub fn nudge(db: &mut Db, id: i64, delta: i64) -> Result<Vec<Topic>> {
    let topic = get(db, id)?;
    let siblings: Vec<i64> = {
        let mut stmt = db.conn().prepare(
            "SELECT id FROM scheme_topics
              WHERE subject_id = ?1 AND parent_id IS ?2
              ORDER BY position, id",
        )?;
        let rows = stmt.query_map((topic.subject_id, topic.parent_id), |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let from = siblings
        .iter()
        .position(|s| *s == id)
        .ok_or(CoreError::NotFound { kind: "topic", id })?;
    let to = (from as i64 + delta).clamp(0, siblings.len() as i64 - 1) as usize;
    let mut order = siblings;
    if from != to {
        let moved = order.remove(from);
        order.insert(to, moved);
    }
    reorder(db, topic.subject_id, topic.parent_id, &order)
}

/// Close gaps left by a delete so positions stay 0..n-1.
fn compact_positions(db: &Db, subject_id: i64, parent_id: Option<i64>) -> Result<()> {
    let ids: Vec<i64> = {
        let mut stmt = db.conn().prepare(
            "SELECT id FROM scheme_topics
              WHERE subject_id = ?1 AND parent_id IS ?2
              ORDER BY position, id",
        )?;
        let rows = stmt.query_map((subject_id, parent_id), |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (position, id) in ids.iter().enumerate() {
        db.conn().execute(
            "UPDATE scheme_topics SET position = ?2 WHERE id = ?1",
            (id, position as i64),
        )?;
    }
    Ok(())
}
