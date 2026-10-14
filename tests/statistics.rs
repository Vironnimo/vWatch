mod common;
use chrono::Duration;
use common::*;
use serde_json::{json, Value};

fn count(db: &rusqlite::Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn observations_keep_every_timestamp_deduplicate_payloads_and_compare_known_measurements() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    std::fs::write(repo.join("app.py"), "# comment\nprint('one')\n\n").unwrap();
    std::fs::write(
        repo.join("README.md"),
        "# Docs\n```python\nprint('example')\n```\n",
    )
    .unwrap();
    std::fs::create_dir(repo.join("node_modules")).unwrap();
    std::fs::write(repo.join("node_modules/vendor.js"), "lots.of.code();\n").unwrap();
    std::fs::write(repo.join("package-lock.json"), "{}\n").unwrap();
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    let f = svc.facts("demo").unwrap();
    assert_eq!(f["code"]["source_code"], 1);
    assert_eq!(f["code"]["ok"], true);
    assert_eq!(f["code"]["excluded_files"], 2);
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    let payloads = count(&db, "fact_payloads");
    let saved = svc.store().facts("demo").unwrap().unwrap();
    svc.store()
        .save_facts("demo", &svc.now(), &saved.fingerprint, &saved.data)
        .unwrap();
    assert_eq!(count(&db, "fact_samples"), 2);
    assert_eq!(count(&db, "fact_payloads"), payloads);
    clock.advance(Duration::days(1));
    std::fs::write(
        repo.join("app.py"),
        "# comment\nprint('one')\nprint('two')\n\n",
    )
    .unwrap();
    svc.refresh_facts(None, false).unwrap();
    let stats = svc
        .statistics(Some("demo"), Some("2026-09-30"), Some("2026-10-01"))
        .unwrap();
    assert_eq!(stats["projects"][0]["samples"].as_array().unwrap().len(), 3);
    assert_eq!(stats["projects"][0]["change"]["code"], 1);
    assert_eq!(stats["summary"]["source_code"], 2);
    let observations = svc.store().fact_samples(Some("demo"), None, None).unwrap();
    assert_eq!(observations[0]["facts"], saved.data);
    assert!(db.execute("DELETE FROM fact_samples", []).is_err());
    svc.store()
        .save_facts(
            "demo",
            &svc.now(),
            "failed",
            &json!({"ok":false,"error":"path_missing"}),
        )
        .unwrap();
    let stats = svc
        .statistics(Some("demo"), Some("2026-09-30"), Some("2026-10-01"))
        .unwrap();
    assert!(stats["summary"]["source_code"].is_null());
    assert!(stats["projects"][0]["change"].is_null());
    assert_eq!(count(&db, "fact_samples"), 4);
    svc.remove_project("demo").unwrap();
    svc.restore_project("demo").unwrap();
    assert_eq!(count(&db, "fact_samples"), 4);
    std::fs::write(repo.join("oversized.py"), vec![b'x'; 4_000_001]).unwrap();
    svc.refresh_facts(None, false).unwrap();
    let incomplete = svc.facts("demo").unwrap();
    assert_eq!(incomplete["code"]["partial"], true);
    assert_eq!(incomplete["code"]["unread_files"], 1);
    assert!(svc.statistics(Some("demo"), None, None).unwrap()["summary"]["source_code"].is_null());
}

