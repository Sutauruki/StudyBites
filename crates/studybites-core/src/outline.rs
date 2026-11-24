//! Outline updates (roadmap item 5).
//!
//! When a lecturer hands out a revised scheme of work the student re-imports
//! it. This module answers one question — what actually changed? — and then
//! applies the answer without losing anything.
//!
//! Three rules shape the whole file:
//!
//! * **A topic id never changes.** An update renames, moves, archives or
//!   restores. It never deletes, so every material, note and card keeps the
//!   topic it was filed under.
//! * **Matching is deterministic and offline.** There is no AI call and no
//!   clock: the same two outlines always produce the same diff, in the same
//!   order, on any device.
//! * **Unsure is not guessed.** Two titles are the same topic only when they
//!   are the same words (token-set Jaccard at or above 0.8) or one or two
//!   characters apart. A bolder rename is offered as an addition plus a
//!   removal, and the student can pair them by hand.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::ai::SchemeEntry;
use crate::db::Db;
use crate::error::{clean_name, CoreError, Result};
use crate::models::Topic;
use crate::topics;

const MAX_TITLE: usize = 200;
/// Two token sets this alike are the same topic, reworded or reordered.
const TITLE_SIMILARITY: f64 = 0.8;
/// Titles this many characters apart are the same topic with a typo.
const MAX_EDITS: usize = 2;
/// How many past updates the history command returns.
const HISTORY_LIMIT: i64 = 20;

// ---- normalisation ------------------------------------------------------

