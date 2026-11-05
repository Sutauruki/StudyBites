import { useCallback, useEffect, useState } from "react";
import { backend, errorText, isDesktop, type LicenseStatus, type Subject } from "./api";
import { Screen } from "./components/Screen";
import { Banner } from "./components/ui";
import { CbtPage } from "./routes/CbtPage";
import { HomePage } from "./routes/HomePage";
import { OnboardingPage } from "./routes/OnboardingPage";
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
  | { name: "cbt" }
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
  const [license, setLicense] = useState<LicenseStatus | null>(null);

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

  // Startup. The licence comes first: until it is known, nothing else may be
  // asked for, because every other command refuses while the app is locked.
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [status, info] = await Promise.all([backend.licenseStatus(), backend.appInfo()]);
        if (!alive) return;
        setLicense(status);
        setVersion(info.version);

        if (!status.unlocked) return;
        const saved = await backend.settingGet(THEME_KEY);
        if (!alive) return;
        const next = (saved as Theme) ?? "system";
        setTheme(next);
        applyTheme(next);
      } catch (err) {
        if (alive) setBootError(errorText(err));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (license?.unlocked) void refreshNav();
  }, [refreshNav, view, license?.unlocked]);

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

  // The agreement and activation own the whole window. Everything below this
  // point assumes an unlocked database.
  if (bootError && !license) {
    return (
      <Screen className="gate">
        <div className="gate__inner">
          <h1 className="display">StudyBites could not start</h1>
          <Banner kind="error">{bootError}</Banner>
        </div>
      </Screen>
    );
  }
  if (!license) return null;
  if (!license.eulaAccepted || !license.unlocked) {
    return <OnboardingPage status={license} onChanged={setLicense} />;
  }

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
          <button
            className="sidebar__link"
            aria-current={view.name === "cbt" ? "page" : undefined}
            onClick={() => go({ name: "cbt" })}
          >
            <span>Practice test</span>
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
          {view.name === "cbt" && <CbtPage />}
          {view.name === "settings" && (
            <SettingsPage
              theme={theme}
              onThemeChange={changeTheme}
              version={version}
              license={license}
            />
          )}
        </div>
      </main>
    </div>
  );
}
