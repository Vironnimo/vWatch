mod common;

use chrono::Duration;
use common::{card, demo_repo, make_repo, service};
use serde_json::{json, Value};
use watchtower::Watchtower;

fn token(result: &Value) -> &str {
    result["undo"]
        .as_str()
        .expect("mutation provides an undo command")
        .strip_prefix("watchtower undo ")
        .unwrap()
}

fn suggestion(svc: &Watchtower) -> String {
    svc.put_card("demo", card(), false, false).unwrap();
    svc.card("demo").unwrap()["card"]["suggestions"][0]["id"]
        .as_str()
        .unwrap()
        .into()
}

fn count_receipts(dir: &std::path::Path) -> i64 {
    rusqlite::Connection::open(dir.join("watchtower.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM owner_undo", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn project_undo_is_durable_field_scoped_and_repeated_calls_keep_the_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, clock) = service(dir.path());
    svc.add_project(demo_repo(dir.path()).to_str().unwrap(), None, None)
        .unwrap();
    let original = svc.store().project("demo").unwrap().unwrap();
    let renamed = svc.rename_project("demo", "Changed").unwrap();
    assert_eq!(renamed["name"], "Changed");
    let repeat = svc.rename_project("demo", "Changed").unwrap();
    assert_eq!(repeat["changed"], false);
    assert_eq!(repeat["undo"], renamed["undo"]);
    let goal = svc
        .set_goal("demo", "A separate goal", Some("2026-12-01"))
        .unwrap();
    clock.advance(Duration::days(1));
    svc.state(true).unwrap();
    let visit = svc.store().pref("last_visit").unwrap();
    drop(svc);
    let (svc, _) = service(dir.path());
    let undone = svc.undo(token(&renamed)).unwrap();
    assert_eq!(undone["changed"], true);
    assert_eq!(undone["current"]["name"], original.name);
    assert_eq!(
        svc.store().project("demo").unwrap().unwrap().goal,
        "A separate goal"
    );
    assert_eq!(svc.store().pref("last_visit").unwrap(), visit);
    assert_eq!(svc.undo(token(&renamed)).unwrap()["changed"], false);
    svc.undo(token(&goal)).unwrap();
    assert_eq!(
        svc.store().project("demo").unwrap().unwrap().goal,
        original.goal
    );
    // A later write back to the same text still invalidates an older receipt.
    svc.rename_project("demo", "Other").unwrap();
    svc.rename_project("demo", &original.name).unwrap();
    let error = svc.undo(token(&renamed)).unwrap_err();
    assert_eq!(error.code, "undo_conflict");
    assert!(error.message.contains("Nothing was changed"));
    assert!(error.message.contains("Current values"));
}

#[test]
fn status_and_goal_undo_preserve_unrelated_owner_edits_and_noop_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, clock) = service(dir.path());
    svc.add_project(demo_repo(dir.path()).to_str().unwrap(), None, None)
        .unwrap();
    let goal = svc
        .set_goal("demo", "Original", Some("2026-12-01"))
        .unwrap();
    let at = svc
        .store()
        .project("demo")
        .unwrap()
        .unwrap()
        .status_changed_at;
    clock.advance(Duration::days(1));
    let repeated = svc
        .set_goal("demo", "Original", Some("2026-12-01"))
        .unwrap();
    assert_eq!(repeated["undo"], goal["undo"]);
    assert_eq!(
        svc.store()
            .project("demo")
            .unwrap()
            .unwrap()
            .status_changed_at,
        at
    );
    let status = svc
        .set_status("demo", "parked", Some("Wait for owner"))
        .unwrap();
    let status_at = svc
        .store()
        .project("demo")
        .unwrap()
        .unwrap()
        .status_changed_at;
    svc.undo(token(&goal)).unwrap();
    let project = svc.store().project("demo").unwrap().unwrap();
    assert_eq!(project.status, "parked");
    assert_eq!(project.status_note, "Wait for owner");
    assert_eq!(project.status_changed_at, status_at);
    svc.undo(token(&status)).unwrap();
    let project = svc.store().project("demo").unwrap().unwrap();
    assert_eq!(project.status, "active");
    assert_eq!(project.status_note, "");
}

