//! CBT practice tests (Phase 5).
//!
//! The properties that matter: a test is drawn only from real multiple-choice
//! cards in the chosen subjects, the answer key never leaves the backend until
//! the test is submitted, blanks are marked wrong rather than skipped, and
//! sitting a test does not disturb the spaced-repetition schedule.

use studybites_core::ai::{DistilledNote, GeneratedCard};
use studybites_core::db::DEV_KEY;
use studybites_core::{
    cbt, embed::Embedder, library, subjects, topics, Db, LexicalEmbedder, NewTopic,
};

fn db() -> Db {
    Db::open_in_memory(DEV_KEY).expect("open in-memory db")
}

fn mcq(n: usize) -> GeneratedCard {
    GeneratedCard::Mcq {
        question: format!("Question {n}?"),
        // The right answer is at a different index each time, so a test that
        // passes by always guessing the same letter would be caught.
        options: vec!["A".into(), "B".into(), "C".into(), "D".into()],
        answer: n % 4,
        explanation: format!("Because of reason {n}."),
    }
}

/// A subject carrying `count` multiple-choice cards, plus one flashcard and
/// one AI preview card that must never appear in a test.
fn subject_with_questions(db: &mut Db, name: &str, count: usize) -> i64 {
    let subject = subjects::create(db, name).unwrap();
    let topic = topics::create(
        db,
        &NewTopic {
            subject_id: subject.id,
            parent_id: None,
            title: "Topic".into(),
            week: None,
            is_off_scheme: false,
        },
    )
    .unwrap();

    let embedder = LexicalEmbedder;
    let content = DistilledNote {
        title: "Notes".into(),
        key_ideas: vec!["an idea".into()],
        ..Default::default()
    };
    let vector = embedder.embed(&content.as_plain_text());
    let note = library::note_create(db, None, Some(topic.id), &content, &vector, false).unwrap();

    let cards: Vec<GeneratedCard> = (0..count).map(mcq).collect();
    library::cards_create(db, note.id, Some(topic.id), &cards, false).unwrap();

    // Not multiple choice — must be ignored.
    library::cards_create(
        db,
        note.id,
        Some(topic.id),
        &[GeneratedCard::Flash {
            front: "front".into(),
            back: "back".into(),
        }],
        false,
    )
    .unwrap();

    // Labelled "not from your class", so it has no business in a mock exam.
    library::cards_create(db, note.id, Some(topic.id), &[mcq(99)], true).unwrap();

    subject.id
}

#[test]
fn a_test_draws_only_real_multiple_choice_from_the_chosen_subjects() {
    let mut db = db();
    let wanted = subject_with_questions(&mut db, "CIT 202", 12);
    let other = subject_with_questions(&mut db, "MTH 101", 12);

    let test = cbt::start(&db, &[wanted], 10, 15).unwrap();
    assert_eq!(test.questions.len(), 10);
    assert_eq!(test.minutes, 15);

    // Every question is one of this subject's real MCQs.
    for q in &test.questions {
        assert_eq!(q.options.len(), 4);
        assert!(
            q.question.starts_with("Question "),
            "unexpected question: {}",
            q.question
        );
        assert_ne!(q.question, "Question 99?", "preview cards must be excluded");
        assert_eq!(q.subject_name.as_deref(), Some("CIT 202"));
    }

    // Positions are 0..n in order, which is what the answer grid indexes by.
    let positions: Vec<i64> = test.questions.iter().map(|q| q.position).collect();
    assert_eq!(positions, (0..10).collect::<Vec<_>>());

    // Both subjects together draw on both pools.
    let both = cbt::start(&db, &[wanted, other], 20, 20).unwrap();
    assert_eq!(both.questions.len(), 20);
}

#[test]
fn a_test_refuses_rather_than_padding_itself() {
    let mut db = db();
    let thin = subject_with_questions(&mut db, "GST 101", 3);

    // Three questions is not a test.
    assert!(cbt::start(&db, &[thin], 10, 10).is_err());
    assert!(cbt::start(&db, &[], 10, 10).is_err());

    let plenty = subject_with_questions(&mut db, "CIT 202", 10);
    assert!(
        cbt::start(&db, &[plenty], 1, 10).is_err(),
        "too few asked for"
    );
    assert!(cbt::start(&db, &[plenty], 10, 0).is_err(), "no time at all");

    // Asking for more than exist is fine — you simply get what there is.
    let test = cbt::start(&db, &[plenty], 50, 30).unwrap();
    assert_eq!(test.questions.len(), 10);
}

