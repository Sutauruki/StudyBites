import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  backend,
  errorText,
  type ExamStatus,
  type ExplainResult,
  type Material,
  type Subject,
  type Topic,
  type TopicMastery,
} from "../api";
import { Banner, Empty, Loading, Modal } from "../components/ui";
import { SchemeImportDialog } from "./SchemeImportDialog";
import { UploadDialog } from "./UploadDialog";

/**
 * Subject detail: the scheme-of-work outline editor (Brief 3.2).
 *
 * The scheme is optional. A subject with no topics is a valid, finished state —
 * the empty text says so instead of nagging.
 */
export function SubjectPage({
  subjectId,
  onStudy,
}: {
  subjectId: number;
  onStudy: (minutes: number) => void;
}) {
  const [subject, setSubject] = useState<Subject | null>(null);
  const [topics, setTopics] = useState<Topic[] | null>(null);
  const [materials, setMaterials] = useState<Material[]>([]);
  const [mastery, setMastery] = useState<TopicMastery[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [adding, setAdding] = useState<{ parentId: number | null } | null>(null);
  const [editing, setEditing] = useState<Topic | null>(null);
  const [uploading, setUploading] = useState(false);
  const [importing, setImporting] = useState(false);
  // Subtopics stay folded until asked for: a full scheme of work should
  // fit on one screen.
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const toggleTopic = (id: number) =>
    setExpanded((open) => {
      const next = new Set(open);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const refresh = useCallback(async () => {
    try {
      const [s, t, m, mastered] = await Promise.all([
        backend.subjectGet(subjectId),
        backend.topicsList(subjectId),
        backend.materialsList(subjectId),
        backend.mastery(subjectId),
      ]);
      setSubject(s);
      setTopics(t);
      setMaterials(m);
      setMastery(mastered);
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
      <div className="page-head lede">
        <div className="page-head__text">
          <h1 className="display">{subject?.name ?? "…"}</h1>
          <p className="page-head__sub">
            {flat.length === 0
              ? "No scheme of work yet."
              : `${flat.length} topic${flat.length === 1 ? "" : "s"} in the outline.`}
          </p>
        </div>
        <div className="row">
          <button className="btn" onClick={() => setImporting(true)}>
            Import scheme
          </button>
          <button className="btn" onClick={() => setAdding({ parentId: null })}>
            Add topic
          </button>
          <button className="btn btn--primary" onClick={() => setUploading(true)}>
            Add material
          </button>
        </div>
      </div>

      {error && <Banner kind="error">{error}</Banner>}
      {notice && <Banner kind="info">{notice}</Banner>}

      <MaterialsSection
        materials={materials}
        onChanged={refresh}
        onError={setError}
        onStudy={onStudy}
      />

      <ExamSection subject={subject} onChanged={refresh} onError={setError} />

      <MasterySection mastery={mastery} />

      <ExplainSection subjectId={subjectId} />

      {topics === null ? (
        <Loading label="Loading the outline…" />
      ) : topics.length === 0 ? (
        <Empty
          title="No scheme of work yet"
          body="Add the topics your lecturer listed, in order, with week numbers if you have them. You can also skip this entirely — topics can be created later as you upload material."
          action={
            <div className="row row--wrap" style={{ justifyContent: "center" }}>
              <button className="btn btn--big" onClick={() => setAdding({ parentId: null })}>
                Add topics by hand
              </button>
              <button className="btn btn--primary btn--big" onClick={() => setImporting(true)}>
                Import a scheme of work
              </button>
            </div>
          }
        />
      ) : (
        <div className="outline">
          {topics.map((topic, index) => (
            <OutlineItem
              key={topic.id}
              topic={topic}
              isFirst={index === 0}
              isLast={index === topics.length - 1}
              expanded={expanded.has(topic.id)}
              onToggle={() => toggleTopic(topic.id)}
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

      {importing && (
        <SchemeImportDialog
          subjectId={subjectId}
          hasTopics={(topics ?? []).length > 0}
          onClose={() => setImporting(false)}
          onSaved={(count) => {
            setNotice(`Added ${count} topic${count === 1 ? "" : "s"} from your scheme of work.`);
            void refresh();
          }}
        />
      )}

      {uploading && (
        <UploadDialog
          subjectId={subjectId}
          topics={topics ?? []}
          onClose={() => setUploading(false)}
          onUploaded={(message) => {
            setNotice(message);
            void refresh();
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

/** One topic: a single line, with its subtopics folded away. */
function OutlineItem({
  topic,
  isFirst,
  isLast,
  expanded,
  onToggle,
  onAddChild,
  onEdit,
  onNudge,
  onDelete,
}: {
  topic: Topic;
  isFirst: boolean;
  isLast: boolean;
  expanded: boolean;
  onToggle: () => void;
  onAddChild: () => void;
  onEdit: (topic: Topic) => void;
  onNudge: (id: number, delta: number) => void;
  onDelete: (id: number) => void;
}) {
  const count = topic.children.length;

  return (
    <div className="outline__item">
      <OutlineRow
        topic={topic}
        isFirst={isFirst}
        isLast={isLast}
        onEdit={onEdit}
        onNudge={onNudge}
        onDelete={onDelete}
        onAddChild={onAddChild}
        toggle={
          <button
            className={`outline__toggle${count === 0 ? " outline__toggle--empty" : ""}`}
            onClick={onToggle}
            disabled={count === 0}
            aria-expanded={count === 0 ? undefined : expanded}
            aria-label={expanded ? `Hide subtopics of ${topic.title}` : `Show subtopics of ${topic.title}`}
          >
            {expanded ? "−" : "+"}
          </button>
        }
        trailing={count > 0 && !expanded ? <span className="outline__count">{count}</span> : null}
      />

      {expanded && count > 0 && (
        <div className="outline__children">
          {topic.children.map((child, index) => (
            <div className="outline__item outline__item--child" key={child.id}>
              <OutlineRow
                topic={child}
                isFirst={index === 0}
                isLast={index === count - 1}
                onEdit={onEdit}
                onNudge={onNudge}
                onDelete={onDelete}
                toggle={<span className="outline__toggle outline__toggle--empty" />}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function OutlineRow({
  topic,
  isFirst,
  isLast,
  onEdit,
  onNudge,
  onDelete,
  onAddChild,
  toggle,
  trailing,
}: {
  topic: Topic;
  isFirst: boolean;
  isLast: boolean;
  onEdit: (topic: Topic) => void;
  onNudge: (id: number, delta: number) => void;
  onDelete: (id: number) => void;
  onAddChild?: () => void;
  toggle: ReactNode;
  trailing?: ReactNode;
}) {
  const childCount = topic.children.length;

  return (
    <div className="outline__row">
      {toggle}
      <span className="outline__week">{topic.week !== null ? `W${topic.week}` : ""}</span>
      <span className="outline__title">{topic.title}</span>
      {trailing}
      {topic.isOffScheme && <span className="chip">Off scheme</span>}
      {topic.isPreview && <span className="chip chip--accent">Preview</span>}

      <span className="spacer" />

      <div className="outline__actions">
        <button
          className="btn"
          onClick={() => onNudge(topic.id, -1)}
          disabled={isFirst}
          aria-label={`Move ${topic.title} up`}
        >
          ↑
        </button>
        <button
          className="btn"
          onClick={() => onNudge(topic.id, 1)}
          disabled={isLast}
          aria-label={`Move ${topic.title} down`}
        >
          ↓
        </button>
        {onAddChild && (
          <button className="btn" onClick={onAddChild} aria-label={`Add a subtopic to ${topic.title}`}>
            + sub
          </button>
        )}
        <button className="btn" onClick={() => onEdit(topic)}>
          Edit
        </button>
        <button
          className="btn btn--danger"
          aria-label={`Delete ${topic.title}`}
          onClick={() => {
            const warning =
              childCount > 0
                ? `Delete “${topic.title}” and its ${childCount} subtopic${childCount === 1 ? "" : "s"}?`
                : `Delete “${topic.title}”?`;
            if (window.confirm(warning)) onDelete(topic.id);
          }}
        >
          ✕
        </button>
      </div>
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


/** Uploads and how far each has got through the pipeline (Brief 3.4). */
function MaterialsSection({
  materials,
  onChanged,
  onError,
  onStudy,
}: {
  materials: Material[];
  onChanged: () => void;
  onError: (message: string) => void;
  onStudy: (minutes: number) => void;
}) {
  const label: Record<Material["status"], string> = {
    queued: "Queued",
    processing: "Processing",
    done: "Ready",
    failed: "Failed",
    paused: "Paused — daily limit",
  };

  if (materials.length === 0) {
    return (
      <div className="card card--pad">
        <div className="stack">
          <span className="label">Material</span>
          <p className="muted">
            Add today&rsquo;s notes or a lecture PDF. StudyBites reads it, writes compact notes
            and cards, and files them under the topic you confirm.
          </p>
        </div>
      </div>
    );
  }

  const ready = materials.some((m) => m.status === "done" && m.cardCount > 0);

  return (
    <div className="stack">
      <div className="row">
        <span className="label">Material</span>
        <span className="spacer" />
        {ready && (
          <button className="btn btn--primary" onClick={() => onStudy(10)}>
            Study this subject
          </button>
        )}
      </div>

      <div className="card list">
        {materials.map((material) => (
          <div key={material.id} className="list__item list__item--static">
            <div className="list__text">
              <div className="list__title">{material.originalFilename}</div>
              <div className="list__meta">
                {label[material.status]}
                {material.cardCount > 0 && ` · ${material.cardCount} cards`}
                {material.error && material.status === "failed" && ` · ${material.error}`}
              </div>
            </div>
            <button
              className="btn btn--ghost"
              onClick={async () => {
                try {
                  const destination = await backend.pickSavePath(material.originalFilename);
                  if (destination) await backend.materialSaveOriginal(material.id, destination);
                } catch (err) {
                  onError(errorText(err));
                }
              }}
            >
              Download original
            </button>
            <button
              className="btn btn--danger"
              onClick={async () => {
                if (!window.confirm(`Remove “${material.originalFilename}” and its cards?`)) return;
                try {
                  await backend.materialDelete(material.id);
                  onChanged();
                } catch (err) {
                  onError(errorText(err));
                }
              }}
            >
              Remove
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

/** A simple bar per topic (Brief 3.6). */
function MasterySection({ mastery }: { mastery: TopicMastery[] }) {
  const withCards = mastery.filter((m) => m.totalCards > 0);
  if (withCards.length === 0) return null;

  return (
    <div className="stack">
      <span className="label">Progress</span>
      <div className="card card--pad">
        {withCards.map((topic) => (
          <div key={topic.topicId} className="mastery">
            <div className="mastery__label">
              <span>{topic.title}</span>
              <span className="muted">
                {topic.studiedCards}/{topic.totalCards}
                {topic.dueNow > 0 && ` · ${topic.dueNow} due`}
              </span>
            </div>
            <div className="mastery__track">
              <div
                className="mastery__fill"
                style={{ width: `${Math.round(Math.min(topic.mastery, 1) * 100)}%` }}
              />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

/** Ask a question, answered only from this subject's notes (Brief 3.8). */
function ExplainSection({ subjectId }: { subjectId: number }) {
  const [question, setQuestion] = useState("");
  const [result, setResult] = useState<ExplainResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const ask = async () => {
    if (!question.trim()) return;
    setBusy(true);
    setError(null);
    try {
      setResult(await backend.explain(subjectId, question));
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="stack">
      <span className="label">Ask about this subject</span>
      <div className="card card--pad">
        <div className="row">
          <input
            className="input"
            value={question}
            placeholder="Why does log(ab) equal log a + log b?"
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !busy) void ask();
            }}
          />
          <button className="btn btn--primary" onClick={ask} disabled={busy || !question.trim()}>
            {busy ? "Thinking…" : "Ask"}
          </button>
        </div>
        <p className="muted" style={{ marginTop: "var(--s-2)" }}>
          Answered only from your own uploaded notes for this subject.
        </p>

        {error && <Banner kind="error">{error}</Banner>}
        {result && (
          <div className="stack selectable" style={{ marginTop: "var(--s-4)" }}>
            <p>{result.answer}</p>
            {result.sources.length > 0 && (
              <p className="muted">From your notes: {result.sources.join(", ")}</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}


/**
 * Exam date and what it changes (Brief 3.9).
 *
 * Inside three weeks the session mix shifts toward weak topics. The app says
 * so plainly rather than quietly reordering things behind the student's back.
 */
function ExamSection({
  subject,
  onChanged,
  onError,
}: {
  subject: Subject | null;
  onChanged: () => void;
  onError: (message: string) => void;
}) {
  const [status, setStatus] = useState<ExamStatus | null>(null);

  useEffect(() => {
    if (!subject) return;
    void backend
      .examStatus(subject.id)
      .then(setStatus)
      .catch(() => setStatus(null));
  }, [subject]);

  if (!subject) return null;
  const days = status?.daysUntil ?? null;

  return (
    <section>
      <span className="label">Exam</span>
      <div className="card card--pad">
        <div className="row row--wrap">
          <input
            className="input"
            type="date"
            style={{ maxWidth: "200px" }}
            value={subject.examDate ?? ""}
            aria-label="Exam date"
            onChange={async (e) => {
              try {
                await backend.examDateSet(subject.id, e.target.value || null);
                onChanged();
              } catch (err) {
                onError(errorText(err));
              }
            }}
          />
          {subject.examDate && (
            <button
              className="btn btn--ghost"
              onClick={async () => {
                try {
                  await backend.examDateSet(subject.id, null);
                  onChanged();
                } catch (err) {
                  onError(errorText(err));
                }
              }}
            >
              Clear
            </button>
          )}
          {status?.examMode && <span className="chip chip--accent">Exam mode on</span>}
        </div>

        <p className="muted" style={{ marginTop: "var(--s-3)" }}>
          {days === null
            ? "Set a date and, in the last three weeks, sessions lean toward the topics you know least well."
            : days < 0
              ? "That date has passed. Sessions are back to their usual mix."
              : status?.examMode
                ? `${Math.ceil(days)} day${Math.ceil(days) === 1 ? "" : "s"} to go — sessions are favouring your weakest topics.`
                : `${Math.ceil(days)} days to go. The mix shifts in the last three weeks.`}
        </p>
      </div>
    </section>
  );
}
