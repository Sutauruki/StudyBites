import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  backend,
  errorText,
  type DiagramRect,
  type MaterialPage,
} from "../api";
import { Banner, Modal } from "../components/ui";

type ReviewPage = MaterialPage & { imageUrl: string | null };

export function PhotoReviewDialog({
  materialId,
  onClose,
  onConfirmed,
}: {
  materialId: number;
  onClose: () => void;
  onConfirmed: () => void;
}) {
  const [pages, setPages] = useState<ReviewPage[]>([]);
  const [diagramMode, setDiagramMode] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const stored = await backend.materialPagesGet(materialId);
        const loaded = await Promise.all(
          stored.map(async (page) => {
            const image = await backend.materialPageImageGet(page.id);
            return {
              ...page,
              imageUrl: image ? `data:image/jpeg;base64,${image}` : null,
            };
          }),
        );
        if (active) setPages(loaded);
      } catch (err) {
        if (active) setError(errorText(err));
      }
    })();
    return () => {
      active = false;
    };
  }, [materialId]);

  const updatePage = (pageId: number, patch: Partial<ReviewPage>) => {
    setPages((current) =>
      current.map((page) => (page.id === pageId ? { ...page, ...patch } : page)),
    );
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      for (const page of pages) {
        await backend.materialPageTextSet(page.id, page.transcript ?? "");
        await backend.materialDiagramsSet(page.id, page.diagrams);
      }
      await backend.materialTranscriptConfirm(materialId);
      onConfirmed();
      onClose();
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  };

  return (
    <Modal
      title="Review your notes"
      onClose={onClose}
      wide
      actions={
        <>
          <button className="btn btn--ghost" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn btn--primary" onClick={save} disabled={busy || pages.length === 0}>
            {busy ? "Saving…" : "Looks right"}
          </button>
        </>
      }
    >
      <p className="muted">
        Check the wording against each photo. Edit anything that is wrong before continuing.
      </p>
      <button
        className={`btn${diagramMode ? " btn--primary" : ""}`}
        aria-pressed={diagramMode}
        onClick={() => setDiagramMode((current) => !current)}
        disabled={busy}
      >
        {diagramMode ? "Finish marking diagrams" : "Mark a diagram"}
      </button>
      {error && <Banner kind="error">{error}</Banner>}
      {pages.length === 0 && !error && <p className="muted">Loading your pages…</p>}
      <div className="photo-review">
        {pages.map((page) => (
          <section className="photo-review__page card card--pad" key={page.id}>
            <h3>Page {page.pageIndex + 1}</h3>
            <div className="photo-review__image">
              {page.imageUrl ? (
                <>
                  <img src={page.imageUrl} alt={`Uploaded notes, page ${page.pageIndex + 1}`} />
                  <DiagramOverlay
                    rects={page.diagrams}
                    enabled={diagramMode}
                    onAdd={(rect) => updatePage(page.id, { diagrams: [...page.diagrams, rect] })}
                  />
                </>
              ) : (
                <p className="muted">Page image preview is not available in the browser preview.</p>
              )}
            </div>
            {page.uncertain.length > 0 && (
              <p className="chip chip--accent">
                Check highlighted [?] words against your page.
              </p>
            )}
            <p className="photo-review__transcript" aria-label="Transcript with uncertain words highlighted">
              {(page.transcript ?? "").split(/(\[\?\])/g).map((part, index) =>
                part === "[?]" ? (
                  <mark key={index}>[?]</mark>
                ) : (
                  <span key={index}>{part}</span>
                ),
              )}
            </p>
            <label className="field">
              <span className="field__label">Edit transcript</span>
              <textarea
                className="input photo-review__textarea"
                value={page.transcript ?? ""}
                onChange={(event) => updatePage(page.id, { transcript: event.target.value })}
                disabled={busy}
              />
            </label>
          </section>
        ))}
      </div>
    </Modal>
  );
}

function DiagramOverlay({
  rects,
  enabled,
  onAdd,
}: {
  rects: DiagramRect[];
  enabled: boolean;
  onAdd: (rect: DiagramRect) => void;
}) {
  const svg = useRef<SVGSVGElement>(null);
  const start = useRef<{ x: number; y: number } | null>(null);
  const [draft, setDraft] = useState<DiagramRect | null>(null);

  const point = (event: ReactPointerEvent<SVGSVGElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)),
      y: Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height)),
    };
  };

  const finish = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!start.current) return;
    const end = point(event);
    const x = Math.min(start.current.x, end.x);
    const y = Math.min(start.current.y, end.y);
    const rect = {
      x,
      y,
      width: Math.abs(start.current.x - end.x),
      height: Math.abs(start.current.y - end.y),
    };
    start.current = null;
    setDraft(null);
    if (rect.width > 0.01 && rect.height > 0.01) onAdd(rect);
  };

  return (
    <svg
      ref={svg}
      className={`photo-review__overlay${enabled ? " photo-review__overlay--active" : ""}`}
      viewBox="0 0 1 1"
      preserveAspectRatio="none"
      onPointerDown={(event) => {
        if (!enabled) return;
        const origin = point(event);
        start.current = origin;
        event.currentTarget.setPointerCapture(event.pointerId);
        setDraft({ ...origin, width: 0, height: 0 });
      }}
      onPointerMove={(event) => {
        if (!start.current) return;
        const current = point(event);
        setDraft({
          x: Math.min(start.current.x, current.x),
          y: Math.min(start.current.y, current.y),
          width: Math.abs(start.current.x - current.x),
          height: Math.abs(start.current.y - current.y),
        });
      }}
      onPointerUp={finish}
    >
      {rects.map((rect, index) => (
        <rect
          key={index}
          x={rect.x}
          y={rect.y}
          width={rect.width}
          height={rect.height}
          vectorEffect="non-scaling-stroke"
        />
      ))}
      {draft && (
        <rect
          className="photo-review__draft"
          x={draft.x}
          y={draft.y}
          width={draft.width}
          height={draft.height}
          vectorEffect="non-scaling-stroke"
        />
      )}
    </svg>
  );
}
