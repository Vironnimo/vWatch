mod common;

use chrono::Duration;
use common::{card_with, demo_repo, service};
use serde_json::json;

#[test]
fn saved_ideas_keep_their_source_and_first_appearance_after_reports_change() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card_with(json!({"suggestions": [{
        "id": "sg-saved", "type": "feature", "title": "Suche merken", "why": "Wiedereinstieg verbessern."
    }]})), false, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin", "synergies": [{
        "id": "sy-saved", "title": "Gemeinsame Suche", "why": "Mehrere Projekte durchsuchen.", "projects": ["demo"]
    }]}), false, false).unwrap();
    let first_at = "2026-09-30T08:00:00Z";
    clock.advance(Duration::days(1));
    svc.set_feedback("sg-saved", Some("up"), Some("Gute Idee"))
        .unwrap();
    svc.set_feedback("sy-saved", Some("later"), None).unwrap();
    svc.put_card("demo", json!({"suggestions": []}), true, false)
        .unwrap();
    svc.put_portfolio(json!({"synergies": []}), true, false)
        .unwrap();
    let task = svc
        .create_task(json!({
            "projects": ["demo"], "source_id": "sg-saved", "title": "Suche merken"
        }))
        .unwrap();
    assert_eq!(task["task"]["source_snapshot"]["kind"], "suggestion");
    svc.remove_project("demo").unwrap();
    let state = svc.state(false).unwrap();
    let rating = &state["feedback"]["sg-saved"];
    assert_eq!(rating["project_id"], "demo");
    assert_eq!(rating["title"], "Suche merken");
    assert_eq!(rating["item"]["why"], "Wiedereinstieg verbessern.");
    assert_eq!(rating["first_seen"], first_at);
    assert_eq!(state["feedback"]["sy-saved"]["first_seen"], first_at);
    assert_eq!(state["tasks"][0]["source_kind"], "suggestion");
    assert_eq!(state["tasks"][0]["source_first_seen"], first_at);
    assert_eq!(
        state["tasks"][0]["source_snapshot"],
        task["task"]["source_snapshot"]
    );
}

#[test]
fn task_sources_recover_legacy_idea_kinds_without_inventing_them() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card_with(json!({"suggestions": [{
        "id": "sg-old", "type": "feature", "title": "Export merken", "why": "Später wiederfinden."
    }]})), false, false).unwrap();
    let created = svc
        .create_task(json!({"projects": ["demo"], "source_id": "sg-old", "title": "Export merken"}))
        .unwrap();
    assert_eq!(created["task"]["source_snapshot"]["kind"], "suggestion");
    // Simulate a task written by the older app, before source snapshots existed.
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    db.execute(
        "UPDATE tasks SET data = json_remove(data, '$.source_snapshot')",
        [],
    )
    .unwrap();
    clock.advance(Duration::days(1));
    svc.put_card("demo", json!({"suggestions": []}), true, false)
        .unwrap();
    svc.create_task(json!({"projects": ["demo"], "source_id": "open:unknown", "title": "Anderen Auftrag erledigen"})).unwrap();
    let state = svc.state(false).unwrap();
    let tasks = state["tasks"].as_array().unwrap();
    let old = tasks
        .iter()
        .find(|task| task["source_id"] == "sg-old")
        .unwrap();
    assert_eq!(old["source_kind"], "suggestion");
    assert_eq!(old["source_snapshot"]["title"], "Export merken");
    assert_eq!(old["source_first_seen"], "2026-09-30T08:00:00Z");
    assert!(tasks
        .iter()
        .find(|task| task["source_id"] == "open:unknown")
        .unwrap()["source_kind"]
        .is_null());
    assert!(
        svc.tasks(None).unwrap()["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["source_id"] == "sg-old")
            .unwrap()
            .get("source_snapshot")
            .is_none(),
        "Projection does not rewrite saved legacy tasks"
    );
}

#[test]
fn owner_ideas_have_stable_rating_and_task_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let id = svc.add_idea("Ein gemeinsames Suchfenster").unwrap()["id"]
        .as_i64()
        .unwrap();
    let source = format!("idea:{id}");
    svc.put_portfolio(json!({"greeting": "Moin", "idea_checks": [{
        "idea": id, "verdict": "build", "fits": [], "why": "Fundstellen werden schneller erreichbar."
    }]}), false, false).unwrap();
    svc.set_feedback(&source, Some("up"), None).unwrap();
    let rating = &svc.state(false).unwrap()["feedback"][&source];
    assert_eq!(rating["item"]["kind"], "owner_idea");
    assert_eq!(rating["item"]["assessment"]["verdict"], "build");
    assert_eq!(rating["first_seen"], "2026-09-30T08:00:00Z");
    assert!(rating["project_id"].is_null());
    let task = svc
        .create_task(
            json!({"projects": [], "source_id": source, "title": "Suchfenster vorbereiten"}),
        )
        .unwrap();
    assert_eq!(
        task["task"]["source_snapshot"]["text"],
        "Ein gemeinsames Suchfenster"
    );
    svc.remove_idea(id).unwrap();
    assert_eq!(svc.state(false).unwrap()["ideas"], json!([]));
    assert_eq!(
        svc.state(false).unwrap()["tasks"][0]["source_kind"],
        "owner_idea"
    );
    svc.restore_idea(id).unwrap();
    assert_eq!(
        svc.state(false).unwrap()["feedback"][&source]["value"],
        "up"
    );
    assert_eq!(
        svc.set_feedback(&format!("idea:0{id}"), Some("up"), None)
            .unwrap_err()
            .code,
        "unknown_item"
    );
    assert_eq!(svc.put_portfolio(json!({"new_apps": [{
        "id": source, "title": "Falsche Identität", "pitch": "Neue App", "because": "Eine Idee"
    }]}), true, false).unwrap_err().code, "invalid_portfolio");
    svc.validate_pref("style", &json!("ideas")).unwrap();
}
