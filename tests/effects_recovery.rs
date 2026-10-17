mod common;

use common::{card, card_with, demo_repo, service};
use serde_json::json;

#[test]
fn project_trash_preserves_identity_cards_feedback_tasks_and_owner_instructions() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    svc.set_goal("demo", "Offline bleiben", Some("2026-12-01"))
        .unwrap();
    svc.set_status("demo", "parked", Some("Nächsten Monat"))
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_card("demo", json!({"headline":"Zweite Fassung"}), true, false)
        .unwrap();
    let suggestion = svc.card("demo").unwrap()["card"]["suggestions"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    svc.set_feedback(&suggestion, Some("down"), Some("Nicht erneut vorschlagen"))
        .unwrap();
    svc.create_task(
        json!({"projects":["demo"],"title":"Offline prüfen","done_when":"Kein Netzwerkverkehr"}),
    )
    .unwrap();
    let history = svc.store().card_history("demo").unwrap();
    let tasks = svc.tasks(None).unwrap();
    let feedback = svc.feedback(None).unwrap();
    let removed = svc.remove_project("demo").unwrap();
    assert_eq!(removed["changed"], true);
    assert_eq!(removed["project"]["removed_at"], svc.now());
    assert_eq!(removed["undo"], "watchtower projects restore demo");
    clock.advance(chrono::Duration::minutes(1));
    let removed_again = svc.remove_project("demo").unwrap();
    assert_eq!(removed_again["already_removed"], true);
    assert_eq!(removed_again["project"], removed["project"]);
    assert!(svc.list_projects().unwrap().as_array().unwrap().is_empty());
    assert_eq!(svc.trash_projects().unwrap()["projects"][0]["id"], "demo");
    assert_eq!(svc.card("demo").unwrap_err().code, "project_removed");
    assert_eq!(
        svc.add_project(repo.to_str().unwrap(), None, None)
            .unwrap_err()
            .code,
        "project_removed"
    );
    let restored = svc.restore_project("demo").unwrap();
    assert_eq!(restored["restored"], true);
    assert!(restored["project"]["removed_at"].is_null());
    assert_eq!(restored["project"]["goal"], "Offline bleiben");
    assert_eq!(restored["project"]["status_note"], "Nächsten Monat");
    let restored_again = svc.restore_project("demo").unwrap();
    assert_eq!(restored_again["already_active"], true);
    assert_eq!(restored_again["project"], restored["project"]);
    assert_eq!(svc.store().card_history("demo").unwrap(), history);
    assert_eq!(svc.feedback(None).unwrap(), feedback);
    assert_eq!(svc.tasks(None).unwrap(), tasks);
    assert_eq!(svc.card("demo").unwrap()["version"], 2);
    assert_eq!(
        svc.remove_project("unknown").unwrap_err().code,
        "project_not_found"
    );
}

