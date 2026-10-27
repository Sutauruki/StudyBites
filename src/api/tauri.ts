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
};
