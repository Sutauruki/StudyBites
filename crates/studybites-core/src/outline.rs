//! Outline updates (roadmap item 5).
//!
//! When the lecturer comes back with a changed scheme of work, the student
//! re-imports or edits it and sees exactly what was added, removed or renamed
//! before anything is written. Three rules hold throughout:
//!
//! 1. **A topic id never changes.** Cards, notes and materials point at topic
//!    ids, so re-creating topics would orphan the student's progress.
//! 2. **Nothing is hard-deleted.** A topic that left the outline is archived;
//!    its notes and cards stay exactly where they are, just out of the way.
//! 3. **Matching is deterministic and offline.** No AI call: titles are
//!    normalised, compared exactly, then by token-set similarity with an edit
//!    distance for near-misses. The same outline always produces the same
//!    diff, which is what makes the Changes screen reviewable.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::ai::SchemeEntry;
use crate::db::Db;
use crate::error::{clean_name, CoreError, Result};
use crate::models::Topic;

const MAX_TITLE: usize = 200;
/// Token-set Jaccard score at or above which two titles are the same topic.
const SIMILARITY: f64 = 0.8;
/// A short title can still be a rename with one or two typos.
const MAX_EDIT_DISTANCE: usize = 2;
const MIN_EDIT_LEN: usize = 6;

/// A topic as the diff refers to it.
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
pub struct RenamedTopic {
    pub id: i64,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedTopic {
    pub id: i64,
    pub from_week: Option<i64>,
    pub to_week: Option<i64>,
    pub to_parent_id: Option<i64>,
}

/// A topic the incoming outline brings that the subject does not have yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedTopic {
    pub title: String,
    pub week: Option<i64>,
    /// Existing parent, when the parent is already in the outline.
    #[serde(default)]
    pub parent_id: Option<i64>,
    /// Index in `added` of the new parent, when both are new. Parents are
    /// always listed before their subtopics.
    #[serde(default)]
    pub parent_index: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineDiff {
    pub unchanged: Vec<TopicRef>,
    pub renamed: Vec<RenamedTopic>,
    pub moved: Vec<MovedTopic>,
    pub added: Vec<AddedTopic>,
    pub removed: Vec<TopicRef>,
    /// Topics that come back from the archived list because the new outline
    /// names them again.
    pub restored: Vec<TopicRef>,
}

/// A manual pairing from the Changes screen: the student says this archived
/// topic is really the same topic as that added entry, so its progress stays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub removed_id: i64,
    pub added_index: usize,
}

/// What the student accepted on the Changes screen.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineDecisions {
    #[serde(default)]
    pub renamed: Vec<RenamedTopic>,
    #[serde(default)]
    pub moved: Vec<MovedTopic>,
    #[serde(default)]
    pub added: Vec<AddedTopic>,
    #[serde(default)]
    pub removed: Vec<i64>,
    #[serde(default)]
    pub restored: Vec<i64>,
    #[serde(default)]
    pub pairings: Vec<Pairing>,
}

/// One recorded update, for the short history list.
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

// ---- matching -----------------------------------------------------------

/// Lower-case, trim, drop leading numbering or bullets, collapse spaces.
///
/// "Week 3: 1. Logarithms" and "logarithms" both become "logarithms", which
/// is what makes a re-import of the same outline a no-op.
pub fn normalize(title: &str) -> String {
    let mut text = title.trim().to_lowercase();
    loop {
        let trimmed = text.trim_start();
        let stripped = strip_prefix(trimmed);
        if stripped == trimmed {
            break;
        }
        text = stripped.to_string();
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip one leading bullet, "week 3" marker, or "1." / "2)" / "3:" number.
fn strip_prefix(text: &str) -> &str {
    if let Some(rest) = text.strip_prefix(['-', '•', '*', '·', '–', '—']) {
        return rest.trim_start();
    }
    // "week 3" and "week 3:".
    if let Some(rest) = text.strip_prefix("week") {
        let rest = rest.trim_start();
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            let after = &rest[digits..];
            let after = after.strip_prefix(':').unwrap_or(after);
            return after.trim_start();
        }
    }
    // "1.", "2)" or "3:" — only when it reads as a number, not a decimal.
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let after = &text[digits..];
        if let Some(after) = after.strip_prefix(')').or_else(|| after.strip_prefix(':')) {
            return after.trim_start();
        }
        if let Some(after) = after.strip_prefix('.') {
            // "3.5 Laws" is a number, not a list item.
            if after.starts_with(char::is_whitespace) {
                return after.trim_start();
            }
        }
    }
    text
}

