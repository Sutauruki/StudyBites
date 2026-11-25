//! Outline updates (roadmap item 5) against the real database.
//!
//! These cover the promise the feature makes: an update renames and archives,
//! it never changes a topic id and never deletes a note or a card, and the
//! things that keep their topic stop appearing in sessions and tests while the
//! topic is archived.

use studybites_core::ai::{DistilledNote, GeneratedCard, SchemeEntry};
use studybites_core::db::DEV_KEY;
use studybites_core::embed::{Embedder, LexicalEmbedder};
use studybites_core::library::{self, NewMaterial};
use studybites_core::outline::{self, MoveDecision, OutlineDecisions, RenameDecision};
use studybites_core::{cbt, study, subjects, topics, Db, NewTopic};

fn db() -> Db {
    Db::open_in_memory(DEV_KEY).expect("open in-memory db")
}

fn topic(db: &Db, subject_id: i64, title: &str) -> i64 {
    topics::create(
        db,
        &NewTopic {
            subject_id,
            parent_id: None,
            title: title.to_string(),
            week: None,
            is_off_scheme: false,
        },
    )
    .unwrap()
    .id
}

fn entry(title: &str) -> SchemeEntry {
    SchemeEntry {
        title: title.to_string(),
        week: None,
        subtopics: Vec::new(),
    }
}

fn mcq(n: usize) -> GeneratedCard {
    GeneratedCard::Mcq {
        question: format!("Question {n}?"),
        options: vec!["A".into(), "B".into(), "C".into(), "D".into()],
        answer: n % 4,
        explanation: format!("Because of reason {n}."),
    }
}

fn flash(n: usize) -> GeneratedCard {
    GeneratedCard::Flash {
        front: format!("front {n}"),
        back: format!("back {n}"),
    }
}

fn material(db: &Db, subject_id: i64, topic_id: i64) -> i64 {
    library::material_create(
        db,
        &NewMaterial {
            subject_id,
            topic_id: Some(topic_id),
            original_filename: "week1.pdf",
            mime: "application/pdf",
            size: 100,
            sha256: "abc",
            cold_path: None,
        },
    )
    .unwrap()
    .id
}

fn note(db: &Db, material_id: Option<i64>, topic_id: i64) -> i64 {
    let embedder = LexicalEmbedder;
    let content = DistilledNote {
        title: "Notes".into(),
        key_ideas: vec!["an idea".into()],
        ..Default::default()
    };
    let vector = embedder.embed(&content.as_plain_text());
    library::note_create(db, material_id, Some(topic_id), &content, &vector, false)
        .unwrap()
        .id
}

#[test]
fn a_rename_keeps_the_topic_id_and_its_links() {
    let mut db = db();
    let subject_id = subjects::create(&db, "MTH 101").unwrap().id;
    let root = topic(&db, subject_id, "Sets");
    let file = material(&db, subject_id, root);
    let note_id = note(&db, Some(file), root);
    let cards = library::cards_create(&mut db, note_id, Some(root), &[flash(1)], false).unwrap();

    let decisions = OutlineDecisions {
        renamed: vec![RenameDecision {
            id: root,
            title: "Set theory".into(),
        }],
        ..Default::default()
    };
    outline::apply_outline_update(&mut db, subject_id, &decisions).unwrap();

    let tree = topics::list_tree(&db, subject_id).unwrap();
    assert_eq!(tree.len(), 1, "a rename must not add a topic");
    assert_eq!(tree[0].id, root, "the topic keeps its id");
    assert_eq!(tree[0].title, "Set theory");

    assert_eq!(
        library::material_get(&db, file).unwrap().topic_id,
        Some(root)
    );
    assert_eq!(
        library::note_get(&db, note_id).unwrap().topic_id,
        Some(root)
    );
    assert_eq!(
        library::card_get(&db, cards[0]).unwrap().topic_id,
        Some(root)
    );
}

