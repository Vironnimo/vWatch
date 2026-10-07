mod common;

use chrono::Duration;
use common::{card, card_with, demo_repo, git, service};
use serde_json::{json, Value};

#[test]
fn morning_run_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();

    let due = svc.due(14, true).unwrap();
    assert_eq!(
        due["projects"],
        json!([{"id": "demo", "name": "demo", "reason": "no_card"}])
    );
    assert_eq!(due["portfolio"]["due"], true);

    let saved = svc.put_card("demo", card(), false, false).unwrap();
    assert!(saved["items"][0]["id"].as_str().unwrap().starts_with("sg-"));
    assert_eq!(svc.due(14, true).unwrap()["projects"], json!([]));

    std::fs::write(repo.join("f0.txt"), "more\n").unwrap();
    git(&repo, &["commit", "-qam", "second"]);
    assert_eq!(
        svc.due(14, true).unwrap()["projects"][0],
        json!({"id": "demo", "name": "demo", "reason": "changed", "changes": ["commits"]})
    );
    assert_eq!(
        svc.portfolio_context().unwrap()["projects"][0]["card"]["outdated"],
        true
    );
    let ctx = svc.context("demo").unwrap();
    assert_eq!(ctx["previous_card"]["card"]["headline"], "Alles ruhig");
    assert_eq!(ctx["since_previous_card"]["commit_count"], 1);
    assert!(ctx["since_previous_card"]["commits"][0]
        .as_str()
        .unwrap()
        .ends_with("second"));

    svc.put_card("demo", card(), false, false).unwrap();
    // a card written before Watchtower added a field is due once to fill it
    let mut older = svc.card("demo").unwrap()["card"].clone();
    older.as_object_mut().unwrap().remove("handbook");
    older["suggestions"][0]
        .as_object_mut()
        .unwrap()
        .remove("done_when");
    svc.store().rewrite_latest_card("demo", &older).unwrap();
    let entry = json!({"id": "demo", "name": "demo", "reason": "new_fields", "new_fields": ["suggestions[].done_when", "handbook"]});
    assert_eq!(svc.due(14, true).unwrap()["projects"][0], entry);
    assert_eq!(
        svc.context("demo").unwrap()["new_fields"],
        entry["new_fields"]
    );
    // a merge that leaves them out is told so, and their defaults do not count as changes
    let r = svc
        .put_card("demo", json!({"headline": "Neu"}), true, true)
        .unwrap();
    assert_eq!(
        (&r["unfilled_fields"], &r["changed_fields"]),
        (&entry["new_fields"], &json!(["headline"]))
    );
    assert!(
        r["warnings"][0].as_str().unwrap().contains("saved empty"),
        "{}",
        r["warnings"]
    );
    // a handbook command the owner could not copy as it stands is flagged
    for how in [
        "Im Ordner: `cargo test`",
        "python -m app; http://127.0.0.1:8000",
        "Startmenü „Demo“",
    ] {
        let run = json!({"handbook": {"run": [{"what": "Starten", "how": how}]}});
        let r = svc.put_card("demo", run, true, true).unwrap();
        assert!(
            r["warnings"].to_string().contains("not a plain command"),
            "{how}: {}",
            r["warnings"]
        );
    }
    clock.advance(Duration::days(15));
    assert_eq!(svc.due(14, true).unwrap()["projects"][0]["reason"], "stale");
}

#[test]
fn stored_cards_and_briefs_keep_working_when_content_bases_are_added() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    drop(svc);

    // Simulate the previous database schema with existing data, then reopen it.
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
    db.execute_batch(
        "ALTER TABLE cards DROP COLUMN owner_basis; ALTER TABLE portfolio DROP COLUMN basis;",
    )
    .unwrap();
    drop(db);
    let (svc, clock) = service(tmp.path());
    assert!(svc
        .store()
        .latest_card("demo")
        .unwrap()
        .unwrap()
        .owner_basis
        .is_none());
    assert!(svc
        .store()
        .latest_portfolio()
        .unwrap()
        .unwrap()
        .basis
        .is_none());
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], false);
    clock.advance(Duration::minutes(1));
    svc.set_status("demo", "parked", Some("Später")).unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
    svc.put_card("demo", json!({"headline": "Neu"}), true, false)
        .unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["portfolio"]["reason"],
        "cards_changed"
    );
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["due"], false);
    assert!(svc
        .store()
        .latest_card("demo")
        .unwrap()
        .unwrap()
        .owner_basis
        .is_some());
    assert!(svc
        .store()
        .latest_portfolio()
        .unwrap()
        .unwrap()
        .basis
        .is_some());
    svc.set_goal("demo", "Fertig werden", None).unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "status_changed"
    );
}

