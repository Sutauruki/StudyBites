import { isDesktop } from "../api";
import type { Theme } from "../App";

const THEMES: Array<{ value: Theme; label: string }> = [
  { value: "system", label: "Follow system" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

/**
 * Phase 1 settings: theme only. API keys, the new-card limit, the default bite
 * length and licence info arrive in Phases 2 and 3 (Brief 4).
 */
export function SettingsPage({
  theme,
  onThemeChange,
  version,
}: {
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  version: string;
}) {
  return (
    <>
      <div className="page-head">
        <div className="page-head__text">
          <h1>Settings</h1>
          <p className="page-head__sub">StudyBites {version || "…"}</p>
        </div>
      </div>

      <div className="card" style={{ padding: "var(--s-5)" }}>
        <div className="field">
          <span className="field__label">Theme</span>
          <div className="row row--wrap">
            {THEMES.map((option) => (
              <button
                key={option.value}
                className={`btn${theme === option.value ? " btn--primary" : ""}`}
                aria-pressed={theme === option.value}
                onClick={() => onThemeChange(option.value)}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="card" style={{ padding: "var(--s-5)" }}>
        <div className="stack">
          <h2>Coming next</h2>
          <p className="muted">
            Phase 2 adds your own AI key, uploads and study sessions. Phase 3 adds activation and
            device-locked encryption. Nothing here leaves your laptop.
          </p>
          <p className="muted">
            Storage: {isDesktop ? "encrypted database on this device" : "browser mock data"}
          </p>
        </div>
      </div>
    </>
  );
}
