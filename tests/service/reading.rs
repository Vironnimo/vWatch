use super::*;

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
    let connection = vwatch::store::Store::open(&tmp.path().join("vwatch.db")).unwrap();
    let armed = std::sync::atomic::AtomicBool::new(true);
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-30T08:00:00Z")
        .unwrap()
        .to_utc();
    let during = vwatch::VWatch::with_parts(
        vwatch::store::Store::open(&tmp.path().join("vwatch.db")).unwrap(),
        Box::new(move || {
            if armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
                connection
                    .add_idea("Während der Projektion", "2026-09-30T08:00:00Z")
                    .unwrap();
            }
            now
        }),
        Box::new(|_, _, _, _| panic!("state does not collect")),
    );
    let before = during.data_version().unwrap();
    let (state, version) = during.versioned_state(false).unwrap();
    assert_eq!(version, before);
    assert!(
        during.data_version().unwrap() > version,
        "assembly write remains detectable"
    );
    assert!(state["ideas"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["text"] == "Während der Projektion"));
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
        json!({"style": "handover", "hidden_modules": ["pulse"], "language": "en"})
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
    svc: &vwatch::VWatch,
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
fn assert_state_matches_full_walk(svc: &vwatch::VWatch) -> Value {
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
