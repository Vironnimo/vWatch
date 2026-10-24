use super::*;

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
        svc.portfolio_context(&[]).unwrap()["projects"][0]["card"]["outdated"],
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
    // a card written before vWatch added a field is due once to fill it
    let mut older = svc.card("demo").unwrap()["card"].clone();
    older.as_object_mut().unwrap().remove("handbook");
    older["suggestions"][0]
        .as_object_mut()
        .unwrap()
        .remove("done_when");
    older["suggestions"][0]
        .as_object_mut()
        .unwrap()
        .remove("category");
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
    old["synergies"][0]
        .as_object_mut()
        .unwrap()
        .remove("category");
    svc.store().rewrite_latest_portfolio(&old).unwrap();
    assert_eq!(
        svc.due(14, false).unwrap()["portfolio"]["reason"],
        "new_fields"
    );
    assert_eq!(
        svc.portfolio_context(&[]).unwrap()["new_fields"],
        json!(["synergies[].category"])
    );
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
        svc.portfolio_context(&[]).unwrap()["projects"][0]["card"]["changed_since_brief"],
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
    let ctx = svc.portfolio_context(&[]).unwrap();
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
    let ctx = svc.portfolio_context(&[]).unwrap();
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
        svc.portfolio_context(&[]).unwrap()["review_week"]["week"],
        "2026-W40"
    );
    assert_eq!(
        svc.portfolio_context(&[]).unwrap()["review_week"]["due"],
        true
    );
    // A cached measurement from inside the review week cannot supply its final count.
    let ctx = svc.portfolio_context(&[]).unwrap();
    assert!(ctx["projects"][0]["facts"]["commits_review_week"].is_null());
    assert_eq!(
        ctx["projects"][0]["facts"]["collected_at"],
        "2026-09-30T08:00:00Z"
    );
    svc.refresh_facts(None, false).unwrap();
    assert_eq!(
        svc.portfolio_context(&[]).unwrap()["projects"][0]["facts"]["commits_review_week"],
        1
    );
    assert_eq!(svc.remove_idea(id).unwrap()["removed"], id);
    assert_eq!(svc.remove_idea(id).unwrap()["already_removed"], true);
}

#[test]
fn quiet_active_projects_take_turns_for_an_idea_review() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    for name in ["alpha", "beta", "gamma", "delta"] {
        let repo = common::make_repo(tmp.path(), name, &["2026-09-29T10:00:00"]);
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
        svc.put_card(name, card(), false, false).unwrap();
    }
    svc.set_status("delta", "parked", None).unwrap();
    svc.put_card("delta", card(), false, false).unwrap();
    let reviews = |svc: &vwatch::VWatch| -> Vec<String> {
        svc.due(14, false).unwrap()["projects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["reason"] == "idea_review")
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(reviews(&svc).is_empty(), "fresh cards wait");
    // A week later two active projects are due for new ideas; saving them makes no other one due that day.
    clock.advance(Duration::days(7));
    let first = reviews(&svc);
    assert_eq!(first.len(), 2);
    assert!(
        !first.contains(&"delta".to_owned()),
        "parked projects are left alone"
    );
    for id in &first {
        svc.put_card(id, card(), false, false).unwrap();
    }
    assert!(reviews(&svc).is_empty());
    // The rotation moves on the next day to the project still waiting.
    clock.advance(Duration::days(1));
    let next = reviews(&svc);
    assert!(
        !next.is_empty() && next.iter().all(|id| !first.contains(id)),
        "{next:?}"
    );
}

#[test]
fn unfinished_focus_survives_a_rewrite_in_the_same_clock_tick() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let written = svc
        .put_portfolio(
            json!({"greeting":"Moin", "focus":[{
                "project":"demo", "title":"Export prüfen", "why":"Fehlende Zeilen finden"
            }]}),
            false,
            false,
        )
        .unwrap();
    let id = written["portfolio"]["focus"][0]["id"].as_str().unwrap();
    assert_eq!(
        svc.state(false).unwrap()["brief_persistence"]["dropped_focus"],
        json!([])
    );
    svc.put_portfolio(json!({"focus":[]}), true, false).unwrap();
    let dropped = svc.state(false).unwrap()["brief_persistence"]["dropped_focus"].clone();
    assert_eq!(dropped.as_array().unwrap().len(), 1);
    assert_eq!(dropped[0]["id"], id);
    assert_eq!(
        svc.portfolio_context(&[]).unwrap()["brief_persistence"]["dropped_focus"],
        dropped
    );
    svc.put_portfolio(json!({"focus":[{"id":id}]}), true, false)
        .unwrap();
    assert_eq!(
        svc.state(false).unwrap()["brief_persistence"]["dropped_focus"],
        json!([])
    );
    // Rating the focus while current keeps its completion when it leaves again.
    svc.set_feedback(id, Some("done"), None).unwrap();
    svc.put_portfolio(json!({"focus":[]}), true, false).unwrap();
    assert_eq!(
        svc.state(false).unwrap()["brief_persistence"]["dropped_focus"],
        json!([])
    );
}

#[test]
fn a_ci_turning_red_or_green_makes_the_card_due() {
    use std::sync::{Arc, Mutex};
    let tmp = tempfile::tempdir().unwrap();
    let ci = Arc::new(Mutex::new("green"));
    let seen = ci.clone();
    let collect = move |_path: &str, _today, _ci, _previous: Option<&serde_json::Value>| {
        let mut f = common::measured_facts(1);
        f["ci"] = json!({"state": *seen.lock().unwrap()});
        f
    };
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z")
        .unwrap()
        .to_utc();
    let store = vwatch::store::Store::open(&tmp.path().join("vwatch.db")).unwrap();
    let svc = vwatch::VWatch::with_parts(store, Box::new(move || now), Box::new(collect));
    let repo = tmp.path().join("demo");
    std::fs::create_dir(&repo).unwrap();
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let reasons = |svc: &vwatch::VWatch| {
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
        svc.portfolio_context(&[]).unwrap()["projects"][0]["card"]["outdated"],
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
