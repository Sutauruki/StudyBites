import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  backend,
  errorText,
  type SessionCard,
  type SessionSummary,
  type StudySession,
} from "../api";
import { Banner, Empty, Loading } from "../components/ui";

type Phase = "loading" | "studying" | "finished";

const GRADES = [
  { rating: 1, label: "Again", key: "1" },
  { rating: 2, label: "Hard", key: "2" },
  { rating: 3, label: "Good", key: "3" },
  { rating: 4, label: "Easy", key: "4" },
];

/**
 * A study bite (Brief 3.5).
 *
 * Full keyboard control: Space reveals, 1–4 grade, Esc ends. When the time is
 * up the session ends on its own with a calm summary — no "one more" loop.
 */
export function StudyPage({
  subjectId,
  minutes,
  onDone,
}: {
  subjectId: number | null;
  minutes: number;
  onDone: () => void;
}) {
  const [phase, setPhase] = useState<Phase>("loading");
  const [session, setSession] = useState<StudySession | null>(null);
  const [index, setIndex] = useState(0);
  const [revealed, setRevealed] = useState(false);
  const [chosen, setChosen] = useState<number | null>(null);
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [secondsLeft, setSecondsLeft] = useState(minutes * 60);

  const shownAt = useRef<number>(Date.now());
  const busy = useRef(false);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const started = await backend.sessionStart(subjectId, minutes);
        if (!alive) return;
        setSession(started);
        setPhase(started.cards.length === 0 ? "finished" : "studying");
        shownAt.current = Date.now();
      } catch (err) {
        if (alive) setError(errorText(err));
      }
    })();
    return () => {
      alive = false;
    };
  }, [subjectId, minutes]);

  const finish = useCallback(async () => {
    if (!session) {
      onDone();
      return;
    }
    try {
      setSummary(await backend.sessionEnd(session.id));
    } catch {
      /* the summary is a nicety; never block ending on it */
    }
    setPhase("finished");
  }, [session, onDone]);

  // The clock ends the session, not a card count.
  useEffect(() => {
    if (phase !== "studying") return;
    const timer = window.setInterval(() => {
      setSecondsLeft((left) => {
        if (left <= 1) {
          window.clearInterval(timer);
          void finish();
          return 0;
        }
        return left - 1;
      });
    }, 1000);
    return () => window.clearInterval(timer);
  }, [phase, finish]);

  const current: SessionCard | undefined = session?.cards[index];

  const grade = useCallback(
    async (rating: number) => {
      if (!session || !current || busy.current) return;
      busy.current = true;
      try {
        await backend.sessionGrade(
          session.id,
          current.card.id,
          rating,
          Date.now() - shownAt.current,
        );
        if (index + 1 >= session.cards.length) {
          await finish();
        } else {
          setIndex((i) => i + 1);
          setRevealed(false);
          setChosen(null);
          shownAt.current = Date.now();
        }
      } catch (err) {
        setError(errorText(err));
      } finally {
        busy.current = false;
      }
    },
    [session, current, index, finish],
  );

  useEffect(() => {
    if (phase !== "studying") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void finish();
        return;
      }
      if (e.key === " " || e.key === "Enter") {
        e.preventDefault();
        if (!revealed) setRevealed(true);
        return;
      }
      const match = GRADES.find((g) => g.key === e.key);
      if (match && revealed) void grade(match.rating);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [phase, revealed, grade, finish]);

  const clock = useMemo(() => {
    const m = Math.floor(secondsLeft / 60);
    const s = secondsLeft % 60;
    return `${m}:${String(s).padStart(2, "0")}`;
  }, [secondsLeft]);

  if (error && phase === "loading") return <Banner kind="error">{error}</Banner>;
  if (phase === "loading") return <Loading label="Picking your cards…" />;

  if (phase === "finished") {
    return (
      <SessionDone
        summary={summary}
        hadCards={(session?.cards.length ?? 0) > 0}
        onDone={onDone}
      />
    );
  }

  if (!current) return <Loading label="Finishing up…" />;

  return (
    <>
      <div className="study__bar">
        <span className="muted">
          {index + 1} of {session?.cards.length}
          {current.isNew ? " · new" : ""}
          {current.topicTitle ? ` · ${current.topicTitle}` : ""}
        </span>
        <span className="topbar__spacer" />
        <span className="study__clock" aria-label="time left">
          {clock}
        </span>
        <button className="btn btn--ghost" onClick={() => void finish()}>
          End (Esc)
        </button>
      </div>

      {error && <Banner kind="error">{error}</Banner>}
      {current.card.isPreview && (
        <span className="chip chip--accent">AI preview — not from your class</span>
      )}

      <div className="card study__card selectable">
        <CardFace
          card={current}
          revealed={revealed}
          chosen={chosen}
          onChoose={(i) => {
            setChosen(i);
            setRevealed(true);
          }}
        />
      </div>

      {!revealed ? (
        <button className="btn btn--primary btn--big" onClick={() => setRevealed(true)}>
          Reveal <span className="muted">(Space)</span>
        </button>
      ) : (
        <div className="study__grades">
          {GRADES.map((g) => (
            <button key={g.rating} className="btn" onClick={() => void grade(g.rating)}>
              {g.label} <span className="muted">{g.key}</span>
            </button>
          ))}
        </div>
      )}
    </>
  );
}