#[test]
fn reopening_a_project_after_a_while_shows_what_happened_since() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    assert_eq!(svc.open_project("demo").unwrap()["since"], json!(null));

    clock.advance(Duration::days(5));
    std::fs::write(
        repo.join("f0.txt"),
        "more
",
    )
    .unwrap();
    common::git_at(&repo, &["commit", "-qam", "search"], "2026-10-05T10:00:00");
    let milestones = json!([{"date": "2026-01-01", "title": "Start"}]);
    let changes = json!([{"date": "2026-10-05", "title": "Neue Suche"}]);
    svc.put_card(
        "demo",
        card_with(json!({"phase": "cooling", "recent_changes": changes, "milestones": milestones})),
        false,
        false,
    )
    .unwrap();
    clock.advance(Duration::days(5));

    // reopening within the hour keeps the same comparison point
    for _ in 0..2 {
        let since = svc.open_project("demo").unwrap()["since"].clone();
        assert_eq!(
            (
                since["days"].clone(),
                since["commits"].clone(),
                since["card_updates"].clone()
            ),
            (json!(10), json!(1), json!(1))
        );
        assert_eq!(since["phase"], json!({"from": "active", "to": "cooling"}));
        assert_eq!(
            (
                since["recent_changes"][0]["title"].clone(),
                since["milestones"].clone()
            ),
            (json!("Neue Suche"), json!([]))
        );
        clock.advance(Duration::minutes(10));
    }
    clock.advance(Duration::days(1));
    assert_eq!(svc.open_project("demo").unwrap()["since"], json!(null));
}

#[test]
fn merge_keeps_unsent_fields_and_rated_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let first = svc
        .put_card(
            "demo",
            card_with(json!({
                "features": ["Suche"],
                "handbook": {"installed": {"version": "1.0", "path": "C:/Apps/Demo.exe"}}
            })),
            false,
            false,
        )
        .unwrap();
    let sid = first["items"][0]["id"].as_str().unwrap().to_owned();
    let handbook = svc.card("demo").unwrap()["card"]["handbook"].clone();
    assert_eq!(handbook["installed"]["open"], "");

    let saved = svc
        .put_card("demo", json!({"headline": "Neu"}), true, false)
        .unwrap();
    assert_eq!(saved["changed_fields"], json!(["headline"]));
    let current = &svc.card("demo").unwrap()["card"];
    assert_eq!(
        (current["headline"].clone(), current["features"].clone()),
        (json!("Neu"), json!(["Suche"]))
    );
    assert_eq!(current["suggestions"][0]["id"], json!(sid));
    assert_eq!(current["handbook"], handbook);
}

