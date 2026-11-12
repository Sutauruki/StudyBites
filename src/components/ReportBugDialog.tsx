import { useEffect, useMemo, useState, type ChangeEvent } from "react";
import {
  backend,
  errorText,
  type BugReport,
  type BugReportCategory,
  type BugReportResult,
} from "../api";
import { collectDiagnostics } from "../lib/diag";
import { resizeImage } from "../lib/image";
import { Banner, Modal } from "./ui";

const DEVICE_ID_KEY = "studybites.bugReport.deviceId.v1";
const MAX_ATTACHMENT_BYTES = 1_258_291;
const MAX_REPORT_BYTES = 4 * 1024 * 1024;
const ACCEPTED_IMAGE_TYPES = new Set(["image/jpeg", "image/png", "image/webp"]);

function createUuid(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function getDeviceId(): string {
  const saved = localStorage.getItem(DEVICE_ID_KEY);
  if (
    saved &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(saved)
  ) {
    return saved;
  }
  const id = createUuid();
  localStorage.setItem(DEVICE_ID_KEY, id);
  return id;
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 32_768) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 32_768));
  }
  return btoa(binary);
}

export function ReportBugDialog({
  version,
  view,
  licenseState,
  onClose,
}: {
  version: string;
  view: string;
  licenseState: string;
  onClose: () => void;
}) {
  const [available, setAvailable] = useState<boolean | null>(null);
  const [category, setCategory] = useState<BugReportCategory>("bug");
  const [title, setTitle] = useState("");
  const [details, setDetails] = useState("");
  const [steps, setSteps] = useState("");
  const [contact, setContact] = useState("");
  const [deviceId, setDeviceId] = useState<string | null>(null);
  const [includeDiagnostics, setIncludeDiagnostics] = useState(true);
  const [files, setFiles] = useState<File[]>([]);
  const [diagnostics, setDiagnostics] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<BugReportResult | null>(null);
  const [copied, setCopied] = useState(false);
  const previews = useMemo(() => files.map((file) => URL.createObjectURL(file)), [files]);

  useEffect(() => {
    setDiagnostics(collectDiagnostics({ appVersion: version, view, licenseState }));
  }, [licenseState, version, view]);

  useEffect(() => {
    try {
      setDeviceId(getDeviceId());
    } catch (err) {
      setError(errorText(err));
    }
  }, []);

  useEffect(() => {
    void backend
      .bugReportAvailable()
      .then(setAvailable)
      .catch((err) => {
        setAvailable(false);
        setError(errorText(err));
      });
  }, []);

  useEffect(() => {
    return () => previews.forEach((url) => URL.revokeObjectURL(url));
  }, [previews]);

  const readFiles = (event: ChangeEvent<HTMLInputElement>) => {
    const selected = Array.from(event.currentTarget.files ?? []);
    event.currentTarget.value = "";
    setError(null);
    if (files.length + selected.length > 3) {
      setError("Choose no more than 3 screenshots.");
      return;
    }
    if (selected.some((file) => !ACCEPTED_IMAGE_TYPES.has(file.type))) {
      setError("Screenshots must be JPEG, PNG or WebP images.");
      return;
    }
    setFiles((current) => [...current, ...selected]);
  };

  const buildReport = async (): Promise<BugReport> => {
    if (!deviceId) throw new Error("A local device identifier could not be created.");
    const attachments = await Promise.all(
      files.map(async (file, index) => {
        const bytes = await resizeImage(file, 1280, 0.7);
        if (bytes.length > MAX_ATTACHMENT_BYTES) {
          throw new Error("A screenshot is larger than 1.2 MB after resizing.");
        }
        return {
          filename: `screenshot-${index + 1}.jpg`,
          mime: "image/jpeg" as const,
          data: encodeBase64(bytes),
        };
      }),
    );
    const report: BugReport = {
      id: createUuid(),
      deviceId,
      title: title.trim(),
      details: details.trim(),
      ...(steps.trim() ? { steps: steps.trim() } : {}),
      ...(contact.trim() ? { contact: contact.trim() } : {}),
      category,
      diag: includeDiagnostics ? diagnostics : {},
      attachments,
    };
    if (new TextEncoder().encode(JSON.stringify(report)).length > MAX_REPORT_BYTES) {
      throw new Error("The report is larger than 4 MB. Remove a screenshot and try again.");
    }
    return report;
  };

  const copyReport = async () => {
    setBusy(true);
    setError(null);
    try {
      const report = await buildReport();
      const text = JSON.stringify(report, null, 2);
      try {
        if (!navigator.clipboard?.writeText) throw new Error("Clipboard API unavailable.");
        await navigator.clipboard.writeText(text);
      } catch {
        const field = document.createElement("textarea");
        field.value = text;
        field.style.position = "fixed";
        field.style.opacity = "0";
        document.body.append(field);
        let copiedToClipboard = false;
        try {
          field.select();
          copiedToClipboard = document.execCommand("copy");
        } finally {
          field.remove();
        }
        if (!copiedToClipboard) throw new Error("Clipboard access is not available.");
      }
      setCopied(true);
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const sendReport = async () => {
    setBusy(true);
    setError(null);
    try {
      const report = await buildReport();
      const next = await backend.bugReportSend(report);
      if (next.status === "rejected") {
        setError(next.message);
      } else {
        setResult(next);
      }
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  const finished = result?.status === "sent" || result?.status === "queued";
  const actions = finished ? (
    <button className="btn btn--primary" onClick={onClose}>Close</button>
  ) : (
    <>
      <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
      <button
        className={available === false ? "btn btn--primary" : "btn"}
        onClick={copyReport}
        disabled={busy || !deviceId || title.trim().length === 0 || details.trim().length === 0}
      >
        {copied ? "Copied" : "Copy report"}
      </button>
      {available && (
        <button
          className="btn btn--primary"
          onClick={sendReport}
          disabled={busy || !deviceId || title.trim().length === 0 || details.trim().length === 0}
        >
          {busy ? "Sending…" : "Send report"}
        </button>
      )}
    </>
  );

  return (
    <Modal title="Report a problem" onClose={onClose} actions={actions}>
      <div className="bug-report stack">
        <p className="muted">
          Share what happened so we can improve StudyBites. Technical details never include notes,
          cards, subject or file names, API keys, or licence codes. A random device ID is included
          for daily report limits and is not emailed.
        </p>
        <label className="field">
          <span className="field__label">Type</span>
          <select
            className="input"
            value={category}
            onChange={(event) => {
              const next = event.target.value;
              if (next === "bug" || next === "confusing" || next === "idea") {
                setCategory(next);
              }
            }}
            disabled={finished}
          >
            <option value="bug">Something is broken</option>
            <option value="confusing">Something is confusing</option>
            <option value="idea">I have an idea</option>
          </select>
        </label>
        <label className="field">
          <span className="field__label">Short summary</span>
          <input
            className="input"
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            maxLength={120}
            required
            disabled={finished}
          />
        </label>
        <label className="field">
          <span className="field__label">What happened?</span>
          <textarea
            className="input bug-report__textarea"
            value={details}
            onChange={(event) => setDetails(event.target.value)}
            maxLength={2000}
            required
            disabled={finished}
          />
        </label>
        <label className="field">
          <span className="field__label">Steps to reproduce (optional)</span>
          <textarea
            className="input bug-report__textarea"
            value={steps}
            onChange={(event) => setSteps(event.target.value)}
            maxLength={1500}
            disabled={finished}
          />
        </label>
        <label className="field">
          <span className="field__label">Email for a reply (optional)</span>
          <input
            className="input"
            type="email"
            value={contact}
            onChange={(event) => setContact(event.target.value)}
            maxLength={120}
            autoComplete="email"
            disabled={finished}
          />
        </label>
        <div className="field">
          <span className="field__label">Screenshots (up to 3)</span>
          <label className="btn bug-report__pick">
            Add screenshots
            <input
              className="bug-report__file-input"
              type="file"
              accept="image/*"
              multiple
              onChange={readFiles}
              disabled={finished}
            />
          </label>
          {previews.length > 0 && (
            <div className="bug-report__screenshots">
              {previews.map((url, index) => (
                <div className="bug-report__screenshot" key={url}>
                  <img src={url} alt={`Screenshot ${index + 1}`} />
                  {!finished && (
                    <button
                      type="button"
                      className="btn"
                      onClick={() => setFiles((current) => current.filter((_, i) => i !== index))}
                    >
                      Remove
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
        <label className="row bug-report__consent">
          <input
            type="checkbox"
            checked={includeDiagnostics}
            onChange={(event) => setIncludeDiagnostics(event.target.checked)}
            disabled={finished}
          />
          <span>Include technical details (recommended)</span>
        </label>
        <details className="bug-report__details">
          <summary>See the technical details and device ID</summary>
          <pre>
            {JSON.stringify(
              { deviceId: deviceId ?? "unavailable", diag: includeDiagnostics ? diagnostics : {} },
              null,
              2,
            )}
          </pre>
        </details>
        {available === false && (
          <Banner kind="info">Sending is not configured in this build. Copy the report to share it.</Banner>
        )}
        {available === null && <p className="muted">Checking report delivery…</p>}
        {error && <Banner kind="error">{error}</Banner>}
        {result?.status === "sent" && <Banner kind="info">Report sent. Thank you.</Banner>}
        {result?.status === "queued" && (
          <Banner kind="info">Report saved. It will send when you are online.</Banner>
        )}
        {result?.status === "rateLimited" && (
          <Banner kind="info">Too many reports have been sent today. Please try again tomorrow.</Banner>
        )}
      </div>
    </Modal>
  );
}
