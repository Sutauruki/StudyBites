import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  backend,
  errorText,
  type CbtResult,
  type CbtSummary,
  type CbtTest,
  type Subject,
} from "../api";
import { Banner, Empty, Loading, Modal } from "../components/ui";
import { Screen } from "../components/Screen";

type Stage =
  | { name: "setup" }
  | { name: "sitting"; test: CbtTest }
  | { name: "result"; result: CbtResult };

const LETTERS = ["A", "B", "C", "D", "E", "F"];

/**
 * CBT practice (Phase 5).
 *
 * Laid out the way Nigerian students already practise — a clock running down,
 * lettered options, a numbered grid to jump around, and a score at the end —
 * but in this app's own palette and type rather than a pastiche of someone
 * else's exam software.
 *
 * A test deliberately does not touch the spaced-repetition schedule. It is
 * practice, not review.
 */
export function CbtPage() {
  const [stage, setStage] = useState<Stage>({ name: "setup" });

  if (stage.name === "sitting") {
    return (
      <Sitting
        test={stage.test}
        onDone={(result) => setStage({ name: "result", result })}
        onAbandon={() => setStage({ name: "setup" })}
      />
    );
  }
  if (stage.name === "result") {
    return <Review result={stage.result} onAgain={() => setStage({ name: "setup" })} />;
  }
  return <Setup onStart={(test) => setStage({ name: "sitting", test })} />;
}

// ---- setup --------------------------------------------------------------