#[test]
fn a_removed_topic_is_archived_and_its_cards_are_only_hidden() {
    let mut db = db();
    let subject_id = subjects::create(&db, "BIO 101").unwrap().id;
    let root = topic(&db, subject_id, "Genetics");
    let note_id = note(&db, None, root);
    let cards = [mcq(1), mcq(2), mcq(3)];
    let ids = library::cards_create(&mut db, note_id, Some(root), &cards, false).unwrap();

    assert_eq!(study::due_counts(&db, Some(subject_id)).unwrap().new, 3);
    assert_eq!(cbt::available(&db, &[subject_id]).unwrap(), 3);

    let decisions = OutlineDecisions {
        archived: vec![root],
        ..Default::default()
    };
    outline::apply_outline_update(&mut db, subject_id, &decisions).unwrap();

    assert!(topics::list_tree(&db, subject_id).unwrap().is_empty());
    assert_eq!(topics::list_archived(&db, subject_id).unwrap().len(), 1);
    assert_eq!(
        study::due_counts(&db, Some(subject_id)).unwrap().new,
        0,
        "archived topics are not scheduled"
    );
    let session = study::start_session(&mut db, Some(subject_id), 10, 10).unwrap();
    assert!(session.cards.is_empty(), "archived cards are not selected");
    assert_eq!(cbt::available(&db, &[subject_id]).unwrap(), 0);
    for id in &ids {
        assert!(
            library::card_get(&db, *id).is_ok(),
            "the card row is untouched"
        );
    }

    topics::restore(&db, root).unwrap();
    assert_eq!(study::due_counts(&db, Some(subject_id)).unwrap().new, 3);
    assert_eq!(cbt::available(&db, &[subject_id]).unwrap(), 3);
}

#[test]
fn a_failed_update_leaves_every_topic_and_link_untouched() {
    let mut db = db();
    let subject_id = subjects::create(&db, "PHY 101").unwrap().id;
    let root = topic(&db, subject_id, "Optics");
    let file = material(&db, subject_id, root);
    let note_id = note(&db, Some(file), root);

    // The rename is valid; the move names a topic that does not exist, so the
    // whole update must roll back — including the rename.
    let decisions = OutlineDecisions {
        renamed: vec![RenameDecision {
            id: root,
            title: "Optics and waves".into(),
        }],
        moved: vec![MoveDecision {
            id: 99_999,
            week: Some(3),
            clear_week: false,
            to_parent: None,
            to_parent_title: None,
            to_root: false,
        }],
        ..Default::default()
    };
    assert!(outline::apply_outline_update(&mut db, subject_id, &decisions).is_err());

    let tree = topics::list_tree(&db, subject_id).unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].id, root);
    // The rename was rolled back with everything else.
    assert_eq!(tree[0].title, "Optics");
    assert_eq!(
        library::material_get(&db, file).unwrap().topic_id,
        Some(root)
    );
    assert_eq!(
        library::note_get(&db, note_id).unwrap().topic_id,
        Some(root)
    );
    assert!(outline::history(&db, subject_id).unwrap().is_empty());
}

#[test]
fn reimporting_the_same_scheme_does_not_duplicate_anything() {
    let mut db = db();
    let subject_id = subjects::create(&db, "CHM 101").unwrap().id;
    let sets = topic(&db, subject_id, "Sets");
    topic(&db, subject_id, "Logarithms");

    let incoming = [entry("Sets"), entry("Logarithms")];
    let diff = outline::diff_outline(&db, subject_id, &incoming).unwrap();
    assert!(
        diff.is_empty(),
        "nothing changed, so there is nothing to apply"
    );
    outline::apply_outline_update(&mut db, subject_id, &OutlineDecisions::default()).unwrap();

    let all = topics::list_all(&db, subject_id).unwrap();
    assert_eq!(all.len(), 2, "re-importing must not duplicate topics");
    assert_eq!(all[0].id, sets);
}