#[test]
fn item_ids_cannot_transfer_reactions_between_documents() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    let first = demo_repo(tmp.path());
    let second = common::make_repo(tmp.path(), "second", &["2026-09-29T10:00:00"]);
    svc.add_project(first.to_str().unwrap(), None, None)
        .unwrap();
    svc.add_project(second.to_str().unwrap(), None, None)
        .unwrap();
    let mut original = card();
    original["suggestions"][0]["id"] = json!("shared-item");
    svc.put_card("demo", original.clone(), false, false)
        .unwrap();
    svc.set_feedback("shared-item", Some("down"), Some("Passt nicht"))
        .unwrap();

    // Reusing an id in its own card preserves the owner's rating.
    svc.put_card("demo", original.clone(), false, false)
        .unwrap();
    for dry_run in [true, false] {
        let err = svc
            .put_card("second", original.clone(), false, dry_run)
            .unwrap_err();
        assert_eq!(err.code, "invalid_card");
        assert!(err.message.contains("suggestions[0].id"), "{err:?}");
    }
    assert_eq!(svc.card("second").unwrap_err().code, "no_card");

    let brief = json!({"greeting": "Moin", "new_apps": [{
        "id": "shared-item", "title": "Andere Idee", "pitch": "Ein Werkzeug", "because": "Hilft", "effort": "S"
    }]});
    assert_eq!(
        svc.put_portfolio(brief, false, false).unwrap_err().code,
        "invalid_portfolio"
    );

    // Dropping a rejected item must not free its id to acquire a different meaning elsewhere.
    svc.put_card("demo", json!({"suggestions": []}), true, false)
        .unwrap();
    assert_eq!(
        svc.put_card("second", original, false, false)
            .unwrap_err()
            .code,
        "invalid_card"
    );
    let reactions = svc.feedback(None).unwrap();
    assert_eq!(reactions[0]["project"], "demo");
    assert_eq!(reactions[0]["value"], "down");

    // A focus item's project is its subject; the portfolio still owns its identity.
    let brief = json!({"greeting": "Moin", "focus": [{
        "id": "focus-owned", "project": "demo", "title": "Prüfen", "why": "Hilft", "effort": "1h"
    }]});
    svc.put_portfolio(brief.clone(), false, false).unwrap();
    svc.set_feedback("focus-owned", Some("done"), None).unwrap();
    svc.put_portfolio(brief, false, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    let mut different_item = card();
    different_item["suggestions"][0]["id"] = json!("focus-owned");
    assert_eq!(
        svc.put_card("demo", different_item, false, false)
            .unwrap_err()
            .code,
        "invalid_card"
    );
}

#[test]
fn invalid_cards_are_refused_with_field_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    // A mistyped project id must be corrected before asking for its context.
    for token in [None, Some("wrong-token")] {
        let error = svc
            .put_card_guarded("demoo", card(), false, false, token)
            .unwrap_err();
        assert_eq!(error.code, "project_not_found");
        assert!(error.hint.as_deref().unwrap().contains("demo"));
    }
    let err = svc
        .put_card(
            "demo",
            card_with(json!({"phase": "sleepy", "reentry": {"next_step": "x"}})),
            false,
            false,
        )
        .unwrap_err();
    assert_eq!(err.code, "invalid_card");
    assert!(
        err.message.contains("phase:") && err.message.contains("reentry.effort: Field required"),
        "{}",
        err.message
    );
    let empty_title =
        json!({"suggestions": [{"type": "feature", "title": "", "why": "w", "effort": "S"}]});
    let err = svc
        .put_card("demo", card_with(empty_title), false, false)
        .unwrap_err();
    assert!(
        err.message.contains("suggestions[0].title"),
        "{}",
        err.message
    );
    // the word limits the page is designed for; dashes do not count as words
    let long = json!({"headline": "Eins zwei drei vier fünf sechs sieben acht neun zehn – elf"});
    let err = svc
        .put_card("demo", card_with(long), false, false)
        .unwrap_err();
    assert!(
        err.message
            .contains("headline: Text should have at most 10 words, has 11"),
        "{}",
        err.message
    );
    // a dry run reports what would be saved and saves nothing; new items get their id only when saved
    let tried = svc
        .put_card("demo", card_with(json!({"colour": "blue"})), false, true)
        .unwrap();
    assert_eq!(
        (tried["saved"].as_bool(), tried["warnings"].clone()),
        (Some(false), json!(["ignored unknown field: colour"]))
    );
    assert_eq!(tried["items"][0]["id"], json!(null));
    assert_eq!(svc.card("demo").unwrap_err().code, "no_card");
    let saved = svc
        .put_card(
            "demo",
            card_with(json!({"colour": "blue", "vision": {"statement": "Ziel", "size": 3}})),
            false,
            false,
        )
        .unwrap();
    assert_eq!(
        saved["warnings"],
        json!([
            "ignored unknown field: colour",
            "ignored unknown field: vision.size"
        ])
    );
    let stored = svc.store().latest_card("demo").unwrap().unwrap();
    let old = card_with(json!({"health_notes": ["wort ".repeat(31)]}));
    svc.store()
        .add_card(
            "demo",
            &svc.now(),
            &watchtower::store::CardBasis {
                fingerprint: &stored.fingerprint,
                head: None,
                ci: None,
                parts: &json!(null),
                owner: None,
            },
            &old,
        )
        .unwrap();
    // with --merge, text the agent did not send is kept despite a newer word limit, with a warning
    let kept = svc
        .put_card("demo", json!({"headline": "Neu"}), true, false)
        .unwrap();
    assert!(
        kept["warnings"][0]
            .as_str()
            .unwrap()
            .contains("health_notes[0] (31 of 30 words)"),
        "{}",
        kept["warnings"]
    );
    let err = svc
        .put_card(
            "demo",
            json!({"health_notes": ["wort ".repeat(31)]}),
            true,
            false,
        )
        .unwrap_err();
    assert!(
        err.message
            .contains("health_notes[0]: Text should have at most 30 words, has 31"),
        "{}",
        err.message
    );
}

