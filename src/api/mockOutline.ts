import type {
  OutlineDiff,
  OutlineTopicRef,
  SchemeEntry,
} from "./types";

/**
 * The outline matcher, mirrored from `crates/studybites-core/src/outline.rs`.
 *
 * This is the browser preview's copy of the rules, not a second opinion: the
 * same normalisation, the same thresholds and the same order of decisions, so
 * the Changes screen behaves in the preview exactly as it will in the app.
 */

/** Two token sets this alike are the same topic, reworded or reordered. */
export const TITLE_SIMILARITY = 0.8;
/** Titles this many characters apart are the same topic with a typo. */
export const MAX_EDITS = 2;

export interface OutlineTopic {
  id: number;
  subjectId: number;
  parentId: number | null;
  title: string;
  week: number | null;
  position: number;
  archivedAt: string | null;
}

interface PendingSubtopic {
  title: string;
  entry: number;
  /** The matched parent, or `null` when the parent is being added. */
  parent: number | null;
}

function takeDigits(text: string): number {
  let count = 0;
  while (count < text.length && text[count] >= "0" && text[count] <= "9") count += 1;
  return count;
}

/** One leading enumerator, if there is one. */
function stripLeadingEnum(text: string): string | null {
  if (text.length > 0 && "-–—•*+".includes(text[0])) return text.slice(1);
  // "Week 3: Cell division" is a common way schemes number their topics.
  if (text.startsWith("week")) {
    const after = text.slice(4).trimStart();
    const weekDigits = takeDigits(after);
    if (weekDigits > 0) {
      const tail = after.slice(weekDigits);
      if (tail.startsWith(".") || tail.startsWith(")") || tail.startsWith(":")) {
        return tail.slice(1);
      }
    }
  }
  const digits = takeDigits(text);
  if (digits === 0) return null;
  const rest = text.slice(digits);
  // "3.1" and "10.2": section numbering, both halves stripped.
  if (rest.startsWith(".")) {
    const second = takeDigits(rest.slice(1));
    if (second > 0) {
      const tail = rest.slice(1 + second);
      if (tail === "" || tail.startsWith(" ") || tail.startsWith(":")) return tail;
    }
  }
  if (rest.startsWith(".") || rest.startsWith(")") || rest.startsWith(":")) return rest.slice(1);
  return null;
}

/** The comparison key for a title: lowercase, no numbering, single spaces. */
export function normalizeTitle(title: string): string {
  let text = title.trim().toLowerCase();
  for (;;) {
    const next = stripLeadingEnum(text.trimStart());
    if (next === null) break;
    text = next.trimStart();
  }
  return text
    .split(/\s+/)
    .filter((part) => part.length > 0)
    .join(" ");
}

function tokenSet(text: string): Set<string> {
  return new Set(text.split(/[^\p{L}\p{N}]+/u).filter((token) => token.length > 0));
}

function jaccard(a: string, b: string): number {
  const left = tokenSet(a);
  const right = tokenSet(b);
  const union = new Set([...left, ...right]).size;
  if (union === 0) return 1;
  let shared = 0;
  for (const token of left) if (right.has(token)) shared += 1;
  return shared / union;
}

function editDistance(a: string, b: string): number {
  const left = Array.from(a);
  const right = Array.from(b);
  let previous = Array.from({ length: right.length + 1 }, (_, index) => index);
  let current = new Array<number>(right.length + 1).fill(0);
  left.forEach((ca, i) => {
    current[0] = i + 1;
    right.forEach((cb, j) => {
      const substitution = previous[j] + (ca === cb ? 0 : 1);
      current[j + 1] = Math.min(substitution, previous[j + 1] + 1, current[j] + 1);
    });
    const swap = previous;
    previous = current;
    current = swap;
  });
  return previous[right.length];
}

function titlesMatch(a: string, b: string): boolean {
  if (a === b) return true;
  if (a === "" || b === "") return false;
  if (jaccard(a, b) >= TITLE_SIMILARITY) return true;
  const distance = editDistance(a, b);
  const longest = Math.max(Array.from(a).length, Array.from(b).length);
  return distance <= MAX_EDITS && distance * 4 <= longest;
}

