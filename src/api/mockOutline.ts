import type {
  OutlineDecisions,
  OutlineDiff,
  SchemeEntry,
  Topic,
  TopicRef,
} from "./types";

/**
 * Outline matching for the browser preview.
 *
 * This mirrors `crates/studybites-core/src/outline.rs` step for step — the same
 * normalisation, the same similarity threshold, the same one-topic-per-entry
 * rule — so the Changes screen shows the same diff in the preview as it will in
 * the desktop app. Keep the two in step: a preview that disagrees is worse than
 * no preview.
 */

type Row = Omit<Topic, "children">;

const SIMILARITY = 0.8;
const MAX_EDIT_DISTANCE = 2;
const MIN_EDIT_LEN = 6;

/** Lower-case, trim, drop leading numbering or bullets, collapse spaces. */
export function normalizeTitle(title: string): string {
  let text = title.trim().toLowerCase();
  for (;;) {
    const trimmed = text.trimStart();
    const stripped = stripPrefix(trimmed);
    if (stripped === trimmed) break;
    text = stripped;
  }
  return text.split(/\s+/).filter(Boolean).join(" ");
}

function stripPrefix(text: string): string {
  const first = text[0];
  if (first && "-•*·–—".includes(first)) return text.slice(1).trimStart();

  if (text.startsWith("week")) {
    const rest = text.slice(4).trimStart();
    const digits = /^\d+/.exec(rest)?.[0].length ?? 0;
    if (digits > 0) {
      let after = rest.slice(digits);
      if (after.startsWith(":")) after = after.slice(1);
      return after.trimStart();
    }
  }

  const digits = /^\d+/.exec(text)?.[0].length ?? 0;
  if (digits > 0 && digits <= 3) {
    const after = text.slice(digits);
    if (after.startsWith(")") || after.startsWith(":")) return after.slice(1).trimStart();
    // "3.5 Laws" is a decimal, not a numbered item.
    if (after.startsWith(".") && /^\s/.test(after.slice(1))) return after.slice(1).trimStart();
  }
  return text;
}

function tokenSet(normalized: string): Set<string> {
  return new Set(normalized.split(" ").filter(Boolean));
}

function jaccard(a: string, b: string): number {
  if (a === b) return 1;
  const left = tokenSet(a);
  const right = tokenSet(b);
  if (left.size === 0 || right.size === 0) return 0;
  let shared = 0;
  for (const token of left) if (right.has(token)) shared += 1;
  return shared / (left.size + right.size - shared);
}

function editDistance(a: string, b: string): number {
  const left = [...a];
  const right = [...b];
  if (Math.abs(left.length - right.length) > MAX_EDIT_DISTANCE) return MAX_EDIT_DISTANCE + 1;
  let previous = right.map((_, index) => index);
  previous.unshift(0);
  for (let i = 0; i < left.length; i += 1) {
    const current = [i + 1];
    for (let j = 0; j < right.length; j += 1) {
      const cost = left[i] === right[j] ? 0 : 1;
      current.push(
        Math.min(previous[j] + cost, previous[j + 1] + 1, current[j] + 1),
      );
    }
    previous = current;
  }
  return previous[right.length];
}

function similar(a: string, b: string): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  if (jaccard(a, b) >= SIMILARITY) return true;
  const left = tokenSet(a);
  const right = tokenSet(b);
  const [smaller, larger] = left.size <= right.size ? [left, right] : [right, left];
  let contained = smaller.size >= 2;
  if (contained) {
    for (const token of smaller) {
      if (!larger.has(token)) {
        contained = false;
        break;
      }
    }
  }
  if (contained) return true;
  const shortest = Math.min([...a].length, [...b].length);
  return shortest >= MIN_EDIT_LEN && editDistance(a, b) <= MAX_EDIT_DISTANCE;
}

function topicRef(topic: Row): TopicRef {
  return {
    id: topic.id,
    title: topic.title,
    week: topic.week,
    parentId: topic.parentId,
  };
}

