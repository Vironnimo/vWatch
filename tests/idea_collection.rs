mod common;

use chrono::Duration;
use common::{card_with, demo_repo, service};
use serde_json::json;

#[test]
fn direct_idea_progress_and_archive_preserve_ratings_tasks_and_history() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo",card_with(json!({"suggestions":[{"id":"sg-direct","type":"feature","title":"Suche merken","why":"Wiedereinstieg"}]})),false,false).unwrap();
    let own = svc.add_idea("Eigene Suche").unwrap();
    for id in ["sg-direct".to_string(), format!("idea:{}", own["id"])] {
        svc.set_feedback(&id, Some("up"), Some("Gefällt mir"))
            .unwrap();
        let rating = svc.feedback(None).unwrap();
        let initial = svc.idea_status(&id).unwrap();
        let context = svc.context("demo").unwrap();
        let set = |action: &str, rev: &str| svc.set_idea_status(&id, action, rev).unwrap();
        let working = set(
            "in_progress",
            initial["idea_status"]["revision"].as_str().unwrap(),
        );
        assert_eq!(working["idea_status"]["status"], "in_progress");
        let task = &working["idea_status"]["task"];
        assert_eq!(task["history"][0]["actor"], "owner");
        assert!(task["reported_at"].is_null());
        if id == "sg-direct" {
            assert_ne!(
                context["context_token"],
                svc.context("demo").unwrap()["context_token"]
            );
        }
        let archived = set(
            "archived",
            working["idea_status"]["revision"].as_str().unwrap(),
        );
        assert_eq!(archived["idea_status"]["archived"], true);
        assert_eq!(
            archived["idea_status"]["task"], *task,
            "Archive leaves the running assignment intact"
        );
        assert!(archived["undo"]
            .as_str()
            .unwrap()
            .contains("restore --revision"));
        assert_eq!(
            svc.portfolio_context(&[]).unwrap()["idea_states"][&id]["archived"],
            true
        );
        let restored = set(
            "restore",
            archived["idea_status"]["revision"].as_str().unwrap(),
        );
        let done = set(
            "implemented",
            restored["idea_status"]["revision"].as_str().unwrap(),
        );
        assert_eq!(done["idea_status"]["status"], "implemented");
        assert_eq!(done["idea_status"]["task"]["id"], task["id"]);
        assert_eq!(done["idea_status"]["task"]["status"], "done");
        assert!(
            done["idea_status"]["task"]["reported_at"].is_null(),
            "No invented agent report"
        );
        assert_eq!(svc.feedback(None).unwrap(), rating);
        let repeated = set(
            "implemented",
            done["idea_status"]["revision"].as_str().unwrap(),
        );
        assert_eq!(repeated["changed"], false);
        let open = set("open", done["idea_status"]["revision"].as_str().unwrap());
        assert_eq!(open["idea_status"]["status"], "open");
        assert_eq!(open["idea_status"]["task"]["status"], "cancelled");
        let again = set(
            "in_progress",
            open["idea_status"]["revision"].as_str().unwrap(),
        );
        assert_eq!(again["idea_status"]["task"]["id"], task["id"]);
    }
    svc.put_card("demo", json!({"suggestions":[]}), true, false)
        .unwrap();
    assert_eq!(
        svc.idea_status("sg-direct").unwrap()["idea_status"]["status"],
        "in_progress"
    );
    assert_eq!(
        svc.tasks(None).unwrap()["tasks"].as_array().unwrap().len(),
        2
    );
    assert!(svc
        .history(None, Some("proposal"), Some("sg-direct"), None)
        .unwrap()
        .to_string()
        .contains("archived"));
    svc.put_portfolio(json!({"greeting":"Moin", "new_apps":[{"id":"app-direct","title":"Neue Suche","pitch":"Alle Fundstellen zusammen","because":"Fundstellen liegen in mehreren Projekten"}], "synergies":[{"id":"sy-direct","title":"Suche verbinden","why":"Gemeinsam finden","projects":["demo"]}]}),false,false).unwrap();
    for id in ["app-direct", "sy-direct"] {
        let current = svc.idea_status(id).unwrap();
        let done = svc
            .set_idea_status(
                id,
                "implemented",
                current["idea_status"]["revision"].as_str().unwrap(),
            )
            .unwrap();
        assert_eq!(done["idea_status"]["status"], "implemented");
        assert_eq!(done["idea_status"]["task"]["source_id"], id);
        assert!(done["idea_status"]["task"]["reported_at"].is_null());
    }
}

#[test]
fn direct_completion_guards_new_agent_reports_and_saves_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let id = svc.add_idea("Export").unwrap()["item_id"]
        .as_str()
        .unwrap()
        .to_string();
    let initial = svc.idea_status(&id).unwrap();
    let working = svc
        .set_idea_status(
            &id,
            "in_progress",
            initial["idea_status"]["revision"].as_str().unwrap(),
        )
        .unwrap();
    let task = &working["idea_status"]["task"];
    svc.report_task(
        task["id"].as_i64().unwrap(),
        "done",
        "Export geprüft",
        "Datei geöffnet",
    )
    .unwrap();
    let stale = svc
        .set_idea_status(
            &id,
            "implemented",
            working["idea_status"]["revision"].as_str().unwrap(),
        )
        .unwrap_err();
    assert_eq!(stale.code, "idea_changed");
    let current = svc.idea_status(&id).unwrap();
    assert_eq!(current["idea_status"]["task"]["status"], "review");
    let done = svc
        .set_idea_status(
            &id,
            "implemented",
            current["idea_status"]["revision"].as_str().unwrap(),
        )
        .unwrap();
    assert_eq!(done["idea_status"]["task"]["result"], "Export geprüft");
    let token = svc.portfolio_context(&[]).unwrap()["context_token"].clone();
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_archive BEFORE INSERT ON content_history WHEN NEW.after_data LIKE '%archived%' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(svc
        .set_idea_status(
            &id,
            "archived",
            done["idea_status"]["revision"].as_str().unwrap()
        )
        .is_err());
    assert_eq!(
        svc.idea_status(&id).unwrap()["idea_status"]["archived"],
        false
    );
    assert_eq!(token, svc.portfolio_context(&[]).unwrap()["context_token"]);
    db.execute_batch("DROP TRIGGER fail_archive;").unwrap();
    svc.set_idea_status(
        &id,
        "archived",
        done["idea_status"]["revision"].as_str().unwrap(),
    )
    .unwrap();
    assert_ne!(token, svc.portfolio_context(&[]).unwrap()["context_token"]);
    assert_eq!(svc.idea_status("missing").unwrap_err().code, "unknown_idea");
    drop(svc);
    let reopened = watchtower::Watchtower::open(&tmp.path().join("watchtower.db")).unwrap();
    assert_eq!(
        reopened.idea_status(&id).unwrap()["idea_status"]["archived"],
        true
    );
}

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
    let source_revision = svc.idea_status("sg-saved").unwrap()["idea_status"]["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    svc.remove_project("demo").unwrap();
    let tasks = svc.tasks(None).unwrap();
    assert_eq!(
        svc.idea_status("sg-saved").unwrap_err().code,
        "unknown_idea"
    );
    assert_eq!(
        svc.set_idea_status("sg-saved", "in_progress", &source_revision)
            .unwrap_err()
            .code,
        "unknown_idea"
    );
    assert_eq!(svc.tasks(None).unwrap(), tasks);
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