/// The comparison key for a title.
///
/// Lowercased, trimmed, spaces collapsed, and one leading enumerator or
/// bullet removed at a time (`1.`, `2)`, `Week 3:`, `-`, `•`). It only strips
/// a prefix that genuinely looks like numbering, so "2D vectors" survives.
pub fn normalize(title: &str) -> String {
    let mut text = title.trim().to_lowercase();
    loop {
        let rest = text.trim_start();
        match strip_leading_enum(rest) {
            Some(next) => text = next.trim_start().to_string(),
            None => break,
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One leading enumerator, if there is one.
fn strip_leading_enum(text: &str) -> Option<&str> {
    if let Some(rest) = text.strip_prefix(['-', '–', '—', '•', '*', '+']) {
        return Some(rest);
    }
    // "Week 3: Cell division" is a common way schemes number their topics.
    if let Some(after) = text.strip_prefix("week") {
        let after = after.trim_start();
        let week_digits = after.chars().take_while(char::is_ascii_digit).count();
        if week_digits > 0 {
            if let Some(tail) = after[week_digits..].strip_prefix(['.', ')', ':']) {
                return Some(tail);
            }
        }
    }
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = &text[digits..];
    // "3.1" and "10.2": section numbering, both halves stripped.
    if let Some(after) = rest.strip_prefix('.') {
        let second = after.chars().take_while(char::is_ascii_digit).count();
        if second > 0 {
            let tail = &after[second..];
            if tail.is_empty() || tail.starts_with(' ') || tail.starts_with(':') {
                return Some(tail);
            }
        }
    }
    if let Some(after) = rest.strip_prefix(['.', ')', ':']) {
        return Some(after);
    }
    None
}

fn token_set(text: &str) -> HashSet<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect()
}

fn jaccard(a: &str, b: &str) -> f64 {
    let left = token_set(a);
    let right = token_set(b);
    let union = left.union(&right).count();
    if union == 0 {
        return 1.0;
    }
    left.intersection(&right).count() as f64 / union as f64
}

/// Levenshtein over characters, two rows at a time.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(ca != cb);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

fn titles_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if jaccard(a, b) >= TITLE_SIMILARITY {
        return true;
    }
    let distance = edit_distance(a, b);
    let longest = a.chars().count().max(b.chars().count());
    distance <= MAX_EDITS && distance * 4 <= longest
}

// ---- the diff -----------------------------------------------------------

/// A topic as the diff refers to it. Identical to the stored topic except
/// that it carries no children.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicRef {
    pub id: i64,
    pub title: String,
    pub week: Option<i64>,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Renamed {
    pub id: i64,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    pub id: i64,
    /// The topic's title before the move, for the Changes screen.
    pub title: String,
    pub from_week: Option<i64>,
    pub to_week: Option<i64>,
    pub from_parent: Option<i64>,
    pub to_parent: Option<i64>,
    /// Set when the topic moves under a parent that is itself being added.
    pub to_parent_title: Option<String>,
}

/// A topic the incoming outline has and the stored one does not. A new parent
/// carries its new subtopics; a new subtopic carries its existing parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedTopic {
    pub title: String,
    pub week: Option<i64>,
    pub parent_id: Option<i64>,
    /// The parent's title, so the screen can say where the subtopic goes.
    #[serde(default)]
    pub parent_title: Option<String>,
    #[serde(default)]
    pub subtopics: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineDiff {
    pub unchanged: Vec<TopicRef>,
    pub renamed: Vec<Renamed>,
    pub moved: Vec<Moved>,
    pub added: Vec<AddedTopic>,
    pub removed: Vec<TopicRef>,
    pub restored: Vec<TopicRef>,
}

impl OutlineDiff {
    /// True when applying this diff would change nothing.
    pub fn is_empty(&self) -> bool {
        self.renamed.is_empty()
            && self.moved.is_empty()
            && self.added.is_empty()
            && self.removed.is_empty()
            && self.restored.is_empty()
    }
}

fn topic_ref(topic: &Topic) -> TopicRef {
    TopicRef {
        id: topic.id,
        title: topic.title.clone(),
        week: topic.week,
        parent_id: topic.parent_id,
    }
}

/// How many of the incoming entry's subtopics the stored topic already holds.
/// Used only to break ties between two equally good title matches.
fn subtopic_overlap(entry: &SchemeEntry, topic: &Topic, existing: &[Topic]) -> usize {
    let wanted: HashSet<String> = entry
        .subtopics
        .iter()
        .map(|sub| normalize(sub.as_str()))
        .filter(|key| !key.is_empty())
        .collect();
    existing
        .iter()
        .filter(|child| child.parent_id == Some(topic.id))
        .filter(|child| wanted.contains(&normalize(&child.title)))
        .count()
}

/// Order candidate parents best first: most subtopics already known, then the
/// most similar title, then the lowest id so the result never depends on the
/// order SQLite happened to return rows in.
fn sort_parent_candidates(candidates: &mut [&Topic], entry: &SchemeEntry, existing: &[Topic]) {
    let key = normalize(&entry.title);
    candidates.sort_by(|a, b| {
        let overlap_a = subtopic_overlap(entry, a, existing);
        let overlap_b = subtopic_overlap(entry, b, existing);
        let similarity_a = jaccard(&key, &normalize(&a.title));
        let similarity_b = jaccard(&key, &normalize(&b.title));
        overlap_b
            .cmp(&overlap_a)
            .then_with(|| similarity_b.total_cmp(&similarity_a))
            .then_with(|| a.id.cmp(&b.id))
    });
}

struct PendingSubtopic {
    title: String,
    entry: usize,
    /// The matched parent, or `None` when the parent is being added.
    parent: Option<i64>,
}

/// Compare an incoming scheme of work with what is stored.
///
/// Parents are matched first, then subtopics inside matched parents, then
/// leftover subtopics across parents — that last pass is what recognises a
/// subtopic that moved under a different topic instead of pretending it was
/// removed and added again.
pub fn diff_outline(db: &Db, subject_id: i64, incoming: &[SchemeEntry]) -> Result<OutlineDiff> {
    crate::subjects::get(db, subject_id)?;
    let existing = topics::list_all(db, subject_id)?;
    let mut diff = OutlineDiff::default();

    // A blank row is a parse artefact, not a topic.
    let entries: Vec<&SchemeEntry> = incoming
        .iter()
        .filter(|entry| !normalize(&entry.title).is_empty())
        .collect();
    // An empty import is "nothing to compare", never "archive everything".
    if entries.is_empty() {
        return Ok(diff);
    }

    let roots: Vec<&Topic> = existing
        .iter()
        .filter(|topic| topic.parent_id.is_none())
        .collect();

    let mut used: HashSet<i64> = HashSet::new();
    let mut matched: Vec<Option<i64>> = vec![None; entries.len()];

    // Pass 1: the same title once numbering, case and spacing are ignored.
    for (index, entry) in entries.iter().enumerate() {
        let key = normalize(&entry.title);
        let mut candidates: Vec<&Topic> = roots
            .iter()
            .copied()
            .filter(|topic| !used.contains(&topic.id) && normalize(&topic.title) == key)
            .collect();
        sort_parent_candidates(&mut candidates, entry, &existing);
        if let Some(topic) = candidates.first() {
            matched[index] = Some(topic.id);
            used.insert(topic.id);
        }
    }

    // Pass 2: the same words, or a typo or two away.
    for (index, entry) in entries.iter().enumerate() {
        if matched[index].is_some() {
            continue;
        }
        let key = normalize(&entry.title);
        let mut candidates: Vec<&Topic> = roots
            .iter()
            .copied()
            .filter(|topic| !used.contains(&topic.id))
            .filter(|topic| titles_match(&key, &normalize(&topic.title)))
            .collect();
        sort_parent_candidates(&mut candidates, entry, &existing);
        if let Some(topic) = candidates.first() {
            matched[index] = Some(topic.id);
            used.insert(topic.id);
        }
    }

    let mut pending_subtopics: Vec<PendingSubtopic> = Vec::new();
    let mut pending_children: Vec<&Topic> = Vec::new();

    for (index, entry) in entries.iter().enumerate() {
        let Some(parent_id) = matched[index] else {
            // A new parent: its subtopics may still turn out to be existing
            // topics moving under it, so they join the cross-parent pass.
            for sub in &entry.subtopics {
                let title = sub.trim();
                if !normalize(title).is_empty() {
                    pending_subtopics.push(PendingSubtopic {
                        title: title.to_string(),
                        entry: index,
                        parent: None,
                    });
                }
            }
            continue;
        };
        let parent = match existing.iter().find(|topic| topic.id == parent_id) {
            Some(parent) => parent,
            None => continue,
        };
        let parent_archived = parent.archived_at.is_some();

        if parent_archived {
            diff.restored.push(topic_ref(parent));
        } else if normalize(&entry.title) != normalize(&parent.title) {
            diff.renamed.push(Renamed {
                id: parent.id,
                old: parent.title.clone(),
                new: entry.title.trim().to_string(),
            });
        } else if entry.week != parent.week {
            diff.moved.push(Moved {
                id: parent.id,
                title: parent.title.clone(),
                from_week: parent.week,
                to_week: entry.week,
                from_parent: None,
                to_parent: None,
                to_parent_title: None,
            });
        } else {
            diff.unchanged.push(topic_ref(parent));
        }

        let mut children: Vec<&Topic> = existing
            .iter()
            .filter(|topic| topic.parent_id == Some(parent_id))
            .collect();
        children.sort_by_key(|child| (child.position, child.id));

        for sub in &entry.subtopics {
            let key = normalize(sub);
            if key.is_empty() {
                continue;
            }
            let mut exact: Vec<&Topic> = children
                .iter()
                .copied()
                .filter(|child| !used.contains(&child.id) && normalize(&child.title) == key)
                .collect();
            exact.sort_by_key(|child| child.id);
            let chosen = match exact.first() {
                Some(child) => Some(*child),
                None => {
                    let mut similar: Vec<&Topic> = children
                        .iter()
                        .copied()
                        .filter(|child| !used.contains(&child.id))
                        .filter(|child| titles_match(&key, &normalize(&child.title)))
                        .collect();
                    similar.sort_by(|a, b| {
                        let similarity_a = jaccard(&key, &normalize(&a.title));
                        let similarity_b = jaccard(&key, &normalize(&b.title));
                        similarity_b
                            .total_cmp(&similarity_a)
                            .then_with(|| a.id.cmp(&b.id))
                    });
                    similar.first().copied()
                }
            };
            match chosen {
                Some(child) => {
                    used.insert(child.id);
                    if child.archived_at.is_some() {
                        diff.restored.push(topic_ref(child));
                    } else if key != normalize(&child.title) {
                        diff.renamed.push(Renamed {
                            id: child.id,
                            old: child.title.clone(),
                            new: sub.trim().to_string(),
                        });
                    } else {
                        diff.unchanged.push(topic_ref(child));
                    }
                }
                None => pending_subtopics.push(PendingSubtopic {
                    title: sub.trim().to_string(),
                    entry: index,
                    parent: Some(parent_id),
                }),
            }
        }

        if !parent_archived {
            pending_children.extend(children.into_iter().filter(|child| !used.contains(&child.id)));
        }
    }

    // Leftover subtopics, now searched across the whole outline: this is the
    // "this subtopic moved under another topic" case.
    let mut still_pending: Vec<PendingSubtopic> = Vec::new();
    for pending in pending_subtopics {
        let key = normalize(&pending.title);
        let mut candidates: Vec<&Topic> = pending_children
            .iter()
            .copied()
            .filter(|child| titles_match(&key, &normalize(&child.title)))
            .collect();
        candidates.sort_by(|a, b| {
            let similarity_a = jaccard(&key, &normalize(&a.title));
            let similarity_b = jaccard(&key, &normalize(&b.title));
            similarity_b
                .total_cmp(&similarity_a)
                .then_with(|| a.id.cmp(&b.id))
        });
        match candidates.first() {
            Some(child) => {
                used.insert(child.id);
                pending_children.retain(|candidate| candidate.id != child.id);
                diff.moved.push(Moved {
                    id: child.id,
                    title: child.title.clone(),
                    from_week: child.week,
                    to_week: child.week,
                    from_parent: child.parent_id,
                    to_parent: pending.parent,
                    to_parent_title: match pending.parent {
                        Some(_) => None,
                        None => Some(entries[pending.entry].title.trim().to_string()),
                    },
                });
            }
            None => still_pending.push(pending),
        }
    }

    // Everything the incoming outline names that still has no home is new.
    for pending in &still_pending {
        if let Some(parent_id) = pending.parent {
            diff.added.push(AddedTopic {
                title: pending.title.clone(),
                week: None,
                parent_id: Some(parent_id),
                parent_title: existing
                    .iter()
                    .find(|topic| topic.id == parent_id)
                    .map(|topic| topic.title.clone()),
                subtopics: Vec::new(),
            });
        }
    }
    for (index, entry) in entries.iter().enumerate() {
        if matched[index].is_some() {
            continue;
        }
        let subtopics: Vec<String> = still_pending
            .iter()
            .filter(|pending| pending.entry == index && pending.parent.is_none())
            .map(|pending| pending.title.clone())
            .collect();
        diff.added.push(AddedTopic {
            title: entry.title.trim().to_string(),
            week: entry.week,
            parent_id: None,
            parent_title: None,
            subtopics,
        });
    }

    // What the incoming outline dropped. Already-archived rows are not
    // reported again: there is nothing left to archive.
    for child in pending_children {
        if child.archived_at.is_none() {
            diff.removed.push(topic_ref(child));
        }
    }
    for root in roots {
        if !used.contains(&root.id) && root.archived_at.is_none() {
            diff.removed.push(topic_ref(root));
        }
    }

    Ok(diff)
}

// ---- decisions and apply ------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddDecision {
    pub title: String,
    #[serde(default)]
    pub week: Option<i64>,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub subtopics: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameDecision {
    pub id: i64,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDecision {
    pub id: i64,
    /// Leave out to keep the current week.
    #[serde(default)]
    pub week: Option<i64>,
    /// `week` has no JSON spelling for "clear it", so this says so plainly.
    #[serde(default)]
    pub clear_week: bool,
    #[serde(default)]
    pub to_parent: Option<i64>,
    /// Move under a parent created by this same update.
    #[serde(default)]
    pub to_parent_title: Option<String>,
    #[serde(default)]
    pub to_root: bool,
}

/// A removed row the student says is really an added row, renamed.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDecision {
    pub removed_id: i64,
    /// Index into `added`.
    pub added: usize,
}

/// What the student ticked on the Changes screen.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineDecisions {
    #[serde(default)]
    pub added: Vec<AddDecision>,
    #[serde(default)]
    pub renamed: Vec<RenameDecision>,
    #[serde(default)]
    pub moved: Vec<MoveDecision>,
    #[serde(default)]
    pub archived: Vec<i64>,
    #[serde(default)]
    pub restored: Vec<i64>,
    #[serde(default)]
    pub paired: Vec<PairedDecision>,
}

