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
    older["suggestions"][0].as_object_mut().unwrap().remove("category");
    svc.store().rewrite_latest_card("demo", &older).unwrap();
    let entry = json!({"id": "demo", "name": "demo", "reason": "new_fields", "new_fields": ["suggestions[].category", "suggestions[].done_when", "handbook"]});
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

    // Old documents can carry retired estimates; merging keeps their useful
    // content and item identity without rewriting the historical version.
    let mut legacy = current.clone();
    legacy["reentry"]["effort"] = json!("half-day");
    legacy["suggestions"][0]["effort"] = json!("S");
    svc.store().rewrite_latest_card("demo", &legacy).unwrap();
    let saved = svc
        .put_card("demo", json!({"headline": "Noch neuer"}), true, false)
        .unwrap();
    for field in ["reentry.effort", "suggestions[0].effort"] {
        assert!(saved["warnings"]
            .as_array()
            .unwrap()
            .contains(&json!(format!("ignored unknown field: {field}"))));
    }
    let new_card = &svc.card("demo").unwrap()["card"];
    let mut expected = current.clone();
    expected["headline"] = json!("Noch neuer");
    assert_eq!(new_card, &expected);
    assert_eq!(
        svc.store().card_version("demo", 2).unwrap().unwrap().data,
        legacy
    );
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
        "id": "shared-item", "title": "Andere Idee", "pitch": "Ein Werkzeug", "because": "Hilft"
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
        "id": "focus-owned", "project": "demo", "title": "Prüfen", "why": "Hilft"
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
            card_with(json!({"phase": "sleepy", "reentry": {}})),
            false,
            false,
        )
        .unwrap_err();
    assert_eq!(err.code, "invalid_card");
    assert!(
        err.message.contains("phase:")
            && err
                .message
                .contains("reentry.next_step: This field is required"),
        "{}",
        err.message
    );
    let empty_title = json!({"suggestions": [{"type": "feature", "title": "", "why": "w"}]});
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
            .contains("headline: Text has 11 words; limit is 10"),
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
    // Explicit invalid payload values never turn into deletions or fresh item identities.
    let kept_card = svc.card("demo").unwrap();
    let existing_id = kept_card["card"]["suggestions"][0]["id"].clone();
    for (payload, field) in [
        (json!({"suggestions": null}), "suggestions:"),
        (json!({"handbook": null}), "handbook:"),
        (json!({"stack": null}), "stack:"),
        (
            json!({"suggestions": [
                {"id": existing_id, "type": "feature", "title": "Erster Vorschlag", "why": "Erster Grund"},
                {"id": existing_id, "type": "feature", "title": "Zweiter Vorschlag", "why": "Zweiter Grund"}
            ]}),
            "suggestions[1].id:",
        ),
        (
            json!({"waiting_on_you": [{"id": existing_id, "title": "Frage"}]}),
            "waiting_on_you[0].id:",
        ),
    ] {
        for preview in [true, false] {
            let error = svc
                .put_card("demo", payload.clone(), true, preview)
                .unwrap_err();
            assert_eq!(error.code, "invalid_card");
            assert!(error.message.contains(field), "{error}");
            assert_eq!(svc.card("demo").unwrap(), kept_card);
        }
    }
    // Nullable slots retain their documented empty value.
    svc.put_card("demo", json!({"vision": {"statement": null}}), true, true)
        .unwrap();
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
            .contains("health_notes[0]: Text has 31 words; limit is 30"),
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
    let mut brief =
        json!({"greeting": "Moin", "focus": [{"project": "ghost", "title": "x", "why": "y"}]});
    let err = svc.put_portfolio(brief.clone(), false, false).unwrap_err();
    assert_eq!(err.code, "unknown_project");
    let four = json!({"greeting": "Moin", "focus": vec![brief["focus"][0].clone(); 4]});
    assert!(svc
        .put_portfolio(four, false, false)
        .unwrap_err()
        .message
        .contains("focus: List has 4 items; limit is 3"));
    assert!(err.hint.unwrap().contains("demo"));

    brief["focus"][0]["project"] = json!("demo");
    brief["synergies"] = json!([{"title":"Suche teilen", "why":"Leichter finden", "category":"idea", "projects":["demo"]}]);
    svc.put_portfolio(brief, false, false).unwrap();
    assert_eq!(svc.due(14, true).unwrap()["portfolio"]["due"], false);
    let mut old = svc.portfolio().unwrap()["portfolio"].clone();
    old["synergies"][0].as_object_mut().unwrap().remove("category");
    svc.store().rewrite_latest_portfolio(&old).unwrap();
    assert_eq!(svc.due(14, false).unwrap()["portfolio"]["reason"], "new_fields");
    assert_eq!(svc.portfolio_context().unwrap()["new_fields"], json!(["synergies[].category"]));
    old["synergies"][0]["category"] = json!("idea");
    svc.put_portfolio(old, false, false).unwrap();
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
    let changes = json!([
        {"date": "2026-09-20", "title": "Davor"},
        {"date": "2026-09-21", "title": "Wochenanfang", "detail": "Erste Funktion fertig."},
        {"date": "2026-09-27", "title": "Wochenende", "detail": "Release veröffentlicht."},
        {"date": "2026-09-28", "title": "Diese Woche"}
    ]);
    let milestones = json!([
        {"date": "2026-09-20", "title": "Davor"},
        {"date": "2026-09-27", "title": "Release"},
        {"date": "2026-09-28", "title": "Neuer Anfang"}
    ]);
    svc.put_card(
        "demo",
        card_with(json!({"recent_changes": changes, "milestones": milestones})),
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
    // Current context retains this week's changes; historical evidence is bounded on both sides.
    assert_eq!(
        ctx["projects"][0]["card"]["recent_changes"],
        json!([
            {"date": "2026-09-21", "title": "Wochenanfang"},
            {"date": "2026-09-27", "title": "Wochenende"},
            {"date": "2026-09-28", "title": "Diese Woche"}
        ])
    );
    assert_eq!(
        ctx["projects"][0]["review_week_evidence"],
        json!({
            "recent_changes": [
                {"date": "2026-09-21", "title": "Wochenanfang", "detail": "Erste Funktion fertig."},
                {"date": "2026-09-27", "title": "Wochenende", "detail": "Release veröffentlicht."}
            ],
            "milestones": [{"date": "2026-09-27", "title": "Release"}]
        })
    );
    assert_eq!(ctx["projects"][0]["facts"]["commits_review_week"], 0);

    let check =
        json!({"idea": id, "verdict": "fold_in", "fits": ["demo"], "why": "Passt in demo."});
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

    // A cached measurement from inside the review week cannot supply its final count.
    let ctx = svc.portfolio_context().unwrap();
    assert!(ctx["projects"][0]["facts"]["commits_review_week"].is_null());
    assert_eq!(
        ctx["projects"][0]["facts"]["collected_at"],
        "2026-09-30T08:00:00Z"
    );
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(
        svc.portfolio_context().unwrap()["projects"][0]["facts"]["commits_review_week"],
        1
    );

    assert_eq!(svc.remove_idea(id).unwrap()["removed"], id);
    assert_eq!(svc.remove_idea(id).unwrap()["already_removed"], true);
}

