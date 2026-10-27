import { useEffect, useState } from "react";
import { backend, errorText, type Topic, type TopicMatch } from "../api";
import { Banner, Modal } from "../components/ui";

/** Flatten the outline so a topic can be picked from one list. */
function flatten(topics: Topic[]): Array<{ id: number; label: string }> {
  const out: Array<{ id: number; label: string }> = [];
  for (const topic of topics) {
    out.push({ id: topic.id, label: topic.title });
    for (const child of topic.children) {
      out.push({ id: child.id, label: `   ${child.title}` });
    }
  }
  return out;
}

/**
 * Upload a file and confirm its topic (Brief 3.3).
 *
 * The app never files material silently: the AI pre-selects a topic when it
 * can, and the student confirms, picks another, or describes something else.
 */
export function UploadDialog({
  subjectId,
  topics,
  onClose,
  onUploaded,
}: {
  subjectId: number;
  topics: Topic[];
  onClose: () => void;
  onUploaded: (message: string) => void;
}) {
  const [path, setPath] = useState<string | null>(null);
  const [suggestion, setSuggestion] = useState<TopicMatch | null>(null);
  const [topicId, setTopicId] = useState<number | null>(null);
  const [somethingElse, setSomethingElse] = useState("");
  const [useSomethingElse, setUseSomethingElse] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const options = flatten(topics);

  // Pick the file as soon as the dialog opens — that is why it was opened.
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const picked = await backend.pickFile();
        if (!alive) return;
        if (!picked) {
          onClose();
          return;
        }
        setPath(picked);

        if (options.length === 0) return;
        setStatus("Reading the file and matching it to your outline…");
        const text = await backend.filePreviewText(picked);
        const matched = await backend.topicSuggest(subjectId, text);
        if (!alive) return;
        setSuggestion(matched);
        setStatus(null);
        if (matched.index !== null && options[matched.index]) {
          setTopicId(options[matched.index].id);
        }
      } catch (err) {
        // A failed suggestion is not fatal: the student picks the topic.
        if (alive) {
          setStatus(`Could not suggest a topic — pick one below. (${errorText(err)})`);
        }
      }
    })();
    return () => {
      alive = false;
    };
    // Runs once per dialog; choosing another file means reopening it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const submit = async () => {
    if (!path) return;
    setBusy(true);
    setError(null);
    try {
      let chosen = topicId;

      // "Something else" creates an off-scheme topic (Brief 3.3).
      if (useSomethingElse && somethingElse.trim()) {
        const created = await backend.topicCreate({
          subjectId,
          title: somethingElse.trim(),
          isOffScheme: true,
        });
        chosen = created.id;
      }

      const accepted = await backend.materialUpload(subjectId, chosen, path);
      if (accepted.duplicateOf !== null) {
        onUploaded("You have already uploaded that file, so nothing was re-processed.");
      } else {
        onUploaded(
          `Added “${accepted.material.originalFilename}”. Processing ${accepted.chunkCount} ` +
            `section${accepted.chunkCount === 1 ? "" : "s"} in the background.`,
        );
      }
      onClose();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  const filename = path?.split(/[\\/]/).pop() ?? "";
  const suggestedLabel =
    suggestion?.index !== null && suggestion?.index !== undefined
      ? options[suggestion.index]?.label.trim()
      : undefined;

  return (
    <Modal
      title="Add class material"
      onClose={onClose}
      actions={
        <>
          <button className="btn btn--ghost" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn btn--primary" onClick={submit} disabled={busy || !path}>
            {busy ? "Adding…" : "Confirm and add"}
          </button>
        </>
      }
    >
      {path ? <p className="muted">{filename}</p> : <p className="muted">Choosing a file…</p>}

      {status && <Banner kind="info">{status}</Banner>}

      {options.length > 0 && (
        <>
          {suggestedLabel && (
            <Banner kind="info">
              Suggested: {suggestedLabel}
              {suggestion && suggestion.confidence < 0.6 && " (not very sure)"}
              {suggestion?.reason ? ` — ${suggestion.reason}` : ""}
            </Banner>
          )}

          <div className="field">
            <span className="field__label">Which topic is this?</span>
            <select
              className="input"
              value={useSomethingElse ? "else" : (topicId ?? "")}
              onChange={(e) => {
                if (e.target.value === "else") {
                  setUseSomethingElse(true);
                } else {
                  setUseSomethingElse(false);
                  setTopicId(e.target.value === "" ? null : Number(e.target.value));
                }
              }}
            >
              <option value="">Not sure yet</option>
              {options.map((option) => (
                <option key={option.id} value={option.id}>
                  {option.label}
                </option>
              ))}
              <option value="else">Something else…</option>
            </select>
          </div>
        </>
      )}

      {(useSomethingElse || options.length === 0) && (
        <div className="field">
          <span className="field__label">
            {options.length === 0 ? "Topic (optional)" : "Describe it"}
          </span>
          <input
            className="input"
            value={somethingElse}
            placeholder="Revision on logarithms"
            onChange={(e) => {
              setSomethingElse(e.target.value);
              setUseSomethingElse(true);
            }}
          />
        </div>
      )}

      {error && <Banner kind="error">{error}</Banner>}
    </Modal>
  );
}
