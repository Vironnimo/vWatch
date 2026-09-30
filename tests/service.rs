mod common;

use chrono::Duration;
use common::{card, card_with, demo_repo, git, service};
use serde_json::json;

#[test]
fn morning_run_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();

    let due = svc.due(14, true).unwrap();
    assert_eq!(due["projects"], json!([{"id": "demo", "name": "demo", "reason": "no_card"}]));
    assert_eq!(due["portfolio"]["due"], true);

    let saved = svc.put_card("demo", card(), false, false).unwrap();
    assert!(saved["items"][0]["id"].as_str().unwrap().starts_with("sg-"));
    assert_eq!(svc.due(14, true).unwrap()["projects"], json!([]));

    std::fs::write(repo.join("f0.txt"), "more\n").unwrap();
    git(&repo, &["commit", "-qam", "second"]);
    assert_eq!(svc.due(14, true).unwrap()["projects"][0], json!({"id": "demo", "name": "demo", "reason": "changed", "changes": ["commits"]}));
    assert_eq!(svc.portfolio_context().unwrap()["projects"][0]["card"]["outdated"], true);
    let ctx = svc.context("demo").unwrap();
    assert_eq!(ctx["previous_card"]["card"]["headline"], "Alles ruhig");
    assert_eq!(ctx["since_previous_card"]["commit_count"], 1);
    assert!(ctx["since_previous_card"]["commits"][0].as_str().unwrap().ends_with("second"));

    svc.put_card("demo", card(), false, false).unwrap();
    clock.advance(Duration::days(15));
    assert_eq!(svc.due(14, true).unwrap()["projects"][0]["reason"], "stale");
}

#[test]
fn merge_keeps_unsent_fields_and_rated_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    let first = svc.put_card("demo", card_with(json!({"features": ["Suche"]})), false, false).unwrap();
    let sid = first["items"][0]["id"].as_str().unwrap().to_owned();

    svc.put_card("demo", json!({"headline": "Neu"}), true, false).unwrap();
    let current = &svc.card("demo").unwrap()["card"];
    assert_eq!((current["headline"].clone(), current["features"].clone()), (json!("Neu"), json!(["Suche"])));
    assert_eq!(current["suggestions"][0]["id"], json!(sid));
}

#[test]
fn invalid_cards_are_refused_with_field_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    let err = svc.put_card("demo", card_with(json!({"phase": "sleepy", "reentry": {"next_step": "x"}})), false, false).unwrap_err();
    assert_eq!(err.code, "invalid_card");
    assert!(err.message.contains("phase:") && err.message.contains("reentry.effort: Field required"), "{}", err.message);
    let empty_title = json!({"suggestions": [{"type": "feature", "title": "", "why": "w", "effort": "S"}]});
    let err = svc.put_card("demo", card_with(empty_title), false, false).unwrap_err();
    assert!(err.message.contains("suggestions[0].title"), "{}", err.message);
    // the word limits the page is designed for; dashes do not count as words
    let long = json!({"headline": "Eins zwei drei vier fünf sechs sieben acht neun zehn – elf"});
    let err = svc.put_card("demo", card_with(long), false, false).unwrap_err();
    assert!(err.message.contains("headline: Text should have at most 10 words, has 11"), "{}", err.message);
    // a dry run reports what would be saved and saves nothing; new items get their id only when saved
    let tried = svc.put_card("demo", card_with(json!({"colour": "blue"})), false, true).unwrap();
    assert_eq!((tried["saved"].as_bool(), tried["warnings"].clone()), (Some(false), json!(["ignored unknown field: colour"])));
    assert_eq!(tried["items"][0]["id"], json!(null));
    assert_eq!(svc.card("demo").unwrap_err().code, "no_card");
    let saved = svc.put_card("demo", card_with(json!({"colour": "blue", "vision": {"statement": "Ziel", "size": 3}})), false, false).unwrap();
    assert_eq!(saved["warnings"], json!(["ignored unknown field: colour", "ignored unknown field: vision.size"]));
    // with --merge, a problem in a field the agent did not send is marked as inherited
    let stored = svc.store().latest_card("demo").unwrap().unwrap();
    let old = card_with(json!({"health_notes": ["wort ".repeat(31)]}));
    svc.store().add_card("demo", &svc.now(), &lookout::store::CardBasis { fingerprint: &stored.fingerprint, head: None, ci: None, parts: &json!(null) }, &old).unwrap();
    let err = svc.put_card("demo", json!({"headline": "Neu"}), true, false).unwrap_err();
    assert!(err.message.contains("health_notes[0]: Text should have at most 30 words, has 31 (from the previous card)"), "{}", err.message);
}