#[test]
fn path_undo_keeps_a_later_name_and_discards_measurements_from_the_other_folder() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _) = service(dir.path());
    let original = demo_repo(dir.path());
    let moved = make_repo(dir.path(), "other", &["2026-09-29T10:00:00"]);
    svc.add_project(original.to_str().unwrap(), None, None)
        .unwrap();
    let relinked = svc
        .update_project("demo", None, Some(moved.to_str().unwrap()))
        .unwrap();
    svc.rename_project("demo", "Keep this name").unwrap();
    assert!(svc.store().facts("demo").unwrap().is_some());
    let result = svc.undo(token(&relinked)).unwrap();
    let project = svc.store().project("demo").unwrap().unwrap();
    assert_eq!(project.name, "Keep this name");
    assert_eq!(
        std::path::Path::new(&project.path).canonicalize().unwrap(),
        original.canonicalize().unwrap()
    );
    assert!(svc.store().facts("demo").unwrap().is_none());
    assert_eq!(result["next"], "watchtower facts refresh demo");
    assert_eq!(svc.undo(token(&relinked)).unwrap()["changed"], false);

    let relinked = svc
        .update_project("demo", None, Some(moved.to_str().unwrap()))
        .unwrap();
    svc.add_project(original.to_str().unwrap(), Some("replacement"), None)
        .unwrap();
    let error = svc.undo(token(&relinked)).unwrap_err();
    assert_eq!(error.code, "undo_conflict");
    assert!(error.message.contains("replacement"));
    assert!(error.message.contains("Nothing was changed"));
    assert_eq!(
        std::path::Path::new(&svc.store().project("demo").unwrap().unwrap().path)
            .canonicalize()
            .unwrap(),
        moved.canonicalize().unwrap()
    );
}

#[test]
fn feedback_and_warning_undo_restore_exact_snapshots_even_after_snooze_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, clock) = service(dir.path());
    svc.add_project(demo_repo(dir.path()).to_str().unwrap(), None, None)
        .unwrap();
    let id = suggestion(&svc);
    svc.set_feedback_until(&id, Some("later"), Some("Wait"), Some("2026-10-01"))
        .unwrap();
    let original = svc.feedback(None).unwrap();
    clock.advance(Duration::days(5));
    let cleared = svc.set_feedback(&id, None, None).unwrap();
    assert!(svc.feedback(None).unwrap().as_array().unwrap().is_empty());
    svc.undo(token(&cleared)).unwrap();
    assert_eq!(svc.feedback(None).unwrap(), original);
    assert_eq!(svc.undo(token(&cleared)).unwrap()["changed"], false);
    let changed = svc.set_feedback(&id, Some("up"), Some("Go")).unwrap();
    assert_eq!(
        svc.set_feedback(&id, Some("up"), Some("Go")).unwrap()["undo"],
        changed["undo"]
    );
    svc.set_feedback(&id, Some("down"), Some("Changed mind"))
        .unwrap();
    assert_eq!(svc.undo(token(&changed)).unwrap_err().code, "undo_conflict");

    svc.set_warning_ack("demo", "no_remote", Some("Intentionally local"))
        .unwrap();
    let original = svc.warning_acknowledgements("demo").unwrap();
    let cleared = svc.set_warning_ack("demo", "no_remote", None).unwrap();
    svc.undo(token(&cleared)).unwrap();
    assert_eq!(svc.warning_acknowledgements("demo").unwrap(), original);
}

