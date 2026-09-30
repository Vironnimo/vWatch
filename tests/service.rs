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
    let prose = json!({"handbook": {"run": [{"what": "Tests", "how": "Im Ordner: `cargo test`"}]}});
    let r = svc.put_card("demo", prose, true, true).unwrap();
    assert!(
        r["warnings"].to_string().contains("backticks"),
        "{}",
        r["warnings"]
    );
    clock.advance(Duration::days(15));
    assert_eq!(svc.due(14, true).unwrap()["projects"][0]["reason"], "stale");
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
            card_with(json!({"features": ["Suche"]})),
            false,
            false,
        )
        .unwrap();
    let sid = first["items"][0]["id"].as_str().unwrap().to_owned();

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
}

#[test]
fn invalid_cards_are_refused_with_field_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
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
    clock.advance(Duration::minutes(5));
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
    assert_eq!(svc.remove_idea(id).unwrap_err().code, "idea_not_found");
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
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    let other = common::make_repo(tmp.path(), "Other App", &["2026-09-20T10:00:00"]);
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    assert_eq!(
        svc.add_project(repo.to_str().unwrap(), None, None)
            .unwrap_err()
            .code,
        "already_registered"
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
    assert_eq!(batch["failed"][0]["error"]["code"], "already_registered");

    svc.put_card("demo", card(), false, false).unwrap();
    // versions count per project
    assert_eq!(
        svc.put_card("other-app", card(), false, false).unwrap()["version"],
        1
    );
    clock.advance(Duration::minutes(1));
    svc.set_status("demo", "parked", Some("wartet auf Key"))
        .unwrap();
    let due = svc.due(14, true).unwrap();
    assert!(due["projects"]
        .as_array()
        .unwrap()
        .contains(&json!({"id": "demo", "name": "demo", "reason": "status_changed"})));
    // a goal reaches the agents and makes the card due like a status change
    svc.put_card("demo", card(), true, false).unwrap();
    clock.advance(Duration::minutes(1));
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
    svc.set_status("demo", "archived", None).unwrap();
    let due = svc.due(14, true).unwrap();
    assert!(due["projects"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["id"] != "demo"));
    assert_eq!(
        svc.store().project("demo").unwrap().unwrap().status_note,
        "wartet auf Key"
    );
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
    svc.put_card("demo", card(), false, false).unwrap();
    assert!(reasons(&svc).is_empty());
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
    let (svc, clock) = service(tmp.path());
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
    clock.advance(Duration::minutes(1));
    svc.set_feedback(&qid, Some("answered"), Some("Ja, unterwegs ohne Netz"))
        .unwrap();
    assert_eq!(
        svc.due(14, true).unwrap()["projects"],
        json!([{"id": "demo", "name": "demo", "reason": "answered"}])
    );
    assert_eq!(
        svc.context("demo").unwrap()["feedback"][0]["note"],
        "Ja, unterwegs ohne Netz"
    );
    assert_eq!(
        svc.context("other").unwrap()["feedback"]
            .as_array()
            .unwrap()
            .len(),
        1
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
