import type {
  AppInfo,
  Backend,
  Card,
  DueCounts,
  ExplainResult,
  GradeResult,
  Material,
  NewTopic,
  ProviderInfo,
  QueueCounts,
  LicenseStatus,
  MarkOutcome,
  QueueStatus,
  SchemeEntry,
  SessionSummary,
  StudySession,
  Subject,
  Topic,
  TopicMastery,
  TopicMatch,
  TopicPatch,
  UpdateStatus,
} from "./types";

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
  materials: Material[];
  cards: Array<Card & { due: number; isNewCard: boolean }>;
}

const empty = (): Rows => ({
  nextId: 1,
  subjects: [],
  topics: [],
  settings: {},
  materials: [],
  cards: [],
});

function read(): Rows {
  try {
    const raw = localStorage.getItem(STORE_KEY);
    // Merge with `empty()` so data saved before Phase 2 still loads.
    return raw ? { ...empty(), ...(JSON.parse(raw) as Rows) } : empty();
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
/**
 * The browser preview has no machine to fingerprint and no key to verify
 * with, so it behaves exactly like a build made without a licensing key:
 * already unlocked, and saying so rather than faking an activation.
 */
const mockTyped: Record<number, boolean> = {};

let mockLicense: LicenseStatus = {
  licensingEnabled: false,
  unlocked: true,
  eulaVersion: "1.0",
  eulaAccepted: false,
  buyerName: null,
  requestCode: "AD902-FAYFJ-HSGK6-N8G00-3ZN3",
  deviceChanged: [],
  problem: null,
};

const MOCK_EULA = `# StudyBites — End User Licence Agreement

**Version 1.0**

This is the browser preview, which ships a short stand-in. The real agreement
is bundled with the Windows build and read from \`resources/EULA.md\`.

StudyBites is licensed, not sold, for use on one activated device. Your study
materials stay on your device. AI output can be wrong — check it against your
course material.`;

const tick = <T,>(value: T): Promise<T> =>
  new Promise((resolve) => setTimeout(() => resolve(value), 40));

export const mockBackend: Backend = {
  appInfo: () => tick({ version: "0.1.0-mock", phase: 3 } satisfies AppInfo),

  // The browser preview has no device to fingerprint, so it behaves like a
  // build without a licensing key: unlocked, and honest about why.
  licenseStatus: () => tick(mockLicense),

  // The preview has no provider, so it exercises the local path and then the
  // graceful fallback — which is the behaviour most worth being able to see.
  answerMark: (question: string, expected: string, typed: string) => {
    void question;
    const words = (t: string): string[] => t.toLowerCase().match(/[a-z0-9]+/g) ?? [];
    const typedWords = words(typed);
    const expectedWords = words(expected);
    if (typedWords.length === 0) {
      return tick<MarkOutcome>({
        mark: {
          verdict: "wrong",
          score: 0,
          feedback: "Nothing was written, so there is nothing to mark.",
          modelAnswer: expected,
          fromAi: false,
        },
        unavailable: null,
      });
    }
    const hit = expectedWords.filter((w) => typedWords.includes(w)).length;
    const score = Math.round((hit / Math.max(expectedWords.length, 1)) * 100);
    return tick<MarkOutcome>({
      mark: {
        verdict: score >= 80 ? "correct" : score >= 40 ? "close" : "wrong",
        score,
        feedback:
          "Browser preview: this is a rough word-overlap score, not real marking. The Windows build sends borderline answers to your AI provider.",
        modelAnswer: score >= 80 ? "" : expected,
        fromAi: false,
      },
      unavailable: null,
    });
  },
  typedAnswersGet: (subjectId: number) => tick(mockTyped[subjectId] !== false),
  typedAnswersSet: (subjectId: number, enabled: boolean) => {
    mockTyped[subjectId] = enabled;
    return tick(enabled);
  },
  eulaText: () => tick(MOCK_EULA),
  eulaAccept: () => {
    mockLicense = { ...mockLicense, eulaAccepted: true };
    return tick(mockLicense);
  },
  activate: (code: string) => {
    if (code.trim().length < 20) throw "that activation code is not valid.";
    mockLicense = { ...mockLicense, unlocked: true, buyerName: "Browser Preview" };
    return tick(mockLicense);
  },

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

  // ---- Phase 2 ----------------------------------------------------------
  //
  // The browser mock cannot read files or call an AI, so uploads and the
  // AI-assisted steps report that plainly rather than pretending. Studying
  // works fully, against cards the mock seeds on first use, so the session
  // UI can be built and checked without the desktop shell.

  materialUpload: () =>
    Promise.reject("Uploading needs the desktop app — this is the browser preview."),
  filePreviewText: () =>
    Promise.reject("Reading files needs the desktop app — this is the browser preview."),
  materialsList: (subjectId) =>
    tick(read().materials.filter((m) => m.subjectId === subjectId)),
  materialSetTopic: (id, topicId) => {
    const rows = read();
    const material = rows.materials.find((m) => m.id === id);
    if (!material) throw `material ${id} was not found`;
    material.topicId = topicId;
    write(rows);
    return tick(material);
  },
  materialDelete: (id) => {
    const rows = read();
    rows.materials = rows.materials.filter((m) => m.id !== id);
    write(rows);
    return tick(undefined);
  },
  materialSaveOriginal: () =>
    Promise.reject("Saving files needs the desktop app — this is the browser preview."),

  topicSuggest: () =>
    tick<TopicMatch>({
      index: null,
      confidence: 0,
      reason: "Topic matching needs an AI key, which the browser preview does not have.",
    }),
  schemeParse: () =>
    Promise.reject("Parsing a scheme needs an AI key — this is the browser preview."),
  schemeSave: (subjectId, entries: SchemeEntry[]) => {
    const rows = read();
    for (const entry of entries) {
      if (!entry.title.trim()) continue;
      const parent = {
        id: rows.nextId++,
        subjectId,
        parentId: null,
        title: entry.title.trim(),
        week: entry.week,
        position: siblings(rows, subjectId, null).length,
        isOffScheme: false,
        supportsTopicId: null,
        isPreview: false,
      };
      rows.topics.push(parent);
      for (const sub of entry.subtopics ?? []) {
        if (!sub.trim()) continue;
        rows.topics.push({
          id: rows.nextId++,
          subjectId,
          parentId: parent.id,
          title: sub.trim(),
          week: null,
          position: siblings(rows, subjectId, parent.id).length,
          isOffScheme: false,
          supportsTopicId: null,
          isPreview: false,
        });
      }
    }
    write(rows);
    return tick(tree(rows, subjectId));
  },
  explain: () =>
    tick<ExplainResult>({
      answer:
        "Explanations need an AI key. In the desktop app this answers from your own notes only.",
      sources: [],
    }),

  dueCounts: (subjectId) => {
    const rows = seedCards(read(), subjectId);
    const mine = cardsFor(rows, subjectId);
    const now = Date.now();
    return tick<DueCounts>({
      due: mine.filter((c) => !c.isNewCard && c.due <= now).length,
      new: mine.filter((c) => c.isNewCard).length,
    });
  },
  sessionStart: (subjectId, minutes, newLimit) => {
    const rows = seedCards(read(), subjectId);
    write(rows);
    const now = Date.now();
    const mine = cardsFor(rows, subjectId);
    const due = mine.filter((c) => !c.isNewCard && c.due <= now);
    const fresh = mine.filter((c) => c.isNewCard).slice(0, newLimit ?? 10);
    const chosen = [...due, ...fresh];
    return tick<StudySession>({
      id: 1,
      subjectId,
      plannedMinutes: minutes,
      dueCount: due.length,
      newCount: fresh.length,
      cards: chosen.map((c) => ({
        card: c,
        isNew: c.isNewCard,
        topicTitle: read().topics.find((t) => t.id === c.topicId)?.title ?? null,
        subjectId: subjectId ?? read().topics.find((t) => t.id === c.topicId)?.subjectId ?? null,
      })),
    });
  },
  sessionGrade: (_sessionId, cardId, rating) => {
    const rows = read();
    const card = rows.cards.find((c) => c.id === cardId);
    // Roughly mirrors FSRS: a failure returns soon, success pushes out.
    const days = rating === 1 ? 0 : rating === 2 ? 1 : rating === 3 ? 3 : 7;
    if (card) {
      card.isNewCard = false;
      card.due = Date.now() + days * 86_400_000;
      write(rows);
    }
    return tick<GradeResult>({
      cardId,
      due: new Date(Date.now() + days * 86_400_000).toISOString(),
      intervalDays: days,
      state: rating === 1 ? "learning" : "review",
    });
  },
  sessionEnd: (sessionId) =>
    tick<SessionSummary>({
      sessionId,
      cardsDone: 0,
      topicsTouched: [],
      dueTomorrow: 0,
      minutes: 10,
    }),
  mastery: (subjectId) => {
    const rows = read();
    return tick<TopicMastery[]>(
      rows.topics
        .filter((t) => t.subjectId === subjectId)
        .map((t) => {
          const mine = rows.cards.filter((c) => c.topicId === t.id);
          const studied = mine.filter((c) => !c.isNewCard).length;
          return {
            topicId: t.id,
            title: t.title,
            totalCards: mine.length,
            studiedCards: studied,
            mastery: mine.length ? studied / mine.length : 0,
            dueNow: mine.filter((c) => !c.isNewCard && c.due <= Date.now()).length,
          };
        }),
    );
  },

  queueStatus: () =>
    tick<QueueStatus>({
      counts: { queued: 0, running: 0, done: 0, failed: 0, paused: 0 },
      quota: [],
      resumesAt: null,
    }),
  queueRetry: () =>
    tick<QueueCounts>({ queued: 0, running: 0, done: 0, failed: 0, paused: 0 }),

  providersList: () =>
    tick<ProviderInfo[]>([
      {
        id: "gemini",
        label: "Google Gemini",
        configured: false,
        helpUrl: "https://aistudio.google.com/apikey",
        defaultModel: "gemini-2.0-flash",
      },
      {
        id: "groq",
        label: "Groq",
        configured: false,
        helpUrl: "https://console.groq.com/keys",
        defaultModel: "llama-3.3-70b-versatile",
      },
      {
        id: "openrouter",
        label: "OpenRouter",
        configured: false,
        helpUrl: "https://openrouter.ai/keys",
        defaultModel: "meta-llama/llama-3.3-70b-instruct:free",
      },
    ]),
  providerKeySet: () =>
    Promise.reject("Keys are stored in the OS keychain, which the browser preview has no access to."),
  providerModels: () =>
    Promise.reject("Listing models needs the desktop app — this is the browser preview."),
  providerModelSet: () => tick(undefined),
  githubTokenSet: () =>
    Promise.reject("Keys are stored in the OS keychain, which the browser preview has no access to."),
  githubTokenPresent: () => tick(false),
  updateCheck: () =>
    tick<UpdateStatus>({
      currentVersion: "0.1.0-mock",
      latestVersion: null,
      updateAvailable: false,
      downloadUrl: null,
      notes: null,
      skippedReason: "Update checking only runs in the desktop app.",
    }),

  previewOffer: () => tick(null),
  previewGenerate: () =>
    Promise.reject("Previews need an AI key — this is the browser preview."),
  examDateSet: (subjectId, examDate) => {
    const rows = read();
    const subject = findSubject(rows, subjectId);
    subject.examDate = examDate;
    write(rows);
    return tick(withCount(rows, subject));
  },
  examStatus: () => tick({ daysUntil: null, examMode: false }),

  pickFile: () => tick(null),
  pickSavePath: () => tick(null),
  openUrl: async (url) => {
    window.open(url, "_blank", "noopener,noreferrer");
  },
};

/** Cards belonging to a subject, through their topic. */
function cardsFor(rows: Rows, subjectId: number | null) {
  if (subjectId === null) return rows.cards;
  const topicIds = new Set(
    rows.topics.filter((t) => t.subjectId === subjectId).map((t) => t.id),
  );
  return rows.cards.filter((c) => c.topicId !== null && topicIds.has(c.topicId));
}

/**
 * Give a subject a few sample cards the first time it is studied, so the
 * session UI has something to show in the browser. Marked clearly as sample
 * content; the desktop app never does this.
 */
function seedCards(rows: Rows, subjectId: number | null): Rows {
  if (subjectId === null || cardsFor(rows, subjectId).length > 0) return rows;
  const topic = rows.topics.find((t) => t.subjectId === subjectId);
  if (!topic) return rows;

  const samples: Card["payload"][] = [
    { kind: "flash", front: "Sample card: what is this screen for?", back: "Trying the study flow." },
    {
      kind: "mcq",
      question: "Sample question: which grade means you forgot?",
      options: ["Again", "Hard", "Good", "Easy"],
      answer: 0,
      explanation: "Again sends the card back round in this session.",
    },
    { kind: "short", prompt: "Sample prompt: say anything, then reveal.", answer: "Any answer." },
  ];
  for (const payload of samples) {
    rows.cards.push({
      id: rows.nextId++,
      noteId: null,
      topicId: topic.id,
      payload,
      isPreview: false,
      retired: false,
      createdAt: new Date().toISOString(),
      due: Date.now(),
      isNewCard: true,
    });
  }
  write(rows);
  return rows;
}

