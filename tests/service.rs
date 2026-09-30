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

    let saved = svc.put_card("demo", card(), false).unwrap();
    assert!(saved["rated_items"][0]["id"].as_str().unwrap().starts_with("sg-"));
    assert_eq!(svc.due(14, true).unwrap()["projects"], json!([]));

    std::fs::write(repo.join("f0.txt"), "more\n").unwrap();
    git(&repo, &["commit", "-qam", "second"]);
    assert_eq!(svc.due(14, true).unwrap()["projects"][0]["reason"], "changed");
    let ctx = svc.context("demo").unwrap();
    assert_eq!(ctx["previous_card"]["card"]["headline"], "Alles ruhig");
    assert_eq!(ctx["since_previous_card"]["commit_count"], 1);
    assert!(ctx["since_previous_card"]["commits"][0].as_str().unwrap().ends_with("second"));

    svc.put_card("demo", card(), false).unwrap();
    clock.advance(Duration::days(15));
    assert_eq!(svc.due(14, true).unwrap()["projects"][0]["reason"], "stale");
}

#[test]
fn merge_keeps_unsent_fields_and_rated_ids() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    let first = svc.put_card("demo", card_with(json!({"features": ["Suche"]})), false).unwrap();
    let sid = first["rated_items"][0]["id"].as_str().unwrap().to_owned();
    svc.set_feedback(&sid, Some("down"), Some("demo"), "Dark Mode").unwrap();

    svc.put_card("demo", json!({"headline": "Neu"}), true).unwrap();
    let current = &svc.card("demo").unwrap()["card"];
    assert_eq!((current["headline"].clone(), current["features"].clone()), (json!("Neu"), json!(["Suche"])));
    assert_eq!(current["suggestions"][0]["id"], json!(sid));
    assert_eq!(svc.context("demo").unwrap()["feedback"], json!([{"id": sid, "title": "Dark Mode", "value": "down", "at": svc.now()}]));
}

#[test]
fn invalid_cards_are_refused_with_field_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    let err = svc.put_card("demo", card_with(json!({"phase": "sleepy", "reentry": {"next_step": "x"}})), false).unwrap_err();
    assert_eq!(err.code, "invalid_card");
    assert!(err.message.contains("phase:") && err.message.contains("reentry.effort: Field required"), "{}", err.message);
    let empty_title = json!({"suggestions": [{"type": "feature", "title": "", "why": "w", "effort": "S"}]});
    let err = svc.put_card("demo", card_with(empty_title), false).unwrap_err();
    assert!(err.message.contains("suggestions[0].title"), "{}", err.message);
    let saved = svc.put_card("demo", card_with(json!({"colour": "blue", "vision": {"statement": "Ziel", "size": 3}})), false).unwrap();
    assert_eq!(saved["warnings"], json!(["ignored unknown field: colour", "ignored unknown field: vision.size"]));
}

#[test]
fn portfolio_checks_project_ids_and_is_due_each_day() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None).unwrap();
    svc.put_card("demo", card(), false).unwrap();
    let mut brief = json!({"greeting": "Moin", "focus": [{"project": "ghost", "title": "x", "why": "y", "effort": "1h"}]});
    let err = svc.put_portfolio(brief.clone(), false).unwrap_err();
    assert_eq!(err.code, "unknown_project");
    assert!(err.hint.unwrap().contains("demo"));

    brief["focus"][0]["project"] = json!("demo");
    svc.put_portfolio(brief, false).unwrap();
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

    let batch = svc.add_projects(&[other.to_string_lossy().into_owned(), repo.to_string_lossy().into_owned()]).unwrap();
    assert_eq!(batch["added"][0]["id"], "other-app");
    assert_eq!(batch["failed"][0]["error"]["code"], "already_registered");

    svc.put_card("demo", card(), false).unwrap();
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
    let sid = svc.put_card("demo", card(), false).unwrap()["rated_items"][0]["id"].as_str().unwrap().to_owned();

    svc.set_feedback(&sid, Some("later"), Some("demo"), "Dark Mode").unwrap();
    svc.set_pref("hidden_modules", &json!(["pulse"])).unwrap();
    assert_eq!(svc.set_status("demo", "sleeping", None).unwrap_err().code, "invalid_status");
    assert_eq!(svc.set_pref("colour", &json!("red")).unwrap_err().code, "invalid_pref");

    let state = svc.state(false).unwrap();
    let p = &state["projects"][0];
    assert_eq!((p["card"]["headline"].as_str(), p["facts"]["is_git"].as_bool()), (Some("Alles ruhig"), Some(true)));
    assert_eq!(state["feedback"][&sid]["value"], "later");
    assert_eq!(state["prefs"], json!({"style": "handover", "hidden_modules": ["pulse"]}));
}
