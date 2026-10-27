// Mirrors the serde shapes in crates/studybites-core/src/models.rs.

export interface Subject {
  id: number;
  name: string;
  createdAt: string;
  archived: boolean;
  examDate: string | null;
  topicCount: number;
}

export interface Topic {
  id: number;
  subjectId: number;
  parentId: number | null;
  title: string;
  week: number | null;
  position: number;
  isOffScheme: boolean;
  supportsTopicId: number | null;
  isPreview: boolean;
  children: Topic[];
}

export interface NewTopic {
  subjectId: number;
  parentId?: number | null;
  title: string;
  week?: number | null;
  isOffScheme?: boolean;
}

export interface TopicPatch {
  title?: string;
  /** A number sets the week, `null` clears it, leaving it out keeps it. */
  week?: number | null;
}

export interface AppInfo {
  version: string;
  phase: number;
}

/** Everything the UI is allowed to ask the backend for. */
/** Phase 3: what the onboarding screens and Settings need to know. */
export interface LicenseStatus {
  /** False in a build made without the licensing key; it runs unlocked. */
  licensingEnabled: boolean;
  unlocked: boolean;
  eulaVersion: string;
  eulaAccepted: boolean;
  buyerName: string | null;
  /** Shown on the activation screen, read out to the owner. */
  requestCode: string | null;
  /** Parts of this machine that differ from activation day. Advisory. */
  deviceChanged: string[];
  /** Set when the machine could not be identified at all. */
  problem: string | null;
}

/** Phase 5: how a typed answer was marked. */
export type Verdict = "correct" | "close" | "wrong";

export interface Mark {
  verdict: Verdict;
  /** Out of 100 — "what this would score in an exam". */
  score: number;
  feedback: string;
  /** How to write it for full marks. Empty when the answer was already good. */
  modelAnswer: string;
  /** False when decided locally, with no AI request spent. */
  fromAi: boolean;
}

export interface MarkOutcome {
  mark: Mark | null;
  /** Set when marking could not run; the student grades themselves instead. */
  unavailable: string | null;
}

/** Phase 5: CBT practice tests. */
export interface CbtQuestion {
  cardId: number;
  position: number;
  question: string;
  options: string[];
  subjectName: string | null;
  topicTitle: string | null;
}

export interface CbtTest {
  id: number;
  minutes: number;
  questions: CbtQuestion[];
}

/** Revealed only after submitting — the answer key is not in the page before. */
export interface CbtMarked {
  position: number;
  question: string;
  options: string[];
  correct: number;
  chosen: number | null;
  explanation: string;
  topicTitle: string | null;
}

export interface CbtResult {
  testId: number;
  score: number;
  total: number;
  percent: number;
  answers: CbtMarked[];
}

export interface CbtSummary {
  id: number;
  startedAt: string;
  score: number | null;
  total: number;
  minutes: number;
  subjectNames: string[];
}

export interface Backend extends Phase2Backend {
  appInfo(): Promise<AppInfo>;

  licenseStatus(): Promise<LicenseStatus>;

  answerMark(question: string, expected: string, typed: string): Promise<MarkOutcome>;
  typedAnswersGet(subjectId: number): Promise<boolean>;
  typedAnswersSet(subjectId: number, enabled: boolean): Promise<boolean>;

  cbtAvailable(subjectIds: number[]): Promise<number>;
  cbtStart(subjectIds: number[], count: number, minutes: number): Promise<CbtTest>;
  cbtSubmit(testId: number, chosen: Array<number | null>): Promise<CbtResult>;
  cbtHistory(limit: number): Promise<CbtSummary[]>;
  eulaText(): Promise<string>;
  eulaAccept(): Promise<LicenseStatus>;
  activate(code: string): Promise<LicenseStatus>;

  subjectsList(includeArchived: boolean): Promise<Subject[]>;
  subjectGet(id: number): Promise<Subject>;
  subjectCreate(name: string): Promise<Subject>;
  subjectRename(id: number, name: string): Promise<Subject>;
  subjectSetArchived(id: number, archived: boolean): Promise<Subject>;
  subjectDelete(id: number): Promise<void>;

  topicsList(subjectId: number): Promise<Topic[]>;
  topicCreate(input: NewTopic): Promise<Topic>;
  topicUpdate(id: number, patch: TopicPatch): Promise<Topic>;
  topicDelete(id: number): Promise<void>;
  topicReorder(
    subjectId: number,
    parentId: number | null,
    orderedIds: number[],
  ): Promise<Topic[]>;
  topicNudge(id: number, delta: number): Promise<Topic[]>;

  settingGet(key: string): Promise<string | null>;
  settingSet(key: string, value: string): Promise<void>;
}

// ---- Phase 2 ------------------------------------------------------------

export interface Material {
  id: number;
  subjectId: number;
  topicId: number | null;
  originalFilename: string;
  mime: string;
  size: number;
  sha256: string;
  coldPath: string | null;
  uploadedAt: string;
  status: "queued" | "processing" | "done" | "failed" | "paused";
  error: string | null;
  noteCount: number;
  cardCount: number;
}

