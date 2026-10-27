use super::*;

#[test]
fn space_retries_incomplete_or_uncertified_results_but_reuses_a_complete_day() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    std::fs::create_dir(repo.join("target")).unwrap();
    std::fs::write(repo.join("target/rebuildable"), b"12345").unwrap();
    for legacy in [true, false] {
        let mut stored = svc.store().facts("demo").unwrap().unwrap();
        stored.data["space"] = if legacy {
            json!({"measured_on":svc.today().to_string(),"total_bytes":99999,"dirs":[]})
        } else {
            json!({"measured_on":svc.today().to_string(),"total_bytes":null,"ok":false,"partial":true,"errors":[{"path":"target","reason":"permission_denied"}],"dirs":[]})
        };
        svc.store()
            .save_facts("demo", &svc.now(), &stored.fingerprint, &stored.data)
            .unwrap();
        svc.refresh_facts(None, false).unwrap();
        assert_eq!(svc.facts("demo").unwrap()["space"]["total_bytes"], 5);
        assert_eq!(svc.facts("demo").unwrap()["space"]["ok"], true);
    }
    std::fs::write(repo.join("target/rebuildable"), b"1234567").unwrap();
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(svc.facts("demo").unwrap()["space"]["total_bytes"], 5);
    clock.advance(Duration::days(1));
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(svc.facts("demo").unwrap()["space"]["total_bytes"], 7);
}

#[test]
fn status_and_registration_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    let other = common::make_repo(tmp.path(), "Other App", &["2026-09-20T10:00:00"]);
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    let missing = tmp.path().join("missing folder");
    let error = svc
        .add_project(missing.to_str().unwrap(), None, None)
        .unwrap_err();
    assert_eq!(error.code, "path_missing");
    let hint = error.hint.as_deref().unwrap();
    assert!(hint.contains("projects update with its existing id and --path"));
    assert!(hint.contains("preserve its cards and history"));
    let error = svc.discover(missing.to_str().unwrap()).unwrap_err();
    assert_eq!(error.code, "path_missing");
    assert!(error
        .hint
        .as_deref()
        .unwrap()
        .contains("projects discover with an existing projects folder"));
    for name in ["", "   "] {
        assert_eq!(
            svc.add_project(other.to_str().unwrap(), None, Some(name))
                .unwrap_err()
                .code,
            "invalid_name"
        );
    }
    assert_eq!(
        svc.add_project("", None, None).unwrap_err().code,
        "invalid_path"
    );
    assert_eq!(svc.list_projects().unwrap().as_array().unwrap().len(), 1);
    assert_eq!(
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap()["already_exists"],
        true
    );
    common::make_repo(tmp.path(), "side", &["2026-09-25T10:00:00"]);
    common::make_repo(
        &tmp.path().join("group"),
        "old-tool",
        &["2025-01-01T10:00:00"],
    );
    let picks = |d: &serde_json::Value| -> Vec<(String, bool)> {
        d["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["name"].as_str().unwrap().to_owned(),
                    c["recommended"] == true,
                )
            })
            .collect()
    };
    // a folder of projects: two levels deep, newest first, recently active ones recommended
    let found = svc.discover(tmp.path().to_str().unwrap()).unwrap();
    assert_eq!(
        picks(&found),
        [
            ("side".into(), true),
            ("Other App".into(), true),
            ("old-tool".into(), false)
        ]
    );
    assert_eq!(
        (
            found["folder_is_repo"].as_bool(),
            found["already_watched"].as_u64()
        ),
        (Some(false), Some(1))
    );
    // a single project: it comes first and recommended, its neighbours are offered unticked
    let found = svc.discover(other.to_str().unwrap()).unwrap();
    assert_eq!(
        picks(&found),
        [("Other App".into(), true), ("side".into(), false)]
    );
    assert_eq!(found["folder_is_repo"], true);
    let batch = svc
        .add_projects(&[
            other.to_string_lossy().into_owned(),
            repo.to_string_lossy().into_owned(),
        ])
        .unwrap();
    assert_eq!(batch["added"][0]["id"], "other-app");
    assert_eq!(batch["added"][1]["id"], "demo");
    assert!(batch["failed"].as_array().unwrap().is_empty());
    svc.put_card("demo", card(), false, false).unwrap();
    // versions count per project
    assert_eq!(
        svc.put_card("other-app", card(), false, false).unwrap()["version"],
        1
    );
    svc.set_status("demo", "parked", Some("wartet auf Key"))
        .unwrap();
    let due = svc.due(14, true).unwrap();
    assert!(due["projects"]
        .as_array()
        .unwrap()
        .contains(&json!({"id": "demo", "name": "demo", "reason": "status_changed"})));
    assert_eq!(
        svc.state(false).unwrap()["projects"][0]["card_outdated"],
        true
    );
    assert_eq!(
        svc.portfolio_context(&[]).unwrap()["projects"][0]["card"]["outdated"],
        true
    );
    // a goal reaches the agents and makes the card due like a status change
    svc.put_card("demo", card(), true, false).unwrap();
    svc.set_status("demo", "parked", Some("wartet auf Key"))
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    svc.set_status("demo", "parked", Some("wartet auf Entscheidung"))
        .unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    svc.put_card("demo", card(), true, false).unwrap();
    assert_eq!(
        svc.set_goal("demo", "v1.0 veröffentlichen", Some("15.12."))
            .unwrap_err()
            .code,
        "invalid_date"
    );
    svc.set_goal("demo", "v1.0 veröffentlichen", Some("2026-12-15"))
        .unwrap();
    assert_eq!(
        svc.due(14, true).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    assert_eq!(
        svc.context("demo").unwrap()["project"]["goal_due"],
        "2026-12-15"
    );
    svc.put_card("demo", card(), true, false).unwrap();
    svc.set_goal("demo", "v1.0 veröffentlichen", Some("2026-12-15"))
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    svc.set_status("demo", "archived", None).unwrap();
    let due = svc.due(14, true).unwrap();
    assert!(due["projects"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["id"] != "demo"));
    assert_eq!(
        svc.store().project("demo").unwrap().unwrap().status_note,
        "wartet auf Entscheidung"
    );
}

