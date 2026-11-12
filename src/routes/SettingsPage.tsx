import { useCallback, useEffect, useState } from "react";
import {
  backend,
  errorText,
  isDesktop,
  type DndMode,
  type DndStatus,
  type LicenseStatus,
  type ProviderInfo,
  type QueueStatus,
  type UpdateStatus,
} from "../api";
import type { Theme } from "../App";
import { Banner, Loading } from "../components/ui";
import { ReportBugDialog } from "../components/ReportBugDialog";
import { Prose } from "./OnboardingPage";

const THEMES: Array<{ value: Theme; label: string }> = [
  { value: "system", label: "Follow system" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function SettingsPage({
  theme,
  onThemeChange,
  version,
  license,
  diagnosticView,
}: {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  version: string;
  license: LicenseStatus;
  diagnosticView: string;
}) {
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [queue, setQueue] = useState<QueueStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reportOpen, setReportOpen] = useState(false);

  const refresh = useCallback(async (): Promise<ProviderInfo[]> => {
    try {
      const [list, status] = await Promise.all([backend.providersList(), backend.queueStatus()]);
      setProviders(list);
      setQueue(status);
      return list;
    } catch (err) {
      setError(errorText(err));
      throw err;
    }
  }, []);

  useEffect(() => {
    void refresh().catch(() => {});
  }, [refresh]);

  return (
    <>
      <div className="page-head lede">
        <div className="page-head__text">
          <h1 className="display">Settings</h1>
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

      <DndSection onError={setError} />

      <section className="stack">
        <span className="label">AI keys</span>
        <p className="muted">
          StudyBites uses your own API key, stored securely on this device and sent only to the
          provider you choose. Gemini is tried first, then the others if it runs out for the day.
        </p>
        <div className="card list">
          {providers.map((provider) => (
            <KeyRow key={provider.id} provider={provider} onSaved={refresh} onError={setError} />
          ))}
        </div>
      </section>

      {queue && queue.quota.length > 0 && (
        <section className="stack">
          <span className="label">Today&rsquo;s usage</span>
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

      <AboutSection license={license} />

      <section className="card card--pad">
        <div className="stack">
          <span className="label">Help and feedback</span>
          <p className="muted">
            Send a problem report with optional screenshots. Nothing is sent until you choose to
            submit it.
          </p>
          <div>
            <button className="btn" onClick={() => setReportOpen(true)}>
              Report a problem
            </button>
          </div>
        </div>
      </section>

      <section className="card card--pad">
        <div className="stack">
          <span className="label">Privacy</span>
          <p className="muted">
            Your notes stay on this device. There is no telemetry, analytics or automatic crash
            reporting. Material is sent only to the AI provider you configure, using your key.
            Bug reports are sent only when you choose to submit one.
          </p>
          <p className="muted">
            Storage: {isDesktop ? "secure storage on this device" : "browser mock data"}
          </p>
        </div>
      </section>
      {reportOpen && (
        <ReportBugDialog
          version={version}
          view={diagnosticView}
          licenseState={license.unlocked ? "unlocked" : "locked"}
          onClose={() => setReportOpen(false)}
        />
      )}
    </>
  );
}

function DndSection({ onError }: { onError: (message: string) => void }) {
  const [status, setStatus] = useState<DndStatus | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [mode, setMode] = useState<Exclude<DndMode, "off">>("priority");
  const [waitingForAccess, setWaitingForAccess] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  const saveEnabled = async (value: boolean) => {
    await backend.settingSet("dnd.enabled", String(value));
    setEnabled(value);
  };

  useEffect(() => {
    let active = true;
    void Promise.all([
      backend.dndStatus(),
      backend.settingGet("dnd.enabled"),
      backend.settingGet("dnd.mode"),
    ])
      .then(([nextStatus, savedEnabled, savedMode]) => {
        if (!active) return;
        setStatus(nextStatus);
        setEnabled(savedEnabled === "true");
        if (savedMode === "alarms" || savedMode === "priority") setMode(savedMode);
      })
      .catch((err) => onError(errorText(err)));
    return () => {
      active = false;
    };
  }, [onError]);

  useEffect(() => {
    if (!waitingForAccess) return;
    const onVisible = () => {
      if (document.visibilityState !== "visible") return;
      void (async () => {
        try {
          const nextStatus = await backend.dndStatus();
          setStatus(nextStatus);
          if (nextStatus.granted) {
            await saveEnabled(true);
            setHint(null);
          } else {
            await saveEnabled(false);
            setHint("Allow StudyBites in Do Not Disturb access.");
          }
        } catch (err) {
          onError(errorText(err));
        } finally {
          setWaitingForAccess(false);
        }
      })();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, [onError, waitingForAccess]);

  if (!status?.supported) return null;

  return (
    <section className="card card--pad">
      <div className="stack">
        <label className="row">
          <input
            type="checkbox"
            checked={enabled}
            onChange={async (event) => {
              const next = event.target.checked;
              setHint(null);
              try {
                if (!next) {
                  await saveEnabled(false);
                  return;
                }
                const current = await backend.dndStatus();
                setStatus(current);
                if (current.granted) {
                  await saveEnabled(true);
                } else {
                  setHint("Allow Do Not Disturb access for StudyBites to use this during study.");
                  setWaitingForAccess(true);
                  await backend.dndRequestAccess();
                }
              } catch (err) {
                setEnabled(false);
                setWaitingForAccess(false);
                onError(errorText(err));
              }
            }}
            disabled={waitingForAccess}
          />
          <span className="field__label">Do Not Disturb during study sessions</span>
        </label>
        <p className="muted">
          StudyBites restores your previous mode when a session ends. Priority only is recommended.
        </p>
        <label className="field">
          <span className="field__label">Do Not Disturb mode</span>
          <select
            className="input"
            value={mode}
            disabled={!enabled}
            onChange={async (event) => {
              const next: Exclude<DndMode, "off"> =
                event.target.value === "alarms" ? "alarms" : "priority";
              setMode(next);
              try {
                await backend.settingSet("dnd.mode", next);
              } catch (err) {
                onError(errorText(err));
              }
            }}
          >
            <option value="priority">Priority only (recommended)</option>
            <option value="alarms">Alarms only</option>
          </select>
        </label>
        {hint && <Banner kind="info">{hint}</Banner>}
      </div>
    </section>
  );
}

function KeyRow({
  provider,
  onSaved,
  onError,
}: {
  provider: ProviderInfo;
  onSaved: () => Promise<ProviderInfo[]>;
  onError: (message: string) => void;
}) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [models, setModels] = useState<string[] | null>(null);
  const [loadingModels, setLoadingModels] = useState(false);
  const [selectedModel, setSelectedModel] = useState(provider.selectedModel);

  useEffect(() => {
    setSelectedModel(provider.selectedModel);
  }, [provider.selectedModel]);

  const save = async () => {
    setBusy(true);
    try {
      await backend.providerKeySet(provider.id, value);
      const refreshed = await onSaved();
      if (!refreshed.some((item) => item.id === provider.id && item.configured)) {
        throw new Error("The key did not appear in the saved provider list. Please try again.");
      }
      setValue("");
      setSaved(true);
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
          <span>Recommended: {provider.defaultModel}</span>
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
            {loadingModels ? "Loading models…" : "Change model"}
          </button>
        ) : (
          <select
            className="input"
            value={selectedModel}
            onChange={async (e) => {
              const nextModel = e.target.value;
              setSelectedModel(nextModel);
              try {
                await backend.providerModelSet(provider.id, nextModel);
                const refreshed = await onSaved();
                const updated = refreshed.find((item) => item.id === provider.id);
                if (!updated || updated.selectedModel !== nextModel) {
                  throw new Error("The selected model was not confirmed as saved.");
                }
              } catch (err) {
                setSelectedModel(provider.selectedModel);
                onError(errorText(err));
              }
            }}
          >
            {!models.includes(provider.defaultModel) && (
              <option value={provider.defaultModel}>
                {provider.defaultModel} (recommended)
              </option>
            )}
            {selectedModel !== provider.defaultModel && !models.includes(selectedModel) && (
              <option value={selectedModel}>{selectedModel} (currently selected)</option>
            )}
            {models.map((model) => (
              <option key={model} value={model}>
                {model}{model === provider.defaultModel ? " (recommended)" : ""}
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
                await onSaved();
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
      <span className="label">Updates</span>
      <div className="card card--pad">
        <div className="stack">
          <p className="muted">
            StudyBites can check whether a newer build has been published and tell you. It never
            downloads or installs anything on its own — you stay in control of what runs.
          </p>
          <p className="muted">
            Because the project repository is private, checking needs a GitHub token with read
            access. Like the AI keys, it is kept in this device&rsquo;s secure storage.
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

/**
 * Settings → About (Brief 13): who this copy is licensed to, and the
 * agreement, read-only.
 */
function AboutSection({ license }: { license: LicenseStatus }) {
  const [eula, setEula] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open || eula !== null) return;
    void backend
      .eulaText()
      .then(setEula)
      .catch((err) => setError(errorText(err)));
  }, [open, eula]);

  return (
    <section className="stack">
      <span className="label">About</span>
      <div className="card card--pad">
        <div className="stack">
          {license.licensingEnabled ? (
            <p className="muted">
              Licensed to <strong>{license.buyerName ?? "this device"}</strong>, for this device
              only.
            </p>
          ) : (
            <Banner kind="info">
              This is a development build: it was made without a licensing key, so it runs without
              activation and its database uses the development key. It is not a copy to sell.
            </Banner>
          )}

          {license.deviceChanged.length > 0 && (
            <p className="muted">
              Parts of this device have changed since activation ({license.deviceChanged.join(", ")}).
              Your licence still works.
            </p>
          )}

          <p className="muted">Agreement version {license.eulaVersion}, accepted on this device.</p>

          <div className="action">
            <button className="btn" onClick={() => setOpen((o) => !o)}>
              {open ? "Hide the agreement" : "Read the agreement"}
            </button>
          </div>

          {error && <Banner kind="error">{error}</Banner>}
          {open &&
            (eula === null && !error ? (
              <Loading label="Opening the agreement…" />
            ) : (
              <article className="gate__doc selectable" tabIndex={0}>
                <Prose markdown={eula ?? ""} />
              </article>
            ))}
        </div>
      </div>
    </section>
  );
}
