mod common;
use chrono::Duration;
use common::*;
use serde_json::{json, Value};
fn count(db: &rusqlite::Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn observations_record_changes_deduplicate_payloads_and_compare_known_measurements() {
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
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    let payloads = count(&db, "fact_payloads");
    let saved = svc.store().facts("demo").unwrap().unwrap();
    svc.store()
        .save_facts("demo", &svc.now(), &saved.fingerprint, &saved.data)
        .unwrap();
    assert_eq!(count(&db, "fact_samples"), 1);
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
    assert_eq!(stats["projects"][0]["samples"].as_array().unwrap().len(), 2);
    let point = &stats["projects"][0]["samples"][1];
    assert_eq!(point["code"]["source_code"], 2);
    assert!(point["code"]["languages"].is_null() && point["worktrees"]["items"].is_null());
    assert!(stats["projects"][0]["current"]["code"]["languages"].is_array());
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
    assert_eq!(count(&db, "fact_samples"), 3);
    svc.remove_project("demo").unwrap();
    svc.restore_project("demo").unwrap();
    assert_eq!(count(&db, "fact_samples"), 3);
    std::fs::write(repo.join("oversized.py"), vec![b'x'; 4_000_001]).unwrap();
    svc.refresh_facts(None, false).unwrap();
    let incomplete = svc.facts("demo").unwrap();
    assert_eq!(incomplete["code"]["partial"], true);
    assert_eq!(incomplete["code"]["unread_files"], 1);
    assert!(incomplete["dirty"]["lines_added"].is_null());
    assert!(svc.facts_history("demo").unwrap()["samples"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["facts"]["dirty"]["lines_added"]
        .is_null());
    assert!(svc.statistics(Some("demo"), None, None).unwrap()["summary"]["source_code"].is_null());

    // Uncertified older totals remain immutable in storage, but cannot appear as exact values
    // through current facts, full history, period samples or current/baseline read models.
    let mut legacy = common::measured_facts(4);
    legacy["dirty"]
        .as_object_mut()
        .unwrap()
        .remove("line_counts");
    legacy["dirty"]["lines_added"] = json!(1234);
    legacy["space"] = json!({"measured_on":svc.today().to_string(),"total_bytes":1234,"dirs":[{"path":"target","bytes":1234}]});
    svc.store()
        .save_facts("demo", &svc.now(), "legacy", &legacy)
        .unwrap();
    let facts = svc.facts("demo").unwrap();
    assert!(facts["dirty"]["lines_added"].is_null());
    assert!(facts["space"]["total_bytes"].is_null());
    assert!(facts["space"]["dirs"][0]["bytes"].is_null());
    let history = svc.facts_history("demo").unwrap();
    let historical = &history["samples"].as_array().unwrap().last().unwrap()["facts"];
    assert!(historical["dirty"]["lines_added"].is_null());
    assert!(historical["space"]["total_bytes"].is_null());
    let stats = svc.statistics(Some("demo"), None, None).unwrap();
    let project = &stats["projects"][0];
    assert!(project["current"]["dirty"]["lines_added"].is_null());
    assert!(project["current"]["space"]["total_bytes"].is_null());
    assert!(
        project["samples"].as_array().unwrap().last().unwrap()["dirty"]["lines_added"].is_null()
    );
    let raw: String = db
        .query_row(
            "SELECT data FROM facts WHERE project_id='demo'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&raw).unwrap(), legacy);

    // Complete endpoints cannot bridge a failed or incompatible intermediate observation.
    for barrier in ["failed", "partial", "path", "branch", "method"] {
        let fixture = tempfile::tempdir().unwrap();
        let (model, clock) = service(fixture.path());
        model
            .store()
            .add_project("fixture", "Fixture", "original", &model.now())
            .unwrap();
        let save = |data: &Value| {
            model
                .store()
                .save_facts(
                    "fixture",
                    &model.now(),
                    &vwatch::facts::fingerprint(data),
                    data,
                )
                .unwrap()
        };
        let initial = common::measured_facts(3);
        save(&initial);
        clock.advance(Duration::minutes(1));
        let db = rusqlite::Connection::open(fixture.path().join("vwatch.db")).unwrap();
        let mut middle = initial.clone();
        match barrier {
            "failed" => middle["ok"] = json!(false),
            "partial" => {
                middle["code"]["ok"] = json!(false);
                middle["code"]["partial"] = json!(true);
            }
            "branch" => middle["branch"] = json!("side"),
            "method" => middle["code"]["method"] = json!("other"),
            "path" => {
                db.execute("UPDATE projects SET path='other' WHERE id='fixture'", [])
                    .unwrap();
            }
            _ => unreachable!(),
        }
        save(&middle);
        clock.advance(Duration::minutes(1));
        db.execute("UPDATE projects SET path='original' WHERE id='fixture'", [])
            .unwrap();
        save(&common::measured_facts(8));
        let result = model
            .statistics(Some("fixture"), Some("2026-09-30"), Some("2026-09-30"))
            .unwrap();
        assert_eq!(result["projects"][0]["comparable"], false, "{barrier}");
        assert!(result["projects"][0]["change"].is_null(), "{barrier}");
    }
}

#[test]
fn identical_refreshes_add_no_sample_but_a_new_day_a_change_or_a_relink_does() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    assert_eq!(count(&db, "fact_samples"), 1);
    clock.advance(Duration::minutes(20));
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(count(&db, "fact_samples"), 1);
    let stored = svc.store().facts("demo").unwrap().unwrap();
    assert_eq!(stored.collected_at, "2026-09-30T08:20:00Z");
    // The first observation of each UTC day is kept, so durations stay measurable.
    clock.advance(Duration::hours(16));
    let save = |data: &Value| {
        svc.store()
            .save_facts("demo", &svc.now(), &stored.fingerprint, data)
            .unwrap()
    };
    save(&stored.data);
    assert_eq!(count(&db, "fact_samples"), 2);
    save(&stored.data);
    assert_eq!(count(&db, "fact_samples"), 2);
    let mut ci = stored.data.clone();
    ci["ci"] = json!({"state":"red"});
    save(&ci);
    assert_eq!(count(&db, "fact_samples"), 3);
    std::fs::write(repo.join("new.py"), "print('new')\n").unwrap();
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(count(&db, "fact_samples"), 4);
    let changed = svc.store().facts("demo").unwrap().unwrap();
    db.execute(
        "UPDATE projects SET identity_revision=identity_revision+1 WHERE id='demo'",
        [],
    )
    .unwrap();
    svc.store()
        .save_facts("demo", &svc.now(), &changed.fingerprint, &changed.data)
        .unwrap();
    assert_eq!(count(&db, "fact_samples"), 5);
    let history = svc.facts_history("demo").unwrap();
    assert_eq!(history["samples"].as_array().unwrap().len(), 5);
    assert!(history["note"]
        .as_str()
        .unwrap()
        .contains("first of its UTC day"));
}

#[test]
fn statistics_skip_repeated_observations_without_changing_their_results() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.store()
        .add_project("demo", "Demo", "fixture/demo", &svc.now())
        .unwrap();
    let save = |lines| {
        let mut data = common::measured_facts(lines);
        data["activity"]["through"] = json!(svc.today().to_string());
        data["activity"]["from"] = json!((svc.today() - Duration::days(29)).to_string());
        svc.store()
            .save_facts(
                "demo",
                &svc.now(),
                &vwatch::facts::fingerprint(&data),
                &data,
            )
            .unwrap();
    };
    save(0);
    for (step, lines) in [
        (Duration::hours(2), 1),
        (Duration::days(1), 1),
        (Duration::hours(1), 2),
        (Duration::days(1), 2),
    ] {
        clock.advance(step);
        save(lines);
    }
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    assert_eq!(count(&db, "fact_samples"), 5);
    let read = || {
        (
            svc.statistics(Some("demo"), Some("2026-09-30"), Some("2026-10-02"))
                .unwrap(),
            svc.statistics(None, Some("2026-09-30"), None).unwrap(),
        )
    };
    let before = read();
    let points = before.0["projects"][0]["samples"].as_array().unwrap();
    assert_eq!(points.len(), 5);
    assert_eq!(before.0["projects"][0]["change"]["code"], 2);
    // Earlier versions saved every refresh: repeat all but the latest observation a minute later.
    db.execute_batch("INSERT INTO fact_samples(project_id,at,fingerprint,path,identity_revision,origin,data,code_payload,activity_payload)
        SELECT project_id,strftime('%Y-%m-%dT%H:%M:%SZ',at,'+1 minute'),fingerprint,path,identity_revision,origin,data,code_payload,activity_payload
        FROM fact_samples WHERE id<(SELECT max(id) FROM fact_samples)").unwrap();
    assert_eq!(count(&db, "fact_samples"), 9);
    assert_eq!(read(), before);
}

#[test]
fn databases_from_the_first_history_version_stop_recording_repeats_and_keep_saved_samples() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    drop(svc);
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    db.execute_batch("DELETE FROM history_migrations WHERE name='changed-samples-v1'; DROP TRIGGER samples_update;
        CREATE TRIGGER samples_update AFTER UPDATE ON facts BEGIN
        INSERT INTO fact_samples(project_id,at,fingerprint,path,identity_revision,origin,data,code_payload,activity_payload)
        SELECT NEW.project_id,NEW.collected_at,NEW.fingerprint,p.path,p.identity_revision,'observed',json_remove(NEW.data,'$.code','$.activity'),
        (SELECT id FROM fact_payloads WHERE data=json_extract(NEW.data,'$.code')),
        (SELECT id FROM fact_payloads WHERE data=json_extract(NEW.data,'$.activity'))
        FROM (SELECT 1) LEFT JOIN projects p ON p.id=NEW.project_id; END;
        UPDATE facts SET collected_at='2026-09-30T08:10:00Z';").unwrap();
    assert_eq!(count(&db, "fact_samples"), 2);
    let (svc, clock) = service(tmp.path());
    assert_eq!(count(&db, "fact_samples"), 2);
    clock.advance(Duration::minutes(20));
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(count(&db, "fact_samples"), 2);
    let applied: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM history_migrations WHERE name='changed-samples-v1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(applied, 1);
    let stats = svc.statistics(Some("demo"), None, None).unwrap();
    assert_eq!(
        stats["projects"][0]["current"]["at"],
        "2026-09-30T08:10:00Z"
    );
}

#[test]
fn refreshes_keep_the_code_count_until_the_working_tree_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    std::fs::write(repo.join("app.py"), "print('one')\n").unwrap();
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    let code = |svc: &vwatch::VWatch| {
        svc.store().facts("demo").unwrap().unwrap().data["code"]["source_code"].clone()
    };
    assert_eq!(code(&svc), 1);
    // a marked count can only survive as the previous measurement
    let saved = svc.store().facts("demo").unwrap().unwrap();
    let mut marked = saved.data.clone();
    marked["code"]["source_code"] = json!(999);
    svc.store()
        .save_facts("demo", &svc.now(), &saved.fingerprint, &marked)
        .unwrap();
    clock.advance(Duration::hours(1));
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(code(&svc), 999);
    std::fs::write(repo.join("app.py"), "print('one')\nprint('two')\n").unwrap();
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(code(&svc), 2);
    // hotspots report their own 90-day window, not the activity window
    let facts = svc.store().facts("demo").unwrap().unwrap().data;
    let stats = svc.statistics(Some("demo"), None, None).unwrap();
    assert_eq!(
        stats["projects"][0]["activity"]["hotspots_from"],
        facts["activity"]["hotspots_from"]
    );
    assert_ne!(
        stats["projects"][0]["activity"]["hotspots_from"],
        facts["activity"]["from"]
    );
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

    svc.store()
        .add_project("rebase", "Rebase", "fixture/rebase", &svc.now())
        .unwrap();
    let mut data = common::measured_facts(1);
    data["activity"]["from"] = json!("2026-09-28");
    data["activity"]["through"] = json!("2026-09-30");
    data["activity"]["days"] = json!([{"date":"2026-09-28","commits":3,"added":3,"removed":0},{"date":"2026-09-29","commits":1,"added":1,"removed":0}]);
    svc.store()
        .save_facts("rebase", &svc.now(), "before", &data)
        .unwrap();
    clock.advance(Duration::minutes(1));
    data["head"] = json!("rebased");
    data["activity"]["days"] = json!([{"date":"2026-09-29","commits":1,"added":1,"removed":0}]);
    svc.store()
        .save_facts("rebase", &svc.now(), "after", &data)
        .unwrap();
    let replaced = svc
        .statistics(Some("rebase"), Some("2026-09-28"), Some("2026-09-30"))
        .unwrap();
    assert_eq!(replaced["projects"][0]["activity"]["complete"], true);
    assert_eq!(
        replaced["projects"][0]["activity"]["days"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(replaced["projects"][0]["activity"]["totals"]["commits"], 1);
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
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
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
    let proposals = svc.proposals(Some("demo"), None).unwrap();
    assert_eq!(proposals["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(proposals["proposals"][0]["in_report"], false);
    assert_eq!(
        proposals["proposals"][0]["first_seen"],
        "2026-09-30T08:00:00Z"
    );
    svc.set_feedback(id, Some("up"), None).unwrap();
    assert_eq!(
        svc.proposals(Some("demo"), Some("idea")).unwrap()["proposals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        svc.proposals(Some("demo"), Some("recommendation")).unwrap()["proposals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
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
    let mut reclassified = proposal.clone();
    reclassified["category"] = json!("recommendation");
    svc.put_card("demo", json!({"suggestions":[reclassified]}), true, false)
        .unwrap();
    let recommendations = svc.proposals(Some("demo"), Some("recommendation")).unwrap();
    assert_eq!(recommendations["proposals"][0]["id"], id);
    assert_eq!(
        recommendations["proposals"][0]["item"]["category"],
        "recommendation"
    );
    assert_eq!(svc.feedback(None).unwrap()[0]["value"], "up");
    assert_eq!(
        svc.proposals(Some("demo"), None).unwrap()["proposals"][0]["first_seen"],
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
fn document_normalization_updates_history_and_retains_new_item_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(json!({"greeting":"Hallo"}), false, false)
        .unwrap();
    let old_card = svc.store().latest_card("demo").unwrap().unwrap();
    let old_brief = svc.store().latest_portfolio().unwrap().unwrap();
    let mut normalized_card = old_card.data.clone();
    normalized_card["suggestions"][0]["id"] = json!("normalized-suggestion");
    let mut normalized_brief = old_brief.data.clone();
    normalized_brief["new_apps"] = json!([{"id":"normalized-app","title":"Retained app"}]);
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    // A failed journal write must also roll back the normalization and retained item.
    db.execute_batch("CREATE TRIGGER reject_normalization BEFORE INSERT ON content_history BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(svc
        .store()
        .rewrite_card_if_current("demo", old_card.version, &old_card.data, &normalized_card)
        .is_err());
    assert_eq!(
        svc.store().latest_card("demo").unwrap().unwrap().data,
        old_card.data
    );
    db.execute_batch("DROP TRIGGER reject_normalization")
        .unwrap();
    assert!(svc
        .store()
        .rewrite_card_if_current("demo", old_card.version, &old_card.data, &normalized_card)
        .unwrap());
    assert!(svc
        .store()
        .rewrite_portfolio_if_current(old_brief.version, &old_brief.data, &normalized_brief)
        .unwrap());
    let current = svc.store().latest_card("demo").unwrap().unwrap();
    assert_eq!(current.version, old_card.version);
    assert_eq!(current.created_at, old_card.created_at);
    let event = &svc
        .store()
        .content_history(None, Some("card"), Some("demo"), None, 1)
        .unwrap()[0];
    assert_eq!(event["before"], old_card.data);
    assert_eq!(event["after"], normalized_card);
    let event = &svc
        .store()
        .content_history(None, Some("portfolio"), None, None, 1)
        .unwrap()[0];
    assert_eq!(event["before"], old_brief.data);
    assert_eq!(event["after"], normalized_brief);
    let event_count = count(&db, "content_history");
    svc.store()
        .rewrite_latest_card("demo", &normalized_card)
        .unwrap();
    svc.store()
        .rewrite_latest_portfolio(&normalized_brief)
        .unwrap();
    assert_eq!(count(&db, "content_history"), event_count);
    svc.put_card("demo", json!({"suggestions":[]}), true, false)
        .unwrap();
    svc.put_portfolio(json!({"new_apps":[]}), true, false)
        .unwrap();
    let proposals = svc.store().proposals().unwrap();
    for id in ["normalized-suggestion", "normalized-app"] {
        let retained = proposals.iter().find(|p| p["id"] == id).unwrap();
        assert_eq!(retained["in_report"], false);
        assert_eq!(retained["first_seen"], "2026-09-30T08:00:00Z");
    }
}

#[test]
fn document_update_migration_repairs_already_normalized_proposals_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    drop(svc);
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    db.execute_batch(
        "DROP TRIGGER history_cards_update; DROP TRIGGER history_portfolio_update;
        DELETE FROM history_migrations WHERE name='document-updates-v1';
        UPDATE cards SET data=json_set(data,'$.suggestions[0].id','previously-normalized');",
    )
    .unwrap();
    let (svc, _) = service(tmp.path());
    let proposals = svc.store().proposals().unwrap();
    assert!(proposals
        .iter()
        .any(|p| p["id"] == "previously-normalized" && p["in_report"] == true));
    assert!(proposals
        .iter()
        .filter(|p| p["id"] != "previously-normalized")
        .all(|p| p["in_report"] == false));
    let event_count = count(&db, "content_history");
    drop(svc);
    let (svc, _) = service(tmp.path());
    assert_eq!(svc.store().proposals().unwrap(), proposals);
    assert_eq!(count(&db, "content_history"), event_count);
}

#[test]
fn history_filters_and_pages_keep_task_project_membership_and_exact_boundaries() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    for (entity, id, project, before, after) in [
        (
            "card",
            "demo",
            Some("demo"),
            json!(null),
            json!({"headline":"One"}),
        ),
        ("task", "1", None, json!(null), json!({"projects":["demo"]})),
        (
            "task",
            "1",
            None,
            json!({"projects":["demo"]}),
            json!({"projects":["other"]}),
        ),
        (
            "card",
            "other",
            Some("other"),
            json!(null),
            json!({"headline":"Two"}),
        ),
    ] {
        db.execute("INSERT INTO content_history(entity,entity_id,project_id,operation,origin,before_data,after_data) VALUES(?1,?2,?3,'update','write',?4,?5)", rusqlite::params![entity,id,project,before.to_string(),after.to_string()]).unwrap();
    }
    let ids = |project, entity, id, before, limit| {
        svc.store()
            .content_history(project, entity, id, before, limit)
            .unwrap()
            .into_iter()
            .map(|e| e["id"].as_i64().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(None, None, None, None, 2), [4, 3]);
    assert_eq!(ids(None, None, None, Some(3), 2), [2, 1]);
    assert_eq!(ids(Some("demo"), None, None, None, 10), [3, 2, 1]);
    assert_eq!(ids(Some("demo"), Some("card"), None, None, 10), [1]);
    assert_eq!(ids(Some("demo"), Some("task"), Some("1"), Some(3), 10), [2]);
    assert_eq!(ids(Some("other"), Some("task"), None, None, 10), [3]);
    assert_eq!(ids(None, None, Some("demo"), None, 10), [1]);
}

#[test]
fn confirmed_completions_count_owner_events_and_exclude_agent_review() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.store()
        .add_project("demo", "Demo", "fixture/demo", &svc.now())
        .unwrap();
    let created = svc
        .create_task(json!({"projects":["demo"],"title":"Finish"}))
        .unwrap();
    let id = created["task"]["id"].as_i64().unwrap();
    let count = || {
        svc.statistics(Some("demo"), Some("2026-09-30"), Some("2026-09-30"))
            .unwrap()["projects"][0]["confirmed_completions"]
            .clone()
    };
    assert_eq!(count(), 0);
    let reported = svc
        .report_task(id, "done", "Finished", "Contract checked")
        .unwrap();
    assert_eq!(reported["task"]["status"], "review");
    assert_eq!(count(), 0);
    svc.set_task_status_guarded(id, "done", reported["task"]["revision"].as_i64().unwrap())
        .unwrap();
    assert_eq!(count(), 1);
    svc.report_task(id, "done", "Finished", "Contract checked")
        .unwrap();
    assert_eq!(
        count(),
        1,
        "repeated report does not invent another owner completion"
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
        svc.portfolio_context(&[]).unwrap()["owner_ideas"][0]["checked"],
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
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    remove_history_schema(&db);
    db.execute_batch("ALTER TABLE facts DROP COLUMN revision;")
        .unwrap();
    db.execute(
        "UPDATE facts SET data=json_remove(data,'$.code','$.activity')",
        [],
    )
    .unwrap();
    let raw: String = db
        .query_row(
            "SELECT data FROM facts WHERE project_id='demo'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let raw: Value = serde_json::from_str(&raw).unwrap();
    drop(db);
    let (svc, _) = service(tmp.path());
    let project = svc.store().project("demo").unwrap().unwrap();
    let migrated = svc.store().facts("demo").unwrap().unwrap();
    assert_eq!(migrated.revision, 0);
    svc.store()
        .save_facts("demo", &migrated.collected_at, &migrated.fingerprint, &raw)
        .unwrap();
    assert_eq!(svc.store().facts("demo").unwrap().unwrap().revision, 1);
    assert_eq!(
        svc.store()
            .save_project_facts(
                &project,
                Some(migrated.revision),
                &migrated.collected_at,
                &migrated.fingerprint,
                &migrated.data
            )
            .unwrap(),
        vwatch::store::FactsSave::FactsChanged
    );
    assert_eq!(svc.card_history("demo").unwrap()["current_version"], 2);
    assert_eq!(
        svc.proposals(None, None).unwrap()["proposals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    let events = count(&db, "content_history");
    drop(svc);
    let (svc, clock) = service(tmp.path());
    assert_eq!(svc.store().facts("demo").unwrap().unwrap().revision, 1);
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
    clock.advance(Duration::days(1));
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
        std::process::Command::new(env!("CARGO_BIN_EXE_vWatch"))
            .env("VWATCH_HOME", tmp.path())
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
        vec!["proposals", "--category", "idea"],
        vec![
            "proposals",
            "--category",
            "recommendation",
            "--project",
            "demo",
        ],
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
    let out = run(&["proposals", "--category", "idea", "--json"]);
    let proposals: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(proposals["category"], "idea");
    assert_eq!(proposals["proposals"][0]["item"]["category"], "idea");
    assert!(!run(&["proposals", "--category", "invalid"])
        .status
        .success());
    assert!(!run(&["proposals", "--project", "missing"]).status.success());
    let out = run(&["history", "list", "--json"]);
    let page: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(page["events"].as_array().unwrap().len(), 30);
    assert!(page["next_before"].is_number());
    let out = run(&["history", "list"]);
    let page = String::from_utf8(out.stdout).unwrap();
    let next = page
        .lines()
        .find_map(|line| line.strip_prefix("Next: vwatch "))
        .unwrap();
    assert!(next.contains("--instance"));
    let args: Vec<_> = next.split_whitespace().collect();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_vWatch"))
        .env_remove("VWATCH_HOME")
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