fn ensure_subject(tx: &rusqlite::Transaction<'_>, id: i64, subject_id: i64) -> Result<()> {
    let owner: Option<i64> = match tx.query_row(
        "SELECT subject_id FROM scheme_topics WHERE id = ?1",
        [id],
        |row| row.get(0),
    ) {
        Ok(value) => Some(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(err) => return Err(err.into()),
    };
    match owner {
        Some(owner) if owner == subject_id => Ok(()),
        Some(_) => Err(CoreError::Invalid(
            "That topic belongs to a different subject".into(),
        )),
        None => Err(CoreError::NotFound { kind: "topic", id }),
    }
}

fn ensure_root(tx: &rusqlite::Transaction<'_>, id: i64) -> Result<()> {
    let parent: Option<i64> = match tx.query_row(
        "SELECT parent_id FROM scheme_topics WHERE id = ?1",
        [id],
        |row| row.get(0),
    ) {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(CoreError::NotFound { kind: "topic", id })
        }
        Err(err) => return Err(err.into()),
    };
    if parent.is_some() {
        return Err(CoreError::Invalid(
            "Topics can only be nested one level deep".into(),
        ));
    }
    Ok(())
}

fn next_position(
    tx: &rusqlite::Transaction<'_>,
    subject_id: i64,
    parent_id: Option<i64>,
) -> Result<i64> {
    Ok(tx.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM scheme_topics
          WHERE subject_id = ?1 AND parent_id IS ?2",
        (subject_id, parent_id),
        |row| row.get(0),
    )?)
}

