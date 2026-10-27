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
  const waiting = (counts?.due ?? 0) + (counts?.new ?? 0);
  const subjectName = subjectId === null ? null : subjects.find((s) => s.id === subjectId)?.name;

  // One line that says where you stand. The app is a nightly ritual, so
  // this is the whole point of the screen; everything else is quieter.
  const statement =
    counts === null
      ? "…"
      : waiting === 0
        ? "Nothing due right now."
        : counts.due > 0 && counts.new > 0
          ? `${counts.due} to review, ${counts.new} new.`
          : counts.due > 0
            ? `${counts.due} card${counts.due === 1 ? "" : "s"} to review.`
            : `${counts.new} new card${counts.new === 1 ? "" : "s"} waiting.`;

  return (
    <>
      <div className="lede">
        <h1 className="display">{statement}</h1>

        <div className="action">
          <button
            className="btn btn--primary btn--big"
            onClick={() => onStart(subjectId, minutes)}
            disabled={waiting === 0}
          >
            Start a {minutes}-minute bite
          </button>
        </div>

        <div className="choices">
          {LENGTHS.map((length) => (
            <button
              key={length}
              className="choice"
              aria-pressed={minutes === length}
              onClick={() => setMinutes(length)}
            >
              {length} min
            </button>
          ))}
          <span className="muted" aria-hidden="true">
            ·
          </span>
          <select
            className="choice"
            value={subjectId ?? ""}
            aria-label="Subject to study"
            onChange={(e) => setSubjectId(e.target.value === "" ? null : Number(e.target.value))}
          >
            <option value="">everything</option>
            {subjects.map((subject) => (
              <option key={subject.id} value={subject.id}>
                {subject.name}
              </option>
            ))}
          </select>
        </div>

        {waiting === 0 && (
          <p className="muted">
            {subjectName
              ? `Nothing waiting in ${subjectName}. Add class material to it, or study everything.`
              : "Add today's notes to a subject and they will turn up here as cards."}
          </p>
        )}
      </div>

      {error && <Banner kind="error">{error}</Banner>}

      {working > 0 && (
        <section>
          <div className="spinner" />
          <p className="muted">
            Reading {working} section{working === 1 ? "" : "s"} of your material. You can study
            while it works.
          </p>
        </section>
      )}

      {paused > 0 && (
        <Banner kind="info">
          Daily AI limit reached — this picks up again after it resets
          {queue?.resumesAt ? ` (${new Date(queue.resumesAt).toLocaleString()})` : ""}. Studying
          works offline meanwhile.
        </Banner>
      )}

      {failed > 0 && (
        <Banner kind="error">
          {failed} section{failed === 1 ? "" : "s"} could not be processed.{" "}
          <button
            className="link"
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

      <section>
        <span className="label">Subjects</span>
        <div className="grid">
          {subjects.map((subject) => (
            <button key={subject.id} className="tile" onClick={() => onOpenSubject(subject.id)}>
              <span className="tile__title">{subject.name}</span>
              <span className="muted">
                {subject.topicCount === 0
                  ? "No topics yet"
                  : `${subject.topicCount} topic${subject.topicCount === 1 ? "" : "s"}`}
              </span>
            </button>
          ))}
        </div>
      </section>
    </>
  );
}
