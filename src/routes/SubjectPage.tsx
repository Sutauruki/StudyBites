import { useCallback, useEffect, useState, type ReactNode } from "react";
import { backend, errorText, type Subject, type Topic } from "../api";
import { Banner, Empty, Loading, Modal } from "../components/ui";

/**
 * Subject detail: the scheme-of-work outline editor (Brief 3.2).
 *
 * The scheme is optional. A subject with no topics is a valid, finished state —
 * the empty text says so instead of nagging.
 */
export function SubjectPage({ subjectId, onBack }: { subjectId: number; onBack: () => void }) {
  const [subject, setSubject] = useState<Subject | null>(null);
  const [topics, setTopics] = useState<Topic[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState<{ parentId: number | null } | null>(null);
  const [editing, setEditing] = useState<Topic | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [s, t] = await Promise.all([
        backend.subjectGet(subjectId),
        backend.topicsList(subjectId),
      ]);
      setSubject(s);
      setTopics(t);
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  }, [subjectId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
      await refresh();
    } catch (err) {
      setError(errorText(err));
    }
  };

  const flat = (topics ?? []).flatMap((t) => [t, ...t.children]);

  return (
    <>
      <div className="page-head">
        <div className="page-head__text">
          <button className="btn btn--ghost" onClick={onBack}>
            ← All subjects
          </button>
          <h1 style={{ marginTop: "var(--s-2)" }}>{subject?.name ?? "…"}</h1>
          <p className="page-head__sub">
            Scheme of work — optional. {flat.length} topic{flat.length === 1 ? "" : "s"}.
          </p>
        </div>
        <button className="btn btn--primary" onClick={() => setAdding({ parentId: null })}>
          Add topic
        </button>
      </div>

      {error && <Banner kind="error">{error}</Banner>}

      {topics === null ? (
        <Loading label="Loading the outline…" />
      ) : topics.length === 0 ? (
        <Empty
          title="No scheme of work yet"
          body="Add the topics your lecturer listed, in order, with week numbers if you have them. You can also skip this entirely — topics can be created later as you upload material."
          action={
            <button
              className="btn btn--primary btn--big"
              onClick={() => setAdding({ parentId: null })}
            >
              Add the first topic
            </button>
          }
        />
      ) : (
        <div className="card list">
          {topics.map((topic, index) => (
            <TopicRows
              key={topic.id}
              topic={topic}
              isFirst={index === 0}
              isLast={index === topics.length - 1}
              onAddChild={() => setAdding({ parentId: topic.id })}
              onEdit={setEditing}
              onNudge={(id, delta) => void run(() => backend.topicNudge(id, delta))}
              onDelete={(id) => void run(() => backend.topicDelete(id))}
            />
          ))}
        </div>
      )}

      {adding && (
        <TopicPrompt
          title={adding.parentId === null ? "Add topic" : "Add subtopic"}
          confirmLabel="Add"
          onCancel={() => setAdding(null)}
          onSubmit={async (title, week) => {
            await backend.topicCreate({ subjectId, parentId: adding.parentId, title, week });
            setAdding(null);
            await refresh();
          }}
        />
      )}

      {editing && (
        <TopicPrompt
          title="Edit topic"
          confirmLabel="Save"
          initialTitle={editing.title}
          initialWeek={editing.week}
          onCancel={() => setEditing(null)}
          onSubmit={async (title, week) => {
            await backend.topicUpdate(editing.id, { title, week });
            setEditing(null);
            await refresh();
          }}
        />
      )}
    </>
  );
}

function TopicRows({
  topic,
  isFirst,
  isLast,
  onAddChild,
  onEdit,
  onNudge,
  onDelete,
}: {
  topic: Topic;
  isFirst: boolean;
  isLast: boolean;
  onAddChild: () => void;
  onEdit: (topic: Topic) => void;
  onNudge: (id: number, delta: number) => void;
  onDelete: (id: number) => void;
}) {
  return (
    <>
      <Row
        topic={topic}
        isFirst={isFirst}
        isLast={isLast}
        onEdit={onEdit}
        onNudge={onNudge}
        onDelete={onDelete}
        extra={
          <button className="btn btn--ghost" onClick={onAddChild}>
            + Subtopic
          </button>
        }
      />
      {topic.children.map((child, index) => (
        <Row
          key={child.id}
          topic={child}
          nested
          isFirst={index === 0}
          isLast={index === topic.children.length - 1}
          onEdit={onEdit}
          onNudge={onNudge}
          onDelete={onDelete}
        />
      ))}
    </>
  );
}