#[test]
fn database_errors_distinguish_retryable_locks_from_access_and_corruption() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("vwatch.db");
    let writable = rusqlite::Connection::open(&path).unwrap();
    writable
        .execute_batch("CREATE TABLE example (id INTEGER);")
        .unwrap();
    let readonly =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let error = vwatch::VWatchError::from(
        readonly
            .execute("INSERT INTO example VALUES (1)", [])
            .unwrap_err(),
    );
    assert_eq!(error.code, "storage_error");
    let hint = error.hint.as_deref().unwrap();
    assert!(hint.contains("Restore the intended access"));
    assert!(hint.contains("read the current state before retrying a write"));
    assert!(!hint.contains("Wait for"));
    let contender = rusqlite::Connection::open(&path).unwrap();
    contender.busy_timeout(std::time::Duration::ZERO).unwrap();
    writable.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let error = vwatch::VWatchError::from(
        contender
            .execute("INSERT INTO example VALUES (2)", [])
            .unwrap_err(),
    );
    assert_eq!(error.code, "storage_error");
    let hint = error.hint.as_deref().unwrap();
    assert!(hint.contains("Wait for the other database operation"));
    assert!(hint.contains("does not confirm that earlier steps were undone"));
    writable.execute_batch("ROLLBACK;").unwrap();
    let damaged = tmp.path().join("damaged.db");
    let bytes = b"This is not a SQLite database.";
    std::fs::write(&damaged, bytes).unwrap();
    let error = match vwatch::VWatch::open(&damaged) {
        Ok(_) => panic!("An invalid database must not be opened or replaced."),
        Err(error) => error,
    };
    assert_eq!(error.code, "storage_error");
    let hint = error.hint.as_deref().unwrap();
    assert!(hint.contains("Stop writing and preserve the database"));
    assert!(hint.contains("do not delete or recreate"));
    assert_eq!(std::fs::read(&damaged).unwrap(), bytes);
}