#[test]
fn portfolio_checks_project_ids_and_is_due_each_day() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let mut brief = json!({"greeting": "Moin", "focus": [{"project": "ghost", "title": "x", "why": "y", "effort": "1h"}]});
    let err = svc.put_portfolio(brief.clone(), false, false).unwrap_err();
    assert_eq!(err.code, "unknown_project");
    let four = json!({"greeting": "Moin", "focus": vec![brief["focus"][0].clone(); 4]});
    assert!(svc.put_portfolio(four, false, false).unwrap_err().message.contains("focus: List should have at most 3 items"));
    assert!(err.hint.unwrap().contains("demo"));

    brief["focus"][0]["project"] = json!("demo");
    svc.put_portfolio(brief, false, false).unwrap();
    assert_eq!(svc.due(14, true).unwrap()["portfolio"]["due"], false);
    clock.advance(Duration::days(1));
    assert_eq!(svc.due(14, true).unwrap()["portfolio"]["reason"], "new_day");
}

#[test]
fn visits_remember_the_previous_one() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    assert!(svc.state(true).unwrap()["previous_visit"].is_null());
    let start = svc.now();
    clock.advance(Duration::minutes(10));
    assert!(svc.state(true).unwrap()["previous_visit"].is_null(), "a reload is not a new visit");
    clock.advance(Duration::hours(5));
    let previous = svc.state(true).unwrap()["previous_visit"].as_str().unwrap().to_owned();
    assert!(previous > start);
}

#[test]
fn status_and_registration_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    let repo = demo_repo(tmp.path());
    let other = common::make_repo(tmp.path(), "Other App", &["2026-09-20T10:00:00"]);
    svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    assert_eq!(svc.add_project(repo.to_str().unwrap(), None, None).unwrap_err().code, "already_registered");

    common::make_repo(tmp.path(), "side", &["2026-09-25T10:00:00"]);
    common::make_repo(&tmp.path().join("group"), "old-tool", &["2025-01-01T10:00:00"]);
    let picks = |d: &serde_json::Value| -> Vec<(String, bool)> {
        d["candidates"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().to_owned(), c["recommended"] == true)).collect()
    };
    // a folder of projects: two levels deep, newest first, recently active ones recommended
    let found = svc.discover(tmp.path().to_str().unwrap()).unwrap();
    assert_eq!(picks(&found), [("side".into(), true), ("Other App".into(), true), ("old-tool".into(), false)]);
    assert_eq!((found["folder_is_repo"].as_bool(), found["already_watched"].as_u64()), (Some(false), Some(1)));
    // a single project: it comes first and recommended, its neighbours are offered unticked
    let found = svc.discover(other.to_str().unwrap()).unwrap();
    assert_eq!(picks(&found), [("Other App".into(), true), ("side".into(), false)]);
    assert_eq!(found["folder_is_repo"], true);

    let batch = svc.add_projects(&[other.to_string_lossy().into_owned(), repo.to_string_lossy().into_owned()]).unwrap();
    assert_eq!(batch["added"][0]["id"], "other-app");
    assert_eq!(batch["failed"][0]["error"]["code"], "already_registered");

    svc.put_card("demo", card(), false, false).unwrap();
    // versions count per project
    assert_eq!(svc.put_card("other-app", card(), false, false).unwrap()["version"], 1);
    clock.advance(Duration::minutes(1));
    svc.set_status("demo", "parked", Some("wartet auf Key")).unwrap();
    let due = svc.due(14, true).unwrap();
    assert!(due["projects"].as_array().unwrap().contains(&json!({"id": "demo", "name": "demo", "reason": "status_changed"})));
    svc.set_status("demo", "archived", None).unwrap();
    let due = svc.due(14, true).unwrap();
    assert!(due["projects"].as_array().unwrap().iter().all(|p| p["id"] != "demo"));
    assert_eq!(svc.store().project("demo").unwrap().unwrap().status_note, "wartet auf Key");
}

#[test]
fn owner_choices_show_up_in_the_window_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    let sid = svc.put_card("demo", card(), false, false).unwrap()["items"][0]["id"].as_str().unwrap().to_owned();

    svc.set_feedback(&sid, Some("later"), Some("nach dem Release")).unwrap();
    svc.set_pref("hidden_modules", &json!(["pulse"])).unwrap();
    assert_eq!(svc.set_status("demo", "sleeping", None).unwrap_err().code, "invalid_status");
    assert_eq!(svc.set_pref("colour", &json!("red")).unwrap_err().code, "invalid_pref");

    let state = svc.state(false).unwrap();
    let p = &state["projects"][0];
    assert_eq!((p["card"]["headline"].as_str(), p["facts"]["is_git"].as_bool()), (Some("Alles ruhig"), Some(true)));
    assert_eq!((state["feedback"][&sid]["value"].as_str(), state["feedback"][&sid]["note"].as_str()), (Some("later"), Some("nach dem Release")));
    assert_eq!(state["prefs"], json!({"style": "handover", "hidden_modules": ["pulse"]}));
}