#[test]
fn preferences_undo_is_atomic_scoped_and_failure_creates_no_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _) = service(dir.path());
    let original = svc
        .update_prefs(&[
            ("style", json!("board")),
            ("hidden_modules", json!(["pulse"])),
        ])
        .unwrap();
    let before_count = count_receipts(dir.path());
    assert!(svc
        .update_prefs(&[
            ("style", json!("handbook")),
            ("hidden_modules", json!(["not-a-module"]))
        ])
        .is_err());
    assert_eq!(count_receipts(dir.path()), before_count);
    assert_eq!(svc.store().pref("style").unwrap(), Some(json!("board")));
    svc.store()
        .set_pref("last_visit", &json!("new visit"))
        .unwrap();
    svc.undo(token(&original)).unwrap();
    assert_eq!(svc.store().pref("style").unwrap(), None);
    assert_eq!(svc.store().pref("hidden_modules").unwrap(), None);
    assert_eq!(
        svc.store().pref("last_visit").unwrap(),
        Some(json!("new visit"))
    );
    let style = svc.update_prefs(&[("style", json!("board"))]).unwrap();
    svc.update_prefs(&[("hidden_modules", json!(["pulse"]))])
        .unwrap();
    svc.undo(token(&style)).unwrap();
    assert_eq!(
        svc.store().pref("hidden_modules").unwrap(),
        Some(json!(["pulse"]))
    );
    assert_eq!(svc.undo("missing").unwrap_err().code, "unknown_undo");
}

#[test]
fn a_failed_receipt_write_rolls_back_the_edit_and_failed_undo_rolls_back_restoration() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _) = service(dir.path());
    svc.add_project(demo_repo(dir.path()).to_str().unwrap(), None, None)
        .unwrap();
    let db = rusqlite::Connection::open(dir.path().join("watchtower.db")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_receipt BEFORE INSERT ON owner_undo BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(svc.set_goal("demo", "Must roll back", None).is_err());
    assert_eq!(svc.store().project("demo").unwrap().unwrap().goal, "");
    assert_eq!(count_receipts(dir.path()), 0);
    db.execute_batch("DROP TRIGGER reject_receipt;").unwrap();
    let saved = svc.set_goal("demo", "Keep on failed undo", None).unwrap();
    db.execute_batch("CREATE TRIGGER reject_undo BEFORE UPDATE ON owner_undo BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(svc.undo(token(&saved)).is_err());
    assert_eq!(
        svc.store().project("demo").unwrap().unwrap().goal,
        "Keep on failed undo"
    );
    db.execute_batch("DROP TRIGGER reject_undo;").unwrap();
    svc.undo(token(&saved)).unwrap();
    assert_eq!(svc.store().project("demo").unwrap().unwrap().goal, "");
}

#[test]
fn concurrent_feedback_writers_preserve_omitted_notes_and_inherited_snooze_dates() {
    let dir = tempfile::tempdir().unwrap();
    let (svc, _) = service(dir.path());
    svc.add_project(demo_repo(dir.path()).to_str().unwrap(), None, None)
        .unwrap();
    let id = suggestion(&svc);
    for attempt in 0..20 {
        svc.set_feedback_until(&id, Some("later"), Some("Original"), Some("2026-10-01"))
            .unwrap();
        let gate = std::sync::Arc::new(std::sync::Barrier::new(3));
        let writers: Vec<_> = [true, false]
            .into_iter()
            .map(|explicit| {
                let gate = gate.clone();
                let id = id.clone();
                let path = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    let (svc, _) = service(&path);
                    gate.wait();
                    if explicit {
                        svc.set_feedback_until(
                            &id,
                            Some("later"),
                            Some("Newer owner note"),
                            Some("2026-10-14"),
                        )
                    } else {
                        svc.set_feedback(&id, Some("later"), None)
                    }
                    .unwrap()
                })
            })
            .collect();
        gate.wait();
        for writer in writers {
            writer.join().unwrap();
        }
        let current = svc.feedback(None).unwrap();
        assert_eq!(current[0]["note"], "Newer owner note", "attempt {attempt}");
        assert_eq!(
            current[0]["snoozed_until"], "2026-10-14",
            "attempt {attempt}"
        );
    }
}