function topicRef(topic: OutlineTopic): OutlineTopicRef {
  return { id: topic.id, title: topic.title, week: topic.week, parentId: topic.parentId };
}

function sortTopics(topics: OutlineTopic[]): OutlineTopic[] {
  return [...topics].sort((a, b) => {
    const rootA = a.parentId === null ? 0 : 1;
    const rootB = b.parentId === null ? 0 : 1;
    if (rootA !== rootB) return rootA - rootB;
    const parentA = a.parentId ?? 0;
    const parentB = b.parentId ?? 0;
    if (parentA !== parentB) return parentA - parentB;
    if (a.position !== b.position) return a.position - b.position;
    return a.id - b.id;
  });
}

/** How many of the incoming entry's subtopics the stored topic already holds. */
function subtopicOverlap(entry: SchemeEntry, topic: OutlineTopic, topics: OutlineTopic[]): number {
  const wanted = new Set(
    entry.subtopics.map((sub) => normalizeTitle(sub)).filter((key) => key !== ""),
  );
  return topics.filter(
    (child) => child.parentId === topic.id && wanted.has(normalizeTitle(child.title)),
  ).length;
}

function compareParent(
  entry: SchemeEntry,
  a: OutlineTopic,
  b: OutlineTopic,
  topics: OutlineTopic[],
): number {
  const overlap = subtopicOverlap(entry, b, topics) - subtopicOverlap(entry, a, topics);
  if (overlap !== 0) return overlap;
  const key = normalizeTitle(entry.title);
  const similarityA = jaccard(key, normalizeTitle(a.title));
  const similarityB = jaccard(key, normalizeTitle(b.title));
  if (similarityB !== similarityA) return similarityB - similarityA;
  return a.id - b.id;
}

function compareChild(key: string, a: OutlineTopic, b: OutlineTopic): number {
  const similarityA = jaccard(key, normalizeTitle(a.title));
  const similarityB = jaccard(key, normalizeTitle(b.title));
  if (similarityB !== similarityA) return similarityB - similarityA;
  return a.id - b.id;
}

/**
 * Compare an incoming scheme of work with what the preview has stored.
 *
 * Parents first, then subtopics inside matched parents, then leftover
 * subtopics across parents. An empty import proposes nothing.
 */
