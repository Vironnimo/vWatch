mod common;

use chrono::Duration;
use common::{card, card_with, demo_repo, make_repo, service};
use serde_json::{json, Value};

fn project<'a>(status: &'a Value, id: &str) -> &'a Value {
    status["latest"]["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap()
}

fn run_id(status: &Value) -> i64 {
    status["run_id"]
        .as_i64()
        .or_else(|| status["latest"]["id"].as_i64())
        .unwrap()
}

fn brief() -> Value {
    json!({"greeting": "Guten Morgen"})
}

#[test]
fn an_untracked_run_is_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());

    let status = svc.run_status().unwrap();
    assert_eq!(status["state"], "unknown");
    assert!(status["latest"].is_null());
    assert!(status["last_successful"].is_null());
    assert_eq!(status["pending_owner_replies"], 0);
    assert_eq!(status["history"], json!([]));
}

#[test]
fn partial_runs_can_be_finished_after_repair_and_success_is_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    for repo in [
        demo_repo(tmp.path()),
        make_repo(tmp.path(), "second", &["2026-09-29T10:00:00"]),
    ] {
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    }

    let started = svc.run_start(false).unwrap();
    let id = run_id(&started);
    assert_eq!(started["state"], "running");
    assert_eq!(
        started["latest"]["counts"],
        json!({"total": 2, "current": 0, "pending": 2, "failed": 0})
    );
    assert_eq!(project(&started, "demo")["reason"], "no_card");

    let first = svc.put_card("demo", card(), false, false).unwrap();
    let partial = svc.run_status().unwrap();
    assert_eq!(project(&partial, "demo")["status"], "current");
    assert_eq!(project(&partial, "demo")["card_version"], first["version"]);
    assert_eq!(project(&partial, "second")["status"], "pending");
    assert_eq!(partial["latest"]["portfolio"]["status"], "pending");

    let unfinished = svc.run_finish(id).unwrap();
    assert_eq!(unfinished["state"], "incomplete");
    assert!(unfinished["latest"]["finished_at"].is_string());
    assert!(unfinished["last_successful"].is_null());
    assert!(unfinished["repair_task"]
        .as_str()
        .unwrap()
        .contains("second"));

    svc.put_card("second", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    let completed = svc.run_finish(id).unwrap();
    assert_eq!(completed["state"], "complete");
    assert_eq!(
        completed["latest"]["counts"],
        json!({"total": 2, "current": 2, "pending": 0, "failed": 0})
    );
    assert_eq!(completed["last_successful"]["id"], id);
    let snapshot = completed["latest"].clone();

    // Later owner changes must not rewrite the outcome of a completed run.
    svc.set_goal("demo", "Ein neues Ziel", None).unwrap();
    assert_eq!(svc.run_status().unwrap()["latest"], snapshot);
    drop(svc);

    let (svc, _) = service(tmp.path());
    let reopened = svc.run_status().unwrap();
    assert_eq!(reopened["latest"], snapshot);
    assert_eq!(reopened["last_successful"]["id"], id);
    assert!(reopened["history"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"] == id && entry["state"] == "complete"));

    let next = svc.run_start(false).unwrap();
    assert_ne!(run_id(&next), id);
    assert_eq!(next["last_successful"], reopened["last_successful"]);
    assert_eq!(project(&next, "demo")["status"], "pending");
}

#[test]
fn current_cards_need_no_rewrite_and_status_does_not_refresh_git() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    let archived = make_repo(tmp.path(), "archived", &["2026-09-29T10:00:00"]);
    svc.add_project(archived.to_str().unwrap(), None, None)
        .unwrap();
    svc.set_status("archived", "archived", None).unwrap();
    let saved = svc.put_card("demo", card(), false, false).unwrap();
    let saved_brief = svc.put_portfolio(brief(), false, false).unwrap();

    let started = svc.run_start(false).unwrap();
    assert_eq!(started["state"], "running");
    assert_eq!(started["latest"]["counts"]["total"], 1);
    assert_eq!(
        project(&started, "demo")["initial_version"],
        saved["version"]
    );
    assert_eq!(project(&started, "demo")["card_version"], saved["version"]);
    assert_eq!(started["latest"]["portfolio"]["status"], "current");
    assert_eq!(
        started["latest"]["portfolio"]["initial_version"],
        saved_brief["version"]
    );

    let measured = svc.store().facts("demo").unwrap().unwrap();
    std::fs::write(repo.join("f0.txt"), "changed after the last measurement\n").unwrap();
    let status = svc.run_status().unwrap();
    assert_eq!(project(&status, "demo")["status"], "current");
    let after_status = svc.store().facts("demo").unwrap().unwrap();
    assert_eq!(after_status.fingerprint, measured.fingerprint);
    assert_eq!(after_status.collected_at, measured.collected_at);

    // An explicit refresh exposes the edit, even though status itself is read-only.
    svc.refresh_facts(None, false).unwrap();
    let changed = svc.run_status().unwrap();
    assert_eq!(project(&changed, "demo")["status"], "pending");
    assert_eq!(project(&changed, "demo")["reason"], "changed");
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    assert_eq!(
        svc.run_finish(run_id(&started)).unwrap()["state"],
        "complete"
    );
}

#[test]
fn failures_stay_visible_until_new_current_versions_repair_them() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let started = svc.run_start(false).unwrap();
    let id = run_id(&started);

    // Failures are measured against the version at failure time, which may have
    // advanced since this run began. All writes happen at the same fixed time.
    let written_card = svc.put_card("demo", card(), false, false).unwrap();
    let written_brief = svc.put_portfolio(brief(), false, false).unwrap();
    svc.run_fail(id, Some("demo"), "Die Untersuchung wurde abgebrochen")
        .unwrap();
    svc.run_fail(id, None, "Die Zusammenfassung konnte nicht fertig werden")
        .unwrap();
    let failed = svc.run_finish(id).unwrap();
    assert_eq!(failed["state"], "failed");
    assert_eq!(project(&failed, "demo")["status"], "failed");
    assert_eq!(
        project(&failed, "demo")["error"],
        "Die Untersuchung wurde abgebrochen"
    );
    assert_eq!(
        project(&failed, "demo")["card_version"],
        written_card["version"]
    );
    assert_eq!(failed["latest"]["portfolio"]["status"], "failed");
    assert_eq!(
        failed["latest"]["portfolio"]["version"],
        written_brief["version"]
    );
    assert!(failed["last_successful"].is_null());

    svc.put_card("demo", card(), false, false).unwrap();
    let repaired_card = svc.run_status().unwrap();
    assert_eq!(project(&repaired_card, "demo")["status"], "current");
    assert_eq!(repaired_card["latest"]["portfolio"]["status"], "failed");
    svc.put_portfolio(brief(), false, false).unwrap();
    let repaired = svc.run_finish(id).unwrap();
    assert_eq!(repaired["state"], "complete");
    assert_eq!(repaired["latest"]["portfolio"]["status"], "current");
}

