use studybites_core::db::{Key, DEV_KEY};
use studybites_core::error::CoreError;
use studybites_core::{migrations, subjects, topics, Db, NewTopic};

fn db() -> Db {
    Db::open_in_memory(DEV_KEY).expect("open in-memory db")
}

#[test]
fn migrations_bring_schema_to_current_version() {
    let db = db();
    assert_eq!(db.schema_version().unwrap(), migrations::SCHEMA_VERSION);
    let tables: i64 = db
        .conn()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        tables, 14,
        "the Section 5 tables, plus EULA acceptance and the CBT tables"
    );
}

#[test]
fn migrations_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sb.db");
    for _ in 0..3 {
        let db = Db::open(&path, DEV_KEY).unwrap();
        assert_eq!(db.schema_version().unwrap(), migrations::SCHEMA_VERSION);
    }
}

#[test]
fn database_is_encrypted_and_rejects_the_wrong_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sb.db");
    {
        let db = Db::open(&path, Key::Passphrase("correct-horse")).unwrap();
        subjects::create(&db, "MTH 101").unwrap();
    }

    // The file must not look like a plain SQLite database.
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes.starts_with(b"SQLite format 3"),
        "the database file is not encrypted"
    );

    // Phase 3 depends on this: a copied database cannot be opened elsewhere.
    assert!(matches!(
        Db::open(&path, Key::Passphrase("wrong-key")),
        Err(CoreError::Locked)
    ));

    // The right key still works.
    let db = Db::open(&path, Key::Passphrase("correct-horse")).unwrap();
    assert_eq!(subjects::list(&db, false).unwrap().len(), 1);
}

#[test]
fn settings_round_trip() {
    let db = db();
    assert_eq!(db.setting("theme").unwrap(), None);
    db.set_setting("theme", "dark").unwrap();
    db.set_setting("theme", "system").unwrap();
    assert_eq!(db.setting("theme").unwrap().as_deref(), Some("system"));
}

#[test]
fn subject_crud() {
    let db = db();
    let created = subjects::create(&db, "  Intro to Biology  ").unwrap();
    assert_eq!(created.name, "Intro to Biology", "names are trimmed");
    assert!(!created.archived);
    assert_eq!(created.topic_count, 0);

    let renamed = subjects::rename(&db, created.id, "Biology 101").unwrap();
    assert_eq!(renamed.name, "Biology 101");

    let archived = subjects::set_archived(&db, created.id, true).unwrap();
    assert!(archived.archived);
    assert!(subjects::list(&db, false).unwrap().is_empty());
    assert_eq!(subjects::list(&db, true).unwrap().len(), 1);

    subjects::set_archived(&db, created.id, false).unwrap();
    assert_eq!(subjects::list(&db, false).unwrap().len(), 1);

    subjects::delete(&db, created.id).unwrap();
    assert!(subjects::list(&db, true).unwrap().is_empty());
}

#[test]
fn subject_validation() {
    let db = db();
    assert!(matches!(
        subjects::create(&db, "   "),
        Err(CoreError::Invalid(_))
    ));
    assert!(matches!(
        subjects::create(&db, &"x".repeat(121)),
        Err(CoreError::Invalid(_))
    ));
    subjects::create(&db, "MTH 101").unwrap();
    assert!(
        matches!(subjects::create(&db, "MTH 101"), Err(CoreError::Invalid(_))),
        "duplicate names are reported, not crashed on"
    );
    assert!(matches!(
        subjects::rename(&db, 9999, "Nope"),
        Err(CoreError::NotFound { .. })
    ));
}

#[test]
fn subject_list_puts_archived_last_and_sorts_by_name() {
    let db = db();
    let zoo = subjects::create(&db, "Zoology").unwrap();
    subjects::create(&db, "anatomy").unwrap();
    subjects::create(&db, "Biology").unwrap();
    subjects::set_archived(&db, zoo.id, true).unwrap();

    let names: Vec<_> = subjects::list(&db, true)
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, vec!["anatomy", "Biology", "Zoology"]);
}

fn topic(db: &Db, subject_id: i64, title: &str, parent: Option<i64>) -> studybites_core::Topic {
    topics::create(
        db,
        &NewTopic {
            subject_id,
            parent_id: parent,
            title: title.to_string(),
            week: None,
            is_off_scheme: false,
        },
    )
    .unwrap()
}

#[test]
fn topic_outline_builds_a_tree_in_order() {
    let db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let t1 = topic(&db, subject.id, "Sets", None);
    let t2 = topic(&db, subject.id, "Logarithms", None);
    let c1 = topic(&db, subject.id, "Laws of logs", Some(t2.id));
    topic(&db, subject.id, "Change of base", Some(t2.id));

    assert_eq!(t1.position, 0);
    assert_eq!(t2.position, 1);
    assert_eq!(c1.position, 0, "children have their own position scale");

    let tree = topics::list_tree(&db, subject.id).unwrap();
    assert_eq!(tree.len(), 2);
    assert_eq!(tree[0].title, "Sets");
    assert_eq!(tree[1].title, "Logarithms");
    let child_titles: Vec<_> = tree[1].children.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(child_titles, vec!["Laws of logs", "Change of base"]);

    assert_eq!(subjects::get(&db, subject.id).unwrap().topic_count, 4);
}