#[test]
fn submitting_scores_the_paper_and_only_then_reveals_the_answers() {
    let mut db = db();
    let subject = subject_with_questions(&mut db, "CIT 202", 10);
    let test = cbt::start(&db, &[subject], 8, 10).unwrap();

    // What the student was given carries no answer key: the only way to know
    // the right option is to submit. (The type has no field for it at all.)
    let chosen: Vec<Option<usize>> = test
        .questions
        .iter()
        .enumerate()
        // Answer the first four, deliberately leave the rest blank.
        .map(|(i, _)| if i < 4 { Some(i % 4) } else { None })
        .collect();

    let result = cbt::submit(&mut db, test.id, &chosen).unwrap();
    assert_eq!(result.total, 8);
    assert_eq!(result.answers.len(), 8);
    assert_eq!(result.percent, result.score * 100 / result.total);

    for marked in &result.answers {
        assert!(marked.correct < marked.options.len());
        assert!(!marked.explanation.is_empty(), "review needs the reason");
        if marked.position >= 4 {
            assert_eq!(marked.chosen, None, "unanswered stays unanswered");
        }
    }

    // Blanks are wrong, never quietly dropped from the denominator.
    let answered_right = result
        .answers
        .iter()
        .filter(|m| m.chosen == Some(m.correct))
        .count() as i64;
    assert_eq!(result.score, answered_right);
}

#[test]
fn a_perfect_paper_scores_full_marks() {
    let mut db = db();
    let subject = subject_with_questions(&mut db, "CIT 202", 10);
    let test = cbt::start(&db, &[subject], 6, 10).unwrap();

    let blind = cbt::submit(&mut db, test.id, &[None; 6]).unwrap();
    assert_eq!(blind.score, 0, "answering nothing scores nothing");

    // Sit a paper covering every question, so the key below is complete
    // whichever six the next test happens to draw.
    let whole = cbt::start(&db, &[subject], 10, 10).unwrap();
    let learned = cbt::submit(&mut db, whole.id, &[None; 10]).unwrap();

    let again = cbt::start(&db, &[subject], 6, 10).unwrap();
    let key: Vec<Option<usize>> = again
        .questions
        .iter()
        .map(|q| {
            learned
                .answers
                .iter()
                .find(|m| m.question == q.question)
                .map(|m| m.correct)
        })
        .collect();

    let perfect = cbt::submit(&mut db, again.id, &key).unwrap();
    assert_eq!(perfect.score, perfect.total);
    assert_eq!(perfect.percent, 100);
}

#[test]
fn sitting_a_test_leaves_the_study_schedule_alone() {
    let mut db = db();
    let subject = subject_with_questions(&mut db, "CIT 202", 10);

    let before: Vec<(i64, String, i64)> = db
        .conn()
        .prepare("SELECT card_id, due, reps FROM card_state ORDER BY card_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();

    let test = cbt::start(&db, &[subject], 8, 10).unwrap();
    cbt::submit(&mut db, test.id, &[Some(0); 8]).unwrap();

    let after: Vec<(i64, String, i64)> = db
        .conn()
        .prepare("SELECT card_id, due, reps FROM card_state ORDER BY card_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();

    assert_eq!(
        before, after,
        "a practice test must not reschedule the student's deck"
    );

    let reviews: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM review_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reviews, 0, "a test is not a review");
}

#[test]
fn history_lists_finished_tests_newest_first() {
    let mut db = db();
    let subject = subject_with_questions(&mut db, "CIT 202", 10);

    // An unfinished test is not history.
    cbt::start(&db, &[subject], 5, 10).unwrap();
    assert!(cbt::history(&db, 10).unwrap().is_empty());

    let test = cbt::start(&db, &[subject], 5, 10).unwrap();
    cbt::submit(&mut db, test.id, &[None; 5]).unwrap();

    let history = cbt::history(&db, 10).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].total, 5);
    assert_eq!(history[0].score, Some(0));
    assert_eq!(history[0].subject_names, vec!["CIT 202".to_string()]);
}
