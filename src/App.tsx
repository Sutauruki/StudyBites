import { useCallback, useEffect, useState } from "react";
import { backend, errorText, isDesktop, type Subject } from "./api";
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
  const [subjects, setSubjects] = useState<Subject[]>([]);
  const [due, setDue] = useState(0);

  // The sidebar needs the subject list and the due count; both are cheap
  // database reads, so they refresh whenever the view changes.
  const refreshNav = useCallback(async () => {
    try {
      const [list, counts] = await Promise.all([
        backend.subjectsList(false),
        backend.dueCounts(null),
      ]);
      setSubjects(list);
      setDue(counts.due + counts.new);
    } catch {
      /* the sidebar is navigation, not content: never block on it */
    }
  }, []);

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

  useEffect(() => {
    void refreshNav();
  }, [refreshNav, view]);

  const changeTheme = useCallback(async (next: Theme) => {
    setTheme(next);
    applyTheme(next);
    try {
      await backend.settingSet(THEME_KEY, next);
    } catch {
      /* the theme still applies for this session */
    }
  }, []);

  const go = (next: View) => setView(next);

  return (
    <div className="app">
      <nav className="sidebar" aria-label="Main">
        <button className="sidebar__brand" onClick={() => go({ name: "home" })}>
          <span className="sidebar__mark" aria-hidden="true" />
          StudyBites
        </button>

        <div className="sidebar__group">
          <button
            className="sidebar__link"
            aria-current={view.name === "home" ? "page" : undefined}
            onClick={() => go({ name: "home" })}
          >
            <span>Today</span>
            {due > 0 && <span className="sidebar__count">{due}</span>}
          </button>
          <button
            className="sidebar__link"
            aria-current={view.name === "subjects" ? "page" : undefined}
            onClick={() => go({ name: "subjects" })}
          >
            <span>All subjects</span>
          </button>
        </div>

        {subjects.length > 0 && (
          <div className="sidebar__group sidebar__subjects">
            <span className="sidebar__label">Subjects</span>
            {subjects.map((subject) => (
              <button
                key={subject.id}
                className="sidebar__link"
                aria-current={
                  view.name === "subject" && view.id === subject.id ? "page" : undefined
                }
                onClick={() => go({ name: "subject", id: subject.id })}
              >
                <span>{subject.name}</span>
              </button>
            ))}
          </div>
        )}

        <div className="sidebar__group sidebar__foot">
          {!isDesktop && <span className="dev-badge">browser preview</span>}
          <button
            className="sidebar__link"
            aria-current={view.name === "settings" ? "page" : undefined}
            onClick={() => go({ name: "settings" })}
          >
            <span>Settings</span>
          </button>
        </div>
      </nav>

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
              onStudy={(minutes) => setView({ name: "study", subjectId: view.id, minutes })}
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
