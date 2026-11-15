import { useEffect, useRef, useState } from "react";
import { backend, errorText, type Topic, type TopicMatch } from "../api";
import { Banner, Modal } from "../components/ui";
import { resizeImage } from "../lib/image";

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
  const [mode, setMode] = useState<"choose" | "document" | "photos">("choose");
  const [path, setPath] = useState<string | null>(null);
  const [photos, setPhotos] = useState<File[]>([]);
  const [photoUrls, setPhotoUrls] = useState<string[]>([]);
  const [suggestion, setSuggestion] = useState<TopicMatch | null>(null);
  const [topicId, setTopicId] = useState<number | null>(null);
  const [somethingElse, setSomethingElse] = useState("");
  const [useSomethingElse, setUseSomethingElse] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const photoInput = useRef<HTMLInputElement>(null);

  const options = flatten(topics);

  useEffect(() => {
    const urls = photos.map((file) => URL.createObjectURL(file));
    setPhotoUrls(urls);
    return () => urls.forEach((url) => URL.revokeObjectURL(url));
  }, [photos]);

  const pickDocument = async () => {
    setMode("document");
    setStatus(null);
    setError(null);
    try {
      const picked = await backend.pickFile();
      if (!picked) {
        setMode("choose");
        return;
      }
      setPath(picked);
      if (options.length === 0) return;
      setStatus("Reading the file and matching it to your outline…");
      try {
        const text = await backend.filePreviewText(picked);
        const matched = await backend.topicSuggest(subjectId, text);
        setSuggestion(matched);
        if (matched.index !== null && options[matched.index]) {
          setTopicId(options[matched.index].id);
        }
      } catch (err) {
        setStatus(`Could not suggest a topic — pick one below. (${errorText(err)})`);
      } finally {
        setStatus((current) =>
          current?.startsWith("Reading the file") ? null : current,
        );
      }
    } catch (err) {
      setError(errorText(err));
      setMode("choose");
    }
  };

  const onPhotoChange = (files: FileList | null) => {
    const selected = Array.from(files ?? []);
    if (selected.length < 1 || selected.length > 20) {
      setError("Choose between 1 and 20 photos.");
      return;
    }
    if (selected.some((file) => !["image/jpeg", "image/png", "image/webp"].includes(file.type))) {
      setError("Choose JPEG, PNG or WebP photos.");
      return;
    }
    setError(null);
    setMode("photos");
    setPath(null);
    setPhotos(selected);
    setTopicId(null);
    setUseSomethingElse(false);
    setSuggestion(null);
  };

  const movePhoto = (index: number, delta: number) => {
    setPhotos((current) => {
      const next = [...current];
      const destination = index + delta;
      if (destination < 0 || destination >= next.length) return current;
      [next[index], next[destination]] = [next[destination], next[index]];
      return next;
    });
  };

  const submit = async () => {
    if (mode === "document" && !path) return;
    if (mode === "photos" && photos.length === 0) return;
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

      const accepted =
        mode === "photos"
          ? await withTimeout(
              Promise.all(photos.map((photo) => resizeImage(photo, 1600, 0.8))).then((pages) => {
                if (pages.some((page) => page.byteLength > 4 * 1024 * 1024)) {
                  throw new Error(
                    "Each photo must be 4 MB or smaller after resizing. Choose fewer or smaller photos.",
                  );
                }
                return backend.materialUploadPhotos(subjectId, chosen, pages);
              }),
              120_000,
              "The photos took too long to add. Try fewer photos.",
            )
          : await withTimeout(
              backend.materialUpload(subjectId, chosen, path!),
              120_000,
              "The upload took too long. Check that the file is under 64 MB and try again.",
            );
      if (accepted.duplicateOf !== null) {
        onUploaded("You have already uploaded that file, so nothing was re-processed.");
      } else {
        onUploaded(
          mode === "photos"
            ? `Added “${accepted.material.originalFilename}”. The photos are waiting for transcription.`
            : `Added “${accepted.material.originalFilename}”. Processing ${accepted.chunkCount} ` +
                `section${accepted.chunkCount === 1 ? "" : "s"} in the background.`,
        );
      }
      onClose();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const timer = window.setTimeout(() => reject(new Error(message)), timeoutMs);
      promise.then(
        (value) => {
          window.clearTimeout(timer);
          resolve(value);
        },
        (error) => {
          window.clearTimeout(timer);
          reject(error);
        },
      );
    });
  }

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
          <button
            className="btn btn--primary"
            onClick={submit}
            disabled={busy || (mode === "document" ? !path : mode === "photos" ? !photos.length : true)}
          >
            {busy ? "Adding…" : "Confirm and add"}
          </button>
        </>
      }
    >
      {mode === "choose" && (
        <div className="stack">
          <p className="muted">Choose a document or photos of your notes.</p>
          <div className="row row--wrap">
            <button className="btn" onClick={() => void pickDocument()}>
              Choose a document
            </button>
            <button
              className="btn"
              onClick={() => {
                setMode("photos");
                photoInput.current?.click();
              }}
            >
              Choose photos
            </button>
          </div>
        </div>
      )}

      <input
        ref={photoInput}
        type="file"
        accept="image/jpeg,image/png,image/webp"
        capture="environment"
        multiple
        hidden
        onChange={(event) => {
          onPhotoChange(event.currentTarget.files);
          event.currentTarget.value = "";
        }}
      />

      {mode === "document" && (
        <div className="row row--wrap">
          <p className="muted">{path ? filename : "Choosing a document…"}</p>
          <button className="btn btn--ghost" onClick={() => setMode("choose")} disabled={busy}>
            Choose photos instead
          </button>
        </div>
      )}

      {mode === "photos" && (
        <div className="stack">
          <div className="row row--wrap">
            <span className="muted">{photos.length} photo{photos.length === 1 ? "" : "s"}</span>
            <button
              className="btn btn--ghost"
              onClick={() => photoInput.current?.click()}
              disabled={busy}
            >
              Add photos
            </button>
            <button className="btn btn--ghost" onClick={() => setMode("choose")} disabled={busy}>
              Choose a document instead
            </button>
          </div>
          {photos.map((photo, index) => (
            <div className="row row--wrap" key={`${photo.name}-${index}`}>
              <img
                src={photoUrls[index]}
                alt={`Photo ${index + 1}`}
                className="photo-upload__thumbnail"
              />
              <span className="muted">{photo.name || `Photo ${index + 1}`}</span>
              <button
                className="btn btn--ghost"
                aria-label={`Move photo ${index + 1} up`}
                disabled={index === 0 || busy}
                onClick={() => movePhoto(index, -1)}
              >
                Move up
              </button>
              <button
                className="btn btn--ghost"
                aria-label={`Move photo ${index + 1} down`}
                disabled={index === photos.length - 1 || busy}
                onClick={() => movePhoto(index, 1)}
              >
                Move down
              </button>
              <button
                className="btn btn--danger"
                disabled={busy}
                onClick={() => setPhotos((current) => current.filter((_, i) => i !== index))}
              >
                Remove
              </button>
            </div>
          ))}
        </div>
      )}

      {status && <Banner kind="info">{status}</Banner>}

      {mode !== "choose" && options.length > 0 && (
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

      {mode !== "choose" && (useSomethingElse || options.length === 0) && (
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
