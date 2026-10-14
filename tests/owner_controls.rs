mod common;

use chrono::Duration;
use common::{card, demo_repo, service};
use serde_json::{json, Value};
use watchtower::Watchtower;

fn warning(svc: &Watchtower, key: &str) -> Value {
    let warnings = svc.project_warnings("demo").unwrap();
    warnings
        .as_array()
        .unwrap()
        .iter()
        .find(|warning| warning["key"] == key)
        .unwrap_or_else(|| panic!("Missing warning {key}: {warnings}"))
        .clone()
}

fn save_facts(svc: &Watchtower, facts: &Value) {
    svc.store()
        .save_facts(
            "demo",
            &svc.now(),
            &watchtower::facts::fingerprint(facts),
            facts,
        )
        .unwrap();
}

#[test]
fn warning_acknowledgments_keep_facts_and_reappear_when_local_risk_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let initial = svc.store().facts("demo").unwrap().unwrap().data;
    assert_eq!(warning(&svc, "no_remote")["acknowledged"], false);
    let original_facts = svc.facts("demo").unwrap();
    svc.set_warning_ack("demo", "no_remote", Some("Absichtlich nur lokal"))
        .unwrap();
    let acknowledged = warning(&svc, "no_remote");
    assert_eq!(acknowledged["acknowledged"], true);
    assert_eq!(acknowledged["note"], "Absichtlich nur lokal");
    assert_eq!(svc.facts("demo").unwrap(), original_facts);

    // A newer measurement and file timestamps alone do not change the owner's decision.
    clock.advance(Duration::days(1));
    let mut measured_later = initial.clone();
    measured_later["docs_mtime"] = json!("2026-10-01T08:00:00Z");
    measured_later["dirty"]["oldest_change"] = json!("2026-10-01T07:00:00Z");
    measured_later["dirty"]["newest_change"] = json!("2026-10-01T08:00:00Z");
    save_facts(&svc, &measured_later);
    assert_eq!(warning(&svc, "no_remote")["acknowledged"], true);

    for (field, changed) in [
        ("head", json!("a-new-commit")),
        ("dirty", {
            let mut dirty = initial["dirty"].clone();
            dirty["files"] = json!(2);
            dirty["untracked"] = json!(2);
            dirty
        }),
    ] {
        save_facts(&svc, &initial);
        svc.set_warning_ack("demo", "no_remote", Some("Weiterhin lokal"))
            .unwrap();
        let mut facts = initial.clone();
        facts[field] = changed;
        save_facts(&svc, &facts);
        assert_eq!(
            warning(&svc, "no_remote")["acknowledged"],
            false,
            "A material change to {field} must bring the warning back"
        );
        assert_eq!(svc.store().facts("demo").unwrap().unwrap().data, facts);
    }
}

#[test]
fn worktree_acknowledgments_follow_the_work_and_persist_across_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let mut initial = svc.store().facts("demo").unwrap().unwrap().data;
    initial["worktrees"] = json!({
        "count": 2,
        "prunable": 0,
        "items": [{"path": "C:/work/demo-task", "head": "original-head", "branch": "task",
            "dirty_files": 0, "dirty_fingerprint": "clean", "prunable": false},
            {"path": "C:/work/demo-finished", "head": "finished-head", "branch": "finished",
            "dirty_files": 0, "dirty_fingerprint": "clean", "prunable": false}]
    });
    save_facts(&svc, &initial);
    svc.set_warning_ack("demo", "worktrees", Some("Agent arbeitet hier"))
        .unwrap();
    drop(svc);
    let (svc, _) = service(tmp.path());
    assert_eq!(warning(&svc, "worktrees")["acknowledged"], true);
    assert_eq!(warning(&svc, "worktrees")["note"], "Agent arbeitet hier");

    let mut fewer_worktrees = initial.clone();
    fewer_worktrees["worktrees"]["items"]
        .as_array_mut()
        .unwrap()
        .pop();
    fewer_worktrees["worktrees"]["count"] = json!(1);
    save_facts(&svc, &fewer_worktrees);
    assert_eq!(warning(&svc, "worktrees")["acknowledged"], true);

    for (field, value) in [
        ("path", json!("C:/work/different-task")),
        ("head", json!("a-new-worktree-commit")),
        ("dirty_files", json!(1)),
        ("dirty_fingerprint", json!("different-content")),
        ("prunable", json!(true)),
    ] {
        save_facts(&svc, &initial);
        svc.set_warning_ack("demo", "worktrees", Some("Agent arbeitet hier"))
            .unwrap();
        let mut changed = initial.clone();
        changed["worktrees"]["items"][0][field] = value;
        if field == "prunable" {
            changed["worktrees"]["prunable"] = json!(1);
        }
        save_facts(&svc, &changed);
        assert_eq!(
            warning(&svc, "worktrees")["acknowledged"],
            false,
            "A material worktree change to {field} must bring the warning back"
        );
        assert_eq!(svc.store().facts("demo").unwrap().unwrap().data, changed);
    }
}

