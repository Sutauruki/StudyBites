import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, Backend, NewTopic, Subject, Topic, TopicPatch } from "./types";

/**
 * The real backend. Every call goes through Tauri IPC — the WebView never
 * touches the network or the filesystem itself (Brief 10).
 */
export const tauriBackend: Backend = {
  appInfo: () => invoke<AppInfo>("app_info"),

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

  settingGet: (key) => invoke<string | null>("setting_get", { key }),
  settingSet: (key, value) => invoke<void>("setting_set", { key, value }),

  // ---- Phase 2 ----------------------------------------------------------

  materialUpload: (subjectId, topicId, path) =>
    invoke("material_upload", { subjectId, topicId, path }),
  filePreviewText: (path) => invoke("file_preview_text", { path }),
  materialsList: (subjectId) => invoke("materials_list", { subjectId }),
  materialSetTopic: (id, topicId) =>
    invoke("material_set_topic", { id, topicId }),
  materialDelete: (id) => invoke("material_delete", { id }),
  materialSaveOriginal: (id, destination) =>
    invoke("material_save_original", { id, destination }),

  topicSuggest: (subjectId, text) => invoke("topic_suggest", { subjectId, text }),
  schemeParse: (text) => invoke("scheme_parse", { text }),
  schemeSave: (subjectId, entries) => invoke("scheme_save", { subjectId, entries }),
  explain: (subjectId, question) => invoke("explain", { subjectId, question }),

  dueCounts: (subjectId) => invoke("due_counts", { subjectId }),
  sessionStart: (subjectId, minutes, newLimit) =>
    invoke("session_start", { subjectId, minutes, newLimit: newLimit ?? null }),
  sessionGrade: (sessionId, cardId, rating, elapsedMs) =>
    invoke("session_grade", { sessionId, cardId, rating, elapsedMs }),
  sessionEnd: (sessionId) => invoke("session_end", { sessionId }),
  mastery: (subjectId) => invoke("mastery", { subjectId }),

  queueStatus: () => invoke("queue_status"),
  queueRetry: () => invoke("queue_retry"),

  providersList: () => invoke("providers_list"),
  providerKeySet: (provider, key) => invoke("provider_key_set", { provider, key }),
  providerModels: (provider) => invoke("provider_models", { provider }),
  providerModelSet: (provider, model) => invoke("provider_model_set", { provider, model }),
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