function Setup({ onStart }: { onStart: (test: CbtTest) => void }) {
  const [subjects, setSubjects] = useState<Subject[] | null>(null);
  const [picked, setPicked] = useState<number[]>([]);
  const [available, setAvailable] = useState<number | null>(null);
  const [count, setCount] = useState(20);
  const [minutes, setMinutes] = useState(20);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<CbtSummary[]>([]);

  useEffect(() => {
    void backend
      .subjectsList(false)
      .then(setSubjects)
      .catch((err) => setError(errorText(err)));
    void backend
      .cbtHistory(5)
      .then(setHistory)
      .catch(() => setHistory([]));
  }, []);

  // How many questions exist decides what lengths are worth offering.
  useEffect(() => {
    if (picked.length === 0) {
      setAvailable(null);
      return;
    }
    void backend
      .cbtAvailable(picked)
      .then(setAvailable)
      .catch(() => setAvailable(null));
  }, [picked]);

  const max = available ?? 0;
  const lengths = [10, 20, 40, 60].filter((n) => n <= max);
  const effective = Math.min(count, max);

  if (subjects === null) return <Loading label="Loading your subjects…" />;

  if (subjects.length === 0) {
    return (
      <Empty
        title="Nothing to practise on yet"
        body="CBT questions come from the multiple-choice cards StudyBites makes out of your own material. Add a subject and upload something first."
      />
    );
  }

  return (
    <>
      <div className="page-head lede">
        <div className="page-head__text">
          <h1 className="display">Practice test</h1>
          <p className="page-head__sub">
            A timed mock from your own material. It does not affect your revision schedule.
          </p>
        </div>
      </div>

      {error && <Banner kind="error">{error}</Banner>}

      <section className="stack">
        <span className="label">Subjects</span>
        <div className="card list">
          {subjects.map((subject) => (
            <label key={subject.id} className="pick">
              <input
                type="checkbox"
                checked={picked.includes(subject.id)}
                onChange={(e) =>
                  setPicked((current) =>
                    e.target.checked
                      ? [...current, subject.id]
                      : current.filter((id) => id !== subject.id),
                  )
                }
              />
              <span className="pick__name">{subject.name}</span>
            </label>
          ))}
        </div>
        {picked.length > 0 && (
          <p className="muted">
            {available === null
              ? "Counting questions…"
              : max === 0
                ? "No multiple-choice questions in these subjects yet."
                : `${max} question${max === 1 ? "" : "s"} available.`}
          </p>
        )}
      </section>

      {max >= 5 && (
        <>
          <section className="stack">
            <span className="label">How many questions</span>
            <div className="segmented">
              {[...lengths, max].filter((n, i, a) => a.indexOf(n) === i).map((n) => (
                <button
                  key={n}
                  className="btn"
                  aria-pressed={effective === n}
                  onClick={() => {
                    setCount(n);
                    // A minute a question is the usual CBT pace.
                    setMinutes(n);
                  }}
                >
                  {n}
                </button>
              ))}
            </div>
          </section>

          <section className="stack">
            <span className="label">How long</span>
            <div className="segmented">
              {[Math.ceil(effective / 2), effective, effective * 2].map((n) => (
                <button
                  key={n}
                  className="btn"
                  aria-pressed={minutes === n}
                  onClick={() => setMinutes(n)}
                >
                  {n} min
                </button>
              ))}
            </div>
          </section>

          <div className="action">
            <button
              className="btn btn--primary btn--big"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError(null);
                try {
                  onStart(await backend.cbtStart(picked, effective, minutes));
                } catch (err) {
                  setError(errorText(err));
                  setBusy(false);
                }
              }}
            >
              {busy ? "Preparing…" : `Start ${effective} questions · ${minutes} min`}
            </button>
          </div>
        </>
      )}

      {history.length > 0 && (
        <section className="stack">
          <span className="label">Recent attempts</span>
          <div className="card list">
            {history.map((attempt) => (
              <div key={attempt.id} className="list__item list__item--static">
                <div className="list__text">
                  <div className="list__title">
                    {attempt.score}/{attempt.total}
                    <span className="muted">
                      {" "}
                      ·{" "}
                      {attempt.total > 0
                        ? Math.floor((attempt.score ?? 0) * 100 / attempt.total)
                        : 0}
                      %
                    </span>
                  </div>
                  <div className="list__meta">
                    {attempt.subjectNames.join(", ")} ·{" "}
                    {new Date(attempt.startedAt).toLocaleDateString()}
                  </div>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}
    </>
  );
}

// ---- sitting the test ---------------------------------------------------

function Sitting({
  test,
  onDone,
  onAbandon,
}: {
  test: CbtTest;
  onDone: (result: CbtResult) => void;
  onAbandon: () => void;
}) {
  const [chosen, setChosen] = useState<Array<number | null>>(() =>
    test.questions.map(() => null),
  );
  const [flagged, setFlagged] = useState<Set<number>>(new Set());
  const [at, setAt] = useState(0);
  const [secondsLeft, setSecondsLeft] = useState(test.minutes * 60);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submitted = useRef(false);

  const submit = useCallback(async () => {
    if (submitted.current) return;
    submitted.current = true;
    try {
      onDone(await backend.cbtSubmit(test.id, chosen));
    } catch (err) {
      submitted.current = false;
      setError(errorText(err));
    }
  }, [test.id, chosen, onDone]);

  // The clock ends the test, exactly as a real CBT does.
  useEffect(() => {
    const timer = window.setInterval(() => {
      setSecondsLeft((left) => {
        if (left <= 1) {
          window.clearInterval(timer);
          void submit();
          return 0;
        }
        return left - 1;
      });
    }, 1000);
    return () => window.clearInterval(timer);
  }, [submit]);

  const question = test.questions[at];

  const pick = useCallback(
    (option: number) => {
      setChosen((current) => {
        const next = [...current];
        // Clicking the chosen option again clears it: a blank is a real
        // answer in a CBT, and being stuck with a guess is worse.
        next[at] = next[at] === option ? null : option;
        return next;
      });
    },
    [at],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target?.tagName === "INPUT" || target?.tagName === "TEXTAREA") return;

      if (e.key === "ArrowRight") setAt((i) => Math.min(i + 1, test.questions.length - 1));
      else if (e.key === "ArrowLeft") setAt((i) => Math.max(i - 1, 0));
      else {
        const letter = LETTERS.indexOf(e.key.toUpperCase());
        const digit = Number.parseInt(e.key, 10) - 1;
        const option = letter >= 0 ? letter : digit;
        if (option >= 0 && option < (question?.options.length ?? 0)) pick(option);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pick, question, test.questions.length]);

  const unanswered = chosen.filter((c) => c === null).length;
  const clock = useMemo(() => {
    const m = Math.floor(secondsLeft / 60);
    const s = secondsLeft % 60;
    return `${m}:${String(s).padStart(2, "0")}`;
  }, [secondsLeft]);

  if (!question) return <Loading label="Preparing…" />;

  return (
    <Screen
      className="screen--nav screen--cbt"
      header={
        <div className="cbt__bar">
        <span className="cbt__count">
          Question {at + 1} <span className="muted">of {test.questions.length}</span>
        </span>
        <span className="spacer" />
        <span
          className={`cbt__clock${secondsLeft <= 60 ? " cbt__clock--low" : ""}`}
          aria-label="time remaining"
          role="timer"
        >
          {clock}
        </span>
        </div>
      }
      footer={
        <div className="cbt__footer">
          <div className="cbt__move">
            <button className="btn" disabled={at === 0} onClick={() => setAt((i) => i - 1)}>
              Previous
            </button>
            <button
              className="btn btn--ghost"
              aria-pressed={flagged.has(at)}
              onClick={() =>
                setFlagged((current) => {
                  const next = new Set(current);
                  if (!next.delete(at)) next.add(at);
                  return next;
                })
              }
            >
              {flagged.has(at) ? "Unflag" : "Flag for review"}
            </button>
            <button
              className="btn"
              disabled={at === test.questions.length - 1}
              onClick={() => setAt((i) => i + 1)}
            >
              Next
            </button>
          </div>
          <button className="btn btn--primary" onClick={() => setConfirming(true)}>
            Submit
          </button>
        </div>
      }
    >

      {error && <Banner kind="error">{error}</Banner>}

      <div className="cbt__body">
        <main className="cbt__main selectable">
          {question.subjectName && (
            <span className="cbt__subject">
              {question.subjectName}
              {question.topicTitle ? ` · ${question.topicTitle}` : ""}
            </span>
          )}
          <p className="cbt__question">{question.question}</p>

          <div className="cbt__options">
            {question.options.map((option, i) => (
              <button
                key={i}
                className={`cbt__option${chosen[at] === i ? " cbt__option--picked" : ""}`}
                aria-pressed={chosen[at] === i}
                onClick={() => pick(i)}
              >
                <span className="cbt__letter">{LETTERS[i]}</span>
                <span>{option}</span>
              </button>
            ))}
          </div>

        </main>

        {/* The grid every CBT has: where you are, what is done, what is left. */}
        <nav className="cbt__grid" aria-label="Questions">
          {test.questions.map((_, i) => {
            const state = chosen[i] !== null ? " cbt__cell--done" : "";
            const flag = flagged.has(i) ? " cbt__cell--flagged" : "";
            const here = i === at ? " cbt__cell--here" : "";
            return (
              <button
                key={i}
                className={`cbt__cell${state}${flag}${here}`}
                aria-current={i === at ? "true" : undefined}
                onClick={() => setAt(i)}
              >
                {i + 1}
              </button>
            );
          })}
        </nav>
      </div>

      {confirming && (
        <Modal
          title="Submit your answers?"
          onClose={() => setConfirming(false)}
          actions={
            <>
              <button className="btn btn--ghost" onClick={() => setConfirming(false)}>
                Keep working
              </button>
              <button className="btn btn--primary" onClick={() => void submit()}>
                Submit
              </button>
            </>
          }
        >
          <p>
            {unanswered === 0
              ? "Every question is answered."
              : `${unanswered} question${unanswered === 1 ? " is" : "s are"} still blank, and ` +
                `will be marked wrong.`}
          </p>
          {flagged.size > 0 && (
            <p className="muted">
              {flagged.size} flagged for review: {[...flagged].map((i) => i + 1).join(", ")}
            </p>
          )}
          <button className="link" onClick={onAbandon}>
            Leave without submitting
          </button>
        </Modal>
      )}
    </Screen>
  );
}

// ---- the result ---------------------------------------------------------

function Review({ result, onAgain }: { result: CbtResult; onAgain: () => void }) {
  const [showAll, setShowAll] = useState(false);
  const wrong = result.answers.filter((a) => a.chosen !== a.correct);
  const shown = showAll ? result.answers : wrong;

  return (
    <>
      <div className="page-head lede">
        <div className="page-head__text">
          <h1 className="display">
            {result.score} <span className="muted">/ {result.total}</span>
          </h1>
          <p className="page-head__sub">
            {result.percent}% · {wrong.length} to look at
          </p>
        </div>
        <button className="btn btn--primary" onClick={onAgain}>
          Another test
        </button>
      </div>

      {wrong.length === 0 && (
        <Banner kind="info">Every answer was right. Nothing to correct.</Banner>
      )}

      <section className="stack">
        <div className="row">
          <span className="label">{showAll ? "All questions" : "What you missed"}</span>
          <span className="spacer" />
          <button className="link" onClick={() => setShowAll((s) => !s)}>
            {showAll ? "Only the ones I missed" : "Show every question"}
          </button>
        </div>

        <div className="stack">
          {shown.map((answer) => (
            <article key={answer.position} className="card card--pad review selectable">
              <span className="muted">
                Question {answer.position + 1}
                {answer.topicTitle ? ` · ${answer.topicTitle}` : ""}
              </span>
              <p className="review__question">{answer.question}</p>

              <div className="review__options">
                {answer.options.map((option, i) => {
                  const right = i === answer.correct;
                  const mine = i === answer.chosen;
                  const tone = right
                    ? " review__option--right"
                    : mine
                      ? " review__option--mine"
                      : "";
                  return (
                    <div key={i} className={`review__option${tone}`}>
                      <span className="cbt__letter">{LETTERS[i]}</span>
                      <span>{option}</span>
                      {right && <span className="chip chip--ok">Correct</span>}
                      {mine && !right && <span className="chip">You chose this</span>}
                    </div>
                  );
                })}
              </div>

              {answer.chosen === null && <p className="muted">You left this one blank.</p>}
              {answer.explanation && <p className="review__why">{answer.explanation}</p>}
            </article>
          ))}
        </div>
      </section>
    </>
  );
}