#[test]
fn unreadable_facts_cannot_be_covered_up_by_a_new_card() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let readable = svc.store().facts("demo").unwrap().unwrap();
    let missing = json!({"ok": false, "is_git": false, "error": "path_missing"});
    svc.store()
        .save_facts(
            "demo",
            &svc.now(),
            &watchtower::facts::fingerprint(&missing),
            &missing,
        )
        .unwrap();
    // A saved card matching the failure fingerprint still cannot establish
    // that the repository was successfully examined.
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();

    let started = svc.run_start(false).unwrap();
    let id = run_id(&started);
    let blocked = svc.run_finish(id).unwrap();
    assert_ne!(blocked["state"], "complete");
    assert_ne!(project(&blocked, "demo")["status"], "current");
    assert_eq!(project(&blocked, "demo")["reason"], "path_missing");
    assert!(blocked["last_successful"].is_null());
    assert!(!blocked["repair_task"].as_str().unwrap().is_empty());

    svc.store()
        .save_facts("demo", &svc.now(), &readable.fingerprint, &readable.data)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    assert_eq!(svc.run_finish(id).unwrap()["state"], "complete");
}

#[test]
fn owner_replies_remain_pending_when_another_reason_makes_the_card_due() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    svc.put_card(
        "demo",
        card_with(json!({"waiting_on_you": [{
            "id": "question-offline", "title": "Soll die App offline laufen?"
        }]})),
        false,
        false,
    )
    .unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    svc.set_feedback("question-offline", Some("answered"), Some("Ja, bitte"))
        .unwrap();
    std::fs::write(repo.join("f0.txt"), "new work\n").unwrap();
    svc.refresh_facts(None, false).unwrap();

    let started = svc.run_start(false).unwrap();
    assert_eq!(project(&started, "demo")["reason"], "changed");
    assert_eq!(started["pending_owner_replies"], 1);
    assert_eq!(project(&started, "demo")["pending_owner_replies"], 1);
    let unfinished = svc.run_finish(run_id(&started)).unwrap();
    assert_eq!(unfinished["pending_owner_replies"], 1);

    svc.put_card("demo", card(), false, false).unwrap();
    let acknowledged = svc.run_status().unwrap();
    assert_eq!(acknowledged["pending_owner_replies"], 0);
    assert_eq!(project(&acknowledged, "demo")["pending_owner_replies"], 0);
    svc.put_portfolio(brief(), false, false).unwrap();
    assert_eq!(
        svc.run_finish(run_id(&started)).unwrap()["state"],
        "complete"
    );
}