#[test]
fn portfolio_checks_project_ids_and_is_due_each_day() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let mut brief = json!({"greeting": "Moin", "focus": [{"project": "ghost", "title": "x", "why": "y", "effort": "1h"}]});
    let err = svc.put_portfolio(brief.clone(), false, false).unwrap_err();
    assert_eq!(err.code, "unknown_project");
    let four = json!({"greeting": "Moin", "focus": vec![brief["focus"][0].clone(); 4]});
    assert!(svc
        .put_portfolio(four, false, false)
        .unwrap_err()
        .message
        .contains("focus: List should have at most 3 items"));
    assert!(err.hint.unwrap().contains("demo"));

    brief["focus"][0]["project"] = json!("demo");
    svc.put_portfolio(brief, false, false).unwrap();
    assert_eq!(svc.due(14, true).unwrap()["portfolio"]["due"], false);
    // a card change the brief does not build on leaves it current; one it builds on makes it due
    // Writes within one clock tick must still compare with the card the brief read.
    svc.put_card(
        "demo",
        json!({"handbook": {"architecture": "Ein Dienst."}}),
        true,
        false,
    )
    .unwrap();
    assert_eq!(svc.due(14, true).unwrap()["portfolio"]["due"], false);
    svc.put_card("demo", json!({"headline": "Neu"}), true, false)
        .unwrap();
    assert_eq!(
        svc.due(14, true).unwrap()["portfolio"]["reason"],
        "cards_changed"
    );
    assert_eq!(
        svc.portfolio_context().unwrap()["projects"][0]["card"]["changed_since_brief"],
        json!(["headline"])
    );
    clock.advance(Duration::days(1));
    assert_eq!(svc.due(14, true).unwrap()["portfolio"]["reason"], "new_day");
    // Removing or archiving a project invalidates a brief that still recommends it.
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    svc.set_status("demo", "archived", None).unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["portfolio"]["reason"],
        "cards_changed"
    );
    svc.set_status("demo", "active", None).unwrap();
    svc.put_card("demo", json!({}), true, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    svc.remove_project("demo").unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["portfolio"]["reason"],
        "cards_changed"
    );
}

