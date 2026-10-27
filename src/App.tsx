import { useCallback, useEffect, useState } from "react";
import { backend, errorText, isDesktop } from "./api";
import { Banner } from "./components/ui";
import { HomePage } from "./routes/HomePage";
import { SettingsPage } from "./routes/SettingsPage";
import { StudyPage } from "./routes/StudyPage";
import { SubjectPage } from "./routes/SubjectPage";
import { SubjectsPage } from "./routes/SubjectsPage";

export type Theme = "system" | "light" | "dark";

/** Still a handful of screens, so this stands in for a router. */
type View =
  | { name: "home" }
  | { name: "subjects" }
  | { name: "subject"; id: number }
  | { name: "study"; subjectId: number | null; minutes: number }
  | { name: "settings" };

const THEME_KEY = "theme";

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", theme);
}

export function App() {
  const [view, setView] = useState<View>({ name: "home" });
  const [theme, setTheme] = useState<Theme>("system");
  const [version, setVersion] = useState("");
  const [bootError, setBootError] = useState<string | null>(null);

  // One backend round trip at startup: the saved theme and the app version.
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [saved, info] = await Promise.all([
          backend.settingGet(THEME_KEY),
          backend.appInfo(),
        ]);
        if (!alive) return;
        const next = (saved as Theme) ?? "system";
        setTheme(next);
        applyTheme(next);
        setVersion(info.version);
      } catch (err) {
        if (alive) setBootError(errorText(err));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const changeTheme = useCallback(async (next: Theme) => {
    setTheme(next);
    applyTheme(next);
    try {
      await backend.settingSet(THEME_KEY, next);
    } catch {
      /* the theme still applies for this session */
    }
  }, []);

  return (
    <div className="app">
      <header className="topbar">
        <button
          className="topbar__brand btn btn--ghost"
          onClick={() => setView({ name: "home" })}
          aria-label="StudyBites home"
        >
          <span className="topbar__mark" aria-hidden="true" />
          StudyBites
        </button>
        {!isDesktop && <span className="dev-badge">browser preview — mock data</span>}
        <span className="topbar__spacer" />
        <button
          className="btn btn--ghost"
          onClick={() => setView({ name: "subjects" })}
          aria-current={view.name === "subjects" ? "page" : undefined}
        >
          Subjects
        </button>
        <button
          className="btn btn--ghost"
          onClick={() => setView({ name: "settings" })}
          aria-current={view.name === "settings" ? "page" : undefined}
        >
          Settings
        </button>
      </header>

      <main className="main">
        <div className="content">
          {bootError && <Banner kind="error">{bootError}</Banner>}

          {view.name === "home" && (
            <HomePage
              onStart={(subjectId, minutes) => setView({ name: "study", subjectId, minutes })}
              onOpenSubject={(id) => setView({ name: "subject", id })}
              onNewSubject={() => setView({ name: "subjects" })}
            />
          )}
          {view.name === "subjects" && (
            <SubjectsPage onOpen={(id) => setView({ name: "subject", id })} />
          )}
          {view.name === "subject" && (
            <SubjectPage
              subjectId={view.id}
              onBack={() => setView({ name: "home" })}
              onStudy={(minutes) =>
                setView({ name: "study", subjectId: view.id, minutes })
              }
            />
          )}
          {view.name === "study" && (
            <StudyPage
              subjectId={view.subjectId}
              minutes={view.minutes}
              onDone={() => setView({ name: "home" })}
            />
          )}
          {view.name === "settings" && (
            <SettingsPage theme={theme} onThemeChange={changeTheme} version={version} />
          )}
        </div>
      </main>
    </div>
  );
}
