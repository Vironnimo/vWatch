mod common;

use std::sync::Barrier;

use common::{card, card_with, demo_repo, make_repo, service};
use serde_json::{json, Value};
use watchtower::service::Result;
use watchtower::Watchtower;

fn card_token(svc: &Watchtower, id: &str) -> String {
    svc.context(id).unwrap()["context_token"]
        .as_str()
        .expect("card context carries a token")
        .to_owned()
}

fn portfolio_token(svc: &Watchtower) -> String {
    svc.portfolio_context().unwrap()["context_token"]
        .as_str()
        .expect("portfolio context carries a token")
        .to_owned()
}

fn one_winner(results: &[Result<Value>], rejection: &str) -> usize {
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "exactly one writer must save: {results:?}"
    );
    let error = results.iter().find_map(|r| r.as_ref().err()).unwrap();
    assert_eq!(error.code, rejection, "{error:?}");
    results.iter().position(|r| r.is_ok()).unwrap()
}

fn rejected_card_leaves_state(svc: &Watchtower, token: &str) {
    let before = svc.card("demo").unwrap();
    let history = svc.store().card_history("demo").unwrap();
    let feedback = svc.feedback(None).unwrap();
    let due = svc.due(14, false).unwrap();
    let error = svc
        .put_card_guarded(
            "demo",
            json!({"headline": "Veraltete Analyse", "waiting_on_you": []}),
            true,
            false,
            Some(token),
        )
        .unwrap_err();
    assert_eq!(error.code, "version_conflict");
    assert_eq!(svc.card("demo").unwrap(), before);
    assert_eq!(svc.store().card_history("demo").unwrap(), history);
    assert_eq!(svc.feedback(None).unwrap(), feedback);
    assert_eq!(svc.due(14, false).unwrap(), due);
}

fn rejected_portfolio_leaves_state(svc: &Watchtower, token: &str) {
    let before = svc.portfolio().unwrap();
    let feedback = svc.feedback(None).unwrap();
    let due = svc.due(14, false).unwrap();
    let error = svc
        .put_portfolio_guarded(
            json!({"greeting": "Veralteter Brief", "focus": []}),
            true,
            false,
            Some(token),
        )
        .unwrap_err();
    assert_eq!(error.code, "version_conflict");
    assert_eq!(svc.portfolio().unwrap(), before);
    assert_eq!(svc.feedback(None).unwrap(), feedback);
    assert_eq!(svc.due(14, false).unwrap(), due);
}

#[test]
fn guarded_writes_require_context_but_dry_runs_do_not_consume_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();

    assert_eq!(
        svc.put_card_guarded("demo", card(), false, false, None)
            .unwrap_err()
            .code,
        "context_required"
    );
    let token = card_token(&svc, "demo");
    assert_eq!(
        svc.put_card_guarded("demo", card(), false, true, None)
            .unwrap()["saved"],
        false
    );
    assert!(svc.store().latest_card("demo").unwrap().is_none());
    assert_eq!(card_token(&svc, "demo"), token);
    assert_eq!(
        svc.put_card_guarded("demo", card(), false, false, Some(&token))
            .unwrap()["version"],
        1
    );

    let brief = json!({"greeting": "Guten Morgen"});
    assert_eq!(
        svc.put_portfolio_guarded(brief.clone(), false, false, None)
            .unwrap_err()
            .code,
        "context_required"
    );
    let token = portfolio_token(&svc);
    assert_eq!(
        svc.put_portfolio_guarded(brief.clone(), false, true, None)
            .unwrap()["saved"],
        false
    );
    assert!(svc.store().latest_portfolio().unwrap().is_none());
    assert_eq!(portfolio_token(&svc), token);
    assert_eq!(
        svc.put_portfolio_guarded(brief, false, false, Some(&token))
            .unwrap()["version"],
        1
    );
}

#[test]
fn stale_card_writes_preserve_new_owner_instructions_facts_and_cards() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card(
        "demo",
        card_with(json!({
            "waiting_on_you": [{"id": "q-context", "title": "Soll die App offline laufen?"}]
        })),
        false,
        false,
    )
    .unwrap();

    // No clock advance: owner changes and competing writes in one second still matter.
    let token = card_token(&svc, "demo");
    svc.set_status("demo", "parked", Some("Warte auf meine Entscheidung"))
        .unwrap();
    rejected_card_leaves_state(&svc, &token);

    let token = card_token(&svc, "demo");
    svc.set_goal("demo", "Offline nutzbar machen", Some("2026-12-01"))
        .unwrap();
    rejected_card_leaves_state(&svc, &token);

    let token = card_token(&svc, "demo");
    svc.set_feedback("q-context", Some("answered"), Some("Ja, offline"))
        .unwrap();
    rejected_card_leaves_state(&svc, &token);

    let token = card_token(&svc, "demo");
    svc.set_feedback("q-context", None, None).unwrap();
    rejected_card_leaves_state(&svc, &token);

    let token = card_token(&svc, "demo");
    let mut facts = svc.store().facts("demo").unwrap().unwrap();
    facts.data["ci"] = json!({"state": "red"});
    svc.store()
        .save_facts("demo", &svc.now(), &facts.fingerprint, &facts.data)
        .unwrap();
    rejected_card_leaves_state(&svc, &token);

    let token = card_token(&svc, "demo");
    facts.data["head"] = json!("new-head");
    let fingerprint = watchtower::facts::fingerprint(&facts.data);
    assert_ne!(fingerprint, facts.fingerprint);
    svc.store()
        .save_facts("demo", &svc.now(), &fingerprint, &facts.data)
        .unwrap();
    rejected_card_leaves_state(&svc, &token);

    let token = card_token(&svc, "demo");
    svc.put_card("demo", json!({"headline": "Aktuelle Analyse"}), true, false)
        .unwrap();
    rejected_card_leaves_state(&svc, &token);
    assert_eq!(svc.card("demo").unwrap()["version"], 2);
}

