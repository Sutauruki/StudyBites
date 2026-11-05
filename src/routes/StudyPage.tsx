import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  backend,
  errorText,
  type MarkOutcome,
  type PreviewOffer,
  type SessionCard,
  type SessionSummary,
  type StudySession,
} from "../api";
import { Banner, Empty, Loading } from "../components/ui";
import { Screen } from "../components/Screen";

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

  // Typing the answer out is a per-subject choice, and a session can span
  // subjects, so the setting is looked up once per subject in the session.
  const [typedSubjects, setTypedSubjects] = useState<Record<number, boolean>>({});
  const [written, setWritten] = useState("");
  const [marking, setMarking] = useState(false);
  const [outcome, setOutcome] = useState<MarkOutcome | null>(null);

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

        // One lookup per subject present, not per card.
        const ids = [
          ...new Set(
            started.cards
              .map((c) => c.subjectId)
              .filter((id): id is number => typeof id === "number"),
          ),
        ];
        const flags = await Promise.all(
          ids.map(async (id) => [id, await backend.typedAnswersGet(id)] as const),
        );
        if (!alive) return;
        setTypedSubjects(Object.fromEntries(flags));
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
          setWritten("");
          setOutcome(null);
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
      // Space and the digits are shortcuts everywhere except inside the
      // answer box, where they are just the student writing.
      const target = e.target as HTMLElement | null;
      const isTyping =
        target?.tagName === "TEXTAREA" ||
        target?.tagName === "INPUT" ||
        target?.isContentEditable === true;
      if (isTyping) return;

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

  // Short-answer cards are written out, unless this subject has typing off.
  // Flashcards and MCQs are unchanged: they are recognition tasks by design.
  const writeIn =
    current?.card.payload.kind === "short" &&
    (current.subjectId === null || typedSubjects[current.subjectId] !== false);

  const suggested = outcome?.mark ? (RATING_FOR[outcome.mark.verdict] ?? null) : null;

  const submitWritten = useCallback(async () => {
    const payload = current?.card.payload;
    if (!payload || payload.kind !== "short" || marking) return;
    setMarking(true);
    try {
      setOutcome(await backend.answerMark(payload.prompt, payload.answer, written));
    } catch (err) {
      // Marking failing must never cost the card. Fall through to the answer
      // and let the student grade themselves.
      setOutcome({ mark: null, unavailable: errorText(err) });
    } finally {
      setMarking(false);
      setRevealed(true);
    }
  }, [current, written, marking]);

  if (error && phase === "loading") return <Banner kind="error">{error}</Banner>;
  if (phase === "loading") return <Loading label="Picking your cards…" />;

  if (phase === "finished") {
    return (
      <SessionDone
        summary={summary}
        hadCards={(session?.cards.length ?? 0) > 0}
        subjectId={subjectId}
        onDone={onDone}
      />
    );
  }

  if (!current) return <Loading label="Finishing up…" />;

  return (
    <Screen
      className="screen--nav screen--study"
      header={
        <div className="study__bar">
        <span className="muted">
          {index + 1} of {session?.cards.length}
          {current.isNew ? " · new" : ""}
          {current.topicTitle ? ` · ${current.topicTitle}` : ""}
        </span>
        <span className="spacer" />
        <span className="study__clock" aria-label="time left">
          {clock}
        </span>
        <button className="btn btn--ghost" onClick={() => void finish()}>
          End <kbd>Esc</kbd>
        </button>
        </div>
      }
      footer={
        !revealed && !writeIn ? (
          <button className="btn btn--primary btn--big" onClick={() => setRevealed(true)}>
            Reveal <kbd>Space</kbd>
          </button>
        ) : revealed ? (
          <div className="study__grades">
            {GRADES.map((g) => (
              <button
                key={g.rating}
                className={`btn${g.rating === suggested ? " btn--primary" : ""}`}
                onClick={() => void grade(g.rating)}
              >
                {g.label} <kbd>{g.key}</kbd>
              </button>
            ))}
          </div>
        ) : undefined
      }
    >

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
        {revealed && outcome && <MarkPanel outcome={outcome} />}
      </div>

      {!revealed && writeIn ? (
        <WriteAnswer
          value={written}
          busy={marking}
          onChange={setWritten}
          onSkip={() => setRevealed(true)}
          onSubmit={submitWritten}
        />
      ) : null}
      {revealed && suggested !== null && (
        <p className="muted study__suggestion">
          Marked for you — change it if that is not how it felt.
        </p>
      )}
    </Screen>
  );
}