function subtopicOverlap(rows: Row[], parentId: number, entry: SchemeEntry): number {
  const children = rows
    .filter((t) => t.parentId === parentId)
    .map((t) => normalizeTitle(t.title));
  return entry.subtopics
    .filter((sub) => sub.trim())
    .filter((sub) => children.includes(normalizeTitle(sub)))
    .length;
}

/** What a re-imported or edited outline changes. Writes nothing. */
export function diffOutline(
  rows: Row[],
  subjectId: number,
  incoming: SchemeEntry[],
): OutlineDiff {
  const existing = rows
    .filter((t) => t.subjectId === subjectId)
    .sort((a, b) => a.id - b.id);
  const diff: OutlineDiff = {
    unchanged: [],
    renamed: [],
    moved: [],
    added: [],
    removed: [],
    restored: [],
  };

  const used = new Set<number>();
  const matched: Array<number | null> = incoming.map(() => null);
  const parents = existing.filter((t) => t.parentId === null);

  const claim = (index: number, topic: Row) => {
    used.add(topic.id);
    matched[index] = topic.id;
  };

  // Pass 1: exact match on the normalised title.
  incoming.forEach((entry, index) => {
    if (!entry.title.trim()) return;
    const wanted = normalizeTitle(entry.title);
    const found = parents
      .filter((p) => !used.has(p.id) && normalizeTitle(p.title) === wanted)
      .sort((a, b) => a.id - b.id)[0];
    if (found) claim(index, found);
  });

  // Pass 2: near-misses, subtopic overlap breaking ties.
  incoming.forEach((entry, index) => {
    if (!entry.title.trim() || matched[index] !== null) return;
    const wanted = normalizeTitle(entry.title);
    let best: { topic: Row; overlap: number; score: number } | null = null;
    for (const candidate of parents) {
      if (used.has(candidate.id)) continue;
      const other = normalizeTitle(candidate.title);
      if (!similar(wanted, other)) continue;
      const overlap = subtopicOverlap(existing, candidate.id, entry);
      const score = jaccard(wanted, other);
      if (
        !best ||
        overlap > best.overlap ||
        (overlap === best.overlap && score > best.score)
      ) {
        best = { topic: candidate, overlap, score };
      }
    }
    if (best) claim(index, best.topic);
  });

  const classify = (old: Row, newTitle: string, parentId: number | null) => {
    if (old.archivedAt !== null) {
      diff.restored.push(topicRef(old));
      return;
    }
    let changed = false;
    if (normalizeTitle(newTitle) !== normalizeTitle(old.title)) {
      diff.renamed.push({ id: old.id, old: old.title, new: newTitle });
      changed = true;
    }
    if (old.parentId !== parentId) {
      diff.moved.push({
        id: old.id,
        fromWeek: old.week,
        toWeek: old.week,
        toParentId: parentId,
      });
      changed = true;
    }
    if (!changed) diff.unchanged.push(topicRef(old));
  };

  // Matched parents: renamed, moved or unchanged.
  incoming.forEach((entry, index) => {
    const id = matched[index];
    if (id === null) return;
    const old = existing.find((t) => t.id === id);
    if (!old) return;
    if (old.archivedAt !== null) {
      diff.restored.push(topicRef(old));
      return;
    }
    const newTitle = entry.title.trim();
    let changed = false;
    if (normalizeTitle(newTitle) !== normalizeTitle(old.title)) {
      diff.renamed.push({ id: old.id, old: old.title, new: newTitle });
      changed = true;
    }
    if (old.week !== entry.week) {
      diff.moved.push({
        id: old.id,
        fromWeek: old.week,
        toWeek: entry.week,
        toParentId: old.parentId,
      });
      changed = true;
    }
    if (!changed) diff.unchanged.push(topicRef(old));
  });

  // Added parents first, so their subtopics can point at them.
  const addedParentIndex: Array<number | null> = incoming.map(() => null);
  incoming.forEach((entry, index) => {
    if (!entry.title.trim() || matched[index] !== null) return;
    addedParentIndex[index] = diff.added.length;
    diff.added.push({
      title: entry.title.trim(),
      week: entry.week,
      parentId: null,
      parentIndex: null,
    });
  });

  const leftovers: Array<{ parentId: number; title: string }> = [];
  incoming.forEach((entry, index) => {
    const subs = entry.subtopics.filter((sub) => sub.trim());
    if (subs.length === 0) return;

    const addedIndex = addedParentIndex[index];
    if (addedIndex !== null) {
      for (const sub of subs) {
        diff.added.push({
          title: sub.trim(),
          week: null,
          parentId: null,
          parentIndex: addedIndex,
        });
      }
      return;
    }

    const parentId = matched[index];
    if (parentId === null) return;

    const children = existing.filter(
      (t) => t.parentId === parentId && !used.has(t.id),
    );
    const slots: Array<number | null> = subs.map(() => null);

    subs.forEach((sub, slot) => {
      const wanted = normalizeTitle(sub);
      const found = children
        .filter((c) => !used.has(c.id) && normalizeTitle(c.title) === wanted)
        .sort((a, b) => a.id - b.id)[0];
      if (found) {
        used.add(found.id);
        slots[slot] = found.id;
      }
    });

    subs.forEach((sub, slot) => {
      if (slots[slot] !== null) return;
      const wanted = normalizeTitle(sub);
      let best: { topic: Row; score: number } | null = null;
      for (const candidate of children) {
        if (used.has(candidate.id)) continue;
        const other = normalizeTitle(candidate.title);
        if (!similar(wanted, other)) continue;
        const score = jaccard(wanted, other);
        if (!best || score > best.score) best = { topic: candidate, score };
      }
      if (best) {
        used.add(best.topic.id);
        slots[slot] = best.topic.id;
      }
    });

    subs.forEach((sub, slot) => {
      const id = slots[slot];
      const old = id === null ? undefined : existing.find((t) => t.id === id);
      if (old) classify(old, sub.trim(), parentId);
      else leftovers.push({ parentId, title: sub.trim() });
    });
  });

  // A leftover subtopic may have moved under another topic that still exists.
  for (const leftover of leftovers) {
    const wanted = normalizeTitle(leftover.title);
    const candidates = existing.filter((t) => t.parentId !== null && !used.has(t.id));
    const exact = candidates
      .filter((c) => normalizeTitle(c.title) === wanted)
      .sort((a, b) => a.id - b.id)[0];
    let found = exact;
    if (!found) {
      let best: { topic: Row; score: number } | null = null;
      for (const candidate of candidates) {
        const other = normalizeTitle(candidate.title);
        if (!similar(wanted, other)) continue;
        const score = jaccard(wanted, other);
        if (!best || score > best.score) best = { topic: candidate, score };
      }
      found = best?.topic;
    }
    if (found) {
      used.add(found.id);
      classify(found, leftover.title, leftover.parentId);
    } else {
      diff.added.push({
        title: leftover.title,
        week: null,
        parentId: leftover.parentId,
        parentIndex: null,
      });
    }
  }

  // Whatever the new outline did not claim leaves the outline. A removed
  // parent takes its subtopics with it, so they are not listed one by one.
  for (const topic of existing.filter((t) => t.parentId === null)) {
    if (used.has(topic.id) || topic.archivedAt !== null) continue;
    diff.removed.push(topicRef(topic));
    for (const child of existing.filter((c) => c.parentId === topic.id)) {
      used.add(child.id);
    }
  }
  for (const topic of existing.filter((t) => t.parentId !== null)) {
    if (used.has(topic.id) || topic.archivedAt !== null) continue;
    diff.removed.push(topicRef(topic));
  }

  return diff;
}