#[test]
fn warning_acknowledgments_require_a_reason_and_can_be_withdrawn() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let mut facts = svc.store().facts("demo").unwrap().unwrap().data;
    facts["ci"] = json!({"state": "none"});
    save_facts(&svc, &facts);
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], false);
    svc.set_warning_ack("demo", "ci_none", Some("Lokale Tests genügen"))
        .unwrap();
    let due = svc.due(14, false).unwrap();
    assert_eq!(due["projects"][0]["reason"], "status_changed");
    assert_eq!(due["portfolio"]["due"], true);
    assert_eq!(
        svc.context("demo").unwrap()["warning_acknowledgements"][0]["note"],
        "Lokale Tests genügen"
    );
    assert_eq!(
        svc.portfolio_context().unwrap()["projects"][0]["warning_acknowledgements"][0]["note"],
        "Lokale Tests genügen"
    );
    svc.put_card("demo", json!({"headline": "Alles ruhig"}), true, false)
        .unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    clock.advance(Duration::minutes(1));
    svc.set_warning_ack("demo", "ci_none", Some("Lokale Tests genügen"))
        .unwrap();
    let unchanged = svc.due(14, false).unwrap();
    assert_eq!(unchanged["projects"], json!([]));
    assert_eq!(unchanged["portfolio"]["due"], false);
    let before = svc.project_warnings("demo").unwrap();
    for (key, note) in [("ci_none", ""), ("ci_none", "   "), ("unknown", "Grund")] {
        assert!(svc.set_warning_ack("demo", key, Some(note)).is_err());
        assert_eq!(svc.project_warnings("demo").unwrap(), before);
        assert_eq!(svc.store().facts("demo").unwrap().unwrap().data, facts);
    }
    assert!(svc.set_warning_ack("demo", "unknown", None).is_err());
    assert!(svc
        .set_warning_ack("missing", "ci_none", Some("Grund"))
        .is_err());
    assert_eq!(svc.project_warnings("demo").unwrap(), before);
    svc.set_warning_ack("demo", "ci_none", None).unwrap();
    assert_eq!(warning(&svc, "ci_none")["acknowledged"], false);
    assert_eq!(svc.store().facts("demo").unwrap().unwrap().data, facts);
    let due = svc.due(14, false).unwrap();
    assert_eq!(due["projects"][0]["reason"], "status_changed");
    assert_eq!(due["portfolio"]["due"], true);

    facts["ci"] = json!({"state": "red", "failing": ["Tests"]});
    save_facts(&svc, &facts);
    assert_eq!(warning(&svc, "ci_red")["can_acknowledge"], false);
    assert!(svc
        .set_warning_ack("demo", "ci_red", Some("Ignorieren"))
        .is_err());
    assert_eq!(warning(&svc, "ci_red")["acknowledged"], false);
    assert_eq!(svc.store().facts("demo").unwrap().unwrap().data, facts);
}

fn suggestion(svc: &Watchtower) -> String {
    svc.put_card("demo", card(), false, false).unwrap()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn snooze_has_a_calendar_deadline_that_note_edits_do_not_reset() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let id = suggestion(&svc);
    svc.set_feedback(&id, Some("later"), Some("Nach dem Release"))
        .unwrap();
    assert_eq!(
        svc.feedback(None).unwrap()[0]["snoozed_until"],
        "2026-10-14"
    );
    assert_eq!(
        svc.state(false).unwrap()["feedback"][&id]["snoozed_until"],
        "2026-10-14"
    );
    assert_eq!(
        svc.context("demo").unwrap()["feedback"][0]["snoozed_until"],
        "2026-10-14"
    );
    clock.advance(Duration::days(3));
    svc.set_feedback(&id, Some("later"), Some("Release braucht noch Zeit"))
        .unwrap();
    let updated = &svc.feedback(None).unwrap()[0];
    assert_eq!(updated["note"], "Release braucht noch Zeit");
    assert_eq!(updated["snoozed_until"], "2026-10-14");

    svc.set_feedback_until(&id, Some("later"), None, Some("2026-11-01"))
        .unwrap();
    assert_eq!(
        svc.feedback(None).unwrap()[0]["snoozed_until"],
        "2026-11-01"
    );
    svc.set_feedback(&id, Some("up"), None).unwrap();
    assert!(svc.feedback(None).unwrap()[0]["snoozed_until"].is_null());
    svc.set_feedback(&id, Some("later"), None).unwrap();
    assert_eq!(
        svc.feedback(None).unwrap()[0]["snoozed_until"],
        "2026-10-17"
    );
    svc.set_feedback(&id, None, None).unwrap();
    assert_eq!(svc.feedback(None).unwrap(), json!([]));
}

