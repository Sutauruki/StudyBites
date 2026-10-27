import type { AppInfo, Backend, NewTopic, Subject, Topic, TopicPatch } from "./types";

/**
 * Browser-only stand-in for the Rust backend (Brief 11.3), so the UI can be
 * built in Codespaces with no native shell.
 *
 * It mirrors the *rules* enforced in Rust — trimming, duplicate names, the
 * one-level nesting limit, strict reorder — so the UI meets the same errors it
 * will meet in the real app. State lives in localStorage and is synthetic.
 */

const STORE_KEY = "studybites.mock.v1";

interface Rows {
  nextId: number;
  subjects: Array<Omit<Subject, "topicCount">>;
  topics: Array<Omit<Topic, "children">>;
  settings: Record<string, string>;
}

const empty = (): Rows => ({ nextId: 1, subjects: [], topics: [], settings: {} });

function read(): Rows {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    return raw ? (JSON.parse(raw) as Rows) : empty();
  } catch {
    return empty();
  }
}

function write(rows: Rows): void {
  try {
    localStorage.setItem(STORE_KEY, JSON.stringify(rows));
  } catch {
    /* private window or blocked storage: the session just will not persist */
  }
}

/** Matches `clean_name` in crates/studybites-core/src/error.rs. */
function cleanName(raw: string, field: string, max: number): string {
  const name = raw.trim();
  if (!name) throw `${field} cannot be empty`;
  if ([...name].length > max) throw `${field} cannot be longer than ${max} characters`;
  return name;
}

const MAX_SUBJECT = 120;
const MAX_TITLE = 200;

function withCount(rows: Rows, subject: Omit<Subject, "topicCount">): Subject {
  return {
    ...subject,
    topicCount: rows.topics.filter((t) => t.subjectId === subject.id).length,
  };
}

function findSubject(rows: Rows, id: number): Omit<Subject, "topicCount"> {
  const subject = rows.subjects.find((s) => s.id === id);
  if (!subject) throw `subject ${id} was not found`;
  return subject;
}

function findTopic(rows: Rows, id: number): Omit<Topic, "children"> {
  const topic = rows.topics.find((t) => t.id === id);
  if (!topic) throw `topic ${id} was not found`;
  return topic;
}

function siblings(rows: Rows, subjectId: number, parentId: number | null): number[] {
  return rows.topics
    .filter((t) => t.subjectId === subjectId && (t.parentId ?? null) === parentId)
    .sort((a, b) => a.position - b.position || a.id - b.id)
    .map((t) => t.id);
}

function compact(rows: Rows, subjectId: number, parentId: number | null): void {
  siblings(rows, subjectId, parentId).forEach((id, index) => {
    findTopic(rows, id).position = index;
  });
}

function tree(rows: Rows, subjectId: number): Topic[] {
  const all = rows.topics.filter((t) => t.subjectId === subjectId);
  const roots = all
    .filter((t) => t.parentId === null)
    .sort((a, b) => a.position - b.position || a.id - b.id)
    .map((t) => ({ ...t, children: [] as Topic[] }));
  all
    .filter((t) => t.parentId !== null)
    .sort((a, b) => a.position - b.position || a.id - b.id)
    .forEach((child) => {
      const parent = roots.find((r) => r.id === child.parentId);
      if (parent) parent.children.push({ ...child, children: [] });
      else roots.push({ ...child, children: [] });
    });
  return roots;
}

/** A touch of latency so loading states get exercised during UI work. */
const tick = <T,>(value: T): Promise<T> =>
  new Promise((resolve) => setTimeout(() => resolve(value), 40));

