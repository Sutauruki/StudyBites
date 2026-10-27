import { useCallback, useEffect, useState } from "react";
import {
  backend,
  errorText,
  isDesktop,
  type ProviderInfo,
  type QueueStatus,
  type UpdateStatus,
} from "../api";
import type { Theme } from "../App";
import { Banner } from "../components/ui";

const THEMES: Array<{ value: Theme; label: string }> = [
  { value: "system", label: "Follow system" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function SettingsPage({
  theme,
  onThemeChange,
  version,
}: {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  version: string;
}) {
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [queue, setQueue] = useState<QueueStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [list, status] = await Promise.all([backend.providersList(), backend.queueStatus()]);
      setProviders(list);
      setQueue(status);
    } catch (err) {
      setError(errorText(err));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <>
      <div className="page-head">
        <div className="page-head__text">
          <h1>Settings</h1>
          <p className="page-head__sub">StudyBites {version || "…"}</p>
        </div>
      </div>

      {error && <Banner kind="error">{error}</Banner>}

      <section className="card card--pad">
        <div className="field">
          <span className="field__label">Theme</span>
          <div className="segmented">
            {THEMES.map((option) => (
              <button
                key={option.value}
                className="btn"
                aria-pressed={theme === option.value}
                onClick={() => onThemeChange(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
      </section>

      <section className="stack">
        <h2>AI keys</h2>
        <p className="muted">
          StudyBites uses your own free API key, stored in Windows&rsquo; own credential store —
          never in the app&rsquo;s database, and never sent anywhere except the provider you
          choose. Gemini is tried first, then the others if it runs out for the day.
        </p>
        <div className="card list">
          {providers.map((provider) => (
            <KeyRow key={provider.id} provider={provider} onSaved={refresh} onError={setError} />
          ))}
        </div>
      </section>

      {queue && queue.quota.length > 0 && (
        <section className="stack">
          <h2>Today&rsquo;s usage</h2>
          <div className="card list">
            {queue.quota.map((usage) => (
              <div key={usage.provider} className="list__item list__item--static">
                <div className="list__text">
                  <div className="list__title">{usage.provider}</div>
                  <div className="list__meta">
                    {usage.requests} request{usage.requests === 1 ? "" : "s"}
                    {usage.exhaustedUntil
                      ? ` · limit reached, resets ${new Date(
                          usage.exhaustedUntil,
                        ).toLocaleString()}`
                      : ""}
                  </div>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      <UpdateSection />

      <section className="card card--pad">
        <div className="stack">
          <h2>Privacy</h2>
          <p className="muted">
            Your notes stay on this device. There is no telemetry, no analytics and no crash
            reporting. Material is sent only to the AI provider you configure, using your key.
          </p>
          <p className="muted">
            Storage: {isDesktop ? "encrypted database on this device" : "browser mock data"}
          </p>
        </div>
      </section>
    </>
  );
}

function KeyRow({
  provider,
  onSaved,
  onError,
}: {
  provider: ProviderInfo;
  onSaved: () => void;
  onError: (message: string) => void;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [models, setModels] = useState<string[] | null>(null);
  const [loadingModels, setLoadingModels] = useState(false);

  const save = async () => {
    setBusy(true);
    try {
      await backend.providerKeySet(provider.id, value);
      setValue("");
      setSaved(true);
      onSaved();
      window.setTimeout(() => setSaved(false), 2500);
    } catch (err) {
      onError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="key-row">
      <div className="key-row__head">
        <span className="key-row__name">{provider.label}</span>
        {provider.configured ? (
          <span className="chip chip--ok">Key saved</span>
        ) : (
          <span className="chip">No key</span>
        )}
      </div>

      <div className="key-row__meta">
        {provider.defaultModel ? (
          <span>Model: {provider.defaultModel}</span>
        ) : (
          <span>No model chosen yet</span>
        )}
        <span aria-hidden="true">·</span>
        <button className="link" onClick={() => void backend.openUrl(provider.helpUrl)}>
          Get a free key
        </button>
      </div>

      {/* Models are listed by the provider itself, so a retired one can
          always be swapped without waiting for a new build. */}
      <div className="key-row__controls">
        {models === null ? (
          <button
            className="btn"
            disabled={loadingModels}
            onClick={async () => {
              setLoadingModels(true);
              try {
                setModels(await backend.providerModels(provider.id));
              } catch (err) {
                onError(errorText(err));
              } finally {
                setLoadingModels(false);
              }
            }}
          >
            {loadingModels ? "Loading models…" : "Choose model"}
          </button>
        ) : (
          <select
            className="input"
            value={provider.defaultModel}
            onChange={async (e) => {
              try {
                await backend.providerModelSet(provider.id, e.target.value);
                onSaved();
              } catch (err) {
                onError(errorText(err));
              }
            }}
          >
            <option value="">Choose a model…</option>
            {models.map((model) => (
              <option key={model} value={model}>
                {model}
              </option>
            ))}
          </select>
        )}
      </div>

      <div className="key-row__controls">
        <input
          className="input"
          type="password"
          value={value}
          placeholder={provider.configured ? "Replace key…" : "Paste key…"}
          autoComplete="off"
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !busy) void save();
          }}
        />
        <button className="btn" onClick={save} disabled={busy || !value.trim()}>
          {saved ? "Saved" : "Save"}
        </button>
        {provider.configured && (
          <button
            className="btn btn--danger"
            onClick={async () => {
              try {
                await backend.providerKeySet(provider.id, "");
                onSaved();
              } catch (err) {
                onError(errorText(err));
              }
            }}
          >
            Remove
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * Update checking (notify only).
 *
 * StudyBites never downloads or installs anything by itself — it tells you a
 * newer build exists and opens the download page if you want it.
 */
function UpdateSection() {
  const [hasToken, setHasToken] = useState(false);
  const [token, setToken] = useState("");
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void backend
      .githubTokenPresent()
      .then(setHasToken)
      .catch(() => setHasToken(false));
  }, []);

  const check = async () => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await backend.updateCheck());
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="stack">
      <h2>Updates</h2>
      <div className="card card--pad">
        <div className="stack">
          <p className="muted">
            StudyBites can check whether a newer build has been published and tell you. It never
            downloads or installs anything on its own — you stay in control of what runs.
          </p>
          <p className="muted">
            Because the project repository is private, checking needs a GitHub token with read
            access. Like the AI keys, it is kept in Windows&rsquo; credential store, never inside
            the app.
          </p>

          <div className="key-row__controls">
            <input
              className="input"
              type="password"
              value={token}
              placeholder={hasToken ? "Replace token…" : "GitHub token (optional)"}
              autoComplete="off"
              onChange={(e) => setToken(e.target.value)}
            />
            <button
              className="btn"
              disabled={busy}
              onClick={async () => {
                try {
                  await backend.githubTokenSet(token);
                  setToken("");
                  setHasToken(await backend.githubTokenPresent());
                } catch (err) {
                  setError(errorText(err));
                }
              }}
            >
              Save token
            </button>
            <button className="btn btn--primary" onClick={check} disabled={busy}>
              {busy ? "Checking…" : "Check now"}
            </button>
          </div>

          {error && <Banner kind="error">{error}</Banner>}

          {status?.skippedReason && <Banner kind="info">{status.skippedReason}</Banner>}

          {status && !status.skippedReason && !status.updateAvailable && (
            <Banner kind="info">You are on the latest build ({status.currentVersion}).</Banner>
          )}

          {status?.updateAvailable && (
            <Banner kind="info">
              Version {status.latestVersion} is available — you have {status.currentVersion}.{" "}
              {status.downloadUrl && (
                <button
                  className="btn btn--ghost"
                  onClick={() => {
                    const url = status.downloadUrl;
                    if (url) void backend.openUrl(url);
                  }}
                >
                  Open the download page
                </button>
              )}
            </Banner>
          )}
        </div>
      </div>
    </section>
  );
}