#[test]
fn invalid_snooze_dates_leave_existing_feedback_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let id = suggestion(&svc);
    svc.set_feedback_until(&id, Some("later"), Some("Warten"), Some("2026-10-01"))
        .unwrap();
    let before = svc.feedback(None).unwrap();
    for until in [
        "",
        "2026-09-29",
        "2026-09-30",
        "2026-02-30",
        "2026-13-01",
        "2026-10-1",
        "2026-10-01T00:00:00Z",
        " 2026-10-01 ",
    ] {
        assert!(
            svc.set_feedback_until(&id, Some("later"), Some("Nicht speichern"), Some(until))
                .is_err(),
            "Accepted invalid deadline {until:?}"
        );
        assert_eq!(svc.feedback(None).unwrap(), before);
    }
    for value in [Some("up"), Some("down"), None] {
        assert!(svc
            .set_feedback_until(&id, value, Some("Nicht speichern"), Some("2026-11-01"))
            .is_err());
        assert_eq!(svc.feedback(None).unwrap(), before);
    }
}

#[test]
fn legacy_snoozes_keep_the_original_deadline_after_migration_and_note_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let id = suggestion(&svc);
    svc.set_feedback(&id, Some("later"), Some("Ursprünglicher Aufschub"))
        .unwrap();
    drop(svc);
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    common::remove_history_schema(&db);
    db.execute_batch("ALTER TABLE feedback DROP COLUMN snoozed_until;")
        .unwrap();
    drop(db);

    let (svc, clock) = service(tmp.path());
    clock.advance(Duration::days(7));
    assert_eq!(
        svc.feedback(None).unwrap()[0]["snoozed_until"],
        "2026-10-14"
    );
    svc.set_feedback(&id, Some("later"), Some("Nur der Grund ändert sich"))
        .unwrap();
    assert_eq!(
        svc.feedback(None).unwrap()[0]["snoozed_until"],
        "2026-10-14"
    );
}

#[test]
fn undo_restores_an_expired_snooze_exactly_and_rejects_invalid_snapshots() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let id = suggestion(&svc);
    let original = json!({
        "value": "later", "note": "Nach dem Release", "snoozed_until": "2026-10-01"
    });
    svc.set_feedback_until(
        &id,
        Some("later"),
        Some("Nach dem Release"),
        Some("2026-10-01"),
    )
    .unwrap();
    clock.advance(Duration::days(2));
    svc.set_feedback(&id, Some("up"), Some("Schon erledigen"))
        .unwrap();
    svc.restore_feedback(&id, Some(&original)).unwrap();
    let restored = svc.feedback(None).unwrap();
    assert_eq!(restored[0]["value"], original["value"]);
    assert_eq!(restored[0]["note"], original["note"]);
    assert_eq!(restored[0]["snoozed_until"], original["snoozed_until"]);
    assert!(restored[0]["snoozed_until"].as_str().unwrap() < svc.today().to_string().as_str());

    for invalid in [
        json!({"value": "later", "note": "Ungültig", "snoozed_until": "2026-02-30"}),
        json!({"value": "later", "note": "Ungültig", "snoozed_until": "2026-10-1"}),
        json!({"value": "up", "note": "Ungültig", "snoozed_until": "2026-10-01"}),
        json!({"value": "answered", "note": "Passt nicht zu Vorschlägen", "snoozed_until": null}),
        json!({"value": "later", "note": 42, "snoozed_until": "2026-10-01"}),
        json!({"value": "later", "note": "Frist fehlt"}),
        json!({"value": "later", "note": "Frist fehlt", "snoozed_until": null}),
        json!({"note": "Bewertung fehlt", "snoozed_until": null}),
        json!([]),
    ] {
        assert!(
            svc.restore_feedback(&id, Some(&invalid)).is_err(),
            "Accepted invalid undo snapshot {invalid}"
        );
        assert_eq!(svc.feedback(None).unwrap(), restored);
    }
    svc.restore_feedback(&id, None).unwrap();
    assert_eq!(svc.feedback(None).unwrap(), json!([]));
}
