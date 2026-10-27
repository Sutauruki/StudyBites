import { useCallback, useEffect, useState } from "react";
import { backend, errorText, type Subject } from "../api";
import { Banner, Empty, Loading, Modal } from "../components/ui";

export function SubjectsPage({ onOpen }: { onOpen: (id: number) => void }) {
  const [subjects, setSubjects] = useState<Subject[] | null>(null);
  const [showArchived, setShowArchived] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState<Subject | null>(null);

  const refresh = useCallback(async (includeArchived: boolean) => {
    try {
      setSubjects(await backend.subjectsList(includeArchived));
      setError(null);
    } catch (err) {
      setError(errorText(err));
    }
  }, []);

  useEffect(() => {
    void refresh(showArchived);
  }, [refresh, showArchived]);

  const visible = subjects ?? [];
  const activeCount = visible.filter((s) => !s.archived).length;

  return (
    <>
      <div className="page-head">
        <div className="page-head__text">
          <h1>Your subjects</h1>
          <p className="page-head__sub">
            {subjects === null
              ? "Loading…"
              : activeCount === 0
                ? "Nothing yet"
                : `${activeCount} subject${activeCount === 1 ? "" : "s"}`}
          </p>
        </div>
        <button className="btn btn--primary" onClick={() => setCreating(true)}>
          New subject
        </button>
      </div>

      {error && <Banner kind="error">{error}</Banner>}

      {subjects === null ? (
        <Loading label="Opening your library…" />
      ) : visible.length === 0 ? (
        <Empty
          title="Start with one subject"
          body="Add a course you are taking — for example “MTH 101”. Then sketch its scheme of work, or leave it empty and build the outline as you go."
          action={
            <button className="btn btn--primary btn--big" onClick={() => setCreating(true)}>
              Create your first subject
            </button>
          }
        />
      ) : (
        <div className="card list">
          {visible.map((subject) => (
            <div key={subject.id} className="list__item list__item--static">
              <button
                className="list__text btn btn--ghost"
                style={{ textAlign: "left" }}
                onClick={() => onOpen(subject.id)}
              >
                <div className="list__title">{subject.name}</div>
                <div className="list__meta">
                  {subject.topicCount === 0
                    ? "No topics yet"
                    : `${subject.topicCount} topic${subject.topicCount === 1 ? "" : "s"}`}
                </div>
              </button>
              {subject.archived && <span className="chip">Archived</span>}
              <button className="btn btn--ghost" onClick={() => setRenaming(subject)}>
                Rename
              </button>
              <button
                className="btn btn--ghost"
                onClick={async () => {
                  try {
                    await backend.subjectSetArchived(subject.id, !subject.archived);
                    await refresh(showArchived);
                  } catch (err) {
                    setError(errorText(err));
                  }
                }}
              >
                {subject.archived ? "Restore" : "Archive"}
              </button>
            </div>
          ))}
        </div>
      )}

      {subjects !== null && (
        <label className="row muted">
          <input
            type="checkbox"
            checked={showArchived}
            onChange={(e) => setShowArchived(e.target.checked)}
          />
          Show archived subjects
        </label>
      )}

      {creating && (
        <NamePrompt
          title="New subject"
          label="Subject name"
          placeholder="MTH 101"
          confirmLabel="Create"
          onCancel={() => setCreating(false)}
          onSubmit={async (name) => {
            await backend.subjectCreate(name);
            setCreating(false);
            await refresh(showArchived);
          }}
        />
      )}

      {renaming && (
        <NamePrompt
          title="Rename subject"
          label="Subject name"
          initial={renaming.name}
          confirmLabel="Save"
          onCancel={() => setRenaming(null)}
          onSubmit={async (name) => {
            await backend.subjectRename(renaming.id, name);
            setRenaming(null);
            await refresh(showArchived);
          }}
        />
      )}
    </>
  );
}

/**
 * One-field prompt. Backend validation errors are shown in place, so the typed
 * name is never lost.
 */
export function NamePrompt({
  title,
  label,
  initial = "",
  placeholder,
  confirmLabel,
  onCancel,
  onSubmit,
}: {
  title: string;
  label: string;
  initial?: string;
  placeholder?: string;
  confirmLabel: string;
  onCancel: () => void;
  onSubmit: (value: string) => Promise<void>;
}) {
  const [value, setValue] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    try {
      await onSubmit(value);
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
        <span className="field__label">{label}</span>
        <input
          className="input"
          value={value}
          placeholder={placeholder}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !busy) void submit();
          }}
        />
      </div>
      {error && <Banner kind="error">{error}</Banner>}
    </Modal>
  );
}