#[test]
fn project_updates_keep_identity_history_and_owner_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    svc.set_goal("demo", "Version veröffentlichen", Some("2026-12-01"))
        .unwrap();
    svc.set_status("demo", "parked", Some("Erst nach dem Urlaub"))
        .unwrap();
    svc.open_project("demo").unwrap();
    svc.put_card(
        "demo",
        card_with(json!({"waiting_on_you": [{"id": "q-rename", "title": "Offline?"}]})),
        false,
        false,
    )
    .unwrap();
    svc.set_feedback("q-rename", Some("answered"), Some("Ja"))
        .unwrap();
    svc.set_warning_ack("demo", "no_remote", Some("Absichtlich lokal"))
        .unwrap();
    svc.create_task(json!({"projects": ["demo"], "source_id": "sg-kept", "title": "Export bauen"}))
        .unwrap();
    svc.put_card("demo", json!({}), true, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    let run_id = svc.run_start(false).unwrap()["run"]["id"].as_i64().unwrap();
    let before = svc.list_projects().unwrap()[0].clone();
    let history = svc.store().card_history("demo").unwrap();
    let feedback = svc.feedback(None).unwrap();
    let tasks = svc.tasks(None).unwrap();
    let acks = svc.store().warning_acknowledgements("demo").unwrap();
    let run_initial = svc.store().agent_run(run_id).unwrap().unwrap().initial;
    let old_card_token = svc.context("demo").unwrap()["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let old_brief_token = svc.portfolio_context(&[]).unwrap()["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let facts_before = svc.facts("demo").unwrap();
    let renamed = svc
        .update_project("demo", Some("  vUtility  "), None)
        .unwrap();
    assert_eq!(renamed["project"]["id"], "demo");
    assert_eq!(renamed["project"]["name"], "vUtility");
    assert_eq!(renamed["changed"], true);
    assert!(renamed["project"].get("identity_revision").is_none());
    assert_eq!(svc.facts("demo").unwrap(), facts_before);
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    assert_eq!(
        svc.due(14, false).unwrap()["portfolio"]["reason"],
        "cards_changed"
    );
    assert_eq!(
        svc.put_card_guarded("demo", json!({}), true, false, Some(&old_card_token))
            .unwrap_err()
            .code,
        "version_conflict"
    );
    assert_eq!(
        svc.put_portfolio_guarded(json!({}), true, false, Some(&old_brief_token))
            .unwrap_err()
            .code,
        "version_conflict"
    );
    svc.put_card("demo", json!({}), true, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    let token = svc.context("demo").unwrap()["context_token"].clone();
    assert_eq!(
        svc.update_project("demo", Some("vUtility"), Some(repo.to_str().unwrap()))
            .unwrap()["changed"],
        false
    );
    assert_eq!(svc.context("demo").unwrap()["context_token"], token);
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], false);
    let moved = tmp.path().join("vUtility");
    std::fs::rename(&repo, &moved).unwrap();
    // Same-day disk figures must be measured again at the new location.
    let mut old_facts = svc.store().facts("demo").unwrap().unwrap();
    old_facts.data["space"]["total_bytes"] = json!(99999);
    svc.store()
        .save_facts("demo", &svc.now(), &old_facts.fingerprint, &old_facts.data)
        .unwrap();
    let old_project = svc.store().project("demo").unwrap().unwrap();
    let relinked = svc
        .update_project("demo", None, Some(moved.to_str().unwrap()))
        .unwrap();
    assert_eq!(relinked["project"]["id"], "demo");
    assert_eq!(relinked["facts_refresh"]["ok"], true);
    assert_eq!(svc.facts("demo").unwrap()["space"]["total_bytes"], 0);
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    assert_eq!(
        svc.due(14, false).unwrap()["portfolio"]["reason"],
        "cards_changed"
    );
    // A refresh that captured the previous project cannot resurrect its facts.
    assert_eq!(
        svc.store()
            .save_project_facts(
                &old_project,
                Some(old_facts.revision),
                &svc.now(),
                &old_facts.fingerprint,
                &old_facts.data
            )
            .unwrap(),
        vwatch::store::FactsSave::ProjectChanged
    );
    assert_eq!(svc.facts("demo").unwrap()["space"]["total_bytes"], 0);
    let after = svc.list_projects().unwrap()[0].clone();
    for field in [
        "id",
        "status",
        "status_note",
        "status_changed_at",
        "added_at",
        "opened_at",
        "previous_opened_at",
        "goal",
        "goal_due",
    ] {
        assert_eq!(after[field], before[field], "{field}");
    }
    assert_eq!(&svc.store().card_history("demo").unwrap()[..2], &history);
    assert_eq!(svc.feedback(None).unwrap(), feedback);
    assert_eq!(svc.tasks(None).unwrap(), tasks);
    assert_eq!(svc.store().warning_acknowledgements("demo").unwrap(), acks);
    assert_eq!(
        svc.store().agent_run(run_id).unwrap().unwrap().initial,
        run_initial
    );
    assert_eq!(
        svc.rename_project("demo", "Final Name").unwrap()["id"],
        "demo"
    );
}

