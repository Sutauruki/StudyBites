import { useEffect, useState } from "react";
import { backend, errorText, type SchemeEntry } from "../api";
import { Banner, Modal } from "../components/ui";

type Stage = "reading" | "review" | "saving";

/**
 * Import a scheme of work (Brief 3.2).
 *
 * The AI drafts the outline; the student edits it and only then is anything
 * saved. Nothing reaches the database until "Save outline" is pressed, so a
 * poor parse costs a click to abandon, not a cleanup.
 */
export function SchemeImportDialog({
  subjectId,
  hasTopics,
  onClose,
  onSaved,
}: {
  subjectId: number;
  hasTopics: boolean;
  onClose: () => void;
  onSaved: (count: number) => void;
}) {
  const [stage, setStage] = useState<Stage>("reading");
  const [entries, setEntries] = useState<SchemeEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const path = await backend.pickFile();
        if (!alive) return;
        if (!path) {
          onClose();
          return;
        }
        const text = await backend.filePreviewText(path);
        const draft = await backend.schemeParse(text);
        if (!alive) return;
        setEntries(draft);
        setStage("review");
      } catch (err) {
        if (alive) {
          setError(errorText(err));
          setStage("review");
        }
      }
    })();
    return () => {
      alive = false;
    };
    // Runs once; choosing another file means reopening the dialog.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const update = (index: number, patch: Partial<SchemeEntry>) =>
    setEntries((list) => list.map((e, i) => (i === index ? { ...e, ...patch } : e)));

  const remove = (index: number) =>
    setEntries((list) => list.filter((_, i) => i !== index));

  const move = (index: number, delta: number) =>
    setEntries((list) => {
      const to = index + delta;
      if (to < 0 || to >= list.length) return list;
      const next = [...list];
      const [moved] = next.splice(index, 1);
      next.splice(to, 0, moved);
      return next;
    });

  const save = async () => {
    const keep = entries.filter((e) => e.title.trim());
    if (keep.length === 0) return;
    setStage("saving");
    try {
      await backend.schemeSave(subjectId, keep);
      onSaved(keep.length);
      onClose();
    } catch (err) {
      setError(errorText(err));
      setStage("review");
    }
  };

  const busy = stage !== "review";
  const usable = entries.filter((e) => e.title.trim()).length;

  return (
    <Modal
      title="Import scheme of work"
      wide
      onClose={onClose}
      actions={
        <>
          <button className="btn btn--ghost" onClick={onClose} disabled={stage === "saving"}>
            Cancel
          </button>
          <button className="btn btn--primary" onClick={save} disabled={busy || usable === 0}>
            {stage === "saving" ? "Saving…" : `Save ${usable} topic${usable === 1 ? "" : "s"}`}
          </button>
        </>
      }
    >
      {stage === "reading" && (
        <div className="stack">
          <div className="spinner" />
          <p className="muted">Reading the document and drafting an outline…</p>
        </div>
      )}

      {error && <Banner kind="error">{error}</Banner>}

      {stage !== "reading" && entries.length > 0 && (
        <>
          <p className="muted">
            Check this against your handout before saving — edit anything that came out wrong,
            and remove what you don&rsquo;t need.
            {hasTopics && " These are added to the topics you already have."}
          </p>

          <div className="scheme">
            {entries.map((entry, index) => (
              <div className="scheme__row" key={index}>
                <div className="scheme__order">
                  <button
                    className="btn btn--ghost btn--icon"
                    onClick={() => move(index, -1)}
                    disabled={index === 0}
                    aria-label={`Move ${entry.title} up`}
                  >
                    ↑
                  </button>
                  <button
                    className="btn btn--ghost btn--icon"
                    onClick={() => move(index, 1)}
                    disabled={index === entries.length - 1}
                    aria-label={`Move ${entry.title} down`}
                  >
                    ↓
                  </button>
                </div>

                <div className="scheme__fields">
                  <input
                    className="input"
                    value={entry.title}
                    placeholder="Topic title"
                    onChange={(e) => update(index, { title: e.target.value })}
                  />
                  {entry.subtopics.length > 0 && (
                    <p className="scheme__subs">{entry.subtopics.join(" · ")}</p>
                  )}
                </div>

                <input
                  className="input input--week"
                  value={entry.week ?? ""}
                  inputMode="numeric"
                  placeholder="Week"
                  aria-label={`Week for ${entry.title}`}
                  onChange={(e) => {
                    const raw = e.target.value.trim();
                    update(index, { week: raw === "" ? null : Number(raw) || null });
                  }}
                />

                <button
                  className="btn btn--danger btn--icon"
                  onClick={() => remove(index)}
                  aria-label={`Remove ${entry.title}`}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        </>
      )}

      {stage === "review" && entries.length === 0 && !error && (
        <p className="muted">No topics could be read from that document.</p>
      )}
    </Modal>
  );
}
