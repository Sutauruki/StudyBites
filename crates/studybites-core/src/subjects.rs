//! Subjects CRUD (Brief 3.1).

use crate::db::Db;
use crate::error::{clean_name, CoreError, Result};
use crate::models::Subject;
use rusqlite::Row;

const MAX_NAME: usize = 120;

const SELECT: &str = "SELECT s.id, s.name, s.created_at, s.archived, s.exam_date,
        (SELECT count(*) FROM scheme_topics t WHERE t.subject_id = s.id) AS topic_count
   FROM subjects s";

fn row_to_subject(row: &Row) -> rusqlite::Result<Subject> {
    Ok(Subject {
        id: row.get("id")?,
        name: row.get("name")?,
        created_at: row.get("created_at")?,
        archived: row.get::<_, i64>("archived")? != 0,
        exam_date: row.get("exam_date")?,
        topic_count: row.get("topic_count")?,
    })
}

/// All subjects, archived ones last, then alphabetical.
pub fn list(db: &Db, include_archived: bool) -> Result<Vec<Subject>> {
    let sql = format!(
        "{SELECT} {} ORDER BY s.archived ASC, s.name COLLATE NOCASE ASC",
        if include_archived {
            ""
        } else {
            "WHERE s.archived = 0"
        }
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map([], row_to_subject)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(db: &Db, id: i64) -> Result<Subject> {
    let sql = format!("{SELECT} WHERE s.id = ?1");
    let mut stmt = db.conn().prepare(&sql)?;
    let mut rows = stmt.query([id])?;
    match rows.next()? {
        Some(row) => Ok(row_to_subject(row)?),
        None => Err(CoreError::NotFound {
            kind: "subject",
            id,
        }),
    }
}

pub fn create(db: &Db, name: &str) -> Result<Subject> {
    let name = clean_name(name, "Subject name", MAX_NAME)?;
    let res = db
        .conn()
        .execute("INSERT INTO subjects(name) VALUES (?1)", [&name]);
    match res {
        Ok(_) => get(db, db.conn().last_insert_rowid()),
        Err(e) if is_unique_violation(&e) => Err(CoreError::Invalid(format!(
            "A subject called \"{name}\" already exists"
        ))),
        Err(e) => Err(e.into()),
    }
}

pub fn rename(db: &Db, id: i64, name: &str) -> Result<Subject> {
    let name = clean_name(name, "Subject name", MAX_NAME)?;
    let res = db
        .conn()
        .execute("UPDATE subjects SET name = ?2 WHERE id = ?1", (id, &name));
    match res {
        Ok(0) => Err(CoreError::NotFound {
            kind: "subject",
            id,
        }),
        Ok(_) => get(db, id),
        Err(e) if is_unique_violation(&e) => Err(CoreError::Invalid(format!(
            "A subject called \"{name}\" already exists"
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Archive or restore. Archiving keeps all data; it only hides the subject.
pub fn set_archived(db: &Db, id: i64, archived: bool) -> Result<Subject> {
    let changed = db.conn().execute(
        "UPDATE subjects SET archived = ?2 WHERE id = ?1",
        (id, i64::from(archived)),
    )?;
    if changed == 0 {
        return Err(CoreError::NotFound {
            kind: "subject",
            id,
        });
    }
    get(db, id)
}

/// Permanently delete a subject and everything under it (ON DELETE CASCADE).
pub fn delete(db: &Db, id: i64) -> Result<()> {
    let changed = db
        .conn()
        .execute("DELETE FROM subjects WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(CoreError::NotFound {
            kind: "subject",
            id,
        });
    }
    Ok(())
}

/// SQLITE_CONSTRAINT_UNIQUE (2067) specifically, not any constraint failure.
fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(err, rusqlite::Error::SqliteFailure(e, _) if e.extended_code == 2067)
}

/// Set the exam date for a subject, or unset it with `None` (Brief 3.9).
///
/// Stored as `YYYY-MM-DD`; anything SQLite cannot read as a date is refused
/// rather than silently producing arithmetic nobody intended.
pub fn set_exam_date(db: &Db, id: i64, exam_date: Option<&str>) -> Result<Subject> {
    let cleaned = match exam_date.map(str::trim).filter(|d| !d.is_empty()) {
        Some(date) => {
            let valid: Option<String> = db
                .conn()
                .query_row("SELECT date(?1)", [date], |r| r.get(0))
                .unwrap_or(None);
            match valid {
                Some(d) => Some(d),
                None => {
                    return Err(CoreError::Invalid(
                        "that is not a date StudyBites understands".into(),
                    ))
                }
            }
        }
        None => None,
    };

    let changed = db.conn().execute(
        "UPDATE subjects SET exam_date = ?2 WHERE id = ?1",
        (id, cleaned.as_deref()),
    )?;
    if changed == 0 {
        return Err(CoreError::NotFound {
            kind: "subject",
            id,
        });
    }
    get(db, id)
}