fn token_set(normalized: &str) -> HashSet<&str> {
    normalized.split(' ').filter(|t| !t.is_empty()).collect()
}

fn jaccard(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let (left, right) = (token_set(a), token_set(b));
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let shared = left.intersection(&right).count() as f64;
    let union = left.union(&right).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        shared / union
    }
}

/// Levenshtein distance over characters, giving up early once it is too large
/// to matter.
fn edit_distance(a: &str, b: &str) -> usize {
    let left: Vec<char> = a.chars().collect();
    let right: Vec<char> = b.chars().collect();
    if left.len().abs_diff(right.len()) > MAX_EDIT_DISTANCE {
        return MAX_EDIT_DISTANCE + 1;
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (i, lc) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, rc) in right.iter().enumerate() {
            let cost = usize::from(lc != rc);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// Is the incoming title the same topic as the existing one, spelled
/// differently?
fn similar(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if jaccard(a, b) >= SIMILARITY {
        return true;
    }
    // "the laws of logarithms" is the same topic as "laws of logarithms":
    // one set of words contains the other and is not a single word.
    let (left, right) = (token_set(a), token_set(b));
    let (smaller, larger) = if left.len() <= right.len() {
        (&left, &right)
    } else {
        (&right, &left)
    };
    if smaller.len() >= 2 && smaller.is_subset(larger) {
        return true;
    }
    let shortest = a.chars().count().min(b.chars().count());
    shortest >= MIN_EDIT_LEN && edit_distance(a, b) <= MAX_EDIT_DISTANCE
}

fn topic_ref(topic: &Topic) -> TopicRef {
    TopicRef {
        id: topic.id,
        title: topic.title.clone(),
        week: topic.week,
        parent_id: topic.parent_id,
    }
}

/// How many of this entry's subtopics already exist under `parent_id`. Used
/// only to break ties between two similar parent titles.
fn subtopic_overlap(existing: &[Topic], parent_id: i64, entry: &SchemeEntry) -> usize {
    let children: Vec<String> = existing
        .iter()
        .filter(|t| t.parent_id == Some(parent_id))
        .map(|t| normalize(&t.title))
        .collect();
    entry
        .subtopics
        .iter()
        .filter(|sub| !sub.trim().is_empty())
        .filter(|sub| children.contains(&normalize(sub)))
        .count()
}

/// Work out what changed, without writing anything.
pub fn diff_outline(db: &Db, subject_id: i64, incoming: &[SchemeEntry]) -> Result<OutlineDiff> {
    let existing = crate::topics::list_all(db, subject_id)?;
    let mut diff = OutlineDiff::default();

    // Topic ids already claimed by an incoming entry, so two entries can never
    // point at the same topic.
    let mut used: HashSet<i64> = HashSet::new();
    let mut matched_parents: Vec<Option<i64>> = vec![None; incoming.len()];
    let parents: Vec<&Topic> = existing.iter().filter(|t| t.parent_id.is_none()).collect();

    // Pass 1: exact match on the normalised title.
    for (index, entry) in incoming.iter().enumerate() {
        if entry.title.trim().is_empty() {
            continue;
        }
        let wanted = normalize(&entry.title);
        if let Some(found) = parents
            .iter()
            .copied()
            .filter(|p| !used.contains(&p.id) && normalize(&p.title) == wanted)
            .min_by_key(|p| p.id)
        {
            used.insert(found.id);
            matched_parents[index] = Some(found.id);
        }
    }

    // Pass 2: near-misses, best match first, subtopic overlap breaking ties.
    for (index, entry) in incoming.iter().enumerate() {
        if entry.title.trim().is_empty() || matched_parents[index].is_some() {
            continue;
        }
        let wanted = normalize(&entry.title);
        let mut best: Option<(&Topic, usize, f64)> = None;
        for candidate in parents.iter().copied().filter(|p| !used.contains(&p.id)) {
            let other = normalize(&candidate.title);
            if !similar(&wanted, &other) {
                continue;
            }
            let overlap = subtopic_overlap(&existing, candidate.id, entry);
            let score = jaccard(&wanted, &other);
            let better = match best {
                None => true,
                Some((_, best_overlap, best_score)) => (overlap, score) > (best_overlap, best_score),
            };
            if better {
                best = Some((candidate, overlap, score));
            }
        }
        if let Some((found, _, _)) = best {
            used.insert(found.id);
            matched_parents[index] = Some(found.id);
        }
    }

    // Matched parents: renamed, moved or unchanged.
    for (index, entry) in incoming.iter().enumerate() {
        let Some(id) = matched_parents[index] else {
            continue;
        };
        let old = existing
            .iter()
            .find(|t| t.id == id)
            .expect("a matched topic exists");
        classify_topic(&mut diff, old, entry);
    }

    // Added parents come first, so their subtopics can point at them.
    let mut added_parent_index: Vec<Option<usize>> = vec![None; incoming.len()];
    for (index, entry) in incoming.iter().enumerate() {
        if entry.title.trim().is_empty() || matched_parents[index].is_some() {
            continue;
        }
        diff.added.push(AddedTopic {
            title: entry.title.trim().to_string(),
            week: entry.week,
            parent_id: None,
            parent_index: None,
        });
        added_parent_index[index] = Some(diff.added.len() - 1);
    }

    // Subtopics of a matched parent: matched, renamed or moved inside it.
    // Subtopics of an added parent are simply added.
    let mut leftovers: Vec<(i64, String)> = Vec::new();
    for (index, entry) in incoming.iter().enumerate() {
        let subs: Vec<&String> = entry
            .subtopics
            .iter()
            .filter(|sub| !sub.trim().is_empty())
            .collect();
        if subs.is_empty() {
            continue;
        }
        if let Some(added_index) = added_parent_index[index] {
            for sub in subs {
                diff.added.push(AddedTopic {
                    title: sub.trim().to_string(),
                    week: None,
                    parent_id: None,
                    parent_index: Some(added_index),
                });
            }
            continue;
        }
        let Some(parent_id) = matched_parents[index] else {
            continue;
        };
        let children: Vec<&Topic> = existing
            .iter()
            .filter(|t| t.parent_id == Some(parent_id) && !used.contains(&t.id))
            .collect();

        let mut matched: Vec<Option<i64>> = vec![None; subs.len()];
        for (slot, sub) in subs.iter().enumerate() {
            let wanted = normalize(sub);
            if let Some(found) = children
                .iter()
                .copied()
                .filter(|c| !used.contains(&c.id) && normalize(&c.title) == wanted)
                .min_by_key(|c| c.id)
            {
                used.insert(found.id);
                matched[slot] = Some(found.id);
            }
        }
        for (slot, sub) in subs.iter().enumerate() {
            if matched[slot].is_some() {
                continue;
            }
            let wanted = normalize(sub);
            let mut best: Option<(&Topic, f64)> = None;
            for candidate in children.iter().copied().filter(|c| !used.contains(&c.id)) {
                let other = normalize(&candidate.title);
                if !similar(&wanted, &other) {
                    continue;
                }
                let score = jaccard(&wanted, &other);
                if best.map_or(true, |(_, best_score)| score > best_score) {
                    best = Some((candidate, score));
                }
            }
            if let Some((found, _)) = best {
                used.insert(found.id);
                matched[slot] = Some(found.id);
            }
        }
        for (slot, sub) in subs.iter().enumerate() {
            match matched[slot] {
                Some(id) => {
                    let old = existing
                        .iter()
                        .find(|t| t.id == id)
                        .expect("a matched topic exists");
                    classify_subtopic(&mut diff, old, sub.trim(), parent_id);
                }
                None => leftovers.push((parent_id, sub.trim().to_string())),
            }
        }
    }

    // Leftover subtopics may have moved under another topic that still exists.
    for (parent_id, title) in leftovers {
        let wanted = normalize(&title);
        let candidates: Vec<&Topic> = existing
            .iter()
            .filter(|t| t.parent_id.is_some() && !used.contains(&t.id))
            .collect();
        let exact = candidates
            .iter()
            .filter(|c| normalize(&c.title) == wanted)
            .min_by_key(|c| c.id)
            .copied();
        let found = exact.or_else(|| {
            let mut best: Option<(&Topic, f64)> = None;
            for candidate in candidates.iter().copied() {
                let other = normalize(&candidate.title);
                if !similar(&wanted, &other) {
                    continue;
                }
                let score = jaccard(&wanted, &other);
                if best.map_or(true, |(_, best_score)| score > best_score) {
                    best = Some((candidate, score));
                }
            }
            best.map(|(topic, _)| topic)
        });
        match found {
            Some(old) => {
                used.insert(old.id);
                classify_subtopic(&mut diff, old, &title, parent_id);
            }
            None => diff.added.push(AddedTopic {
                title,
                week: None,
                parent_id: Some(parent_id),
                parent_index: None,
            }),
        }
    }

    // Whatever the new outline did not claim is archived. A removed parent
    // takes its subtopics with it, so they are not listed one by one.
    for topic in existing.iter().filter(|t| t.parent_id.is_none()) {
        if used.contains(&topic.id) || topic.archived_at.is_some() {
            continue;
        }
        diff.removed.push(topic_ref(topic));
        for child in existing.iter().filter(|c| c.parent_id == Some(topic.id)) {
            used.insert(child.id);
        }
    }
    for topic in existing.iter().filter(|t| t.parent_id.is_some()) {
        if used.contains(&topic.id) || topic.archived_at.is_some() {
            continue;
        }
        diff.removed.push(topic_ref(topic));
    }

    Ok(diff)
}

fn classify_topic(diff: &mut OutlineDiff, old: &Topic, entry: &SchemeEntry) {
    if old.archived_at.is_some() {
        // Already archived: bringing it back is all this update does to it.
        diff.restored.push(topic_ref(old));
        return;
    }
    let new_title = entry.title.trim();
    let mut changed = false;
    if normalize(new_title) != normalize(&old.title) {
        diff.renamed.push(RenamedTopic {
            id: old.id,
            old: old.title.clone(),
            new: new_title.to_string(),
        });
        changed = true;
    }
    if old.week != entry.week {
        diff.moved.push(MovedTopic {
            id: old.id,
            from_week: old.week,
            to_week: entry.week,
            to_parent_id: old.parent_id,
        });
        changed = true;
    }
    if !changed {
        diff.unchanged.push(topic_ref(old));
    }
}

fn classify_subtopic(diff: &mut OutlineDiff, old: &Topic, new_title: &str, parent_id: i64) {
    if old.archived_at.is_some() {
        diff.restored.push(topic_ref(old));
        return;
    }
    let mut changed = false;
    if normalize(new_title) != normalize(&old.title) {
        diff.renamed.push(RenamedTopic {
            id: old.id,
            old: old.title.clone(),
            new: new_title.to_string(),
        });
        changed = true;
    }
    if old.parent_id != Some(parent_id) {
        // A subtopic changing parent keeps its week: the update says nothing
        // about it.
        diff.moved.push(MovedTopic {
            id: old.id,
            from_week: old.week,
            to_week: old.week,
            to_parent_id: Some(parent_id),
        });
        changed = true;
    }
    if !changed {
        diff.unchanged.push(topic_ref(old));
    }
}

// ---- applying -----------------------------------------------------------

/// Apply the accepted parts of a diff in one transaction.
///
/// A failure anywhere leaves the outline exactly as it was, which matters: a
/// half-applied update would leave the student's topics in a state neither
/// they nor the app could reason about.
pub fn apply_outline_update(
    db: &mut Db,
    subject_id: i64,
    decisions: &OutlineDecisions,
) -> Result<Vec<Topic>> {
    crate::subjects::get(db, subject_id)?;

    let mut added_count: i64 = 0;
    let mut removed_count: i64 = 0;
    let mut renamed_count: i64 = 0;
    let mut moved_count: i64 = 0;

    let tx = db.conn_mut().transaction()?;

    // 1. Manual pairings are renames: the student said these are the same
    //    topic, so the old id (and its cards) survives under the new title.
    let mut consumed = vec![false; decisions.added.len()];
    for pairing in &decisions.pairings {
        if pairing.added_index >= decisions.added.len() {
            return Err(CoreError::Invalid(
                "A pairing points at a topic that is not in this update".into(),
            ));
        }
        let title = clean_name(
            &decisions.added[pairing.added_index].title,
            "Topic title",
            MAX_TITLE,
        )?;
        let changed = tx.execute(
            "UPDATE scheme_topics SET title = ?2 WHERE id = ?1 AND subject_id = ?3",
            (pairing.removed_id, &title, subject_id),
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound {
                kind: "topic",
                id: pairing.removed_id,
            });
        }
        consumed[pairing.added_index] = true;
        renamed_count += 1;
    }

    // 2. Renames.
    for rename in &decisions.renamed {
        let title = clean_name(&rename.new, "Topic title", MAX_TITLE)?;
        let changed = tx.execute(
            "UPDATE scheme_topics SET title = ?2 WHERE id = ?1 AND subject_id = ?3",
            (rename.id, &title, subject_id),
        )?;
        if changed == 0 {
            return Err(CoreError::NotFound {
                kind: "topic",
                id: rename.id,
            });
        }
        renamed_count += 1;
    }

    // 3. Moves: a new week, a new parent, or both.
    for moved in &decisions.moved {
        let old_parent = parent_of(&tx, subject_id, moved.id)?;
        if let Some(week) = moved.to_week {
            if !(1..=104).contains(&week) {
                return Err(CoreError::Invalid("Week must be between 1 and 104".into()));
            }
        }
        if let Some(parent_id) = moved.to_parent_id {
            if parent_id == moved.id {
                return Err(CoreError::Invalid("A topic cannot be its own parent".into()));
            }
            let parent_parent = parent_of(&tx, subject_id, parent_id)?;
            if parent_parent.is_some() {
                return Err(CoreError::Invalid(
                    "Topics can only be nested one level deep".into(),
                ));
            }
        }
        tx.execute(
            "UPDATE scheme_topics SET week = ?2 WHERE id = ?1",
            (moved.id, moved.to_week),
        )?;
        // Only a change of parent moves the row: a week change alone must not
        // shuffle the topic to the end of its group.
        if old_parent != moved.to_parent_id {
            tx.execute(
                "UPDATE scheme_topics SET parent_id = ?2 WHERE id = ?1",
                (moved.id, moved.to_parent_id),
            )?;
            let position: i64 = tx.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM scheme_topics
                  WHERE subject_id = ?1 AND parent_id IS ?2",
                (subject_id, moved.to_parent_id),
                |r| r.get(0),
            )?;
            tx.execute(
                "UPDATE scheme_topics SET position = ?2 WHERE id = ?1",
                (moved.id, position),
            )?;
            compact_positions(&tx, subject_id, old_parent)?;
        }
        moved_count += 1;
    }

    // 4. Added topics. The diff lists a parent before its subtopics.
    let mut created: HashMap<usize, i64> = HashMap::new();
    for (index, entry) in decisions.added.iter().enumerate() {
        if consumed[index] {
            continue;
        }
        let parent_id = match entry.parent_index {
            Some(parent_index) => match created.get(&parent_index) {
                Some(id) => Some(*id),
                None => {
                    return Err(CoreError::Invalid(
                        "The update lists a subtopic before its parent".into(),
                    ))
                }
            },
            None => entry.parent_id,
        };
        if let Some(parent_id) = parent_id {
            let parent_parent = parent_of(&tx, subject_id, parent_id)?;
            if parent_parent.is_some() {
                return Err(CoreError::Invalid(
                    "Topics can only be nested one level deep".into(),
                ));
            }
        }
        let id = insert_topic(&tx, subject_id, parent_id, &entry.title, entry.week)?;
        created.insert(index, id);
        added_count += 1;
    }

    // 5. Removed topics are archived, with their subtopics. Their cards stay.
    //    A paired topic is not removed: it was renamed into the new entry.
    let paired: HashSet<i64> = decisions.pairings.iter().map(|p| p.removed_id).collect();
    for id in &decisions.removed {
        if paired.contains(id) {
            continue;
        }
        let changed = tx.execute(
            "UPDATE scheme_topics
                SET archived_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
              WHERE (id = ?1 OR parent_id = ?1) AND subject_id = ?2
                AND archived_at IS NULL",
            (id, subject_id),
        )?;
        if changed == 0 {
            // Already archived is fine; missing is not.
            parent_of(&tx, subject_id, *id)?;
        }
        removed_count += 1;
    }

    // 6. Restored topics come back where they were, with their subtopics.
    for id in &decisions.restored {
        let changed = tx.execute(
            "UPDATE scheme_topics
                SET archived_at = NULL
              WHERE (id = ?1 OR parent_id = ?1) AND subject_id = ?2
                AND archived_at IS NOT NULL",
            (id, subject_id),
        )?;
        if changed == 0 {
            parent_of(&tx, subject_id, *id)?;
        }
    }

    // 7. One history row for the whole update.
    tx.execute(
        "INSERT INTO outline_updates(subject_id, added, removed, renamed, moved)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (subject_id, added_count, removed_count, renamed_count, moved_count),
    )?;
    tx.commit()?;

    crate::topics::list_tree(db, subject_id)
}