export const mockBackend: Backend = {
  appInfo: () => tick({ version: "0.1.0-mock", phase: 1 } satisfies AppInfo),

  subjectsList: (includeArchived) => {
    const rows = read();
    const list = rows.subjects
      .filter((s) => includeArchived || !s.archived)
      .map((s) => withCount(rows, s))
      .sort(
        (a, b) =>
          Number(a.archived) - Number(b.archived) ||
          a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
      );
    return tick(list);
  },

  subjectGet: (id) => {
    const rows = read();
    return tick(withCount(rows, findSubject(rows, id)));
  },

  subjectCreate: (rawName) => {
    const rows = read();
    const name = cleanName(rawName, "Subject name", MAX_SUBJECT);
    if (rows.subjects.some((s) => s.name.toLowerCase() === name.toLowerCase())) {
      throw `A subject called "${name}" already exists`;
    }
    const subject = {
      id: rows.nextId++,
      name,
      createdAt: new Date().toISOString(),
      archived: false,
      examDate: null,
    };
    rows.subjects.push(subject);
    write(rows);
    return tick(withCount(rows, subject));
  },

  subjectRename: (id, rawName) => {
    const rows = read();
    const name = cleanName(rawName, "Subject name", MAX_SUBJECT);
    const subject = findSubject(rows, id);
    if (rows.subjects.some((s) => s.id !== id && s.name.toLowerCase() === name.toLowerCase())) {
      throw `A subject called "${name}" already exists`;
    }
    subject.name = name;
    write(rows);
    return tick(withCount(rows, subject));
  },

  subjectSetArchived: (id, archived) => {
    const rows = read();
    const subject = findSubject(rows, id);
    subject.archived = archived;
    write(rows);
    return tick(withCount(rows, subject));
  },

  subjectDelete: (id) => {
    const rows = read();
    findSubject(rows, id);
    rows.subjects = rows.subjects.filter((s) => s.id !== id);
    rows.topics = rows.topics.filter((t) => t.subjectId !== id);
    write(rows);
    return tick(undefined);
  },

  topicsList: (subjectId) => tick(tree(read(), subjectId)),

  topicCreate: (input: NewTopic) => {
    const rows = read();
    const title = cleanName(input.title, "Topic title", MAX_TITLE);
    const week = input.week ?? null;
    if (week !== null && (week < 1 || week > 104)) throw "Week must be between 1 and 104";
    findSubject(rows, input.subjectId);

    const parentId = input.parentId ?? null;
    if (parentId !== null) {
      const parent = findTopic(rows, parentId);
      if (parent.subjectId !== input.subjectId) {
        throw "The parent topic belongs to a different subject";
      }
      if (parent.parentId !== null) throw "Topics can only be nested one level deep";
    }

    const topic = {
      id: rows.nextId++,
      subjectId: input.subjectId,
      parentId,
      title,
      week,
      position: siblings(rows, input.subjectId, parentId).length,
      isOffScheme: input.isOffScheme ?? false,
      supportsTopicId: null,
      isPreview: false,
    };
    rows.topics.push(topic);
    write(rows);
    return tick({ ...topic, children: [] });
  },

  topicUpdate: (id, patch: TopicPatch) => {
    const rows = read();
    const topic = findTopic(rows, id);
    if (patch.title !== undefined) {
      topic.title = cleanName(patch.title, "Topic title", MAX_TITLE);
    }
    if (patch.week !== undefined) {
      if (patch.week !== null && (patch.week < 1 || patch.week > 104)) {
        throw "Week must be between 1 and 104";
      }
      topic.week = patch.week;
    }
    write(rows);
    return tick({ ...topic, children: [] });
  },

  topicDelete: (id) => {
    const rows = read();
    const topic = findTopic(rows, id);
    rows.topics = rows.topics.filter((t) => t.id !== id && t.parentId !== id);
    compact(rows, topic.subjectId, topic.parentId);
    write(rows);
    return tick(undefined);
  },

  topicReorder: (subjectId, parentId, orderedIds) => {
    const rows = read();
    const current = [...siblings(rows, subjectId, parentId)].sort((a, b) => a - b);
    const wanted = [...orderedIds].sort((a, b) => a - b);
    if (current.length !== wanted.length || current.some((id, i) => id !== wanted[i])) {
      throw "The new order must list exactly the topics in that group";
    }
    orderedIds.forEach((id, index) => {
      findTopic(rows, id).position = index;
    });
    write(rows);
    return tick(tree(rows, subjectId));
  },

  topicNudge: (id, delta) => {
    const rows = read();
    const topic = findTopic(rows, id);
    const order = siblings(rows, topic.subjectId, topic.parentId);
    const from = order.indexOf(id);
    const to = Math.min(Math.max(from + delta, 0), order.length - 1);
    if (from !== to) {
      order.splice(to, 0, ...order.splice(from, 1));
    }
    order.forEach((topicId, index) => {
      findTopic(rows, topicId).position = index;
    });
    write(rows);
    return tick(tree(rows, topic.subjectId));
  },

  settingGet: (key) => tick(read().settings[key] ?? null),

  settingSet: (key, value) => {
    const rows = read();
    rows.settings[key] = value;
    write(rows);
    return tick(undefined);
  },
};
