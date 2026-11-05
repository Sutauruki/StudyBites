import { useEffect, useState } from "react";
import { backend, errorText, type LicenseStatus } from "../api";
import { Banner, Loading } from "../components/ui";

/**
 * First launch: the agreement, then activation (Brief 8.2 and 13).
 *
 * Both are gates, so they own the whole window rather than sitting inside the
 * app shell — there is no sidebar to navigate to yet, and offering one would
 * only invite people to look for a way around.
 */
export function OnboardingPage({
  status,
  onChanged,
}: {
  status: LicenseStatus;
  onChanged: (status: LicenseStatus) => void;
}) {
  if (!status.eulaAccepted) {
    return <EulaGate onAccepted={onChanged} />;
  }
  return <ActivationGate status={status} onActivated={onChanged} />;
}

function EulaGate({ onAccepted }: { onAccepted: (status: LicenseStatus) => void }) {
  const [text, setText] = useState<string | null>(null);
  const [agreed, setAgreed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void backend
      .eulaText()
      .then(setText)
      .catch((err) => setError(errorText(err)));
  }, []);

  return (
    <div className="gate">
      <div className="gate__inner">
        <h1 className="display">Before you start</h1>
        <p className="gate__lede">
          StudyBites is licensed to you for one device, your notes stay on this device, and AI can
          be wrong, so check it against your course material.
        </p>

        {error && <Banner kind="error">{error}</Banner>}

        {text === null && !error ? (
          <Loading label="Opening the agreement…" />
        ) : (
          <article className="gate__doc selectable" tabIndex={0}>
            <Prose markdown={text ?? ""} />
          </article>
        )}

        <label className="gate__agree">
          <input
            type="checkbox"
            checked={agreed}
            onChange={(e) => setAgreed(e.target.checked)}
          />
          <span>I have read and accept this agreement.</span>
        </label>

        <div className="action">
          <button
            className="btn btn--primary btn--big"
            disabled={!agreed || busy || text === null}
            onClick={async () => {
              setBusy(true);
              try {
                onAccepted(await backend.eulaAccept());
              } catch (err) {
                setError(errorText(err));
                setBusy(false);
              }
            }}
          >
            {busy ? "One moment…" : "Accept and continue"}
          </button>
        </div>
      </div>
    </div>
  );
}

function ActivationGate({
  status,
  onActivated,
}: {
  status: LicenseStatus;
  onActivated: (status: LicenseStatus) => void;
}) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Nothing to identify the machine by means nothing can be activated. Say so
  // rather than showing a box that will always reject what is typed into it.
  if (status.problem) {
    return (
      <div className="gate">
        <div className="gate__inner">
          <h1 className="display">StudyBites cannot run here</h1>
          <Banner kind="error">{status.problem}</Banner>
        </div>
      </div>
    );
  }

  return (
    <div className="gate">
      <div className="gate__inner">
        <h1 className="display">Activate StudyBites</h1>
        <p className="gate__lede">
          Send your request code to the owner, then paste the activation code they send back.
        </p>

        <section className="stack">
          <span className="label">1 · Your request code</span>
          <div className="gate__code selectable">{status.requestCode ?? "—"}</div>
          <div className="action">
            <button
              className="btn"
              disabled={!status.requestCode}
              onClick={async () => {
                if (!status.requestCode) return;
                try {
                  await navigator.clipboard.writeText(status.requestCode);
                  setCopied(true);
                  window.setTimeout(() => setCopied(false), 2000);
                } catch {
                  /* clipboard can be refused; the code is selectable anyway */
                }
              }}
            >
              {copied ? "Copied" : "Copy"}
            </button>
            <span className="muted">Safe to share. It only identifies this device.</span>
          </div>
        </section>

        <section className="stack">
          <span className="label">2 · Your activation code</span>
          <textarea
            className="input gate__paste selectable"
            value={code}
            rows={4}
            spellCheck={false}
            autoComplete="off"
            placeholder="Paste your activation code"
            onChange={(e) => setCode(e.target.value)}
          />
        </section>

        {error && <Banner kind="error">{error}</Banner>}

        <div className="action">
          <button
            className="btn btn--primary btn--big"
            disabled={busy || code.trim().length === 0}
            onClick={async () => {
              setBusy(true);
              setError(null);
              try {
                onActivated(await backend.activate(code));
              } catch (err) {
                setError(errorText(err));
              } finally {
                setBusy(false);
              }
            }}
          >
            {busy ? "Checking…" : "Activate"}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * The agreement is the owner's own Markdown file, so it is rendered rather
 * than pasted in as a wall of text. Deliberately tiny: headings, bold, lists
 * and paragraphs, and nothing that could execute — no HTML is ever inserted.
 */
export function Prose({ markdown }: { markdown: string }) {
  const blocks: JSX.Element[] = [];
  let list: string[] = [];

  const flushList = () => {
    if (list.length === 0) return;
    blocks.push(
      <ul key={`ul-${blocks.length}`}>
        {list.map((item, i) => (
          <li key={i}>{inline(item)}</li>
        ))}
      </ul>,
    );
    list = [];
  };

  for (const raw of markdown.split("\n")) {
    const line = raw.trimEnd();
    if (line.startsWith("- ")) {
      list.push(line.slice(2));
      continue;
    }
    flushList();
    if (line.trim() === "") continue;

    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      const text = inline(heading[2]);
      const key = `h-${blocks.length}`;
      blocks.push(
        level <= 1 ? (
          <h2 key={key}>{text}</h2>
        ) : level === 2 ? (
          <h3 key={key}>{text}</h3>
        ) : (
          <h4 key={key}>{text}</h4>
        ),
      );
      continue;
    }
    if (line.startsWith("> ")) {
      blocks.push(
        <blockquote key={`q-${blocks.length}`}>{inline(line.slice(2))}</blockquote>,
      );
      continue;
    }
    blocks.push(<p key={`p-${blocks.length}`}>{inline(line)}</p>);
  }
  flushList();

  return <>{blocks}</>;
}

/** `**bold**` and `` `code` ``, as text nodes. Never HTML. */
function inline(text: string): JSX.Element {
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`)/g);
  return (
    <>
      {parts.map((part, i) => {
        if (part.startsWith("**") && part.endsWith("**")) {
          return <strong key={i}>{part.slice(2, -2)}</strong>;
        }
        if (part.startsWith("`") && part.endsWith("`") && part.length > 1) {
          return <code key={i}>{part.slice(1, -1)}</code>;
        }
        return <span key={i}>{part}</span>;
      })}
    </>
  );
}