const RATING_FOR: Record<string, number> = { correct: 3, close: 2, wrong: 1 };

/** The box the student writes into, before anything is revealed. */
function WriteAnswer({
  value,
  busy,
  onChange,
  onSkip,
  onSubmit,
}: {
  value: string;
  busy: boolean;
  onChange: (value: string) => void;
  onSkip: () => void;
  onSubmit: () => void;
}) {
  return (
    <div className="study__write">
      <textarea
        className="input study__answerbox selectable"
        value={value}
        rows={4}
        autoFocus
        placeholder="Write your answer…"
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          // Enter makes paragraphs; Ctrl/Cmd+Enter submits.
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !busy) {
            e.preventDefault();
            onSubmit();
          }
        }}
      />
      <div className="action">
        <button
          className="btn btn--primary btn--big"
          disabled={busy || value.trim().length === 0}
          onClick={onSubmit}
        >
          {busy ? "Marking…" : "Submit"} <kbd>Ctrl ↵</kbd>
        </button>
        <button className="btn btn--ghost" disabled={busy} onClick={onSkip}>
          I don&rsquo;t know
        </button>
      </div>
    </div>
  );
}

/** The mark, shown above the real answer rather than instead of it. */
function MarkPanel({ outcome }: { outcome: MarkOutcome }) {
  if (!outcome.mark) {
    return (
      <p className="muted study__markfail">
        {outcome.unavailable} Grade yourself instead — your answer still counts.
      </p>
    );
  }

  const { verdict, score, feedback, modelAnswer, fromAi } = outcome.mark;
  const label = verdict === "correct" ? "Correct" : verdict === "close" ? "Nearly" : "Not yet";

  return (
    <div className={`mark mark--${verdict}`}>
      <div className="mark__head">
        <span className="mark__verdict">{label}</span>
        <span className="mark__score">{score}/100</span>
        {!fromAi && <span className="chip">checked offline</span>}
      </div>
      <p className="mark__feedback">{feedback}</p>
      {modelAnswer && (
        <div className="mark__model">
          <span className="label">How to write it for full marks</span>
          <p>{modelAnswer}</p>
        </div>
      )}
    </div>
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
  subjectId,
  onDone,
}: {
  summary: SessionSummary | null;
  hadCards: boolean;
  subjectId: number | null;
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
      {subjectId !== null && <NextClassOffer subjectId={subjectId} />}

      <button className="btn btn--primary btn--big" onClick={onDone}>
        Finish
      </button>
    </div>
  );
}

/**
 * "Prepare for next class" (Brief 3.7).
 *
 * Offered after a session, never forced, and dismissable. Nothing is
 * generated until the student asks for it.
 */
function NextClassOffer({ subjectId }: { subjectId: number }) {
  const [offer, setOffer] = useState<PreviewOffer | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void backend
      .previewOffer(subjectId)
      .then(setOffer)
      .catch(() => setOffer(null));
  }, [subjectId]);

  if (!offer || dismissed || offer.alreadyPrepared) return null;

  if (done !== null) {
    return (
      <p className="muted">
        {done} preview card{done === 1 ? "" : "s"} ready for {offer.title}, labelled as AI
        preview.
      </p>
    );
  }

  return (
    <div className="offer">
      <p className="offer__text">
        Next is probably <strong>{offer.title}</strong>. Want a 5-minute head start?
      </p>
      <p className="muted">{offer.reason} · AI preview, not from your class</p>
      {error && <Banner kind="error">{error}</Banner>}
      <div className="row">
        <button className="btn btn--ghost" onClick={() => setDismissed(true)} disabled={busy}>
          Not now
        </button>
        <button
          className="btn btn--primary"
          disabled={busy}
          onClick={async () => {
            setBusy(true);
            setError(null);
            try {
              setDone(await backend.previewGenerate(subjectId, offer.topicId, offer.title));
            } catch (err) {
              setError(errorText(err));
            } finally {
              setBusy(false);
            }
          }}
        >
          {busy ? "Preparing…" : "Prepare it"}
        </button>
      </div>
    </div>
  );
}