#[test]
fn an_unrelated_card_write_does_not_invalidate_a_project_context() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    for name in ["demo", "other"] {
        let repo = make_repo(tmp.path(), name, &["2026-09-29T10:00:00"]);
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    }
    let token = card_token(&svc, "demo");
    let (other_writer, _) = service(tmp.path());
    other_writer
        .put_card("other", card(), false, false)
        .unwrap();
    assert_eq!(card_token(&svc, "demo"), token);
    assert_eq!(
        svc.put_card_guarded("demo", card(), false, false, Some(&token))
            .unwrap()["version"],
        1
    );
}

#[test]
fn stale_briefs_preserve_cards_owner_reactions_and_ideas() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(
        json!({
            "greeting": "Guten Morgen",
            "focus": [{"id": "focus-context", "project": "demo", "title": "Tests prüfen", "why": "Release vorbereiten"}]
        }),
        false,
        false,
    )
    .unwrap();

    let token = portfolio_token(&svc);
    svc.put_card(
        "demo",
        json!({"headline": "Tests schlagen fehl"}),
        true,
        false,
    )
    .unwrap();
    rejected_portfolio_leaves_state(&svc, &token);

    let token = portfolio_token(&svc);
    svc.set_status("demo", "parked", Some("Erst nächste Woche"))
        .unwrap();
    rejected_portfolio_leaves_state(&svc, &token);

    let token = portfolio_token(&svc);
    svc.set_feedback("focus-context", Some("done"), None)
        .unwrap();
    rejected_portfolio_leaves_state(&svc, &token);

    let token = portfolio_token(&svc);
    svc.add_idea("Ein persönlicher Release-Kalender").unwrap();
    rejected_portfolio_leaves_state(&svc, &token);

    let token = portfolio_token(&svc);
    let mut facts = svc.store().facts("demo").unwrap().unwrap();
    facts.data["ci"] = json!({"state": "red"});
    svc.store()
        .save_facts("demo", &svc.now(), &facts.fingerprint, &facts.data)
        .unwrap();
    rejected_portfolio_leaves_state(&svc, &token);

    let token = portfolio_token(&svc);
    svc.put_portfolio(json!({"greeting": "Aktueller Brief"}), true, false)
        .unwrap();
    rejected_portfolio_leaves_state(&svc, &token);
    assert_eq!(svc.portfolio().unwrap()["version"], 2);
}

#[test]
fn changing_only_a_snooze_date_invalidates_both_contexts() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let saved = svc.put_card("demo", card(), false, false).unwrap();
    let item = saved["items"][0]["id"].as_str().unwrap();
    svc.put_portfolio(json!({"greeting": "Guten Morgen"}), false, false)
        .unwrap();
    svc.set_feedback_until(
        item,
        Some("later"),
        Some("Nach dem Release"),
        Some("2026-10-07"),
    )
    .unwrap();
    let card_token = card_token(&svc, "demo");
    let portfolio_token = portfolio_token(&svc);
    svc.set_feedback_until(
        item,
        Some("later"),
        Some("Nach dem Release"),
        Some("2026-10-14"),
    )
    .unwrap();
    rejected_card_leaves_state(&svc, &card_token);
    rejected_portfolio_leaves_state(&svc, &portfolio_token);
}