#[test]
fn project_update_validation_is_atomic_and_old_databases_stay_current() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    let other = common::make_repo(tmp.path(), "other", &["2026-09-29T10:00:00"]);
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    svc.add_project(other.to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_card("other", card(), false, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    drop(svc);
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    common::remove_history_schema(&db);
    db.execute_batch("ALTER TABLE projects DROP COLUMN identity_revision;")
        .unwrap();
    drop(db);
    let (svc, _) = service(tmp.path());
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], false);
    let before = svc.list_projects().unwrap();
    let token = svc.context("demo").unwrap()["context_token"].clone();
    let missing = tmp.path().join("missing");
    for (name, path, error) in [
        (None, None, "invalid_update"),
        (Some("  "), None, "invalid_name"),
        (Some("Changed"), Some("  "), "invalid_path"),
        (
            Some("Changed"),
            Some(missing.to_str().unwrap()),
            "path_missing",
        ),
        (
            Some("Changed"),
            Some(other.to_str().unwrap()),
            "already_registered",
        ),
    ] {
        let rejected = svc.update_project("demo", name, path).unwrap_err();
        assert_eq!(rejected.code, error);
        assert!(rejected.hint.is_some());
        assert_eq!(svc.list_projects().unwrap(), before);
        assert_eq!(svc.context("demo").unwrap()["context_token"], token);
    }
    // Older cards with no owner basis also become due after a same-clock rename.
    let db = rusqlite::Connection::open(tmp.path().join("vwatch.db")).unwrap();
    db.execute(
        "UPDATE cards SET owner_basis = NULL WHERE project_id = 'demo'",
        [],
    )
    .unwrap();
    svc.rename_project("demo", "Renamed").unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    svc.put_card("demo", json!({}), true, false).unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    // A valid existing non-git folder remains supported, with explicit measurement failure.
    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    let updated = svc
        .update_project("demo", None, Some(plain.to_str().unwrap()))
        .unwrap();
    assert_eq!(updated["changed"], true);
    assert_eq!(updated["facts_refresh"]["ok"], false);
    assert_eq!(updated["facts_refresh"]["error"], "not_a_git_repository");
    assert_eq!(svc.card("demo").unwrap()["version"], 2);
}