#[test]
fn archived_git_windows_still_cover_old_periods_and_do_not_turn_gaps_into_zeroes() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    clock.advance(Duration::days(800));
    svc.refresh_facts(None, false).unwrap();
    let old = svc
        .statistics(Some("demo"), Some("2026-09-01"), Some("2026-09-30"))
        .unwrap();
    assert_eq!(old["projects"][0]["activity"]["complete"], true);
    assert_eq!(old["projects"][0]["activity"]["totals"]["commits"], 1);
    let gap = svc
        .statistics(Some("demo"), Some("2026-09-01"), Some("2028-12-01"))
        .unwrap();
    assert_eq!(gap["projects"][0]["activity"]["complete"], false);
    assert!(gap["projects"][0]["activity"]["totals"].is_null());
    assert_eq!(
        gap["projects"][0]["activity"]["coverage"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn proposals_survive_report_omission_and_card_replays_do_not_duplicate_history() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let context = svc.context("demo").unwrap();
    let token = context["context_token"].as_str().unwrap();
    let saved = svc
        .put_card_guarded("demo", card(), false, false, Some(token))
        .unwrap();
    let proposal = saved["card"]["suggestions"][0].clone();
    let id = proposal["id"].as_str().unwrap();
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    let events = count(&db, "content_history");
    svc.put_card_guarded("demo", card(), false, false, Some(token))
        .unwrap();
    assert_eq!(count(&db, "content_history"), events);
    clock.advance(Duration::days(1));
    svc.put_card(
        "demo",
        json!({"headline":"Weiter","suggestions":[]}),
        true,
        false,
    )
    .unwrap();
    let proposals = svc.proposals(Some("demo")).unwrap();
    assert_eq!(proposals["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(proposals["proposals"][0]["in_report"], false);
    assert_eq!(
        proposals["proposals"][0]["first_seen"],
        "2026-09-30T08:00:00Z"
    );
    svc.set_feedback(id, Some("up"), None).unwrap();
    assert_eq!(
        svc.feedback(None).unwrap()[0]["item"]["title"],
        proposal["title"]
    );
    assert_eq!(
        svc.context("demo").unwrap()["retained_proposals"][0]["id"],
        id
    );
    let compare = svc.compare_documents(Some("demo"), 1, 2).unwrap();
    assert!(compare["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["field"] == "suggestions"));
    svc.put_card("demo", json!({"suggestions":[proposal]}), true, false)
        .unwrap();
    assert_eq!(
        svc.proposals(Some("demo")).unwrap()["proposals"][0]["first_seen"],
        "2026-09-30T08:00:00Z"
    );
    assert!(
        svc.history(Some("demo"), Some("proposal"), Some(id), None)
            .unwrap()["events"]
            .as_array()
            .unwrap()
            .len()
            >= 3
    );
}

#[test]
fn owner_edits_undo_and_removal_are_journaled_and_newer_idea_edits_are_guarded() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let id = svc.add_idea("Erste Idee").unwrap()["id"].as_i64().unwrap();
    svc.put_portfolio(
        json!({"greeting":"Hallo","idea_checks":[{"idea":id,"verdict":"build","why":"Nützlich"}]}),
        false,
        false,
    )
    .unwrap();
    let edit = svc.update_idea(id, "Andere Idee", 1).unwrap();
    assert_eq!(edit["idea"]["revision"], 2);
    assert!(svc.update_idea(id, "Falscher alter Stand", 1).is_err());
    assert_eq!(
        svc.portfolio_context().unwrap()["owner_ideas"][0]["checked"],
        false
    );
    let token = edit["undo"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .last()
        .unwrap();
    svc.undo(token).unwrap();
    assert_eq!(svc.ideas().unwrap()[0]["text"], "Erste Idee");
    assert_eq!(svc.ideas().unwrap()[0]["revision"], 3);
    svc.remove_idea(id).unwrap();
    svc.restore_idea(id).unwrap();
    let journal = svc
        .history(None, Some("idea"), Some(&id.to_string()), None)
        .unwrap();
    assert_eq!(journal["events"].as_array().unwrap().len(), 5);
    let event = svc
        .history_event(journal["events"][2]["id"].as_i64().unwrap())
        .unwrap();
    assert_eq!(event["event"]["before"]["text"], "Andere Idee");
    assert_eq!(event["event"]["after"]["text"], "Erste Idee");
}

#[test]
fn migration_preserves_old_documents_and_is_idempotent_and_writes_are_atomic() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_card(
        "demo",
        json!({"headline":"Version zwei","suggestions":[]}),
        true,
        false,
    )
    .unwrap();
    drop(svc);
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    remove_history_schema(&db);
    db.execute(
        "UPDATE facts SET data=json_remove(data,'$.code','$.activity')",
        [],
    )
    .unwrap();
    drop(db);
    let (svc, _) = service(tmp.path());
    assert_eq!(svc.card_history("demo").unwrap()["current_version"], 2);
    assert_eq!(
        svc.proposals(None).unwrap()["proposals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    let events = count(&db, "content_history");
    drop(svc);
    let (svc, _) = service(tmp.path());
    assert_eq!(count(&db, "content_history"), events);
    assert_eq!(count(&db, "fact_samples"), 1);
    db.execute_batch("CREATE TRIGGER test_abort BEFORE INSERT ON content_history BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(svc
        .put_card("demo", json!({"headline":"Nicht gespeichert"}), true, false)
        .is_err());
    assert_eq!(svc.card_history("demo").unwrap()["current_version"], 2);
    assert_eq!(count(&db, "content_history"), events);
    db.execute_batch("DROP TRIGGER test_abort; CREATE TRIGGER test_sample_abort BEFORE INSERT ON fact_samples BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    let before = svc.store().facts("demo").unwrap().unwrap().data;
    assert!(svc
        .store()
        .save_facts("demo", &svc.now(), "bad", &json!({"ok":false}))
        .is_err());
    assert_eq!(svc.store().facts("demo").unwrap().unwrap().data, before);
    db.execute_batch("DROP TRIGGER test_sample_abort").unwrap();
    svc.refresh_facts(None, false).unwrap();
    svc.refresh_facts(None, false).unwrap();
    let statistics = svc.statistics(Some("demo"), None, None).unwrap();
    assert_eq!(statistics["projects"][0]["comparable"], true);
    assert_eq!(statistics["projects"][0]["baseline"]["code"]["ok"], true);
}

#[test]
fn real_console_exposes_statistics_history_comparisons_and_guarded_idea_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_card("demo", json!({"headline":"Heute neu"}), true, false)
        .unwrap();
    for i in 0..35 {
        svc.add_idea(&format!("Idee {i}")).unwrap();
    }
    drop(svc);
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_Watchtower"))
            .env("WATCHTOWER_HOME", tmp.path())
            .args(args)
            .output()
            .unwrap()
    };
    for args in [
        vec!["statistics", "--project", "demo"],
        vec!["history", "list"],
        vec!["history", "show", "1"],
        vec!["card", "compare", "demo", "--from", "1", "--to", "2"],
        vec!["proposals"],
        vec!["ideas", "update", "1", "Überarbeitet", "--revision", "1"],
    ] {
        let out = run(&args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(out.stderr.is_empty());
        assert!(!out.stdout.is_empty());
    }
    let out = run(&["history", "list", "--json"]);
    let page: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(page["events"].as_array().unwrap().len(), 30);
    assert!(page["next_before"].is_number());
    let out = run(&["history", "list"]);
    let page = String::from_utf8(out.stdout).unwrap();
    let next = page
        .lines()
        .find_map(|line| line.strip_prefix("Next: watchtower "))
        .unwrap();
    assert!(next.contains("--instance"));
    let args: Vec<_> = next.split_whitespace().collect();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_Watchtower"))
        .env_remove("WATCHTOWER_HOME")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let out = run(&["statistics", "--project", "missing", "--json"]);
    assert!(!out.status.success());
    let out = run(&["statistics", "--from", "not-a-date"]);
    assert!(!out.status.success());
    let out = run(&["card", "compare", "demo", "--from", "1"]);
    assert_eq!(out.status.code(), Some(2));
}