#[test]
fn tasks_and_warning_decisions_invalidate_only_relevant_contexts() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    for name in ["demo", "other"] {
        let repo = make_repo(tmp.path(), name, &["2026-09-29T10:00:00"]);
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    }
    svc.put_card("demo", card(), false, false).unwrap();
    svc.put_portfolio(json!({"greeting": "Guten Morgen"}), false, false)
        .unwrap();

    let own_token = card_token(&svc, "demo");
    let brief_token = portfolio_token(&svc);
    svc.create_task(json!({"projects": ["other"], "title": "Andere Tests prüfen"}))
        .unwrap();
    assert_eq!(card_token(&svc, "demo"), own_token);
    rejected_portfolio_leaves_state(&svc, &brief_token);

    let brief_token = portfolio_token(&svc);
    let task = svc
        .create_task(json!({"projects": ["demo"], "title": "Tests prüfen"}))
        .unwrap();
    rejected_card_leaves_state(&svc, &own_token);
    rejected_portfolio_leaves_state(&svc, &brief_token);

    let own_token = card_token(&svc, "demo");
    let brief_token = portfolio_token(&svc);
    svc.report_task(
        task["task"]["id"].as_i64().unwrap(),
        "done",
        "Alle Tests erfolgreich",
        "cargo test: 8 Tests erfolgreich",
    )
    .unwrap();
    rejected_card_leaves_state(&svc, &own_token);
    rejected_portfolio_leaves_state(&svc, &brief_token);

    let own_token = card_token(&svc, "demo");
    let brief_token = portfolio_token(&svc);
    svc.set_warning_ack("demo", "no_remote", Some("Bleibt absichtlich lokal"))
        .unwrap();
    rejected_card_leaves_state(&svc, &own_token);
    rejected_portfolio_leaves_state(&svc, &brief_token);

    let own_token = card_token(&svc, "demo");
    let brief_token = portfolio_token(&svc);
    svc.set_warning_ack("demo", "no_remote", None).unwrap();
    rejected_card_leaves_state(&svc, &own_token);
    rejected_portfolio_leaves_state(&svc, &brief_token);
}

#[test]
fn simultaneous_card_writers_save_exactly_one_version() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card("demo", card(), false, false).unwrap();
    let token = card_token(&svc, "demo");
    let (first, _) = service(tmp.path());
    let (second, _) = service(tmp.path());
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = [first, second]
            .into_iter()
            .enumerate()
            .map(|(writer, svc)| {
                let barrier = &barrier;
                let token = &token;
                scope.spawn(move || {
                    barrier.wait();
                    svc.put_card_guarded(
                        "demo",
                        json!({"headline": format!("Analyse {writer}")}),
                        true,
                        false,
                        Some(token),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let winner = one_winner(&results, "version_conflict");
    assert_eq!(results[winner].as_ref().unwrap()["version"], 2);
    assert_eq!(svc.card("demo").unwrap()["version"], 2);
    assert_eq!(svc.store().card_history("demo").unwrap().len(), 2);
    assert_eq!(
        svc.card("demo").unwrap()["card"]["headline"],
        format!("Analyse {winner}")
    );
}

#[test]
fn simultaneous_portfolio_writers_save_exactly_one_version() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.put_portfolio(json!({"greeting": "Vorher"}), false, false)
        .unwrap();
    let token = portfolio_token(&svc);
    let (first, _) = service(tmp.path());
    let (second, _) = service(tmp.path());
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = [first, second]
            .into_iter()
            .enumerate()
            .map(|(writer, svc)| {
                let barrier = &barrier;
                let token = &token;
                scope.spawn(move || {
                    barrier.wait();
                    svc.put_portfolio_guarded(
                        json!({"greeting": format!("Brief {writer}")}),
                        true,
                        false,
                        Some(token),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let winner = one_winner(&results, "version_conflict");
    assert_eq!(results[winner].as_ref().unwrap()["version"], 2);
    assert_eq!(svc.portfolio().unwrap()["version"], 2);
    assert_eq!(
        svc.portfolio().unwrap()["portfolio"]["greeting"],
        format!("Brief {winner}")
    );
}

#[test]
fn simultaneous_first_opens_create_one_complete_database() {
    let tmp = tempfile::tempdir().unwrap();
    for round in 0..8 {
        let path = tmp.path().join(format!("fresh-{round}.db"));
        let barrier = Barrier::new(8);
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let path = &path;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        watchtower::store::Store::open(path).and_then(|store| store.projects())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        for result in results {
            assert!(result.is_ok(), "simultaneous first open failed: {result:?}");
        }
    }
}

#[test]
fn simultaneous_projects_cannot_claim_the_same_item_id() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    for name in ["demo", "other"] {
        let repo = make_repo(tmp.path(), name, &["2026-09-29T10:00:00"]);
        svc.add_project(repo.to_str().unwrap(), None, None).unwrap();
    }
    let tokens = [card_token(&svc, "demo"), card_token(&svc, "other")];
    let (first, _) = service(tmp.path());
    let (second, _) = service(tmp.path());
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = [first, second]
            .into_iter()
            .zip(["demo", "other"])
            .zip(&tokens)
            .map(|((svc, project), token)| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    svc.put_card_guarded(
                        project,
                        card_with(json!({"suggestions": [{
                            "id": "sg-shared", "type": "feature", "title": "Offline nutzen",
                            "why": "Auch unterwegs arbeiten"
                        }]})),
                        false,
                        false,
                        Some(token),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let winner = one_winner(&results, "invalid_card");
    let projects = ["demo", "other"];
    assert_eq!(svc.card(projects[winner]).unwrap()["version"], 1);
    assert!(svc
        .store()
        .latest_card(projects[1 - winner])
        .unwrap()
        .is_none());
    svc.set_feedback("sg-shared", Some("up"), None).unwrap();
    assert_eq!(svc.feedback(None).unwrap()[0]["project"], projects[winner]);
}