function CardFace({
  card,
  revealed,
  chosen,
  onChoose,
}: {
  card: SessionCard;
  revealed: boolean;
  chosen: number | null;
  onChoose: (index: number) => void;
}) {
  const payload = card.card.payload;

  if (payload.kind === "flash") {
    return (
      <>
        <p className="study__prompt">{payload.front}</p>
        {revealed && <p className="study__answer">{payload.back}</p>}
      </>
    );
  }

  if (payload.kind === "short") {
    return (
      <>
        <p className="study__prompt">{payload.prompt}</p>
        {revealed && (
          <>
            <p className="study__answer">{payload.answer}</p>
            <p className="muted">How close were you? Grade yourself.</p>
          </>
        )}
      </>
    );
  }

  // MCQ: instant feedback with the explanation (Brief 3.5).
  return (
    <>
      <p className="study__prompt">{payload.question}</p>
      <div className="study__options">
        {payload.options.map((option, i) => {
          const isCorrect = i === payload.answer;
          const state = !revealed
            ? ""
            : isCorrect
              ? " study__option--correct"
              : i === chosen
                ? " study__option--wrong"
                : "";
          return (
            <button
              key={i}
              className={`btn study__option${state}`}
              disabled={revealed}
              onClick={() => onChoose(i)}
            >
              {option}
            </button>
          );
        })}
      </div>
      {revealed && payload.explanation && <p className="muted">{payload.explanation}</p>}
    </>
  );
}

function SessionDone({
  summary,
  hadCards,
  onDone,
}: {
  summary: SessionSummary | null;
  hadCards: boolean;
  onDone: () => void;
}) {
  if (!hadCards) {
    return (
      <Empty
        title="Nothing to study yet"
        body="Upload some class material for this subject and StudyBites will turn it into cards. Anything you have already studied comes back here when it is due."
        action={
          <button className="btn btn--primary btn--big" onClick={onDone}>
            Back
          </button>
        }
      />
    );
  }

  return (
    <div className="empty">
      <div className="empty__title">Done for now</div>
      {summary && (
        <p className="empty__body">
          {summary.cardsDone} card{summary.cardsDone === 1 ? "" : "s"} reviewed
          {summary.topicsTouched.length > 0 && <> · {summary.topicsTouched.join(", ")}</>}.
          {summary.dueTomorrow > 0 && (
            <>
              <br />
              {summary.dueTomorrow} waiting tomorrow.
            </>
          )}
        </p>
      )}
      <button className="btn btn--primary btn--big" onClick={onDone}>
        Finish
      </button>
    </div>
  );
}