#[test]
fn parked_ideas_and_the_week_review_reach_the_brief() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let changes = json!([{"date": "2026-09-20", "title": "Davor"}, {"date": "2026-09-22", "title": "In der Woche"}]);
    svc.put_card(
        "demo",
        card_with(json!({"recent_changes": changes})),
        false,
        false,
    )
    .unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    assert_eq!(svc.add_idea("  ").unwrap_err().code, "invalid_idea");

    // a parked idea makes the brief due until a brief assesses it
    let id = svc.add_idea("Ein Stromzähler-Dashboard").unwrap()["id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        svc.due(14, true).unwrap()["portfolio"]["reason"],
        "owner_idea"
    );
    let ctx = svc.portfolio_context().unwrap();
    assert_eq!(
        (
            ctx["owner_ideas"][0]["text"].as_str(),
            ctx["owner_ideas"][0]["checked"].as_bool()
        ),
        (Some("Ein Stromzähler-Dashboard"), Some(false))
    );
    // the clock stands on Wednesday 2026-09-30, so the review looks back on the week before
    assert_eq!(
        ctx["review_week"],
        json!({"week": "2026-W39", "from": "2026-09-21", "to": "2026-09-27", "due": true})
    );
    // the review gets the card changes from the review week's start on
    assert_eq!(
        ctx["projects"][0]["card"]["recent_changes"],
        json!([{"date": "2026-09-22", "title": "In der Woche"}])
    );

    let check = json!({"idea": id, "verdict": "fold_in", "fits": ["demo"], "why": "Passt in demo.", "effort": "S"});
    let with = |c: &Value| json!({"greeting": "Moin", "idea_checks": [c]});
    let altered = |key: &str, value: Value| {
        let mut c = check.clone();
        c[key] = value;
        with(&c)
    };
    assert_eq!(
        svc.put_portfolio(altered("idea", json!(99)), false, false)
            .unwrap_err()
            .code,
        "unknown_idea"
    );
    assert_eq!(
        svc.put_portfolio(altered("fits", json!(["ghost"])), false, false)
            .unwrap_err()
            .code,
        "unknown_project"
    );
    let mut brief = with(&check);
    brief["week_review"] = json!({"week": "2026-W39", "done": ["Release 1.0"], "stalled": [], "time_note": "Fast alles ging in demo."});
    svc.put_portfolio(brief, false, false).unwrap();
    assert_eq!(svc.due(14, true).unwrap()["portfolio"]["due"], false);
    let ctx = svc.portfolio_context().unwrap();
    assert_eq!(
        (
            ctx["owner_ideas"][0]["checked"].as_bool(),
            ctx["review_week"]["due"].as_bool()
        ),
        (Some(true), Some(false))
    );

    // a later brief that week keeps the review; the next week needs a new one
    svc.put_portfolio(with(&check), false, false).unwrap();
    assert_eq!(
        svc.portfolio().unwrap()["portfolio"]["week_review"]["week"],
        "2026-W39"
    );
    clock.advance(Duration::days(7));
    assert_eq!(
        svc.portfolio_context().unwrap()["review_week"]["week"],
        "2026-W40"
    );
    assert_eq!(svc.portfolio_context().unwrap()["review_week"]["due"], true);

    assert_eq!(svc.remove_idea(id).unwrap()["removed"], id);
    assert_eq!(svc.remove_idea(id).unwrap()["already_removed"], true);
}