#[test]
fn abandoned_runs_stall_and_never_succeed_without_an_explicit_finish() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();

    let started = svc.run_start(false).unwrap();
    let id = run_id(&started);
    assert_eq!(started["state"], "running");
    clock.advance(Duration::minutes(119));
    assert_eq!(svc.run_status().unwrap()["state"], "running");
    clock.advance(Duration::minutes(2));
    let stalled = svc.run_status().unwrap();
    assert_eq!(stalled["state"], "stalled");
    assert!(stalled["latest"]["finished_at"].is_null());
    assert!(stalled["last_successful"].is_null());
    assert_eq!(project(&stalled, "demo")["status"], "current");
    assert_eq!(stalled["latest"]["portfolio"]["status"], "current");
    assert_eq!(svc.run_finish(id).unwrap()["state"], "complete");
}

#[test]
fn repairing_an_older_run_reports_its_own_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let older = run_id(&svc.run_start(false).unwrap());
    let newer = run_id(&svc.run_start(false).unwrap());
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    assert_eq!(svc.run_finish(newer).unwrap()["state"], "complete");

    svc.run_fail(older, Some("demo"), "Noch nicht fertig")
        .unwrap();
    let failed = svc.run_finish(older).unwrap();
    assert_eq!(failed["state"], "failed");
    assert_eq!(failed["run_id"], older);
    assert_eq!(failed["run"]["state"], "failed");
    assert_eq!(failed["latest"]["id"], newer);
    assert_eq!(failed["latest"]["state"], "complete");
    assert!(failed["repair_task"]
        .as_str()
        .unwrap()
        .contains(&format!("runs finish {older}")));

    clock.advance(Duration::minutes(1));
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    let repaired = svc.run_finish(older).unwrap();
    assert_eq!(repaired["run"]["state"], "complete");
    assert_eq!(repaired["last_successful"]["id"], older);
}

fn service_with_completion_race(
    dir: &std::path::Path,
    trigger_call: usize,
    change: impl Fn(&watchtower::store::Store) + Send + Sync + 'static,
) -> (
    watchtower::Watchtower,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    let db = dir.join("watchtower.db");
    let store = watchtower::store::Store::open(&db).unwrap();
    let concurrent = watchtower::store::Store::open(&db).unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let clock_armed = armed.clone();
    let clock_calls = calls.clone();
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let svc = watchtower::Watchtower::with_parts(
        store,
        Box::new(move || {
            if clock_armed.load(Ordering::SeqCst)
                && clock_calls.fetch_add(1, Ordering::SeqCst) + 1 == trigger_call
            {
                change(&concurrent);
            }
            now
        }),
        Box::new(|path, today, _ci| watchtower::facts::collect(path, today, false)),
    );
    (svc, armed, calls)
}