#[test]
fn portfolio_context_exposes_action_evidence_and_measurement_limits() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card(
        "demo",
        card_with(json!({
            "open_ends": [{"kind": "todo", "title": "Export fehlt", "detail": "Das Format ist noch offen.", "source": "TODO.md", "age_days": 2}],
            "waiting_on_you": [{"title": "Welches Format?", "source": "README.md", "options": [{"answer": "CSV", "effect": "Tabellen können die Daten lesen."}]}],
            "suggestions": [{"type": "next", "title": "Export umsetzen", "why": "Daten weiterverwenden.", "evidence": "src/export.rs", "done_when": "Die Beispieldatei lässt sich öffnen."}]
        })),
        false,
        false,
    )
    .unwrap();
    let stored_card = svc.card("demo").unwrap();
    let context = svc.portfolio_context().unwrap();
    let project = &context["projects"][0];
    assert_eq!(project["card"]["version"], stored_card["version"]);
    for field in ["open_ends", "waiting_on_you", "suggestions"] {
        assert_eq!(project["card"][field], stored_card["card"][field]);
    }
    assert_eq!(project["facts"]["ok"], true);
    assert!(project["facts"]["error"].is_null());
    assert_eq!(project["facts"]["collected_at"], svc.now());
    assert_eq!(project["facts"]["unmerged_branches"], 0);
    assert_eq!(project["facts"]["extra_worktrees"], 0);

    // A failed read must not masquerade as zero unfinished branches or worktrees.
    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    svc.update_project("demo", None, Some(plain.to_str().unwrap()))
        .unwrap();
    let context = svc.portfolio_context().unwrap();
    let measured = &context["projects"][0]["facts"];
    assert_eq!(measured["ok"], false);
    assert_eq!(measured["error"], "not_a_git_repository");
    assert_eq!(measured["collected_at"], svc.now());
    assert!(measured["unmerged_branches"].is_null());
    assert!(measured["extra_worktrees"].is_null());
    assert!(measured["commits_review_week"].is_null());
}