/// Insert a topic at the end of its sibling group.
fn insert_topic(
    tx: &rusqlite::Transaction<'_>,
    subject_id: i64,
    parent_id: Option<i64>,
    title: &str,
    week: Option<i64>,
) -> Result<i64> {
    if let Some(week) = week {
        if !(1..=104).contains(&week) {
            return Err(CoreError::Invalid("Week must be between 1 and 104".into()));
        }
    }
    let position = next_position(tx, subject_id, parent_id)?;
    tx.execute(
        "INSERT INTO scheme_topics(subject_id, parent_id, title, week, position)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (subject_id, parent_id, title, week, position),
    )?;
    Ok(tx.last_insert_rowid())
}

fn forget_stamp(tx: &rusqlite::Transaction<'_>, id: i64) -> Result<()> {
    tx.execute(
        "UPDATE scheme_topics SET archived_at = NULL WHERE id = ?1 OR parent_id = ?1",
        [id],
    )?;
    Ok(())
}

fn stamp_archived(tx: &rusqlite::Transaction<'_>, id: i64) -> Result<()> {
    tx.execute(
        "UPDATE scheme_topics
            SET archived_at = COALESCE(archived_at, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
          WHERE id = ?1 OR parent_id = ?1",
        [id],
    )?;
    Ok(())
}

/// Apply the changes the student kept, in one transaction.
///
/// Renames keep the topic id, moves keep the topic id, and removals archive
/// rather than delete. A failure part-way through rolls everything back: an
/// outline update is one decision, not a series of half-applied ones.
pub fn apply_outline_update(
    db: &mut Db,
    subject_id: i64,
    decisions: &OutlineDecisions,
) -> Result<Vec<Topic>> {
    crate::subjects::get(db, subject_id)?;
    let mut added = 0i64;
    let mut renamed = 0i64;
    let mut moved = 0i64;
    let mut new_by_title: HashMap<String, i64> = HashMap::new();
    let mut consumed: HashSet<usize> = HashSet::new();
    let mut pair_renames: Vec<(i64, String)> = Vec::new();

    let tx = db.conn_mut().transaction()?;

    for pair in &decisions.paired {
        let Some(entry) = decisions.added.get(pair.added) else {
            return Err(CoreError::Invalid(
                "A paired row is not in the list of added topics".into(),
            ));
        };
        ensure_subject(&tx, pair.removed_id, subject_id)?;
        pair_renames.push((pair.removed_id, entry.title.clone()));
        consumed.insert(pair.added);
    }

    // New topics first, so a move can name one as its parent below.
    for (index, entry) in decisions.added.iter().enumerate() {
        if consumed.contains(&index) {
            continue;
        }
        let title = clean_name(&entry.title, "Topic title", MAX_TITLE)?;
        if let Some(parent_id) = entry.parent_id {
            ensure_subject(&tx, parent_id, subject_id)?;
            ensure_root(&tx, parent_id)?;
        }
        let id = insert_topic(&tx, subject_id, entry.parent_id, &title, entry.week)?;
        added += 1;
        new_by_title.entry(normalize(&title)).or_insert(id);
        for sub in &entry.subtopics {
            let sub_title = clean_name(sub, "Topic title", MAX_TITLE)?;
            let sub_id = insert_topic(&tx, subject_id, Some(id), &sub_title, None)?;
            added += 1;
            new_by_title.entry(normalize(&sub_title)).or_insert(sub_id);
        }
    }

    for id in &decisions.restored {
        ensure_subject(&tx, *id, subject_id)?;
        forget_stamp(&tx, *id)?;
    }

    for (id, title) in &pair_renames {
        let title = clean_name(title, "Topic title", MAX_TITLE)?;
        tx.execute(
            "UPDATE scheme_topics SET title = ?2 WHERE id = ?1",
            (*id, &title),
        )?;
        renamed += 1;
    }
    for rename in &decisions.renamed {
        ensure_subject(&tx, rename.id, subject_id)?;
        let title = clean_name(&rename.title, "Topic title", MAX_TITLE)?;
        tx.execute(
            "UPDATE scheme_topics SET title = ?2 WHERE id = ?1",
            (rename.id, &title),
        )?;
        renamed += 1;
    }

    for decision in &decisions.moved {
        ensure_subject(&tx, decision.id, subject_id)?;
        let (current_parent, current_week, current_position): (Option<i64>, Option<i64>, i64) =
            tx.query_row(
                "SELECT parent_id, week, position FROM scheme_topics WHERE id = ?1",
                [decision.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;

        let parent_id = if decision.to_root {
            if current_parent.is_none() {
                None
            } else {
                let children: i64 = tx.query_row(
                    "SELECT count(*) FROM scheme_topics WHERE parent_id = ?1",
                    [decision.id],
                    |row| row.get(0),
                )?;
                if children > 0 {
                    return Err(CoreError::Invalid(
                        "A topic with subtopics cannot become a subtopic".into(),
                    ));
                }
                None
            }
        } else if let Some(parent) = decision.to_parent {
            ensure_subject(&tx, parent, subject_id)?;
            ensure_root(&tx, parent)?;
            Some(parent)
        } else if let Some(title) = &decision.to_parent_title {
            match new_by_title.get(&normalize(title)) {
                Some(id) => Some(*id),
                None => {
                    return Err(CoreError::Invalid(format!(
                        "\"{title}\" is not part of this update"
                    )))
                }
            }
        } else {
            current_parent
        };

        let week = if decision.clear_week {
            None
        } else {
            decision.week.or(current_week)
        };
        let position = if parent_id != current_parent {
            next_position(&tx, subject_id, parent_id)?
        } else {
            current_position
        };
        if let Some(week) = week {
            if !(1..=104).contains(&week) {
                return Err(CoreError::Invalid("Week must be between 1 and 104".into()));
            }
        }
        tx.execute(
            "UPDATE scheme_topics SET parent_id = ?2, week = ?3, position = ?4 WHERE id = ?1",
            (decision.id, parent_id, week, position),
        )?;
        moved += 1;
    }

    for id in &decisions.archived {
        ensure_subject(&tx, *id, subject_id)?;
        stamp_archived(&tx, *id)?;
    }

    tx.execute(
        "INSERT INTO outline_updates(subject_id, added, removed, renamed, moved)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (subject_id, added, decisions.archived.len() as i64, renamed, moved),
    )?;
    tx.commit()?;

    topics::list_tree(db, subject_id)
}

// ---- history ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineUpdate {
    pub id: i64,
    pub created_at: String,
    pub added: i64,
    pub removed: i64,
    pub renamed: i64,
    pub moved: i64,
}

/// The last few outline updates for a subject, newest first.
pub fn history(db: &Db, subject_id: i64) -> Result<Vec<OutlineUpdate>> {
    crate::subjects::get(db, subject_id)?;
    let mut stmt = db.conn().prepare(
        "SELECT id, created_at, added, removed, renamed, moved
           FROM outline_updates
          WHERE subject_id = ?1
          ORDER BY created_at DESC, id DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map((subject_id, HISTORY_LIMIT), |row| {
        Ok(OutlineUpdate {
            id: row.get(0)?,
            created_at: row.get(1)?,
            added: row.get(2)?,
            removed: row.get(3)?,
            renamed: row.get(4)?,
            moved: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, DEV_KEY};
    use crate::models::NewTopic;

    fn db() -> Db {
        Db::open_in_memory(DEV_KEY).expect("open in-memory db")
    }

    fn subject(db: &Db) -> i64 {
        crate::subjects::create(db, "MTH 101").expect("subject").id
    }

    fn topic(db: &Db, subject_id: i64, title: &str, week: Option<i64>, parent: Option<i64>) -> i64 {
        topics::create(
            db,
            &NewTopic {
                subject_id,
                parent_id: parent,
                title: title.to_string(),
                week,
                is_off_scheme: false,
            },
        )
        .expect("topic")
        .id
    }

    fn entry(title: &str, week: Option<i64>, subtopics: &[&str]) -> SchemeEntry {
        SchemeEntry {
            title: title.to_string(),
            week,
            subtopics: subtopics.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn normalize_ignores_numbering_bullets_and_spacing() {
        assert_eq!(normalize(" 1. Cell   Division "), "cell division");
        assert_eq!(normalize("2) Cell division"), "cell division");
        assert_eq!(normalize("Week 3: Cell division"), "cell division");
        assert_eq!(normalize("- Cell division"), "cell division");
        assert_eq!(normalize("• Cell division"), "cell division");
        assert_eq!(normalize("3.1 Cell division"), "cell division");
        assert_eq!(
            normalize("2d vectors"),
            "2d vectors",
            "a number that is part of the title stays"
        );
        assert_eq!(normalize("   "), "");
        assert_eq!(
            normalize("10.2 Cell division"),
            "cell division",
            "each leading enumerator is stripped in turn"
        );
    }

    #[test]
    fn unchanged_topics_are_reported_and_nothing_else() {
        let db = db();
        let subject_id = subject(&db);
        let id = topic(&db, subject_id, "Cell division", Some(2), None);

        let incoming = [entry("Cell division", Some(2), &[])];
        let diff = diff_outline(&db, subject_id, &incoming).unwrap();
        assert_eq!(diff.unchanged.len(), 1);
        assert_eq!(diff.unchanged[0].id, id);
        assert!(diff.renamed.is_empty());
        assert!(diff.moved.is_empty());
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.restored.is_empty());
    }

    #[test]
    fn a_typo_in_the_new_title_is_a_rename() {
        let db = db();
        let subject_id = subject(&db);
        let id = topic(&db, subject_id, "Laws of logarithms", None, None);

        let incoming = [entry("Laws of logarithm", None, &[])];
        let diff = diff_outline(&db, subject_id, &incoming).unwrap();
        assert_eq!(diff.renamed.len(), 1);
        assert_eq!(diff.renamed[0].id, id);
        assert_eq!(diff.renamed[0].old, "Laws of logarithms");
        assert_eq!(diff.renamed[0].new, "Laws of logarithm");
        assert!(diff.added.is_empty() && diff.removed.is_empty());
    }

    #[test]
    fn a_new_week_is_a_move() {
        let db = db();
        let subject_id = subject(&db);
        let id = topic(&db, subject_id, "Sets", Some(1), None);

        let incoming = [entry("Sets", Some(4), &[])];
        let diff = diff_outline(&db, subject_id, &incoming).unwrap();
        assert_eq!(diff.moved.len(), 1);
        assert_eq!(diff.moved[0].id, id);
        assert_eq!(diff.moved[0].from_week, Some(1));
        assert_eq!(diff.moved[0].to_week, Some(4));
        assert!(diff.unchanged.is_empty());
    }

    #[test]
    fn new_topics_are_added_with_their_subtopics() {
        let db = db();
        let subject_id = subject(&db);
        topic(&db, subject_id, "Sets", Some(1), None);

        let diff = diff_outline(
            &db,
            subject_id,
            &[
                entry("Sets", Some(1), &[]),
                entry("Logarithms", Some(3), &["Laws of logs", "Change of base"]),
            ],
        )
        .unwrap();
        assert_eq!(diff.unchanged.len(), 1);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].title, "Logarithms");
        assert_eq!(diff.added[0].week, Some(3));
        assert_eq!(diff.added[0].subtopics, vec!["Laws of logs", "Change of base"]);
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn dropped_topics_are_removed_not_deleted() {
        let db = db();
        let subject_id = subject(&db);
        topic(&db, subject_id, "Sets", None, None);
        let dropped = topic(&db, subject_id, "Trigonometry", None, None);
        topic(&db, subject_id, "Sine rule", None, Some(dropped));

        let diff = diff_outline(&db, subject_id, &[entry("Sets", None, &[])]).unwrap();
        assert_eq!(diff.removed.len(), 1, "the child is archived with its parent");
        assert_eq!(diff.removed[0].id, dropped);
        assert_eq!(diff.removed[0].title, "Trigonometry");
        let archived = topics::get(&db, dropped).unwrap();
        assert!(
            archived.archived_at.is_none(),
            "the diff only proposes: nothing is archived until applied"
        );
    }

    #[test]
    fn an_archived_topic_comes_back_as_restored() {
        let db = db();
        let subject_id = subject(&db);
        let id = topic(&db, subject_id, "Sets", None, None);
        topics::archive(&db, id).unwrap();
        assert!(topics::list_flat(&db, subject_id).unwrap().is_empty());

        let diff = diff_outline(&db, subject_id, &[entry("Sets", None, &[])]).unwrap();
        assert_eq!(diff.restored.len(), 1);
        assert_eq!(diff.restored[0].id, id);
        assert!(diff.added.is_empty(), "restoring must not create a second copy");
    }

    #[test]
    fn duplicate_titles_never_share_one_topic() {
        let db = db();
        let subject_id = subject(&db);
        let first = topic(&db, subject_id, "Biology", None, None);

        // Two incoming rows with one stored topic: the first is the match,
        // the second is genuinely new.
        let diff = diff_outline(
            &db,
            subject_id,
            &[entry("Biology", None, &[]), entry("biology", None, &[])],
        )
        .unwrap();
        assert_eq!(diff.unchanged.len(), 1);
        assert_eq!(diff.unchanged[0].id, first);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].title, "biology");

        // One incoming row with two stored duplicates: the other is removed.
        topic(&db, subject_id, "Biology", None, None);
        let diff = diff_outline(&db, subject_id, &[entry("Biology", None, &[])]).unwrap();
        assert_eq!(diff.unchanged.len(), 1);
        assert_eq!(diff.removed.len(), 1);
    }

    #[test]
    fn a_subtopic_moved_to_another_parent_is_a_move() {
        let db = db();
        let subject_id = subject(&db);
        let cells = topic(&db, subject_id, "Cells", None, None);
        let genetics = topic(&db, subject_id, "Genetics", None, None);
        let nucleus = topic(&db, subject_id, "Nucleus", None, Some(cells));

        let diff = diff_outline(
            &db,
            subject_id,
            &[
                entry("Cells", None, &[]),
                entry("Genetics", None, &["Nucleus"]),
            ],
        )
        .unwrap();
        assert_eq!(diff.moved.len(), 1);
        assert_eq!(diff.moved[0].id, nucleus);
        assert_eq!(diff.moved[0].from_parent, Some(cells));
        assert_eq!(diff.moved[0].to_parent, Some(genetics));
        assert!(diff.added.is_empty(), "the subtopic must not be recreated");
        assert!(diff.removed.is_empty());
    }

    #[test]
    fn an_empty_import_proposes_nothing() {
        let db = db();
        let subject_id = subject(&db);
        topic(&db, subject_id, "Sets", None, None);

        let diff = diff_outline(&db, subject_id, &[]).unwrap();
        assert!(diff.is_empty(), "an empty import is not a request to archive");
        assert_eq!(diff.removed.len(), 0);
    }

    #[test]
    fn the_same_outlines_always_give_the_same_diff() {
        let db = db();
        let subject_id = subject(&db);
        topic(&db, subject_id, "Sets", Some(1), None);
        topic(&db, subject_id, "Logarithms", None, None);
        topic(&db, subject_id, "Cells", None, None);
        let incoming = [
            entry("2) Cell", None, &[]),
            entry("Sets", Some(2), &[]),
            entry("Vectors", Some(5), &["Addition"]),
        ];
        let first = diff_outline(&db, subject_id, &incoming).unwrap();
        let second = diff_outline(&db, subject_id, &incoming).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn applying_the_diff_keeps_ids_and_archives_removals() {
        let mut db = db();
        let subject_id = subject(&db);
        let sets = topic(&db, subject_id, "Sets", Some(1), None);
        let old = topic(&db, subject_id, "Old topic", None, None);

        let decisions = OutlineDecisions {
            added: vec![AddDecision {
                title: "Vectors".into(),
                week: Some(2),
                parent_id: None,
                subtopics: vec!["Addition".into()],
            }],
            renamed: vec![RenameDecision { id: sets, title: "Sets and relations".into() }],
            moved: vec![],
            archived: vec![old],
            restored: vec![],
            paired: vec![],
        };
        let tree = apply_outline_update(&mut db, subject_id, &decisions).unwrap();
        let titles: Vec<&str> = tree.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["Sets and relations", "Vectors"]);
        assert_eq!(tree[0].id, sets, "a rename keeps the id");
        assert_eq!(tree[1].children.len(), 1);
        assert_eq!(tree[1].children[0].title, "Addition");

        let archived = topics::list_archived(&db, subject_id).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, old);

        let history = history(&db, subject_id).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].added, 2, "the topic and its subtopic");
        assert_eq!(history[0].removed, 1);
        assert_eq!(history[0].renamed, 1);
        assert_eq!(history[0].moved, 0);
    }

    #[test]
    fn a_failed_apply_rolls_everything_back() {
        let mut db = db();
        let subject_id = subject(&db);
        let sets = topic(&db, subject_id, "Sets", None, None);

        // The first rename is valid; the move names a topic that is not there.
        // Nothing may survive the failure.
        let decisions = OutlineDecisions {
            renamed: vec![RenameDecision { id: sets, title: "Renamed".into() }],
            moved: vec![MoveDecision {
                id: 99_999,
                week: Some(2),
                clear_week: false,
                to_parent: None,
                to_parent_title: None,
                to_root: false,
            }],
            ..Default::default()
        };
        assert!(apply_outline_update(&mut db, subject_id, &decisions).is_err());
        assert_eq!(topics::get(&db, sets).unwrap().title, "Sets");
        assert!(history(&db, subject_id).unwrap().is_empty());
    }

    #[test]
    fn a_pairing_renames_instead_of_archiving_and_adding() {
        let mut db = db();
        let subject_id = subject(&db);
        let old = topic(&db, subject_id, "Old name", None, None);

        let decisions = OutlineDecisions {
            added: vec![AddDecision {
                title: "New name".into(),
                week: None,
                parent_id: None,
                subtopics: vec![],
            }],
            paired: vec![PairedDecision { removed_id: old, added: 0 }],
            ..Default::default()
        };
        let tree = apply_outline_update(&mut db, subject_id, &decisions).unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].id, old);
        assert_eq!(tree[0].title, "New name");
        assert!(topics::list_archived(&db, subject_id).unwrap().is_empty());
    }
}
