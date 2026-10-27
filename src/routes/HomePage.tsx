import { useCallback, useEffect, useState } from "react";
import { backend, errorText, type DueCounts, type QueueStatus, type Subject } from "../api";
import { Banner, Empty, Loading } from "../components/ui";

const LENGTHS = [5, 10, 15];

/**
 * Home (Brief 3.5, 4): one primary action, "Start a bite", with a length.
 *
 * At most a gentle "X due today" — no streaks, no badges, no guilt.
 */
export function HomePage({
  onStart,
  onOpenSubject,
  onNewSubject,
}: {
  onStart: (subjectId: number | null, minutes: number) => void;
  onOpenSubject: (id: number) => void;
  onNewSubject: () => void;
}) {
  const [subjects, setSubjects] = useState<Subject[] | null>(null);
  const [counts, setCounts] = useState<DueCounts | null>(null);
  const [queue, setQueue] = useState<QueueStatus | null>(null);
  const [subjectId, setSubjectId] = useState<number | null>(null);
  const [minutes, setMinutes] = useState(10);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [list, due, status] = await Promise.all([
        backend.subjectsList(false),
        backend.dueCounts(subjectId),
        backend.queueStatus(),
      ]);
      setSubjects(list);
      setCounts(due);
      setQueue(status);
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  }, [subjectId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // While uploads are processing, keep the counts moving.
  const working = (queue?.counts.queued ?? 0) + (queue?.counts.running ?? 0);
  useEffect(() => {
    if (working === 0) return;
    const timer = window.setInterval(() => void refresh(), 3000);
    return () => window.clearInterval(timer);
  }, [working, refresh]);

  if (subjects === null) return <Loading label="Opening your library…" />;

  if (subjects.length === 0) {
    return (
      <Empty
        title="Welcome to StudyBites"
        body="Start by creating a subject for one of your courses. Then add your class notes, and StudyBites will turn them into short study sessions."
        action={
          <button className="btn btn--primary btn--big" onClick={onNewSubject}>
            Create your first subject
          </button>
        }
      />
    );
  }

  const paused = queue?.counts.paused ?? 0;
  const failed = queue?.counts.failed ?? 0;
  const nothingToStudy = (counts?.due ?? 0) + (counts?.new ?? 0) === 0;

  return (
    <>
      <div className="page-head">
        <div className="page-head__text">
          <h1>Ready for a bite?</h1>
          <p className="page-head__sub">
            {counts === null
              ? "…"
              : nothingToStudy
                ? "Nothing due right now."
                : [
                    counts.due > 0 ? `${counts.due} due` : null,
                    counts.new > 0 ? `${counts.new} new` : null,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
          </p>
        </div>
      </div>

      {error && <Banner kind="error">{error}</Banner>}

      {working > 0 && (
        <div className="stack">
          <div className="spinner" />
          <p className="muted">
            Processing {working} section{working === 1 ? "" : "s"} of your material in the
            background. You can study while it works.
          </p>
        </div>
      )}

      {paused > 0 && (
        <Banner kind="info">
          Daily AI limit reached — processing resumes automatically after it resets
          {queue?.resumesAt ? ` (${new Date(queue.resumesAt).toLocaleString()})` : ""}. Studying
          keeps working offline.
        </Banner>
      )}

      {failed > 0 && (
        <Banner kind="error">
          {failed} section{failed === 1 ? "" : "s"} could not be processed.{" "}
          <button
            className="btn btn--ghost"
            onClick={async () => {
              try {
                await backend.queueRetry();
                await refresh();
              } catch (err) {
                setError(errorText(err));
              }
            }}
          >
            Try again
          </button>
        </Banner>
      )}

      <div className="bite">
        <div className="field">
          <span className="field__label">Subject</span>
          <select
            className="input"
            value={subjectId ?? ""}
            onChange={(e) => setSubjectId(e.target.value === "" ? null : Number(e.target.value))}
          >
            <option value="">Everything</option>
            {subjects.map((subject) => (
              <option key={subject.id} value={subject.id}>
                {subject.name}
              </option>
            ))}
          </select>
        </div>

        <div className="field">
          <span className="field__label">How long?</span>
          <div className="segmented">
            {LENGTHS.map((length) => (
              <button
                key={length}
                className="btn"
                aria-pressed={minutes === length}
                onClick={() => setMinutes(length)}
              >
                {length} min
              </button>
            ))}
          </div>
        </div>

        <button
          className="btn btn--primary btn--big"
          onClick={() => onStart(subjectId, minutes)}
          disabled={nothingToStudy}
        >
          Start a bite
        </button>
        {nothingToStudy && (
          <p className="muted">
            Add some class material to a subject and it will appear here as cards.
          </p>
        )}
      </div>

      <div className="card list">
        {subjects.map((subject) => (
          <button key={subject.id} className="list__item" onClick={() => onOpenSubject(subject.id)}>
            <div className="list__text">
              <div className="list__title">{subject.name}</div>
              <div className="list__meta">
                {subject.topicCount === 0
                  ? "No topics yet"
                  : `${subject.topicCount} topic${subject.topicCount === 1 ? "" : "s"}`}
              </div>
            </div>
            <span className="muted">›</span>
          </button>
        ))}
      </div>
    </>
  );
}