#[test]
fn visits_remember_the_previous_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    assert!(svc.state(true).unwrap()["previous_visit"].is_null());
    let start = svc.now();
    clock.advance(Duration::minutes(10));
    assert!(
        svc.state(true).unwrap()["previous_visit"].is_null(),
        "a reload is not a new visit"
    );
    clock.advance(Duration::hours(5));
    let previous = svc.state(true).unwrap()["previous_visit"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(previous > start);
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
        svc.portfolio_context().unwrap()["projects"][0]["card"]["outdated"],
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
    let path = tmp.path().join("watchtower.db");
    let writable = rusqlite::Connection::open(&path).unwrap();
    writable
        .execute_batch("CREATE TABLE example (id INTEGER);")
        .unwrap();
    let readonly =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let error = watchtower::WatchtowerError::from(
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
    let error = watchtower::WatchtowerError::from(
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
    let error = match watchtower::Watchtower::open(&damaged) {
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
    let old_brief_token = svc.portfolio_context().unwrap()["context_token"]
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
    assert!(!svc
        .store()
        .save_project_facts(
            &old_project,
            &svc.now(),
            &old_facts.fingerprint,
            &old_facts.data
        )
        .unwrap());
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
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
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
    let db = rusqlite::Connection::open(tmp.path().join("watchtower.db")).unwrap();
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

#[test]
fn owner_choices_show_up_in_the_window_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let sid = svc.put_card("demo", card(), false, false).unwrap()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    svc.set_feedback(&sid, Some("later"), Some("nach dem Release"))
        .unwrap();
    svc.set_pref("hidden_modules", &json!(["pulse"])).unwrap();
    assert_eq!(
        svc.set_status("demo", "sleeping", None).unwrap_err().code,
        "invalid_status"
    );
    assert_eq!(
        svc.set_pref("colour", &json!("red")).unwrap_err().code,
        "invalid_pref"
    );

    let state = svc.state(false).unwrap();
    let p = &state["projects"][0];
    assert_eq!(
        (
            p["card"]["headline"].as_str(),
            p["facts"]["is_git"].as_bool()
        ),
        (Some("Alles ruhig"), Some(true))
    );
    assert_eq!(
        (
            state["feedback"][&sid]["value"].as_str(),
            state["feedback"][&sid]["note"].as_str()
        ),
        (Some("later"), Some("nach dem Release"))
    );
    assert_eq!(
        state["prefs"],
        json!({"style": "handover", "hidden_modules": ["pulse"]})
    );
}

#[test]
fn a_ci_turning_red_or_green_makes_the_card_due() {
    use std::sync::{Arc, Mutex};
    let tmp = tempfile::tempdir().unwrap();
    let ci = Arc::new(Mutex::new("green"));
    let seen = ci.clone();
    let collect = move |path: &str, today, _ci| {
        let mut f = watchtower::facts::collect(path, today, false);
        f["ci"] = json!({"state": *seen.lock().unwrap()});
        f
    };
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z")
        .unwrap()
        .to_utc();
    let store = watchtower::store::Store::open(&tmp.path().join("watchtower.db")).unwrap();
    let svc = watchtower::Watchtower::with_parts(store, Box::new(move || now), Box::new(collect));
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let reasons = |svc: &watchtower::Watchtower| {
        svc.due(14, true).unwrap()["projects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["reason"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };

    // a lookup that settles nothing (running, unknown) never makes a card due
    *ci.lock().unwrap() = "running";
    assert!(reasons(&svc).is_empty());
    *ci.lock().unwrap() = "red";
    assert_eq!(reasons(&svc), ["ci_changed"]);
    assert_eq!(
        svc.store().facts("demo").unwrap().unwrap().fingerprint,
        svc.store()
            .latest_card("demo")
            .unwrap()
            .unwrap()
            .fingerprint,
        "CI changes need not change the repository fingerprint"
    );
    assert_eq!(
        svc.state(false).unwrap()["projects"][0]["card_outdated"],
        true
    );
    assert_eq!(
        svc.portfolio_context().unwrap()["projects"][0]["card"]["outdated"],
        true
    );
    svc.put_card("demo", card(), false, false).unwrap();
    assert!(reasons(&svc).is_empty());
    assert_eq!(
        svc.state(false).unwrap()["projects"][0]["card_outdated"],
        false
    );
    // a card written while CI was still running is due once it settles red, not green
    *ci.lock().unwrap() = "running";
    assert!(reasons(&svc).is_empty());
    svc.put_card("demo", card(), false, false).unwrap();
    *ci.lock().unwrap() = "green";
    assert!(reasons(&svc).is_empty());
    *ci.lock().unwrap() = "red";
    assert_eq!(reasons(&svc), ["ci_changed"]);
}

#[test]
fn owner_reactions_reach_agents_with_what_they_were_about() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.add_project(
        common::make_repo(tmp.path(), "other", &["2026-09-28T10:00:00"])
            .to_str()
            .unwrap(),
        None,
        None,
    )
    .unwrap();
    let saved = svc
        .put_card(
            "demo",
            card_with(json!({"waiting_on_you": [{"title": "Soll die App offline laufen?"}]})),
            false,
            false,
        )
        .unwrap();
    let id_of = |kind: &str| {
        saved["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["kind"] == kind)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (sid, qid) = (id_of("suggestion"), id_of("question"));
    svc.put_card("other", card(), false, false).unwrap();

    // a rating keeps what it was about and why, after the agent dropped the item, and reaches every project
    svc.set_feedback(&sid, Some("down"), Some("Keine UI-Spielereien"))
        .unwrap();
    svc.put_card("demo", card_with(json!({"suggestions": [], "waiting_on_you": [{"id": qid, "title": "Soll die App offline laufen?"}]})), false, false).unwrap();
    let ctx = svc.context("other").unwrap();
    let rating = &ctx["feedback"][0];
    assert_eq!(
        rating["item"],
        json!({"kind": "suggestion", "type": "feature", "title": "Dark Mode", "why": "Weil nachts.", "evidence": "", "effort": "S", "done_when": ""})
    );
    assert_eq!(
        (rating["project"].as_str(), rating["note"].as_str()),
        (Some("demo"), Some("Keine UI-Spielereien"))
    );
    assert_eq!(
        ctx["feedback_summary"],
        json!([{"kind": "suggestion", "type": "feature", "up": 0, "later": 0, "down": 1}])
    );

    // questions take answers, which make the card due and reach only their own project
    assert_eq!(
        svc.set_feedback(&qid, Some("up"), None).unwrap_err().code,
        "invalid_feedback"
    );
    assert_eq!(
        svc.set_feedback(&qid, Some("answered"), Some(" "))
            .unwrap_err()
            .code,
        "answer_missing"
    );
    assert_eq!(
        svc.set_feedback("sg-gone", Some("up"), None)
            .unwrap_err()
            .code,
        "unknown_item"
    );
    svc.set_feedback(&qid, Some("answered"), Some("Ja, unterwegs ohne Netz"))
        .unwrap();
    assert_eq!(
        svc.due(14, true).unwrap()["projects"],
        json!([{"id": "demo", "name": "demo", "reason": "answered"}])
    );
    assert_eq!(
        svc.state(false).unwrap()["projects"][0]["card_outdated"],
        true
    );
    assert_eq!(
        svc.context("demo").unwrap()["feedback"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == qid)
            .unwrap()["note"],
        "Ja, unterwegs ohne Netz"
    );
    assert_eq!(
        svc.context("other").unwrap()["feedback"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // An unchanged reply stays handled; withdrawing or editing it needs a new card.
    svc.put_card("demo", json!({}), true, false).unwrap();
    svc.set_feedback(&qid, Some("answered"), Some("Ja, unterwegs ohne Netz"))
        .unwrap();
    assert_eq!(svc.due(14, false).unwrap()["projects"], json!([]));
    svc.set_feedback(&qid, None, None).unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "answered"
    );
    svc.put_card("demo", json!({}), true, false).unwrap();
    svc.set_feedback(&qid, Some("answered"), Some("Doch nur online"))
        .unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["projects"][0]["reason"],
        "answered"
    );

    // a focus item can be marked done for the next brief
    let brief = json!({"greeting": "Moin", "focus": [{"project": "demo", "title": "Offline klären", "why": "Beantwortet.", "effort": "1h"}]});
    let fid = svc.put_portfolio(brief, false, false).unwrap()["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    svc.set_feedback(&fid, Some("done"), None).unwrap();
    let pctx = svc.portfolio_context().unwrap();
    assert!(pctx["feedback"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["id"] == fid && f["item"]["kind"] == "focus" && f["project"] == "demo"));

    // cards written before questions had ids get them when Watchtower opens
    let stored = svc.store().latest_card("other").unwrap().unwrap();
    let basis = watchtower::store::CardBasis {
        fingerprint: &stored.fingerprint,
        head: None,
        ci: None,
        parts: &json!(null),
        owner: None,
    };
    svc.store()
        .add_card(
            "other",
            &svc.now(),
            &basis,
            &card_with(json!({"waiting_on_you": [{"title": "Alt?"}]})),
        )
        .unwrap();
    let reopened = watchtower::Watchtower::open(&tmp.path().join("watchtower.db")).unwrap();
    assert!(
        reopened.card("other").unwrap()["card"]["waiting_on_you"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("q-")
    );
}
