import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  backend,
  errorText,
  type OutlineDecisions,
  type OutlineDiff,
  type SchemeEntry,
} from "../api";
import { Banner, Modal } from "../components/ui";

type Stage = "reading" | "review" | "changes" | "saving" | "applying";

/**
 * Import a scheme of work (Brief 3.2).
 *
 * The AI drafts the outline; the student edits it and only then is anything
 * saved. A subject with no topics yet is filled in as before. A subject that
 * already has topics goes through the Changes stage instead: the stored
 * outline is compared with the import, and the student applies only the
 * differences, so progress on unchanged topics is kept.
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
  onSaved: (message: string) => void;
}) {
  const [stage, setStage] = useState<Stage>("reading");
  const [entries, setEntries] = useState<SchemeEntry[]>([]);
  const [diff, setDiff] = useState<OutlineDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedAdded, setSelectedAdded] = useState<Set<number>>(new Set());
  const [selectedRenamed, setSelectedRenamed] = useState<Set<number>>(new Set());
  const [selectedMoved, setSelectedMoved] = useState<Set<number>>(new Set());
  const [selectedRemoved, setSelectedRemoved] = useState<Set<number>>(new Set());
  const [selectedRestored, setSelectedRestored] = useState<Set<number>>(new Set());
  /** removed row index -> added row index, when the student says they are one topic. */
  const [pairings, setPairings] = useState<Record<number, number>>({});

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
        if (hasTopics) {
          const found = await backend.outlineDiff(subjectId, draft);
          if (!alive) return;
          setDiff(found);
          setSelectedAdded(selectAll(found.added.length));
          setSelectedRenamed(selectAll(found.renamed.length));
          setSelectedMoved(selectAll(found.moved.length));
          setSelectedRemoved(selectAll(found.removed.length));
          setSelectedRestored(selectAll(found.restored.length));
          setStage("changes");
        } else {
          setStage("review");
        }
      } catch (err) {
        if (alive) {
          setError(errorText(err));
          setStage(hasTopics ? "changes" : "review");
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
      onSaved(`Added ${keep.length} topic${keep.length === 1 ? "" : "s"} from your scheme of work.`);
      onClose();
    } catch (err) {
      setError(errorText(err));
      setStage("review");
    }
  };

  const pairedRemoved = useMemo(() => new Set(Object.keys(pairings).map(Number)), [pairings]);
  const pairedAdded = useMemo(() => new Set(Object.values(pairings)), [pairings]);

  const newCount = diff
    ? diff.added.filter((_, index) => selectedAdded.has(index) && !pairedAdded.has(index)).length
    : 0;
  const renamedCount = diff
    ? diff.renamed.filter((_, index) => selectedRenamed.has(index)).length + pairedRemoved.size
    : 0;
  const archivedCount = diff
    ? diff.removed.filter((_, index) => selectedRemoved.has(index) && !pairedRemoved.has(index))
        .length
    : 0;
  // Every topic that keeps its id — which is every topic the update touches
  // except the ones being archived.
  const keptCount = diff
    ? diff.unchanged.length + diff.renamed.length + diff.moved.length + diff.restored.length
    : 0;
  const actionCount = newCount + renamedCount + archivedCount + selectedRestored.size + selectedMoved.size;

  const apply = async () => {
    if (!diff) return;
    const addedRows = diff.added
      .map((row, index) => ({ row, index }))
      .filter(({ index }) => selectedAdded.has(index) || pairedAdded.has(index));
    const positions = new Map<number, number>();
    addedRows.forEach(({ index }, position) => positions.set(index, position));

    const decisions: OutlineDecisions = {
      added: addedRows.map(({ row }) => ({
        title: row.title,
        week: row.week,
        parentId: row.parentId,
        subtopics: row.subtopics,
      })),
      renamed: diff.renamed
        .filter((_, index) => selectedRenamed.has(index))
        .map((row) => ({ id: row.id, title: row.new })),
      moved: diff.moved
        .filter((_, index) => selectedMoved.has(index))
        .map((row) => ({
          id: row.id,
          week: row.toWeek,
          clearWeek: row.toWeek === null,
          toParent: row.toParent,
          toParentTitle: row.toParentTitle,
          // A subtopic can only leave its parent when the parent is known and
          // no new parent is being added.
          toRoot:
            row.fromParent !== null && row.toParent === null && row.toParentTitle === null,
        })),
      archived: diff.removed
        .filter((_, index) => selectedRemoved.has(index) && !pairedRemoved.has(index))
        .map((row) => row.id),
      restored: diff.restored
        .filter((_, index) => selectedRestored.has(index))
        .map((row) => row.id),
      paired: Object.entries(pairings).flatMap(([removedIndex, addedIndex]) => {
        const added = positions.get(addedIndex);
        if (added === undefined) return [];
        return [{ removedId: diff.removed[Number(removedIndex)].id, added }];
      }),
    };

    setStage("applying");
    try {
      await backend.outlineApply(subjectId, decisions);
      onSaved("Outline updated. Your progress is kept.");
      onClose();
    } catch (err) {
      setError(errorText(err));
      setStage("changes");
    }
  };

  const busy = stage === "reading" || stage === "saving" || stage === "applying";
  const usable = entries.filter((e) => e.title.trim()).length;
  const changes = diff !== null;

  return (
    <Modal
      title={changes ? "Outline changes" : "Import scheme of work"}
      wide
      onClose={onClose}
      actions={
        <>
          <button className="btn btn--ghost" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          {changes ? (
            <button
              className="btn btn--primary"
              onClick={apply}
              disabled={busy || actionCount === 0}
            >
              {stage === "applying" ? "Applying…" : "Apply update"}
            </button>
          ) : (
            <button className="btn btn--primary" onClick={save} disabled={busy || usable === 0}>
              {stage === "saving" ? "Saving…" : `Save ${usable} topic${usable === 1 ? "" : "s"}`}
            </button>
          )}
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

      {stage !== "reading" && !changes && entries.length > 0 && (
        <>
          <p className="muted">
            Check this against your handout before saving — edit anything that came out wrong,
            and remove what you don&rsquo;t need.
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

      {diff && stage !== "reading" && (
        <div className="changes">
          {diff.added.length + diff.renamed.length + diff.moved.length + diff.removed.length +
            diff.restored.length ===
          0 ? (
            <p className="muted">Nothing has changed in this outline.</p>
          ) : (
            <p className="muted">
              Check the changes below, then apply them. Nothing is deleted.
            </p>
          )}

          {diff.added.length > 0 && (
            <ChangeGroup title="Added">
              {diff.added.map((row, index) => (
                <label className="change-row" key={`added-${index}`}>
                  <input
                    type="checkbox"
                    checked={selectedAdded.has(index)}
                    onChange={() => toggle(setSelectedAdded, index)}
                  />
                  <span className="change-row__main">
                    <span className="change-row__title">
                      {row.title}
                      {row.week !== null && <span className="muted"> · Week {row.week}</span>}
                    </span>
                    {row.parentTitle && (
                      <span className="change-row__subs">under {row.parentTitle}</span>
                    )}
                    {row.subtopics.length > 0 && (
                      <span className="change-row__subs">{row.subtopics.join(" · ")}</span>
                    )}
                  </span>
                </label>
              ))}
            </ChangeGroup>
          )}

          {diff.renamed.length > 0 && (
            <ChangeGroup title="Renamed">
              {diff.renamed.map((row, index) => (
                <label className="change-row" key={`renamed-${index}`}>
                  <input
                    type="checkbox"
                    checked={selectedRenamed.has(index)}
                    onChange={() => toggle(setSelectedRenamed, index)}
                  />
                  <span className="change-row__main">
                    <span className="change-row__title">
                      {row.old} <span className="muted">→</span> {row.new}
                    </span>
                  </span>
                </label>
              ))}
            </ChangeGroup>
          )}

          {diff.moved.length > 0 && (
            <ChangeGroup title="Moved">
              {diff.moved.map((row, index) => (
                <label className="change-row" key={`moved-${index}`}>
                  <input
                    type="checkbox"
                    checked={selectedMoved.has(index)}
                    onChange={() => toggle(setSelectedMoved, index)}
                  />
                  <span className="change-row__main">
                    <span className="change-row__title">{row.title}</span>
                    <span className="change-row__subs">
                      {row.fromWeek !== row.toWeek && row.toWeek !== null
                        ? `Week ${row.fromWeek ?? "—"} → Week ${row.toWeek}`
                        : ""}
                      {row.toParentTitle
                        ? `under ${row.toParentTitle}`
                        : row.fromParent !== row.toParent
                          ? "moved to another topic"
                          : ""}
                    </span>
                  </span>
                </label>
              ))}
            </ChangeGroup>
          )}

          {diff.removed.length > 0 && (
            <ChangeGroup title="Removed">
              {diff.removed.map((row, index) => (
                <div className="change-row change-row--removed" key={`removed-${index}`}>
                  <label className="change-row">
                    <input
                      type="checkbox"
                      checked={selectedRemoved.has(index)}
                      onChange={() => {
                        toggle(setSelectedRemoved, index);
                        if (selectedRemoved.has(index)) {
                          setPairings((current) => {
                            const next = { ...current };
                            delete next[index];
                            return next;
                          });
                        }
                      }}
                    />
                    <span className="change-row__main">
                      <span className="change-row__title">{row.title}</span>
                      <span className="change-row__subs">Archive — its notes and cards are kept.</span>
                    </span>
                  </label>
                  <select
                    className="input change-row__pair"
                    aria-label={`Pair ${row.title} with an added topic`}
                    value={pairings[index] ?? ""}
                    onChange={(e) => {
                      const value = e.target.value;
                      setPairings((current) => {
                        const next = { ...current };
                        if (value === "") delete next[index];
                        else next[index] = Number(value);
                        return next;
                      });
                      if (value !== "") {
                        setSelectedAdded((current) => new Set(current).add(Number(value)));
                      }
                    }}
                  >
                    <option value="">Same as…</option>
                    {diff.added.map((added, addedIndex) => (
                      <option
                        key={addedIndex}
                        value={addedIndex}
                        disabled={Object.entries(pairings).some(
                          ([removed, paired]) =>
                            Number(removed) !== index && paired === addedIndex,
                        )}
                      >
                        {added.title}
                      </option>
                    ))}
                  </select>
                </div>
              ))}
            </ChangeGroup>
          )}

          {diff.restored.length > 0 && (
            <ChangeGroup title="Restored">
              {diff.restored.map((row, index) => (
                <label className="change-row" key={`restored-${index}`}>
                  <input
                    type="checkbox"
                    checked={selectedRestored.has(index)}
                    onChange={() => toggle(setSelectedRestored, index)}
                  />
                  <span className="change-row__main">
                    <span className="change-row__title">{row.title}</span>
                    <span className="change-row__subs">Was archived. Its notes and cards return.</span>
                  </span>
                </label>
              ))}
            </ChangeGroup>
          )}

          {diff.unchanged.length > 0 && (
            <p className="muted changes__unchanged">{diff.unchanged.length} unchanged</p>
          )}

          <p className="changes__summary">
            +{newCount} new, {renamedCount} renamed, {archivedCount} archived. Your progress on{" "}
            {keptCount} topic{keptCount === 1 ? "" : "s"} is kept.
          </p>
        </div>
      )}
    </Modal>
  );
}

function ChangeGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="change-group">
      <span className="label">{title}</span>
      {children}
    </div>
  );
}

function selectAll(count: number): Set<number> {
  return new Set(Array.from({ length: count }, (_, index) => index));
}

function toggle(
  update: (change: (current: Set<number>) => Set<number>) => void,
  index: number,
) {
  update((current) => {
    const next = new Set(current);
    if (next.has(index)) next.delete(index);
    else next.add(index);
    return next;
  });
}