export function diffOutline(allTopics: OutlineTopic[], incoming: SchemeEntry[]): OutlineDiff {
  const diff: OutlineDiff = {
    unchanged: [],
    renamed: [],
    moved: [],
    added: [],
    removed: [],
    restored: [],
  };
  const topics = sortTopics(allTopics);
  const entries = incoming.filter((entry) => normalizeTitle(entry.title) !== "");
  if (entries.length === 0) return diff;

  const roots = topics.filter((topic) => topic.parentId === null);
  const used = new Set<number>();
  const matched: Array<number | null> = entries.map(() => null);

  // Pass 1: the same title once numbering, case and spacing are ignored.
  entries.forEach((entry, index) => {
    const key = normalizeTitle(entry.title);
    const candidates = roots
      .filter((topic) => !used.has(topic.id) && normalizeTitle(topic.title) === key)
      .sort((a, b) => compareParent(entry, a, b, topics));
    const best = candidates[0];
    if (best) {
      matched[index] = best.id;
      used.add(best.id);
    }
  });

  // Pass 2: the same words, or a typo or two away.
  entries.forEach((entry, index) => {
    if (matched[index] !== null) return;
    const key = normalizeTitle(entry.title);
    const candidates = roots
      .filter((topic) => !used.has(topic.id))
      .filter((topic) => titlesMatch(key, normalizeTitle(topic.title)))
      .sort((a, b) => compareParent(entry, a, b, topics));
    const best = candidates[0];
    if (best) {
      matched[index] = best.id;
      used.add(best.id);
    }
  });

  const pendingSubtopics: PendingSubtopic[] = [];
  const pendingChildren: OutlineTopic[] = [];
  const pushPending = (sub: string, entry: number, parent: number | null) => {
    const title = sub.trim();
    if (normalizeTitle(title) !== "") pendingSubtopics.push({ title, entry, parent });
  };

  entries.forEach((entry, index) => {
    const parentId = matched[index];
    if (parentId === null) {
      // A new parent: its subtopics may still be existing topics moving here.
      entry.subtopics.forEach((sub) => pushPending(sub, index, null));
      return;
    }
    const parent = topics.find((topic) => topic.id === parentId);
    if (!parent) return;
    const parentArchived = parent.archivedAt !== null;

    if (parentArchived) {
      diff.restored.push(topicRef(parent));
    } else if (normalizeTitle(entry.title) !== normalizeTitle(parent.title)) {
      diff.renamed.push({ id: parent.id, old: parent.title, new: entry.title.trim() });
    } else if (entry.week !== parent.week) {
      diff.moved.push({
        id: parent.id,
        title: parent.title,
        fromWeek: parent.week,
        toWeek: entry.week,
        fromParent: null,
        toParent: null,
        toParentTitle: null,
      });
    } else {
      diff.unchanged.push(topicRef(parent));
    }

    const children = topics
      .filter((topic) => topic.parentId === parentId)
      .sort((a, b) => a.position - b.position || a.id - b.id);

    entry.subtopics.forEach((sub) => {
      const key = normalizeTitle(sub);
      if (key === "") return;
      const exact = children
        .filter((child) => !used.has(child.id) && normalizeTitle(child.title) === key)
        .sort((a, b) => a.id - b.id);
      let chosen: OutlineTopic | undefined = exact[0];
      if (!chosen) {
        chosen = children
          .filter((child) => !used.has(child.id))
          .filter((child) => titlesMatch(key, normalizeTitle(child.title)))
          .sort((a, b) => compareChild(key, a, b))[0];
      }
      if (chosen) {
        used.add(chosen.id);
        if (chosen.archivedAt !== null) {
          diff.restored.push(topicRef(chosen));
        } else if (key !== normalizeTitle(chosen.title)) {
          diff.renamed.push({ id: chosen.id, old: chosen.title, new: sub.trim() });
        } else {
          diff.unchanged.push(topicRef(chosen));
        }
      } else {
        pushPending(sub, index, parentId);
      }
    });

    if (!parentArchived) {
      children
        .filter((child) => !used.has(child.id))
        .forEach((child) => pendingChildren.push(child));
    }
  });

  // Leftover subtopics, searched across the whole outline: this is the
  // "this subtopic moved under another topic" case.
  const stillPending: PendingSubtopic[] = [];
  pendingSubtopics.forEach((pending) => {
    const key = normalizeTitle(pending.title);
    const candidates = pendingChildren
      .filter((child) => titlesMatch(key, normalizeTitle(child.title)))
      .sort((a, b) => compareChild(key, a, b));
    const child = candidates[0];
    if (!child) {
      stillPending.push(pending);
      return;
    }
    used.add(child.id);
    const at = pendingChildren.findIndex((candidate) => candidate.id === child.id);
    if (at >= 0) pendingChildren.splice(at, 1);
    diff.moved.push({
      id: child.id,
      title: child.title,
      fromWeek: child.week,
      toWeek: child.week,
      fromParent: child.parentId,
      toParent: pending.parent,
      toParentTitle: pending.parent === null ? entries[pending.entry].title.trim() : null,
    });
  });

  // Everything the incoming outline names that still has no home is new.
  stillPending.forEach((pending) => {
    if (pending.parent !== null) {
      diff.added.push({
        title: pending.title,
        week: null,
        parentId: pending.parent,
        parentTitle: topics.find((topic) => topic.id === pending.parent)?.title ?? null,
        subtopics: [],
      });
    }
  });
  entries.forEach((entry, index) => {
    if (matched[index] !== null) return;
    const subtopics = stillPending
      .filter((pending) => pending.entry === index && pending.parent === null)
      .map((pending) => pending.title);
    diff.added.push({
      title: entry.title.trim(),
      week: entry.week,
      parentId: null,
      parentTitle: null,
      subtopics,
    });
  });

  // What the incoming outline dropped. Already-archived rows are not reported.
  pendingChildren.forEach((child) => {
    if (child.archivedAt === null) diff.removed.push(topicRef(child));
  });
  roots.forEach((root) => {
    if (!used.has(root.id) && root.archivedAt === null) diff.removed.push(topicRef(root));
  });

  return diff;
}