/** Apply the accepted parts of a diff, all or nothing. */
export function applyOutline(
  rows: Row[],
  subjectId: number,
  decisions: OutlineDecisions,
  nextId: () => number,
  now: () => string,
): void {
  const before = rows.map((row) => ({ ...row }));
  const stamp = now();
  const live = () => rows.filter((t) => t.subjectId === subjectId);
  const find = (id: number): Row => {
    const topic = live().find((t) => t.id === id);
    if (!topic) throw `topic ${id} was not found`;
    return topic;
  };
  const endPosition = (parentId: number | null): number =>
    rows.filter((t) => t.subjectId === subjectId && t.parentId === parentId).length;
  const compact = (parentId: number | null) => {
    rows
      .filter(
        (t) => t.subjectId === subjectId && t.parentId === parentId && t.archivedAt === null,
      )
      .sort((a, b) => a.position - b.position || a.id - b.id)
      .forEach((topic, index) => {
        topic.position = index;
      });
  };

  const guard = <T>(work: () => T): T => {
    try {
      return work();
    } catch (err) {
      rows.splice(0, rows.length, ...before);
      throw err;
    }
  };

  return guard(() => {
    // 1. Manual pairings are renames: the old id keeps its cards.
    const consumed = new Set<number>();
    for (const pairing of decisions.pairings ?? []) {
      const entry = decisions.added[pairing.addedIndex];
      if (!entry) throw "A pairing points at a topic that is not in this update";
      const title = cleanTitle(entry.title);
      find(pairing.removedId).title = title;
      consumed.add(pairing.addedIndex);
    }

    // 2. Renames.
    for (const rename of decisions.renamed ?? []) {
      find(rename.id).title = cleanTitle(rename.new);
    }

    // 3. Moves: a new week, a new parent, or both.
    for (const moved of decisions.moved ?? []) {
      const topic = find(moved.id);
      if (moved.toWeek !== null && (moved.toWeek < 1 || moved.toWeek > 104)) {
        throw "Week must be between 1 and 104";
      }
      if (moved.toParentId !== null) {
        if (moved.toParentId === moved.id) throw "A topic cannot be its own parent";
        const parent = find(moved.toParentId);
        if (parent.parentId !== null) throw "Topics can only be nested one level deep";
      }
      const oldParent = topic.parentId;
      topic.week = moved.toWeek;
      if (oldParent !== moved.toParentId) {
        topic.parentId = moved.toParentId;
        topic.position = endPosition(moved.toParentId);
        compact(oldParent);
      }
    }

    // 4. Added topics. Parents come before their subtopics.
    const created = new Map<number, number>();
    decisions.added.forEach((entry, index) => {
      if (consumed.has(index)) return;
      let parentId = entry.parentId;
      if (entry.parentIndex !== null) {
        const made = created.get(entry.parentIndex);
        if (made === undefined) throw "The update lists a subtopic before its parent";
        parentId = made;
      }
      if (parentId !== null) {
        const parent = find(parentId);
        if (parent.parentId !== null) throw "Topics can only be nested one level deep";
      }
      const id = nextId();
      rows.push({
        id,
        subjectId,
        parentId,
        title: cleanTitle(entry.title),
        week: entry.week,
        position: endPosition(parentId),
        isOffScheme: false,
        supportsTopicId: null,
        isPreview: false,
        archivedAt: null,
        coverage: "unknown",
      });
      created.set(index, id);
    });

    // 5. Removed topics are archived, with their subtopics. Their cards stay.
    const paired = new Set((decisions.pairings ?? []).map((p) => p.removedId));
    for (const id of decisions.removed ?? []) {
      if (paired.has(id)) continue;
      const topic = find(id);
      rows
        .filter((t) => t.id === id || t.parentId === id)
        .forEach((t) => {
          if (t.archivedAt === null) t.archivedAt = stamp;
        });
      compact(topic.parentId);
    }

    // 6. Restored topics come back, with their subtopics.
    for (const id of decisions.restored ?? []) {
      find(id);
      rows
        .filter((t) => t.id === id || t.parentId === id)
        .forEach((t) => {
          t.archivedAt = null;
        });
    }
  });
}

function cleanTitle(raw: string): string {
  const title = raw.trim();
  if (!title) throw "Topic title cannot be empty";
  if ([...title].length > 200) throw "Topic title cannot be longer than 200 characters";
  return title;
}