/// The recorded updates for a subject, newest first.
pub fn history(db: &Db, subject_id: i64, limit: i64) -> Result<Vec<OutlineUpdate>> {
    let mut stmt = db.conn().prepare(
        "SELECT id, created_at, added, removed, renamed, moved
           FROM outline_updates
          WHERE subject_id = ?1
          ORDER BY created_at DESC, id DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map((subject_id, limit.clamp(1, 50)), |row| {
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

/// The parent of a topic in this subject, or `NotFound` when the topic is not
/// the subject's. Used for existence checks inside the transaction.
fn parent_of(
    tx: &rusqlite::Transaction<'_>,
    subject_id: i64,
    id: i64,
) -> Result<Option<i64>> {
    let found: Option<(i64, Option<i64>)> = tx
        .query_row(
            "SELECT subject_id, parent_id FROM scheme_topics WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    match found {
        Some((owner, parent_id)) if owner == subject_id => Ok(parent_id),
        _ => Err(CoreError::NotFound { kind: "topic", id }),
    }
}

fn insert_topic(
    tx: &rusqlite::Transaction<'_>,
    subject_id: i64,
    parent_id: Option<i64>,
    title: &str,
    week: Option<i64>,
) -> Result<i64> {
    let title = clean_name(title, "Topic title", MAX_TITLE)?;
    if let Some(week) = week {
        if !(1..=104).contains(&week) {
            return Err(CoreError::Invalid("Week must be between 1 and 104".into()));
        }
    }
    let position: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM scheme_topics
          WHERE subject_id = ?1 AND parent_id IS ?2",
        (subject_id, parent_id),
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO scheme_topics
            (subject_id, parent_id, title, week, position, is_off_scheme)
         VALUES (?1, ?2, ?3, ?4, ?5, 0)",
        (subject_id, parent_id, &title, week, position),
    )?;
    Ok(tx.last_insert_rowid())
}

fn compact_positions(
    tx: &rusqlite::Transaction<'_>,
    subject_id: i64,
    parent_id: Option<i64>,
) -> Result<()> {
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM scheme_topics
              WHERE subject_id = ?1 AND parent_id IS ?2
              ORDER BY position, id",
        )?;
        let rows = stmt.query_map((subject_id, parent_id), |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (position, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE scheme_topics SET position = ?2 WHERE id = ?1",
            (id, position as i64),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, week: Option<i64>, subs: &[&str]) -> SchemeEntry {
        SchemeEntry {
            title: title.into(),
            week,
            subtopics: subs.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn normalisation_ignores_numbering_case_and_spacing() {
        assert_eq!(normalize("  1. Logarithms "), "logarithms");
        assert_eq!(normalize("2) LOGARITHMS"), "logarithms");
        assert_eq!(normalize("Week 3: Logarithms"), "logarithms");
        assert_eq!(normalize("- Logarithms"), "logarithms");
        assert_eq!(normalize("3.5 Laws"), "3.5 laws", "a decimal is not a list");
        assert_eq!(normalize("Data   Structures"), "data structures");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn similarity_matches_typos_and_word_order_but_not_unrelated_titles() {
        assert!(similar("logarithms", "logarithm"));
        assert!(similar("data structures", "structures data"));
        assert!(similar("the laws of logarithms", "laws of logarithms"));
        assert!(!similar("logarithms", "vectors"));
        assert!(!similar("set theory", "set"));
    }

    #[test]
    fn edit_distance_is_small_and_bounded() {
        assert_eq!(edit_distance("logarithm", "logarithms"), 1);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert!(edit_distance("short", "a much longer title") > MAX_EDIT_DISTANCE);
    }

    #[test]
    fn identical_outline_produces_no_changes() {
        let db = crate::db::Db::open_in_memory(crate::db::DEV_KEY).unwrap();
        let subject = crate::subjects::create(&db, "MTH 101").unwrap();
        crate::ingest::save_scheme(
            &db,
            subject.id,
            &[entry("Logarithms", Some(1), &["Product rule"])],
        )
        .unwrap();

        let diff = diff_outline(&db, subject.id, &[entry("1. logarithms", Some(1), &["product RULE"])])
            .unwrap();
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.renamed.is_empty());
        assert!(diff.moved.is_empty());
        assert_eq!(diff.unchanged.len(), 2, "the topic and its subtopic");
    }

    #[test]
    fn an_empty_incoming_outline_archives_everything() {
        let db = crate::db::Db::open_in_memory(crate::db::DEV_KEY).unwrap();
        let subject = crate::subjects::create(&db, "MTH 101").unwrap();
        crate::ingest::save_scheme(
            &db,
            subject.id,
            &[entry("Logarithms", Some(1), &["Product rule", "Quotient rule"])],
        )
        .unwrap();

        let diff = diff_outline(&db, subject.id, &[]).unwrap();
        assert_eq!(diff.removed.len(), 1, "the parent, not each child as well");
        assert!(diff.removed[0].parent_id.is_none());
        assert!(diff.added.is_empty());
    }

    #[test]
    fn duplicate_incoming_titles_never_claim_one_topic_twice() {
        let db = crate::db::Db::open_in_memory(crate::db::DEV_KEY).unwrap();
        let subject = crate::subjects::create(&db, "MTH 101").unwrap();
        crate::ingest::save_scheme(&db, subject.id, &[entry("Logarithms", None, &[])]).unwrap();

        let diff = diff_outline(
            &db,
            subject.id,
            &[entry("Logarithms", None, &[]), entry("Logarithms", None, &[])],
        )
        .unwrap();
        assert_eq!(diff.unchanged.len(), 1);
        assert_eq!(diff.added.len(), 1, "the second one is genuinely new");
    }
}
