const KEY = "sb.resume.v1";
const VERSION = 1;
const MAX_AGE_MS = 12 * 60 * 60 * 1000;

export type ResumeView =
  | { name: "home" }
  | { name: "subjects" }
  | { name: "subject"; id: number }
  | { name: "study"; subjectId: number | null; minutes: number }
  | { name: "cbt" }
  | { name: "settings" };

export interface ResumeStudy {
  sessionId: number;
  subjectId: number | null;
  minutes: number;
  secondsLeft: number;
  revealed: boolean;
  chosen: number | null;
  written: string;
}

export interface ResumeSnapshot {
  v: 1;
  savedAt: number;
  view: ResumeView;
  study?: ResumeStudy;
}

let pending: ResumeSnapshot | null = null;
let timer: number | null = null;

function stopTimer(): void {
  if (timer !== null) window.clearTimeout(timer);
  timer = null;
}

function write(snapshot: ResumeSnapshot): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(snapshot));
  } catch {
    // Resume is best-effort: private windows and blocked storage are valid.
  }
}

export function saveResume(snapshot: ResumeSnapshot, immediate = false): void {
  try {
    pending = { ...snapshot, savedAt: Date.now() };
    if (immediate) {
      stopTimer();
      write(pending);
      pending = null;
      return;
    }
    stopTimer();
    timer = window.setTimeout(() => {
      timer = null;
      if (pending) write(pending);
      pending = null;
    }, 500);
  } catch {
    pending = null;
    stopTimer();
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function validView(value: unknown): value is ResumeView {
  if (!isRecord(value) || typeof value.name !== "string") return false;
  if (["home", "subjects", "cbt", "settings"].includes(value.name)) return true;
  if (value.name === "subject") return Number.isInteger(value.id);
  return (
    value.name === "study" &&
    (value.subjectId === null || Number.isInteger(value.subjectId)) &&
    Number.isInteger(value.minutes)
  );
}

function validStudy(value: unknown): value is ResumeStudy {
  return (
    isRecord(value) &&
    Number.isInteger(value.sessionId) &&
    (value.subjectId === null || Number.isInteger(value.subjectId)) &&
    Number.isInteger(value.minutes) &&
    Number.isFinite(value.secondsLeft) &&
    typeof value.revealed === "boolean" &&
    (value.chosen === null || Number.isInteger(value.chosen)) &&
    typeof value.written === "string"
  );
}

export function loadResume(): ResumeSnapshot | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const value: unknown = JSON.parse(raw);
    if (
      !isRecord(value) ||
      value.v !== VERSION ||
      typeof value.savedAt !== "number" ||
      !Number.isFinite(value.savedAt) ||
      Date.now() - value.savedAt > MAX_AGE_MS ||
      !validView(value.view) ||
      (value.study !== undefined && !validStudy(value.study))
    ) {
      clearResume();
      return null;
    }
    return value as unknown as ResumeSnapshot;
  } catch {
    clearResume();
    return null;
  }
}

export function clearResume(): void {
  try {
    stopTimer();
    pending = null;
    localStorage.removeItem(KEY);
  } catch {
    pending = null;
  }
}