#[test]
fn registration_retries_keep_the_existing_identity_and_reject_changed_intent() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    let first = svc
        .add_project(repo.to_str().unwrap(), Some("stable"), Some("Friendly"))
        .unwrap();
    let second = svc
        .add_project(repo.to_str().unwrap(), Some("stable"), Some("Friendly"))
        .unwrap();
    assert_eq!(first["id"], second["id"]);
    assert_eq!(second["already_exists"], true);
    assert_eq!(
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap()["name"],
        "Friendly"
    );
    for (id, name) in [(Some("different"), None), (None, Some("Different"))] {
        assert_eq!(
            svc.add_project(repo.to_str().unwrap(), id, name)
                .unwrap_err()
                .code,
            "already_registered"
        );
    }
    assert_eq!(svc.list_projects().unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn a_lost_card_response_can_be_replayed_only_with_its_unchanged_full_context() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let token = svc.context("demo").unwrap()["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = svc
        .put_card_guarded("demo", card(), false, false, Some(&token))
        .unwrap();
    // A fresh service/process uses the durable receipt, not an in-memory cache.
    let (retry, _) = service(tmp.path());
    let repeated = retry
        .put_card_guarded("demo", card(), false, false, Some(&token))
        .unwrap();
    assert_eq!(repeated["already_saved"], true);
    assert_eq!(repeated["version"], first["version"]);
    assert_eq!(repeated["card"], first["card"]);
    assert_eq!(retry.card("demo").unwrap()["version"], 1);
    assert_eq!(
        retry
            .put_card_guarded(
                "demo",
                card_with(json!({"headline":"Andere Absicht"})),
                false,
                false,
                Some(&token)
            )
            .unwrap_err()
            .code,
        "version_conflict"
    );
    // The same document cannot acknowledge owner input that this request never read.
    svc.set_goal("demo", "Neue Vorgabe", None).unwrap();
    assert_eq!(
        retry
            .put_card_guarded("demo", card(), false, false, Some(&token))
            .unwrap_err()
            .code,
        "version_conflict"
    );
    assert_eq!(retry.card("demo").unwrap()["version"], 1);
}

#[test]
fn identical_concurrent_card_requests_write_one_version_and_both_succeed() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let token = svc.context("demo").unwrap()["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let (first, _) = service(tmp.path());
    let (second, _) = service(tmp.path());
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles = [first, second]
            .into_iter()
            .map(|svc| {
                let token = &token;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    svc.put_card_guarded("demo", card(), false, false, Some(token))
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|r| r["version"] == 1));
    assert_eq!(
        results
            .iter()
            .filter(|r| r["already_saved"] == true)
            .count(),
        1
    );
    assert_eq!(svc.store().card_history("demo").unwrap().len(), 1);
}

#[test]
fn portfolio_replays_require_the_same_request_and_owner_context() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let token = svc.portfolio_context().unwrap()["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let doc = json!({"greeting":"Guten Morgen"});
    let first = svc
        .put_portfolio_guarded(doc.clone(), false, false, Some(&token))
        .unwrap();
    let retry = svc
        .put_portfolio_guarded(doc.clone(), false, false, Some(&token))
        .unwrap();
    assert_eq!(retry["version"], first["version"]);
    assert_eq!(retry["already_saved"], true);
    svc.add_idea("Neue Idee des Besitzers").unwrap();
    assert_eq!(
        svc.put_portfolio_guarded(doc, false, false, Some(&token))
            .unwrap_err()
            .code,
        "version_conflict"
    );
    assert_eq!(svc.portfolio().unwrap()["version"], 1);
}

#[test]
fn history_restore_appends_old_content_and_keeps_its_stale_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.set_goal("demo", "Neue Anforderung", None).unwrap();
    svc.put_card("demo", json!({"headline":"Neue Fassung"}), true, false)
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    let history = svc.card_history("demo").unwrap();
    assert_eq!(history["versions"][0]["version"], 1);
    assert_eq!(history["versions"][1]["version"], 2);
    assert_eq!(
        svc.card_version("demo", 1).unwrap()["card"]["headline"],
        "Alles ruhig"
    );
    let token = history["context_token"].as_str().unwrap();
    let restored = svc.restore_card("demo", 1, Some(token)).unwrap();
    assert_eq!(restored["version"], 3);
    assert_eq!(restored["restored_from"], 1);
    assert!(restored["undo"]
        .as_str()
        .unwrap()
        .contains("restore demo --version 2 --context-token"));
    assert_eq!(svc.card("demo").unwrap()["card"]["headline"], "Alles ruhig");
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    assert_eq!(
        svc.restore_card("demo", 1, Some(token)).unwrap()["version"],
        3
    );
    assert_eq!(
        svc.card_history("demo").unwrap()["versions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    svc.set_status("demo", "parked", None).unwrap();
    assert_eq!(
        svc.restore_card("demo", 1, Some(token)).unwrap_err().code,
        "version_conflict"
    );

    svc.put_portfolio(json!({"greeting":"Alter Brief"}), false, false)
        .unwrap();
    svc.put_portfolio(json!({"greeting":"Neuer Brief"}), false, false)
        .unwrap();
    let history = svc.portfolio_history().unwrap();
    assert_eq!(history["current_version"], 2);
    let token = history["context_token"].as_str().unwrap();
    let restored = svc.restore_portfolio(1, Some(token)).unwrap();
    assert_eq!(restored["version"], 3);
    assert_eq!(
        svc.portfolio().unwrap()["portfolio"]["greeting"],
        "Alter Brief"
    );
    assert_eq!(
        svc.restore_portfolio(1, Some(token)).unwrap()["already_saved"],
        true
    );
    assert_eq!(
        svc.portfolio_version(999).unwrap_err().code,
        "version_not_found"
    );
}

#[test]
fn idea_add_remove_and_restore_are_safe_to_repeat() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let first = svc.add_idea("Eine Idee").unwrap();
    let id = first["id"].as_i64().unwrap();
    assert_eq!(svc.add_idea(" Eine Idee ").unwrap()["id"], id);
    assert_eq!(svc.ideas().unwrap().as_array().unwrap().len(), 1);
    svc.remove_idea(id).unwrap();
    assert_eq!(svc.remove_idea(id).unwrap()["already_removed"], true);
    assert!(svc.ideas().unwrap().as_array().unwrap().is_empty());
    assert_eq!(svc.ideas_trash().unwrap()[0]["text"], "Eine Idee");
    assert_eq!(
        svc.restore_idea(id).unwrap()["idea"]["removed_at"],
        json!(null)
    );
    assert_eq!(svc.restore_idea(id).unwrap()["already_active"], true);
    assert_eq!(svc.ideas().unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn report_retries_preserve_owner_confirmation_and_unknown_ids_are_named_first() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    assert_eq!(
        svc.report_task(42, "done", "", "").unwrap_err().code,
        "unknown_task"
    );
    let task = svc
        .create_task(json!({"projects":[],"title":"Prüfen","done_when":"Geprüft"}))
        .unwrap();
    let id = task["task"]["id"].as_i64().unwrap();
    let repeated = svc
        .create_task(json!({"projects":[],"title":"Prüfen","done_when":"Geprüft"}))
        .unwrap();
    assert_eq!(repeated["created"], false);
    assert_eq!(repeated["task"]["id"], id);
    svc.report_task(id, "done", "Fertig", "Prüfung bestanden")
        .unwrap();
    let accepted = svc.set_task_status(id, "done").unwrap();
    let replay = svc
        .report_task(id, "done", "Fertig", "Prüfung bestanden")
        .unwrap();
    assert_eq!(replay["already_reported"], true);
    assert_eq!(replay["task"], accepted["task"]);
    assert_eq!(
        svc.report_task(id, "blocked", "Andere Meldung", "")
            .unwrap_err()
            .code,
        "invalid_task_transition"
    );
}

#[test]
fn repeating_a_run_failure_does_not_make_a_newer_repaired_card_fail_again() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let run = svc.run_start(false).unwrap()["run_id"].as_i64().unwrap();
    svc.run_fail(run, Some("demo"), "Analyse unterbrochen")
        .unwrap();
    let original = svc.store().agent_run(run).unwrap().unwrap().failures;
    svc.put_card("demo", card(), false, false).unwrap();
    clock.advance(chrono::Duration::minutes(10));
    assert_eq!(
        svc.run_fail(run, Some("demo"), "Analyse unterbrochen")
            .unwrap()["already_recorded"],
        true
    );
    assert_eq!(
        svc.store().agent_run(run).unwrap().unwrap().failures,
        original
    );
}

#[test]
fn due_preserves_failed_measurements_and_never_calls_their_cards_current() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    drop(svc);
    let failed = watchtower::Watchtower::with_parts(
        watchtower::store::Store::open(&tmp.path().join("watchtower.db")).unwrap(),
        Box::new(|| {
            chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc)
        }),
        Box::new(|_, _, _, _| json!({"ok":false,"is_git":true,"error":"git_unavailable"})),
    );
    let due = failed.due(14, true).unwrap();
    assert_eq!(due["refreshed"][0]["ok"], false);
    assert_eq!(due["refreshed"][0]["error"], "git_unavailable");
    assert_eq!(due["problems"][0]["problem"], "git_unavailable");
    assert_eq!(due["projects"], json!([]));
    assert_eq!(due["up_to_date"], json!([]));
    // A card written against a failure fingerprint still cannot prove a healthy read.
    failed.put_card("demo", card(), false, false).unwrap();
    assert_eq!(
        failed.due(14, false).unwrap()["problems"][0]["problem"],
        "git_unavailable"
    );
    let started = failed.run_start(false).unwrap();
    assert_eq!(started["due"]["problems"][0]["problem"], "git_unavailable");
    assert_ne!(started["run"]["projects"][0]["status"], "current");
}
