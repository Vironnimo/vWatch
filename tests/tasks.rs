mod common;

use chrono::Duration;
use common::{card, demo_repo, service};
use serde_json::json;
use watchtower::store::Store;

#[test]
fn handoff_keeps_its_snapshot_until_the_owner_confirms_the_result() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    assert_eq!(svc.state(false).unwrap()["tasks"], json!([]));
    let payload = json!({
        "projects": ["demo"], "source_id": "sg-handoff", "title": "Export reparieren",
        "why": "Einträge fehlen.", "evidence": "src/export.rs",
        "done_when": "Alle Einträge stehen in der exportierten Datei.",
    });
    let created = svc.create_task(payload.clone()).unwrap();
    let id = created["task"]["id"].as_i64().unwrap();
    assert_eq!(created["created"], true);
    assert_eq!(created["task"]["status"], "assigned");
    assert_eq!(
        created["task"]["project_snapshots"][0]["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/"),
        repo.to_string_lossy().replace('\\', "/")
    );
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    let mut rewritten = payload;
    rewritten["title"] = json!("Umformulierter Vorschlag");
    let repeated = svc.create_task(rewritten).unwrap();
    assert_eq!(repeated["created"], false);
    assert_eq!(repeated["task"], created["task"]);
    svc.put_card("demo", json!({"suggestions": []}), true, false)
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    assert_eq!(svc.context("demo").unwrap()["tasks"][0]["id"], id);
    assert_eq!(svc.portfolio_context(&[]).unwrap()["tasks"][0]["id"], id);
    assert_eq!(
        svc.tasks(Some("demo")).unwrap()["tasks"][0]["title"],
        "Export reparieren"
    );
    assert_eq!(
        svc.set_task_status(id, "done").unwrap_err().code,
        "invalid_task_transition"
    );
    assert_eq!(
        svc.report_task(id, "done", "Repariert.", "")
            .unwrap_err()
            .code,
        "invalid_task"
    );
    clock.advance(Duration::minutes(5));
    let blocked = svc
        .report_task(id, "blocked", "Testdaten fehlen.", "")
        .unwrap();
    assert_eq!(blocked["task"]["status"], "blocked");
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    svc.set_task_status(id, "assigned").unwrap();
    clock.advance(Duration::minutes(10));
    let report = svc
        .report_task(
            id,
            "done",
            "Alle Einträge werden exportiert.",
            "cargo test export: 4 Tests grün; Commit abc123",
        )
        .unwrap();
    assert_eq!(report["task"]["status"], "review");
    assert!(report["task"]["completed_at"].is_null());
    assert_eq!(report["task"]["done_when"], created["task"]["done_when"]);
    let repeated = svc
        .report_task(
            id,
            "done",
            "Alle Einträge werden exportiert.",
            "cargo test export: 4 Tests grün; Commit abc123",
        )
        .unwrap();
    assert_eq!(repeated["task"], report["task"]);
    assert_eq!(repeated["already_reported"], true);
    clock.advance(Duration::minutes(2));
    let confirmed = svc.set_task_status(id, "done").unwrap();
    assert_eq!(confirmed["task"]["status"], "done");
    assert_eq!(confirmed["task"]["completed_at"], "2026-09-30T08:17:00Z");
    assert_eq!(confirmed["task"]["history"].as_array().unwrap().len(), 5);
    assert_eq!(
        svc.report_task(id, "done", "Anderer Bericht", "Beleg")
            .unwrap_err()
            .code,
        "invalid_task_transition"
    );
    assert_eq!(
        svc.set_task_status(id, "done").unwrap()["task"],
        confirmed["task"]
    );
    let (reopened_service, _) = service(tmp.path());
    assert_eq!(
        reopened_service.tasks(None).unwrap()["tasks"][0],
        confirmed["task"]
    );
    svc.remove_project("demo").unwrap();
    assert_eq!(
        svc.tasks(None).unwrap()["tasks"][0]["result_evidence"],
        report["task"]["result_evidence"]
    );
    assert_eq!(
        svc.tasks(None).unwrap()["tasks"][0]["project_snapshots"],
        created["task"]["project_snapshots"]
    );
}