#[test]
fn current_contexts_hide_legacy_estimates_without_rewriting_evidence_or_history() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let mut legacy_card = svc.card("demo").unwrap()["card"].clone();
    legacy_card["reentry"]["effort"] = json!("half-day");
    legacy_card["suggestions"][0]["effort"] = json!("M");
    // Only the known obsolete slots are removed; arbitrary evidence stays intact.
    legacy_card["preserved_extension"] = json!({"effort": "this is application data"});
    legacy_card["suggestions"][0]["evidence"] = json!("docs/effort.md");
    svc.store()
        .rewrite_latest_card("demo", &legacy_card)
        .unwrap();
    let item = legacy_card["suggestions"][0]["id"].as_str().unwrap();
    svc.set_feedback(item, Some("up"), Some("Bitte behalten"))
        .unwrap();
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    let mut legacy_brief = svc.portfolio().unwrap()["portfolio"].clone();
    for field in ["focus", "synergies", "new_apps", "idea_checks"] {
        legacy_brief[field] =
            json!([{"id": field, "title": "Aufheben", "effort": "S", "evidence": "README.md"}]);
    }
    svc.store().rewrite_latest_portfolio(&legacy_brief).unwrap();
    let card_history = svc.store().card_history("demo").unwrap();
    let portfolio_history = || {
        svc.store()
            .portfolio_history()
            .unwrap()
            .into_iter()
            .map(|p| (p.version, p.created_at, p.data, p.basis))
            .collect::<Vec<_>>()
    };
    let original_portfolio_history = portfolio_history();
    let feedback = svc.feedback(None).unwrap();

    let card_context = svc.context("demo").unwrap();
    assert!(card_context["previous_card"]["card"]["reentry"]
        .get("effort")
        .is_none());
    assert!(card_context["previous_card"]["card"]["suggestions"][0]
        .get("effort")
        .is_none());
    assert_eq!(
        card_context["previous_card"]["card"]["preserved_extension"],
        legacy_card["preserved_extension"]
    );
    assert_eq!(
        card_context["previous_card"]["card"]["suggestions"][0]["evidence"],
        "docs/effort.md"
    );
    assert!(card_context["feedback"][0]["item"].get("effort").is_none());
    assert_eq!(card_context["feedback"][0]["note"], "Bitte behalten");
    let portfolio_context = svc.portfolio_context().unwrap();
    assert!(portfolio_context["projects"][0]["card"]["reentry"]
        .get("effort")
        .is_none());
    for field in ["focus", "synergies", "new_apps", "idea_checks"] {
        let item = &portfolio_context["previous_portfolio"]["portfolio"][field][0];
        assert!(item.get("effort").is_none());
        assert_eq!(item["evidence"], "README.md");
    }
    assert!(portfolio_context["feedback"][0]["item"]
        .get("effort")
        .is_none());
    assert_eq!(svc.card("demo").unwrap()["card"], legacy_card);
    assert_eq!(svc.portfolio().unwrap()["portfolio"], legacy_brief);
    assert_eq!(svc.store().card_history("demo").unwrap(), card_history);
    assert_eq!(portfolio_history(), original_portfolio_history);
    assert_eq!(svc.feedback(None).unwrap(), feedback);
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
fn the_data_version_moves_only_for_writes_of_other_connections() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let before = svc.data_version().unwrap();
    // Visits and the service's own writes are already part of what it returns.
    svc.state(true).unwrap();
    svc.set_pref("style", &json!("board")).unwrap();
    assert_eq!(svc.data_version().unwrap(), before);

    // An agent's CLI call writes through another connection.
    let (agent, _) = service(tmp.path());
    agent.add_idea("Von einem Agenten").unwrap();
    let (state, version) = svc.versioned_state(false).unwrap();
    assert_ne!(version, before);
    assert_eq!(state["ideas"][0]["text"], "Von einem Agenten");
    assert_eq!(svc.data_version().unwrap(), version);
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
fn idea_recency_uses_first_saved_identity_across_edits_and_returns() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let suggestion = |id: &str, title: &str| json!({"id": id, "type": "feature", "title": title, "why": "Hilft beim Wiedereinstieg."});
    let app = |id: &str, title: &str| json!({"id": id, "title": title, "pitch": "Offene Fragen sammeln.", "because": "Mehrere Projekte brauchen das."});
    let old_suggestion = suggestion("sg-first", "Erste Idee");
    let new_suggestion = suggestion("sg-second", "Zweite Idee");
    let old_app = app("ap-first", "Erste App");
    let new_app = app("ap-second", "Zweite App");
    svc.put_card(
        "demo",
        card_with(json!({"suggestions": [old_suggestion]})),
        false,
        false,
    )
    .unwrap();
    clock.advance(Duration::minutes(5));
    svc.put_portfolio(
        json!({"greeting": "Moin", "new_apps": [old_app]}),
        false,
        false,
    )
    .unwrap();
    let first_card_at = "2026-09-30T08:00:00Z";
    let first_app_at = "2026-09-30T08:05:00Z";
    let state = svc.state(false).unwrap();
    assert_eq!(
        state["projects"][0]["suggestion_first_seen"],
        json!({"sg-first": first_card_at})
    );
    assert_eq!(
        state["new_app_first_seen"],
        json!({"ap-first": first_app_at})
    );

    // Rewriting the title or authored order does not make the old idea newer.
    clock.advance(Duration::days(1));
    let second_at = "2026-10-01T08:05:00Z";
    svc.put_card(
        "demo",
        json!({"suggestions": [new_suggestion, suggestion("sg-first", "Präzisierte erste Idee")]}),
        true,
        false,
    )
    .unwrap();
    svc.put_portfolio(
        json!({"new_apps": [new_app, app("ap-first", "Präzisierte erste App")]}),
        true,
        false,
    )
    .unwrap();
    let state = svc.state(false).unwrap();
    let suggestion_dates = json!({"sg-first": first_card_at, "sg-second": second_at});
    let app_dates = json!({"ap-first": first_app_at, "ap-second": second_at});
    assert_eq!(
        state["projects"][0]["suggestion_first_seen"],
        suggestion_dates
    );
    assert_eq!(state["new_app_first_seen"], app_dates);

    // Only current items are sent to the window. Historical occurrences survive
    // disappearance and even a rename or removal of the watched project.
    clock.advance(Duration::days(1));
    svc.put_card("demo", json!({"suggestions": []}), true, false)
        .unwrap();
    svc.put_portfolio(json!({"new_apps": []}), true, false)
        .unwrap();
    let state = svc.state(false).unwrap();
    assert_eq!(state["projects"][0]["suggestion_first_seen"], json!({}));
    assert_eq!(state["new_app_first_seen"], json!({}));
    svc.rename_project("demo", "Anderer Name").unwrap();
    svc.remove_project("demo").unwrap();
    svc.restore_project("demo").unwrap();

    clock.advance(Duration::days(1));
    svc.put_card(
        "demo",
        json!({"suggestions": [old_suggestion, new_suggestion]}),
        true,
        false,
    )
    .unwrap();
    svc.put_portfolio(json!({"new_apps": [old_app, new_app]}), true, false)
        .unwrap();
    let card_history = svc.store().card_history("demo").unwrap();
    let portfolio_version = svc.store().latest_portfolio().unwrap().unwrap();
    drop(svc);

    // Existing databases need no backfill, and deriving display metadata must
    // neither rewrite documents nor change their authored array order.
    let (svc, _) = service(tmp.path());
    let state = svc.state(false).unwrap();
    assert_eq!(
        state["projects"][0]["suggestion_first_seen"],
        suggestion_dates
    );
    assert_eq!(state["new_app_first_seen"], app_dates);
    assert_eq!(
        state["projects"][0]["card"]["suggestions"][0]["id"],
        "sg-first"
    );
    assert_eq!(state["portfolio"]["new_apps"][0]["id"], "ap-first");
    assert_eq!(svc.store().card_history("demo").unwrap(), card_history);
    let portfolio_after = svc.store().latest_portfolio().unwrap().unwrap();
    assert_eq!(portfolio_after.version, portfolio_version.version);
    assert_eq!(portfolio_after.data, portfolio_version.data);
}