#[test]
fn topic_validation() {
    let db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();

    assert!(matches!(
        topics::create(
            &db,
            &NewTopic {
                subject_id: subject.id,
                title: "  ".into(),
                ..Default::default()
            }
        ),
        Err(CoreError::Invalid(_))
    ));
    assert!(matches!(
        topics::create(
            &db,
            &NewTopic {
                subject_id: subject.id,
                title: "Week zero".into(),
                week: Some(0),
                ..Default::default()
            }
        ),
        Err(CoreError::Invalid(_))
    ));
    assert!(
        matches!(
            topics::create(
                &db,
                &NewTopic {
                    subject_id: 4242,
                    title: "Orphan".into(),
                    ..Default::default()
                }
            ),
            Err(CoreError::NotFound { .. })
        ),
        "a topic cannot be created under a subject that does not exist"
    );

    // No nesting beyond one level.
    let parent = topic(&db, subject.id, "Logarithms", None);
    let child = topic(&db, subject.id, "Laws of logs", Some(parent.id));
    assert!(matches!(
        topics::create(
            &db,
            &NewTopic {
                subject_id: subject.id,
                parent_id: Some(child.id),
                title: "Too deep".into(),
                ..Default::default()
            }
        ),
        Err(CoreError::Invalid(_))
    ));

    // A parent from another subject is rejected.
    let other = subjects::create(&db, "BIO 101").unwrap();
    assert!(matches!(
        topics::create(
            &db,
            &NewTopic {
                subject_id: other.id,
                parent_id: Some(parent.id),
                title: "Wrong subject".into(),
                ..Default::default()
            }
        ),
        Err(CoreError::Invalid(_))
    ));
}

#[test]
fn topic_update_edits_title_and_week() {
    let db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let t = topics::create(
        &db,
        &NewTopic {
            subject_id: subject.id,
            title: "Sets".into(),
            week: Some(2),
            ..Default::default()
        },
    )
    .unwrap();

    let only_title = topics::update(&db, t.id, Some("Sets and relations"), None).unwrap();
    assert_eq!(only_title.title, "Sets and relations");
    assert_eq!(only_title.week, Some(2), "week untouched when not supplied");

    let cleared = topics::update(&db, t.id, None, Some(None)).unwrap();
    assert_eq!(cleared.week, None);
    assert_eq!(cleared.title, "Sets and relations");

    assert!(matches!(
        topics::update(&db, t.id, None, Some(Some(999))),
        Err(CoreError::Invalid(_))
    ));
}

#[test]
fn topic_reorder_rewrites_positions() {
    let mut db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let a = topic(&db, subject.id, "A", None);
    let b = topic(&db, subject.id, "B", None);
    let c = topic(&db, subject.id, "C", None);

    let tree = topics::reorder(&mut db, subject.id, None, &[c.id, a.id, b.id]).unwrap();
    let titles: Vec<_> = tree.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, vec!["C", "A", "B"]);
    assert_eq!(tree[0].position, 0);
    assert_eq!(tree[2].position, 2);

    // A partial or padded list is refused rather than half-applied.
    assert!(matches!(
        topics::reorder(&mut db, subject.id, None, &[a.id, b.id]),
        Err(CoreError::Invalid(_))
    ));
    assert!(matches!(
        topics::reorder(&mut db, subject.id, None, &[a.id, b.id, c.id, 9999]),
        Err(CoreError::Invalid(_))
    ));
    let unchanged: Vec<_> = topics::list_tree(&db, subject.id)
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        unchanged,
        vec!["C", "A", "B"],
        "a refused reorder changes nothing"
    );
}

#[test]
fn topic_nudge_moves_one_step_and_stops_at_the_ends() {
    let mut db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let a = topic(&db, subject.id, "A", None);
    let b = topic(&db, subject.id, "B", None);
    let c = topic(&db, subject.id, "C", None);

    let titles = |tree: Vec<studybites_core::Topic>| -> Vec<String> {
        tree.into_iter().map(|t| t.title).collect()
    };

    assert_eq!(
        titles(topics::nudge(&mut db, c.id, -1).unwrap()),
        ["A", "C", "B"]
    );
    assert_eq!(
        titles(topics::nudge(&mut db, a.id, -1).unwrap()),
        ["A", "C", "B"]
    );
    assert_eq!(
        titles(topics::nudge(&mut db, b.id, 1).unwrap()),
        ["A", "C", "B"]
    );
    assert_eq!(
        titles(topics::nudge(&mut db, a.id, 1).unwrap()),
        ["C", "A", "B"]
    );
}

#[test]
fn deleting_a_topic_removes_children_and_closes_gaps() {
    let db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    let a = topic(&db, subject.id, "A", None);
    let b = topic(&db, subject.id, "B", None);
    let c = topic(&db, subject.id, "C", None);
    topic(&db, subject.id, "B child", Some(b.id));

    topics::delete(&db, b.id).unwrap();
    let tree = topics::list_tree(&db, subject.id).unwrap();
    let titles: Vec<_> = tree.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, vec!["A", "C"], "the child went with its parent");
    assert_eq!(tree[0].id, a.id);
    assert_eq!(tree[0].position, 0);
    assert_eq!(tree[1].id, c.id);
    assert_eq!(tree[1].position, 1, "positions were compacted");

    assert!(matches!(
        topics::delete(&db, 9999),
        Err(CoreError::NotFound { .. })
    ));
}

#[test]
fn deleting_a_subject_cascades_to_its_topics() {
    let db = db();
    let subject = subjects::create(&db, "MTH 101").unwrap();
    topic(&db, subject.id, "Sets", None);
    subjects::delete(&db, subject.id).unwrap();

    let left: i64 = db
        .conn()
        .query_row("SELECT count(*) FROM scheme_topics", [], |r| r.get(0))
        .unwrap();
    assert_eq!(left, 0, "foreign keys must be ON for this to hold");
}