function Row({
  topic,
  nested = false,
  isFirst,
  isLast,
  onEdit,
  onNudge,
  onDelete,
  extra,
}: {
  topic: Topic;
  nested?: boolean;
  isFirst: boolean;
  isLast: boolean;
  onEdit: (topic: Topic) => void;
  onNudge: (id: number, delta: number) => void;
  onDelete: (id: number) => void;
  extra?: ReactNode;
}) {
  const childCount = topic.children.length;
  const meta =
    [
      topic.week !== null ? `Week ${topic.week}` : null,
      childCount > 0 ? `${childCount} subtopic${childCount === 1 ? "" : "s"}` : null,
    ]
      .filter(Boolean)
      .join(" · ") || (nested ? "Subtopic" : "Topic");

  return (
    <div className={`list__item list__item--static${nested ? " list__item--child" : ""}`}>
      <div className="list__text">
        <div className="list__title">{topic.title}</div>
        <div className="list__meta">{meta}</div>
      </div>
      {topic.isOffScheme && <span className="chip">Off scheme</span>}
      {topic.isPreview && <span className="chip chip--accent">AI preview</span>}
      <button
        className="btn btn--ghost btn--icon"
        onClick={() => onNudge(topic.id, -1)}
        disabled={isFirst}
        aria-label={`Move ${topic.title} up`}
        title="Move up"
      >
        ↑
      </button>
      <button
        className="btn btn--ghost btn--icon"
        onClick={() => onNudge(topic.id, 1)}
        disabled={isLast}
        aria-label={`Move ${topic.title} down`}
        title="Move down"
      >
        ↓
      </button>
      {extra}
      <button className="btn btn--ghost" onClick={() => onEdit(topic)}>
        Edit
      </button>
      <button
        className="btn btn--danger"
        onClick={() => {
          const warning =
            childCount > 0
              ? `Delete “${topic.title}” and its ${childCount} subtopic${childCount === 1 ? "" : "s"}?`
              : `Delete “${topic.title}”?`;
          if (window.confirm(warning)) onDelete(topic.id);
        }}
      >
        Delete
      </button>
    </div>
  );
}

function TopicPrompt({
  title,
  confirmLabel,
  initialTitle = "",
  initialWeek = null,
  onCancel,
  onSubmit,
}: {
  title: string;
  confirmLabel: string;
  initialTitle?: string;
  initialWeek?: number | null;
  onCancel: () => void;
  onSubmit: (title: string, week: number | null) => Promise<void>;
}) {
  const [text, setText] = useState(initialTitle);
  const [week, setWeek] = useState(initialWeek === null ? "" : String(initialWeek));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    const trimmed = week.trim();
    if (trimmed !== "" && !/^\d{1,3}$/.test(trimmed)) {
      setError("Week must be a whole number between 1 and 104.");
      return;
    }
    setBusy(true);
    try {
      await onSubmit(text, trimmed === "" ? null : Number(trimmed));
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Modal
      title={title}
      onClose={onCancel}
      actions={
        <>
          <button className="btn btn--ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button className="btn btn--primary" onClick={submit} disabled={busy}>
            {confirmLabel}
          </button>
        </>
      }
    >
      <div className="field">
        <span className="field__label">Topic title</span>
        <input
          className="input"
          value={text}
          placeholder="Logarithms"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !busy) void submit();
          }}
        />
      </div>
      <div className="field">
        <span className="field__label">Week (optional)</span>
        <input
          className="input input--week"
          value={week}
          inputMode="numeric"
          placeholder="3"
          onChange={(e) => setWeek(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !busy) void submit();
          }}
        />
      </div>
      {error && <Banner kind="error">{error}</Banner>}
    </Modal>
  );
}