#[test]
fn task_assignment_is_scoped_atomic_and_does_not_duplicate_active_work() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let (other, _) = service(tmp.path());
    let payload = json!({"projects": [], "source_id": "new-app", "title": "Neue App bauen"});
    let (first, second) = std::thread::scope(|threads| {
        let a = threads.spawn(|| svc.create_task(payload.clone()).unwrap());
        let b = threads.spawn(|| other.create_task(payload.clone()).unwrap());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_ne!(first["created"], second["created"]);
    assert_eq!(first["task"]["id"], second["task"]["id"]);
    let id = first["task"]["id"].as_i64().unwrap();
    svc.set_task_status(id, "cancelled").unwrap();
    let replacement = svc.create_task(payload).unwrap();
    assert_eq!(replacement["created"], true);
    assert_ne!(replacement["task"]["id"], id);
    assert_eq!(
        svc.set_task_status(id, "assigned").unwrap_err().code,
        "invalid_task_transition"
    );
    svc.set_task_status(replacement["task"]["id"].as_i64().unwrap(), "cancelled")
        .unwrap();
    let reopened = svc.set_task_status(id, "assigned").unwrap();
    assert_eq!(reopened["task"]["history"].as_array().unwrap().len(), 3);
    assert_eq!(
        svc.tasks(None).unwrap()["tasks"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn task_validation_and_stale_updates_preserve_owner_decisions() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    assert_eq!(
        svc.create_task(json!({"projects": [], "title": " "}))
            .unwrap_err()
            .code,
        "invalid_task"
    );
    assert_eq!(
        svc.create_task(json!({"projects": [], "title": "Auftrag", "done_when": 1}))
            .unwrap_err()
            .code,
        "invalid_task"
    );
    assert_eq!(
        svc.create_task(json!({"projects": ["missing"], "title": "Auftrag"}))
            .unwrap_err()
            .code,
        "project_not_found"
    );
    let task = svc
        .create_task(json!({"projects": [], "title": "Auftrag"}))
        .unwrap()["task"]
        .clone();
    let id = task["id"].as_i64().unwrap();
    assert_eq!(
        svc.report_task(id, "review", "Resultat", "Beleg")
            .unwrap_err()
            .code,
        "invalid_task_status"
    );
    assert_eq!(
        svc.report_task(id, "blocked", "", "").unwrap_err().code,
        "invalid_task"
    );
    assert_eq!(
        svc.set_task_status(id + 100, "assigned").unwrap_err().code,
        "unknown_task"
    );
    for invalid in [
        json!([]),
        json!({"status": "blocked", "result": "Fehlt", "evdience": "Datei"}),
        json!({"status": "blocked", "result": "Fehlt", "evidence": null}),
        json!({"status": null, "result": "Fehlt"}),
        json!({"status": "blocked", "result": 2}),
        json!({"status": "done", "result": "Fertig"}),
        json!({"status": "blocked", "result": " "}),
    ] {
        assert_eq!(
            svc.report_task_document(id, invalid).unwrap_err().code,
            "invalid_task"
        );
        assert_eq!(svc.tasks(None).unwrap()["tasks"][0], task);
    }
    let missing = svc.report_task_document(id + 100, json!({})).unwrap_err();
    assert_eq!(missing.code, "unknown_task");
    assert!(missing.hint.as_deref().unwrap().contains(&format!("{id}:")));
    assert!(missing.hint.as_deref().unwrap().contains("Auftrag"));
    assert!(missing.hint.as_deref().unwrap().contains("assigned"));
    assert_eq!(svc.task(id).unwrap()["task"], task);
    let blocked = svc
        .report_task_document(
            id,
            json!({"status": "BLOCKED", "result": "Zürich fehlt.\n東京 fehlt ebenfalls."}),
        )
        .unwrap();
    assert_eq!(blocked["task"]["status"], "blocked");
    assert_eq!(blocked["task"]["result_evidence"], "");
    let report = json!({"status": "DONE", "result": "Zürich und 東京 ergänzt.", "evidence": "Tests grün.\nExport geprüft."});
    let reported = svc.report_task_document(id, report.clone()).unwrap();
    assert_eq!(reported["task"]["status"], "review");
    assert_eq!(reported["task"]["result_evidence"], report["evidence"]);
    assert_eq!(
        svc.report_task_document(id, report).unwrap()["already_reported"],
        true
    );
    svc.set_task_status(id, "cancelled").unwrap();
    let store = Store::open(&tmp.path().join("watchtower.db")).unwrap();
    assert!(!store
        .replace_task(id, task["revision"].as_i64().unwrap(), "review", &task)
        .unwrap());
    assert_eq!(svc.tasks(None).unwrap()["tasks"][0]["status"], "cancelled");
}

#[test]
fn standalone_task_changes_make_the_portfolio_due() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.put_portfolio(json!({"greeting": "Alles ruhig"}), false, false)
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], false);
    let created = svc
        .create_task(json!({"projects": [], "source_id": "idea:1", "title": "Neue App bauen"}))
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], true);
    svc.put_portfolio(json!({"greeting": "Eine App ist beauftragt"}), false, false)
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], false);
    svc.report_task(
        created["task"]["id"].as_i64().unwrap(),
        "blocked",
        "Richtung fehlt",
        "",
    )
    .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], true);
}

#[test]
fn owner_confirmation_must_match_the_review_the_owner_saw() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let created = svc
        .create_task(json!({"projects": [], "title": "Export prüfen"}))
        .unwrap();
    let id = created["task"]["id"].as_i64().unwrap();
    let displayed = svc
        .report_task(id, "done", "Export geprüft", "Vier Tests grün")
        .unwrap();
    let latest = svc
        .report_task(id, "done", "Nur drei Fälle geprüft", "Drei Tests grün")
        .unwrap();
    assert_ne!(displayed["task"]["revision"], latest["task"]["revision"]);
    for status in ["done", "assigned", "cancelled"] {
        let error = svc
            .set_task_status_guarded(id, status, displayed["task"]["revision"].as_i64().unwrap())
            .unwrap_err();
        assert_eq!(error.code, "task_changed");
    }
    assert_eq!(svc.tasks(None).unwrap()["tasks"][0], latest["task"]);
    assert_eq!(
        svc.set_task_status_guarded(id, "done", latest["task"]["revision"].as_i64().unwrap())
            .unwrap()["task"]["status"],
        "done"
    );
}