#[test]
fn a_ci_turning_red_or_green_makes_the_card_due() {
    use std::sync::{Arc, Mutex};
    let tmp = tempfile::tempdir().unwrap();
    let ci = Arc::new(Mutex::new("green"));
    let seen = ci.clone();
    let collect = move |path: &str, today, _ci| {
        let mut f = lookout::facts::collect(path, today, false);
        f["ci"] = json!({"state": *seen.lock().unwrap()});
        f
    };
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z").unwrap().to_utc();
    let store = lookout::store::Store::open(&tmp.path().join("lookout.db")).unwrap();
    let svc = lookout::Lookout::with_parts(store, Box::new(move || now), Box::new(collect));
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let reasons = |svc: &lookout::Lookout| svc.due(14, true).unwrap()["projects"].as_array().unwrap().iter().map(|p| p["reason"].as_str().unwrap().to_owned()).collect::<Vec<_>>();

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
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    svc.add_project(common::make_repo(tmp.path(), "other", &["2026-09-28T10:00:00"]).to_str().unwrap(), None, None).unwrap();
    let saved = svc.put_card("demo", card_with(json!({"waiting_on_you": [{"title": "Soll die App offline laufen?"}]})), false, false).unwrap();
    let id_of = |kind: &str| saved["items"].as_array().unwrap().iter().find(|i| i["kind"] == kind).unwrap()["id"].as_str().unwrap().to_owned();
    let (sid, qid) = (id_of("suggestion"), id_of("question"));
    svc.put_card("other", card(), false, false).unwrap();

    // a rating keeps what it was about and why, after the agent dropped the item, and reaches every project
    svc.set_feedback(&sid, Some("down"), Some("Keine UI-Spielereien")).unwrap();
    svc.put_card("demo", card_with(json!({"suggestions": [], "waiting_on_you": [{"id": qid, "title": "Soll die App offline laufen?"}]})), false, false).unwrap();
    let ctx = svc.context("other").unwrap();
    let rating = &ctx["feedback"][0];
    assert_eq!(rating["item"], json!({"kind": "suggestion", "type": "feature", "title": "Dark Mode", "why": "Weil nachts.", "evidence": "", "effort": "S"}));
    assert_eq!((rating["project"].as_str(), rating["note"].as_str()), (Some("demo"), Some("Keine UI-Spielereien")));
    assert_eq!(ctx["feedback_summary"], json!([{"kind": "suggestion", "type": "feature", "up": 0, "later": 0, "down": 1}]));

    // questions take answers, which make the card due and reach only their own project
    assert_eq!(svc.set_feedback(&qid, Some("up"), None).unwrap_err().code, "invalid_feedback");
    assert_eq!(svc.set_feedback(&qid, Some("answered"), Some(" ")).unwrap_err().code, "answer_missing");
    assert_eq!(svc.set_feedback("sg-gone", Some("up"), None).unwrap_err().code, "unknown_item");
    clock.advance(Duration::minutes(1));
    svc.set_feedback(&qid, Some("answered"), Some("Ja, unterwegs ohne Netz")).unwrap();
    assert_eq!(svc.due(14, true).unwrap()["projects"], json!([{"id": "demo", "name": "demo", "reason": "answered"}]));
    assert_eq!(svc.context("demo").unwrap()["feedback"][0]["note"], "Ja, unterwegs ohne Netz");
    assert_eq!(svc.context("other").unwrap()["feedback"].as_array().unwrap().len(), 1);

    // a focus item can be marked done for the next brief
    let brief = json!({"greeting": "Moin", "focus": [{"project": "demo", "title": "Offline klären", "why": "Beantwortet.", "effort": "1h"}]});
    let fid = svc.put_portfolio(brief, false, false).unwrap()["items"][0]["id"].as_str().unwrap().to_owned();
    svc.set_feedback(&fid, Some("done"), None).unwrap();
    let pctx = svc.portfolio_context().unwrap();
    assert!(pctx["feedback"].as_array().unwrap().iter().any(|f| f["id"] == fid && f["item"]["kind"] == "focus" && f["project"] == "demo"));

    // cards written before questions had ids get them when Lookout opens
    let stored = svc.store().latest_card("other").unwrap().unwrap();
    let basis = lookout::store::CardBasis { fingerprint: &stored.fingerprint, head: None, ci: None, parts: &json!(null) };
    svc.store().add_card("other", &svc.now(), &basis, &card_with(json!({"waiting_on_you": [{"title": "Alt?"}]}))).unwrap();
    let reopened = lookout::Lookout::open(&tmp.path().join("lookout.db")).unwrap();
    assert!(reopened.card("other").unwrap()["card"]["waiting_on_you"][0]["id"].as_str().unwrap().starts_with("q-"));
}