export interface Accepted {
  material: Material;
  chunkCount: number;
  duplicateOf: number | null;
}

export type CardPayload =
  | { kind: "flash"; front: string; back: string }
  | { kind: "mcq"; question: string; options: string[]; answer: number; explanation: string }
  | { kind: "short"; prompt: string; answer: string };

export interface Card {
  id: number;
  noteId: number | null;
  topicId: number | null;
  payload: CardPayload;
  isPreview: boolean;
  retired: boolean;
  createdAt: string;
}

export interface SessionCard {
  card: Card;
  isNew: boolean;
  topicTitle: string | null;
  /** A session can span subjects, and typed answers is a per-subject choice. */
  subjectId: number | null;
}

export interface StudySession {
  id: number;
  subjectId: number | null;
  plannedMinutes: number;
  cards: SessionCard[];
  dueCount: number;
  newCount: number;
}

export interface GradeResult {
  cardId: number;
  due: string;
  intervalDays: number;
  state: "new" | "learning" | "review" | "relearning";
}

export interface SessionSummary {
  sessionId: number;
  cardsDone: number;
  topicsTouched: string[];
  dueTomorrow: number;
  minutes: number;
}

export interface DueCounts {
  due: number;
  new: number;
}

export interface TopicMastery {
  topicId: number;
  title: string;
  totalCards: number;
  studiedCards: number;
  mastery: number;
  dueNow: number;
}

export interface TopicMatch {
  index: number | null;
  confidence: number;
  reason: string;
}

export interface SchemeEntry {
  title: string;
  week: number | null;
  subtopics: string[];
}

export interface QueueCounts {
  queued: number;
  running: number;
  done: number;
  failed: number;
  paused: number;
}

export interface QuotaUsage {
  provider: string;
  date: string;
  requests: number;
  tokens: number;
  exhaustedUntil: string | null;
}

export interface QueueStatus {
  counts: QueueCounts;
  quota: QuotaUsage[];
  resumesAt: string | null;
}

export interface ProviderInfo {
  id: string;
  label: string;
  configured: boolean;
  helpUrl: string;
  defaultModel: string;
}

export interface ExplainResult {
  answer: string;
  sources: string[];
}

export interface UpdateStatus {
  currentVersion: string;
  latestVersion: string | null;
  updateAvailable: boolean;
  downloadUrl: string | null;
  notes: string | null;
  skippedReason: string | null;
}

export interface PreviewOffer {
  topicId: number | null;
  title: string;
  reason: string;
  fromScheme: boolean;
  alreadyPrepared: boolean;
}

export interface ExamStatus {
  daysUntil: number | null;
  examMode: boolean;
}

/** Phase 2 additions to the backend surface. */
export interface Phase2Backend {
  materialUpload(subjectId: number, topicId: number | null, path: string): Promise<Accepted>;
  filePreviewText(path: string): Promise<string>;
  materialsList(subjectId: number): Promise<Material[]>;
  materialSetTopic(id: number, topicId: number | null): Promise<Material>;
  materialDelete(id: number): Promise<void>;
  materialSaveOriginal(id: number, destination: string): Promise<string>;

  topicSuggest(subjectId: number, text: string): Promise<TopicMatch>;
  schemeParse(text: string): Promise<SchemeEntry[]>;
  schemeSave(subjectId: number, entries: SchemeEntry[]): Promise<Topic[]>;
  explain(subjectId: number, question: string): Promise<ExplainResult>;

  dueCounts(subjectId: number | null): Promise<DueCounts>;
  sessionStart(subjectId: number | null, minutes: number, newLimit?: number): Promise<StudySession>;
  sessionGrade(
    sessionId: number | null,
    cardId: number,
    rating: number,
    elapsedMs: number,
  ): Promise<GradeResult>;
  sessionEnd(sessionId: number): Promise<SessionSummary>;
  mastery(subjectId: number): Promise<TopicMastery[]>;

  queueStatus(): Promise<QueueStatus>;
  queueRetry(): Promise<QueueCounts>;

  providersList(): Promise<ProviderInfo[]>;
  providerKeySet(provider: string, key: string): Promise<void>;
  providerModels(provider: string): Promise<string[]>;
  providerModelSet(provider: string, model: string): Promise<void>;
  githubTokenSet(token: string): Promise<void>;
  githubTokenPresent(): Promise<boolean>;
  updateCheck(): Promise<UpdateStatus>;

  /** Open the OS file picker; null when cancelled. */
  previewOffer(subjectId: number): Promise<PreviewOffer | null>;
  previewGenerate(
    subjectId: number,
    topicId: number | null,
    topicTitle: string | null,
  ): Promise<number>;
  examDateSet(subjectId: number, examDate: string | null): Promise<Subject>;
  examStatus(subjectId: number): Promise<ExamStatus>;

  pickFile(): Promise<string | null>;
  pickSavePath(suggested: string): Promise<string | null>;
  openUrl(url: string): Promise<void>;
}
