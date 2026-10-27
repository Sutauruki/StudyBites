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
export interface Backend {
  appInfo(): Promise<AppInfo>;

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