#[test]
fn run_completion_rechecks_a_project_added_after_the_due_snapshot() {
    use std::sync::atomic::Ordering;

    let tmp = tempfile::tempdir().unwrap();
    let second = make_repo(tmp.path(), "second", &["2026-09-29T10:00:00"]);
    let path = second.to_str().unwrap().to_owned();
    let facts = watchtower::facts::collect(
        &path,
        chrono::NaiveDate::from_ymd_opt(2026, 9, 30).unwrap(),
        false,
    );
    assert_eq!(facts["ok"], true);
    let fingerprint = watchtower::facts::fingerprint(&facts);
    // The third clock read returns the due response's date, after it has
    // examined the old project list but before the run reads that list again.
    let (svc, armed, calls) = service_with_completion_race(tmp.path(), 3, move |store| {
        store
            .add_project("second", "second", &path, "2026-09-30T08:00:00Z")
            .unwrap();
        store
            .save_facts("second", "2026-09-30T08:00:00Z", &fingerprint, &facts)
            .unwrap();
    });
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    let id = run_id(&svc.run_start(false).unwrap());

    armed.store(true, Ordering::SeqCst);
    let finished = svc.run_finish(id).unwrap();
    assert!(calls.load(Ordering::SeqCst) >= 3);
    assert!(svc.store().project("second").unwrap().is_some());
    assert_ne!(finished["state"], "complete", "{finished}");
    assert_eq!(project(&finished, "second")["status"], "pending");
    assert_eq!(project(&finished, "second")["reason"], "no_card");
    assert!(finished["last_successful"].is_null());
}

#[test]
fn run_completion_rechecks_an_owner_change_before_saving_success() {
    use std::sync::atomic::Ordering;

    let tmp = tempfile::tempdir().unwrap();
    // The fourth clock read stamps the computed result: its current statuses
    // have already been evaluated, but success has not reached the database.
    let (svc, armed, calls) = service_with_completion_race(tmp.path(), 4, |store| {
        store
            .set_goal(
                "demo",
                "Vor dem Abschluss neu prüfen",
                None,
                "2026-09-30T08:00:00Z",
            )
            .unwrap();
    });
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    let id = run_id(&svc.run_start(false).unwrap());

    armed.store(true, Ordering::SeqCst);
    let finished = svc.run_finish(id).unwrap();
    assert!(calls.load(Ordering::SeqCst) >= 4);
    assert_eq!(
        svc.store().project("demo").unwrap().unwrap().goal,
        "Vor dem Abschluss neu prüfen"
    );
    assert_ne!(finished["state"], "complete", "{finished}");
    assert_eq!(project(&finished, "demo")["status"], "pending");
    assert_eq!(project(&finished, "demo")["reason"], "status_changed");
    assert!(finished["last_successful"].is_null());
}

#[test]
fn run_completion_rechecks_a_failure_reported_before_saving_success() {
    use std::sync::{
        atomic::{AtomicI64, Ordering},
        Arc,
    };
    let tmp = tempfile::tempdir().unwrap();
    let run = Arc::new(AtomicI64::new(0));
    let reported_run = run.clone();
    let (svc, armed, _) = service_with_completion_race(tmp.path(), 4, move |store| {
        store.fail_agent_run(reported_run.load(Ordering::SeqCst), "demo", &json!({
            "version": 1, "error": "Parallel gemeldeter Analysefehler", "at": "2026-09-30T08:00:00Z"
        })).unwrap();
    });
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(brief(), false, false).unwrap();
    let id = run_id(&svc.run_start(false).unwrap());
    run.store(id, Ordering::SeqCst);
    armed.store(true, Ordering::SeqCst);
    let finished = svc.run_finish(id).unwrap();
    assert_eq!(finished["state"], "failed");
    assert_eq!(
        finished["run"]["projects"][0]["error"],
        "Parallel gemeldeter Analysefehler"
    );
    assert!(finished["last_successful"].is_null());
}

#[test]
fn simultaneous_starts_return_their_own_run_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let (first, _) = service(tmp.path());
    let (second, _) = service(tmp.path());
    let barrier = std::sync::Barrier::new(2);
    let starts = std::thread::scope(|scope| {
        let handles: Vec<_> = [first, second]
            .into_iter()
            .map(|svc| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    svc.run_start(false).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_ne!(starts[0]["run_id"], starts[1]["run_id"]);
    for start in starts {
        let id = start["run_id"].as_i64().unwrap();
        assert_eq!(start["run"]["id"], id);
        assert!(start["next"]
            .as_str()
            .unwrap()
            .contains(&format!("runs finish {id}")));
    }
}