/// First-seen dates and last authored snapshots by a walk over every saved
/// version: watched projects' cards, then the briefs, then the cards of removed
/// projects that ratings or tasks name. Each item snapshot comes from its first list.
fn walked_items(
    svc: &watchtower::Watchtower,
) -> (
    std::collections::HashMap<String, String>,
    std::collections::HashMap<String, Value>,
) {
    let store = svc.store();
    let card_lists = [
        ("suggestions", "suggestion"),
        ("waiting_on_you", "question"),
    ];
    let brief_lists = [
        ("focus", "focus"),
        ("patterns", "pattern"),
        ("synergies", "synergy"),
        ("new_apps", "new_app"),
        ("park_or_revive", "verdict"),
    ];
    let (mut first, mut items) = Default::default();
    let mut remember = |lists: &[(&str, &str)], doc: &Value, at: &str| {
        let (first, items): (
            &mut std::collections::HashMap<String, String>,
            &mut std::collections::HashMap<String, Value>,
        ) = (&mut first, &mut items);
        for (list, _) in lists {
            for entry in doc[*list].as_array().into_iter().flatten() {
                let Some(id) = entry["id"].as_str() else {
                    continue;
                };
                first.entry(id.to_owned()).or_insert_with(|| at.to_owned());
                let (mut snapshot, kind) = lists
                    .iter()
                    .find_map(|(list, kind)| {
                        let found = doc[*list].as_array()?.iter().find(|e| e["id"] == id)?;
                        Some((found.clone(), *kind))
                    })
                    .unwrap();
                snapshot.as_object_mut().unwrap().remove("id");
                snapshot["kind"] = json!(kind);
                items.insert(id.to_owned(), snapshot);
            }
        }
    };
    let watched: Vec<String> = store
        .projects()
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    for id in &watched {
        for (at, doc) in store.card_history(id).unwrap() {
            remember(&card_lists, &doc, &at);
        }
    }
    for brief in store.portfolio_history().unwrap() {
        remember(&brief_lists, &brief.data, &brief.created_at);
    }
    let named: std::collections::BTreeSet<String> = store
        .feedback()
        .unwrap()
        .into_iter()
        .filter_map(|f| f.project_id)
        .chain(store.tasks().unwrap().iter().flat_map(|task| {
            task["projects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        }))
        .filter(|id| !watched.contains(id))
        .collect();
    for id in &named {
        for (at, doc) in store.card_history(id).unwrap() {
            remember(&card_lists, &doc, &at);
        }
    }
    (first, items)
}

/// The window state's dates, card histories and task sources equal a walk
/// over every saved version.
fn assert_state_matches_full_walk(svc: &watchtower::Watchtower) -> Value {
    let state = svc.state(false).unwrap();
    let store = svc.store();
    let (first, items) = walked_items(svc);
    let date = |id: &str| first.get(id).map_or(Value::Null, |at| json!(at));
    for project in state["projects"].as_array().unwrap() {
        let id = project["id"].as_str().unwrap();
        let versions = store.card_history(id).unwrap();
        let current: Vec<&str> = project["card"]["suggestions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["id"].as_str())
            .collect();
        let mut dates = serde_json::Map::new();
        for (at, doc) in &versions {
            for s in doc["suggestions"].as_array().into_iter().flatten() {
                if let Some(sid) = s["id"].as_str().filter(|sid| current.contains(sid)) {
                    dates.entry(sid).or_insert(json!(at));
                }
            }
        }
        assert_eq!(
            project["suggestion_first_seen"],
            Value::Object(dates),
            "{id}"
        );
        let history: Vec<Value> = versions
            .iter()
            .enumerate()
            .map(|(i, (at, doc))| json!({"version": i + 1, "at": at, "headline": doc["headline"], "phase": doc["phase"]}))
            .rev()
            .collect();
        assert_eq!(project["history"], json!(history), "{id}");
    }
    let brief = store
        .latest_portfolio()
        .unwrap()
        .map_or(Value::Null, |b| b.data);
    for (list, field) in [
        ("new_apps", "new_app_first_seen"),
        ("synergies", "synergy_first_seen"),
    ] {
        let dates: serde_json::Map<String, Value> = brief[list]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| e["id"].as_str())
            .map(|id| (id.to_owned(), date(id)))
            .collect();
        assert_eq!(state[field], Value::Object(dates), "{field}");
    }
    let feedback = store.feedback().unwrap();
    for f in &feedback {
        let expected = f
            .item
            .as_ref()
            .filter(|i| i["kind"] == "owner_idea")
            .map_or_else(|| date(&f.item_id), |i| i["added_at"].clone());
        assert_eq!(
            state["feedback"][&f.item_id]["first_seen"], expected,
            "{}",
            f.item_id
        );
    }
    let tasks = store.tasks().unwrap();
    assert_eq!(state["tasks"].as_array().unwrap().len(), tasks.len());
    for (shown, task) in state["tasks"].as_array().unwrap().iter().zip(&tasks) {
        let source = task["source_id"].as_str().unwrap_or("");
        let snapshot = task
            .get("source_snapshot")
            .filter(|s| !s.is_null())
            .cloned()
            .or_else(|| {
                feedback
                    .iter()
                    .find(|f| f.item_id == source)
                    .and_then(|f| f.item.clone())
            })
            .or_else(|| items.get(source).cloned())
            .unwrap_or(Value::Null);
        let expected = if snapshot["kind"] == "owner_idea" {
            snapshot["added_at"].clone()
        } else {
            date(source)
        };
        assert_eq!(shown["source_snapshot"], snapshot, "{source}");
        assert_eq!(shown["source_kind"], snapshot["kind"], "{source}");
        assert_eq!(shown["source_first_seen"], expected, "{source}");
    }
    state
}

#[test]
fn state_dates_and_task_sources_match_a_walk_over_every_saved_version() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    for name in ["demo", "other"] {
        let repo = common::make_repo(tmp.path(), name, &["2026-09-29T10:00:00"]);
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    }
    let suggestion = |id: &str, title: &str| json!({"id": id, "type": "feature", "title": title, "why": "Hilft beim Wiedereinstieg."});
    let question = |id: &str| json!({"id": id, "title": format!("Frage {id}?")});
    let app = |id: &str| json!({"id": id, "title": format!("App {id}"), "pitch": "Offene Fragen sammeln.", "because": "Mehrere Projekte brauchen das."});
    let synergy = |id: &str| json!({"id": id, "title": format!("Synergie {id}"), "why": "Beide brauchen es.", "projects": ["demo", "other"]});
    let tick = || clock.advance(Duration::hours(1));

    svc.put_card("demo", card_with(json!({"headline": "Erster Stand", "suggestions": [suggestion("sg-a", "Idee A")], "waiting_on_you": [question("q-1")]})), false, false)
        .unwrap();
    tick();
    svc.put_card("other", card_with(json!({"suggestions": [suggestion("sg-x", "Idee X")], "waiting_on_you": [question("q-x")]})), false, false)
        .unwrap();
    tick();
    svc.put_portfolio(
        json!({"greeting": "Moin", "new_apps": [app("ap-1")], "synergies": [synergy("sy-1")]}),
        false,
        false,
    )
    .unwrap();
    assert_state_matches_full_walk(&svc);

    // Ratings, edits, dropped items and tasks without a saved source snapshot.
    tick();
    svc.set_feedback("sg-x", Some("up"), None).unwrap();
    svc.set_feedback("ap-1", Some("up"), None).unwrap();
    let idea = svc.add_idea("Ein Werkzeug für Notizen").unwrap();
    let idea = format!("idea:{}", idea["id"]);
    svc.set_feedback(&idea, Some("up"), None).unwrap();
    tick();
    svc.put_card("demo", json!({"headline": "Zweiter Stand", "suggestions": [suggestion("sg-b", "Idee B"), suggestion("sg-a", "Präzisierte Idee A")], "waiting_on_you": []}), true, false)
        .unwrap();
    svc.put_card(
        "other",
        json!({"suggestions": [], "waiting_on_you": []}),
        true,
        false,
    )
    .unwrap();
    for (project, source) in [
        ("demo", "q-1"),
        ("other", "q-x"),
        ("other", "sg-x"),
        ("demo", idea.as_str()),
    ] {
        svc.create_task(json!({"projects": [project], "source_id": source, "title": format!("{source} erledigen")}))
            .unwrap();
    }
    assert_state_matches_full_walk(&svc);

    // The newest versions can be rewritten in place, even with an id the brief
    // shares with a card: cards count first, the brief's snapshot last.
    tick();
    let mut latest = svc.store().latest_card("demo").unwrap().unwrap().data;
    latest["suggestions"]
        .as_array_mut()
        .unwrap()
        .push(suggestion("sg-c", "Idee C"));
    latest["headline"] = json!("Umgeschrieben");
    svc.store().rewrite_latest_card("demo", &latest).unwrap();
    let mut brief = svc.store().latest_portfolio().unwrap().unwrap().data;
    for id in ["sg-b", "sy-2"] {
        brief["synergies"].as_array_mut().unwrap().push(synergy(id));
    }
    svc.store().rewrite_latest_portfolio(&brief).unwrap();
    svc.create_task(json!({"projects": ["demo"], "source_id": "sg-b", "title": "Idee B bauen"}))
        .unwrap();
    let rewritten = assert_state_matches_full_walk(&svc);
    assert_eq!(
        rewritten["synergy_first_seen"],
        json!({"sy-1": "2026-09-30T10:00:00Z", "sg-b": "2026-09-30T12:00:00Z", "sy-2": "2026-09-30T10:00:00Z"})
    );
    assert_eq!(
        rewritten["projects"][0]["suggestion_first_seen"]["sg-c"],
        "2026-09-30T12:00:00Z"
    );

    // A returning idea keeps its first date; a removed project's history still counts.
    tick();
    svc.put_card("demo", json!({"suggestions": []}), true, false)
        .unwrap();
    tick();
    svc.put_card(
        "demo",
        json!({"suggestions": [suggestion("sg-a", "Idee A ist zurück")]}),
        true,
        false,
    )
    .unwrap();
    svc.put_portfolio(
        json!({"new_apps": [app("ap-2"), app("ap-1")], "synergies": []}),
        true,
        false,
    )
    .unwrap();
    svc.remove_project("other").unwrap();
    let warm = assert_state_matches_full_walk(&svc);
    assert_eq!(
        warm["projects"][0]["suggestion_first_seen"],
        json!({"sg-a": "2026-09-30T08:00:00Z"})
    );
    assert_eq!(warm["projects"][0]["history"][0]["version"], 4);
    assert_eq!(
        warm["projects"][0]["history"][2]["headline"],
        "Umgeschrieben"
    );
    assert_eq!(
        warm["feedback"]["sg-x"]["first_seen"],
        "2026-09-30T09:00:00Z"
    );
    let source = |id: &str| {
        warm["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["source_id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(source("q-x")["source_kind"], "question");
    assert_eq!(source("q-x")["source_first_seen"], "2026-09-30T09:00:00Z");
    assert_eq!(source("q-1")["source_snapshot"]["title"], "Frage q-1?");
    assert_eq!(source("sg-b")["source_kind"], "suggestion");
    assert_eq!(source(&idea)["source_kind"], "owner_idea");

    // A fresh service derives the same state as the one that read every step.
    let elapsed = chrono::DateTime::parse_from_rfc3339(warm["now"].as_str().unwrap()).unwrap()
        - chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z").unwrap();
    drop(svc);
    let (svc, clock) = service(tmp.path());
    clock.advance(elapsed);
    assert_eq!(assert_state_matches_full_walk(&svc), warm);
}

#[test]
fn a_ci_turning_red_or_green_makes_the_card_due() {
    use std::sync::{Arc, Mutex};
    let tmp = tempfile::tempdir().unwrap();
    let ci = Arc::new(Mutex::new("green"));
    let seen = ci.clone();
    let collect = move |path: &str, today, _ci, previous: Option<&serde_json::Value>| {
        let mut f = watchtower::facts::collect_reusing(path, today, false, previous);
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
        json!({"kind": "suggestion", "category": "idea", "type": "feature", "title": "Dark Mode", "why": "Weil nachts.", "evidence": "", "done_when": ""})
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
    let brief = json!({"greeting": "Moin", "focus": [{"project": "demo", "title": "Offline klären", "why": "Beantwortet."}]});
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

#[test]
fn contexts_report_how_long_risks_and_items_have_lasted() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    std::fs::write(repo.join("draft.txt"), "one\n").unwrap();
    svc.refresh_facts(None, false).unwrap();
    let question = json!([{"title": "Welches Format?", "options": [{"answer": "CSV"}, {"answer": "JSON"}], "recommended": 2}]);
    svc.put_card("demo", card_with(json!({"waiting_on_you": question})), false, false)
        .unwrap();
    let id = svc.card("demo").unwrap()["card"]["waiting_on_you"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(svc.card("demo").unwrap()["card"]["waiting_on_you"][0]["recommended"], 2);

    clock.advance(Duration::days(3));
    std::fs::write(repo.join("draft.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(repo.join("more.txt"), "four\n").unwrap();
    svc.refresh_facts(None, false).unwrap();
    let mut kept = card_with(json!({"waiting_on_you": [{"id": id, "title": "Welches Format?"}]}));
    kept["open_ends"] = json!([{"kind": "uncommitted", "title": "Entwurf nicht committet"}]);
    svc.put_card("demo", kept, false, false).unwrap();

    let context = svc.context("demo").unwrap();
    let streak = &context["risk_streaks"][0];
    assert_eq!(streak["kind"], "uncommitted");
    assert_eq!(streak["days"], 3);
    // The clean measurement at registration bounds the run.
    assert_eq!(streak["at_least"], false);
    assert_eq!(streak["then"]["files"], 1);
    assert_eq!(streak["now"]["files"], 2);
    assert_eq!(streak["trend"], "growing");
    let items = context["item_persistence"].as_array().unwrap();
    let asked = items.iter().find(|i| i["id"] == id.as_str()).unwrap();
    assert_eq!((asked["kind"].as_str(), asked["days"].as_i64(), asked["versions"].as_i64()), (Some("question"), Some(3), Some(2)));
    let open_end = items.iter().find(|i| i["kind"] == "open_end").unwrap();
    assert_eq!((open_end["days"].as_i64(), open_end["versions"].as_i64()), (Some(0), Some(1)));

    // Committing ends the run; the brief sees long-standing items and the owner's silence.
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "draft"]);
    clock.advance(Duration::days(5));
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(svc.context("demo").unwrap()["risk_streaks"], json!([]));
    let brief = svc.portfolio_context().unwrap();
    let standing = &brief["projects"][0]["long_standing_items"];
    assert!(standing.as_array().unwrap().iter().any(|i| i["id"] == id.as_str() && i["days"] == 8));
    assert_eq!(brief["owner_response"]["open_questions"], 1);
    assert_eq!(brief["owner_response"]["oldest_open_question_days"], 8);
    assert_eq!(brief["owner_response"]["reactions_last_14_days"], 0);
}

#[test]
fn questions_can_wait_and_cards_keep_decisions_short() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let wrong = json!([{"title": "Welches Format?", "options": [{"answer": "CSV"}], "recommended": 2}]);
    let error = svc
        .put_card("demo", card_with(json!({"waiting_on_you": wrong})), false, false)
        .unwrap_err();
    assert_eq!(error.code, "invalid_card");
    assert!(error.message.contains("waiting_on_you[0].recommended"), "{}", error.message);
    let four = json!((0..4).map(|i| json!({"title": format!("Frage {i}?")})).collect::<Vec<_>>());
    let error = svc
        .put_card("demo", card_with(json!({"waiting_on_you": four})), false, false)
        .unwrap_err();
    assert!(error.message.contains("limit is 3"), "{}", error.message);

    svc.put_card("demo", card_with(json!({"waiting_on_you": [{"title": "Welches Format?"}]})), false, false)
        .unwrap();
    let id = svc.card("demo").unwrap()["card"]["waiting_on_you"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    svc.set_feedback_until(&id, Some("later"), None, Some("2026-10-07"))
        .unwrap();
    let state = svc.state(false).unwrap();
    assert_eq!(state["feedback"][&id]["snoozed_until"], "2026-10-07");
    // Postponing is not an answer, so the card does not become due for it.
    assert_eq!(svc.due(30, false).unwrap()["projects"], json!([]));
}
