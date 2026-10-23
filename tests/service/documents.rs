use super::*;

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
fn kept_items_need_only_their_id_and_receipts_name_each_change() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let two = json!([
        {"type": "feature", "title": "Dark Mode", "why": "Weil nachts.", "evidence": "ui/app.css"},
        {"type": "cleanup", "title": "Logs kürzen", "why": "Sie wachsen."}
    ]);
    let saved = svc
        .put_card("demo", card_with(json!({"suggestions": two})), false, false)
        .unwrap();
    let changes = |saved: &Value| {
        saved["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["change"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(changes(&saved), [json!("new"), json!("new")]);
    let (dark, logs) = (
        saved["items"][0]["id"].clone(),
        saved["items"][1]["id"].clone(),
    );
    let original = svc.card("demo").unwrap()["card"]["suggestions"][0].clone();
    // An id alone keeps the saved wording; a sent item with its id is an edit.
    let edited = json!({"suggestions": [{"id": dark}, {"id": logs, "type": "cleanup", "title": "Logs rotieren", "why": "Sie wachsen."}]});
    let dry = svc.put_card("demo", edited.clone(), true, true).unwrap();
    assert_eq!(changes(&dry), [json!("kept"), json!("edited")]);
    let saved = svc.put_card("demo", edited, true, false).unwrap();
    assert_eq!(changes(&saved), [json!("kept"), json!("edited")]);
    assert_eq!(
        svc.card("demo").unwrap()["card"]["suggestions"][0],
        original
    );
    // An item left out of the current card can come back by its id.
    svc.put_card("demo", json!({"suggestions": [{"id": logs}]}), true, false)
        .unwrap();
    let saved = svc
        .put_card(
            "demo",
            json!({"suggestions": [{"id": logs}, {"id": dark}]}),
            true,
            false,
        )
        .unwrap();
    assert_eq!(changes(&saved), [json!("kept"), json!("restored")]);
    assert_eq!(
        svc.card("demo").unwrap()["card"]["suggestions"][1],
        original
    );
    // An id no saved version has is refused when it stands alone and reported when it carries content.
    let error = svc
        .put_card(
            "demo",
            json!({"suggestions": [{"id": "sg-invented"}]}),
            true,
            false,
        )
        .unwrap_err();
    assert_eq!(error.code, "invalid_card");
    assert!(
        error.message.contains("suggestions[0].id"),
        "{}",
        error.message
    );
    let question = json!({"waiting_on_you": [{"title": "Welches Format?"}]});
    let asked = svc.put_card("demo", question, true, false).unwrap();
    let qid = asked["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "question")
        .unwrap()["id"]
        .clone();
    let error = svc
        .put_card("demo", json!({"suggestions": [{"id": qid}]}), true, false)
        .unwrap_err();
    assert_eq!(error.code, "invalid_card");
    let invented = json!({"suggestions": [{"id": "sg-invented", "type": "feature", "title": "Export", "why": "Gewünscht."}]});
    let saved = svc.put_card("demo", invented, true, false).unwrap();
    assert!(
        saved["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("sg-invented")),
        "{saved}"
    );
    // The brief keeps its items the same way.
    let brief =
        json!({"greeting": "Moin", "patterns": [{"title": "Muster", "why": "Überall gleich."}]});
    let saved = svc.put_portfolio(brief, false, false).unwrap();
    assert_eq!(changes(&saved), [json!("new")]);
    let pattern = saved["items"][0]["id"].clone();
    let kept = svc
        .put_portfolio(json!({"patterns": [{"id": pattern}]}), true, false)
        .unwrap();
    assert_eq!(changes(&kept), [json!("kept")]);
    assert_eq!(
        svc.portfolio().unwrap()["portfolio"]["patterns"][0]["why"],
        "Überall gleich."
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
    for (id, removed) in [("unrated-omitted", false), ("removed-source", true)] {
        let proposal = card_with(
            json!({"suggestions":[{"id":id,"type":"feature","title":"Reserviert","why":"Historische Identität"}]}),
        );
        svc.put_card("demo", proposal.clone(), false, false)
            .unwrap();
        if removed {
            svc.remove_project("demo").unwrap();
        } else {
            svc.put_card("demo", json!({"suggestions":[]}), true, false)
                .unwrap();
        }
        for dry_run in [true, false] {
            let error = svc
                .put_card("second", proposal.clone(), false, dry_run)
                .unwrap_err();
            assert_eq!(error.code, "invalid_card");
            assert!(error.message.contains(id));
        }
        if removed {
            svc.restore_project("demo").unwrap();
        }
    }
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
    let context = svc.portfolio_context(&[]).unwrap();
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
    assert_eq!(project["card"]["detail"], "full");
    // Once a brief built on the card, the card is a summary until it changes; detail returns it in full.
    svc.put_portfolio(json!({"greeting": "Moin"}), false, false)
        .unwrap();
    let context = svc.portfolio_context(&[]).unwrap();
    let summary = &context["projects"][0]["card"];
    assert_eq!(summary["detail"], "summary");
    assert_eq!(summary["suggestions"][0]["title"], "Export umsetzen");
    assert!(summary["suggestions"][0].get("evidence").is_none());
    assert_eq!(summary["waiting_on_you"][0]["title"], "Welches Format?");
    assert!(summary["waiting_on_you"][0].get("options").is_none());
    let detail = svc.portfolio_context(&["demo".to_owned()]).unwrap();
    assert_eq!(detail["projects"][0]["card"]["detail"], "full");
    assert_eq!(
        detail["projects"][0]["card"]["suggestions"],
        stored_card["card"]["suggestions"]
    );
    assert_eq!(detail["context_token"], context["context_token"]);
    assert!(detail.get("previous_portfolio").is_none());
    assert_eq!(
        svc.portfolio_context(&["missing".to_owned()])
            .unwrap_err()
            .code,
        "project_not_found"
    );
    // A failed read must not masquerade as zero unfinished branches or worktrees.
    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    svc.update_project("demo", None, Some(plain.to_str().unwrap()))
        .unwrap();
    let context = svc.portfolio_context(&[]).unwrap();
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
    let portfolio_context = svc.portfolio_context(&[]).unwrap();
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
