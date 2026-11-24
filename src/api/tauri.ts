import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  Backend,
  BugReport,
  BugReportResult,
  CbtResult,
  CbtSummary,
  CbtTest,
  DndMode,
  DndStatus,
  LicenseStatus,
  ManagedAiStatus,
  MarkOutcome,
  NewTopic,
  OutlineDecisions,
  OutlineDiff,
  OutlineUpdate,
  Subject,
  Topic,
  TopicCoverage,
  TopicPatch,
} from "./types";

/**
 * The real backend. Every call goes through Tauri IPC — the WebView never
 * touches the network or the filesystem itself (Brief 10).
 */
export const tauriBackend: Backend = {
  appInfo: () => invoke<AppInfo>("app_info"),

  licenseStatus: () => invoke<LicenseStatus>("license_status"),

  answerMark: (question, expected, typed) =>
    invoke<MarkOutcome>("answer_mark", { question, expected, typed }),
  typedAnswersGet: (subjectId) => invoke<boolean>("typed_answers_get", { subjectId }),
  typedAnswersSet: (subjectId, enabled) =>
    invoke<boolean>("typed_answers_set", { subjectId, enabled }),

  cbtAvailable: (subjectIds) => invoke<number>("cbt_available", { subjectIds }),
  cbtStart: (subjectIds, count, minutes) =>
    invoke<CbtTest>("cbt_start", { subjectIds, count, minutes }),
  cbtSubmit: (testId, chosen) => invoke<CbtResult>("cbt_submit", { testId, chosen }),
  cbtHistory: (limit) => invoke<CbtSummary[]>("cbt_history", { limit }),
  eulaText: () => invoke<string>("eula_text"),
  eulaAccept: () => invoke<LicenseStatus>("eula_accept"),
  activate: (code) => invoke<LicenseStatus>("activate", { code }),

  subjectsList: (includeArchived) => invoke<Subject[]>("subjects_list", { includeArchived }),
  subjectGet: (id) => invoke<Subject>("subject_get", { id }),
  subjectCreate: (name) => invoke<Subject>("subject_create", { name }),
  subjectRename: (id, name) => invoke<Subject>("subject_rename", { id, name }),
  subjectSetArchived: (id, archived) => invoke<Subject>("subject_set_archived", { id, archived }),
  subjectDelete: (id) => invoke<void>("subject_delete", { id }),

  topicsList: (subjectId) => invoke<Topic[]>("topics_list", { subjectId }),
  topicCreate: (input: NewTopic) => invoke<Topic>("topic_create", { input }),
  topicUpdate: (id, patch: TopicPatch) =>
    invoke<Topic>("topic_update", {
      id,
      title: patch.title,
      week: patch.week ?? undefined,
      clearWeek: patch.week === null,
    }),
  topicDelete: (id) => invoke<void>("topic_delete", { id }),
  topicReorder: (subjectId, parentId, orderedIds) =>
    invoke<Topic[]>("topic_reorder", { subjectId, parentId, orderedIds }),
  topicNudge: (id, delta) => invoke<Topic[]>("topic_nudge", { id, delta }),
  topicArchive: (id) => invoke<Topic>("topic_archive", { id }),
  topicRestore: (id) => invoke<Topic>("topic_restore", { id }),
  topicsArchived: (subjectId) => invoke<Topic[]>("topics_archived", { subjectId }),
  topicSetCoverage: (id, coverage: TopicCoverage) =>
    invoke<Topic>("topic_set_coverage", { id, coverage }),

  settingGet: (key) => invoke<string | null>("setting_get", { key }),
  settingSet: (key, value) => invoke<void>("setting_set", { key, value }),
  dndStatus: () => invoke<DndStatus>("plugin:dnd|dnd_status"),
  dndRequestAccess: () => invoke<void>("plugin:dnd|dnd_request_access"),
  dndEnable: (mode: Exclude<DndMode, "off">, restoreAfterMinutes) =>
    invoke<DndStatus>("plugin:dnd|dnd_enable", { mode, restoreAfterMinutes }),
  dndRestore: () => invoke<DndStatus>("plugin:dnd|dnd_restore"),
  bugReportAvailable: () => invoke<boolean>("bug_report_available"),
  bugReportSend: (report: BugReport) =>
    invoke<BugReportResult>("bug_report_send", { report }),
  bugReportFlush: () => invoke<number>("bug_report_flush"),

  // ---- Phase 2 ----------------------------------------------------------

  materialUpload: async (subjectId, topicId, path) => {
    if (!path.startsWith("content://")) {
      return invoke("material_upload", { subjectId, topicId, path });
    }
    const filename = pickedFilename(path);
    return invoke("material_upload_uri", { subjectId, topicId, uri: path, filename });
  },
  materialUploadPhotos: (subjectId, topicId, pages) =>
    invoke("material_upload_photos", {
      subjectId,
      topicId,
      pages: pages.map((page) => Array.from(page)),
    }),
  filePreviewText: async (path) => {
    if (!path.startsWith("content://")) {
      return invoke("file_preview_text", { path });
    }
    return invoke<string>("file_preview_text_uri", {
      uri: path,
      filename: pickedFilename(path),
    });
  },
  materialsList: (subjectId) => invoke("materials_list", { subjectId }),
  materialSetTopic: (id, topicId) =>
    invoke("material_set_topic", { id, topicId }),
  materialDelete: (id) => invoke("material_delete", { id }),
  materialSaveOriginal: (id, destination) =>
    invoke("material_save_original", { id, destination }),
  materialPagesGet: (materialId) => invoke("material_pages_get", { materialId }),
  materialPageImageGet: (pageId) => invoke("material_page_image_get", { pageId }),
  materialPageTextSet: (pageId, text) =>
    invoke("material_page_text_set", { pageId, text }),
  materialDiagramsSet: (pageId, rects) =>
    invoke("material_diagrams_set", { pageId, rectsJson: JSON.stringify(rects) }),
  materialTranscriptConfirm: (materialId) =>
    invoke("material_transcript_confirm", { materialId }),
  materialNotesGet: (materialId) => invoke("material_notes_get", { materialId }),

  topicSuggest: (subjectId, text) => invoke("topic_suggest", { subjectId, text }),
  schemeParse: (text) => invoke("scheme_parse", { text }),
  schemeSave: (subjectId, entries) => invoke("scheme_save", { subjectId, entries }),
  outlineDiff: (subjectId, entries) => invoke<OutlineDiff>("outline_diff", { subjectId, entries }),
  outlineApply: (subjectId, decisions: OutlineDecisions) =>
    invoke<Topic[]>("outline_apply", { subjectId, decisions }),
  outlineHistory: (subjectId) => invoke<OutlineUpdate[]>("outline_history", { subjectId }),
  explain: (subjectId, question) => invoke("explain", { subjectId, question }),

  dueCounts: (subjectId) => invoke("due_counts", { subjectId }),
  sessionStart: (subjectId, minutes, newLimit) =>
    invoke("session_start", { subjectId, minutes, newLimit: newLimit ?? null }),
  sessionResume: (sessionId) => invoke("session_resume", { sessionId }),
  sessionGrade: (sessionId, cardId, rating, elapsedMs) =>
    invoke("session_grade", { sessionId, cardId, rating, elapsedMs }),
  sessionEnd: (sessionId) => invoke("session_end", { sessionId }),
  mastery: (subjectId) => invoke("mastery", { subjectId }),

  queueStatus: () => invoke("queue_status"),
  queueRetry: () => invoke("queue_retry"),

  providersList: () => invoke("providers_list"),
  aiStatus: () => invoke<ManagedAiStatus>("ai_status"),
  providerKeySet: async (provider, key) => {
    await invoke("provider_key_set", { provider, key });
    await invoke("queue_wake");
  },
  providerKeyTest: (provider, key) =>
    invoke<string[]>("provider_key_test", { provider, key }),
  providerModels: (provider) => invoke("provider_models", { provider }),
  providerModelSet: async (provider, model) => {
    await invoke("provider_model_set", { provider, model });
    await invoke("queue_wake");
  },
  queueWake: () => invoke("queue_wake"),
  githubTokenSet: (token) => invoke("github_token_set", { token }),
  githubTokenPresent: () => invoke("github_token_present"),
  updateCheck: () => invoke("update_check"),

  previewOffer: (subjectId) => invoke("preview_offer", { subjectId }),
  previewGenerate: (subjectId, topicId, topicTitle) =>
    invoke("preview_generate", { subjectId, topicId, topicTitle }),
  examDateSet: (subjectId, examDate) => invoke("exam_date_set", { subjectId, examDate }),
  examStatus: (subjectId) => invoke("exam_status", { subjectId }),

  pickFile: async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Notes", extensions: ["pdf", "docx", "txt", "md"] }],
    });
    return typeof picked === "string" ? picked : null;
  },
  pickSavePath: async (suggested) => {
    const { save } = await import("@tauri-apps/plugin-dialog");
    return (await save({ defaultPath: suggested })) ?? null;
  },
  openUrl: async (url) => {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  },
};

function pickedFilename(path: string): string {
  const documentPath = path.match(/\/document\/(.+)$/)?.[1];
  const encodedName = documentPath ?? path.split("/").pop() ?? "upload";
  let filename = "upload";
  try {
    filename = decodeURIComponent(encodedName) || filename;
  } catch {
    filename = encodedName || filename;
  }
  filename = filename.split("?")[0] || "upload";
  const slash = filename.lastIndexOf("/");
  filename = (slash >= 0 ? filename.slice(slash + 1) : filename) || "upload";
  // Some Android providers expose only an opaque ID such as document:77062.
  // Never persist that implementation detail as the student's filename.
  return /^document:\w+$/i.test(filename) ? "uploaded-material" : filename;
}
