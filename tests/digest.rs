mod common;

use chrono::Duration;
use common::{card_with, demo_repo, service};
use serde_json::{json, Value};

fn changes(svc: &watchtower::Watchtower) -> Value {
    svc.state(false).unwrap()["changes"].clone()
}

fn titles(digest: &Value, kind: &str) -> Vec<String> {
    digest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == kind)
        .map(|entry| entry["title"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn meaningful_changes_persist_until_the_viewed_snapshot_is_acknowledged() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, clock) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card(
        "demo",
        card_with(json!({
            "open_ends": [{"title": "Alten Export prüfen", "source": "src/export.rs"}],
            "waiting_on_you": [{"title": "Welche Darstellung?", "source": "ui/app.js"}],
        })),
        false,
        false,
    )
    .unwrap();
    assert_eq!(
        changes(&svc)["has_baseline"],
        false,
        "Background reads cannot mark anything read"
    );
    assert_eq!(svc.state(true).unwrap()["changes"]["total"], 0);
    let before = changes(&svc)["since"].clone();

    clock.advance(Duration::hours(2));
    svc.put_card("demo", card_with(json!({
        "open_ends": [{"title": "Neue Suche prüfen", "detail": "Ergebnisse fehlen noch.", "source": "src/search.rs"}],
        // A fresh generated question id must not make an unchanged question new.
        "waiting_on_you": [{"title": "Welche Darstellung?", "source": "ui/app.js"}],
        "recent_changes": [{"date": "2026-09-30", "title": "Suche ergänzt", "detail": "Ergebnisse jetzt im Fenster."}],
    })), false, false).unwrap();
    let digest = svc.state(true).unwrap()["changes"].clone();
    assert_eq!(
        digest["since"], before,
        "Opening the window again does not acknowledge unread work"
    );
    assert_eq!(digest["total"], 3);
    assert!(titles(&digest, "new").contains(&"Neu offen: Neue Suche prüfen".to_owned()));
    assert!(titles(&digest, "new").contains(&"Neu: Suche ergänzt".to_owned()));
    assert!(titles(&digest, "resolved")
        .contains(&"Nicht mehr offen in der Karte: Alten Export prüfen".to_owned()));
    assert_eq!(changes(&svc)["entries"], digest["entries"]);

    drop(svc);
    let (svc, _) = service(tmp.path());
    assert_eq!(
        changes(&svc)["entries"],
        digest["entries"],
        "The baseline survives app restarts"
    );
    svc.put_card(
        "demo",
        json!({"decisions": ["Suche bleibt lokal."]}),
        true,
        false,
    )
    .unwrap();
    let error = svc
        .mark_changes_read(digest["token"].as_str().unwrap())
        .unwrap_err();
    assert_eq!(error.code, "stale_changes");
    assert_eq!(
        changes(&svc)["total"],
        4,
        "A stale acknowledgement loses no changes"
    );
    let current = changes(&svc);
    assert_eq!(
        svc.mark_changes_read(current["token"].as_str().unwrap())
            .unwrap()["changes"]["total"],
        0
    );
    assert_eq!(changes(&svc)["total"], 0);

    // Stable content does not become unread when ids, incidental age or spacing change.
    svc.put_card("demo", json!({
        "waiting_on_you": [{"title": "Welche Darstellung?", "source": "ui/app.js"}],
        "open_ends": [{"title": "Neue  Suche prüfen", "detail": "Ergebnisse fehlen noch.", "source": "src/search.rs", "age_days": 7}],
    }), true, false).unwrap();
    assert_eq!(changes(&svc)["total"], 0);
}

#[test]
fn an_owner_reply_is_only_incorporated_after_a_subsequent_card_consumes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card(
        "demo",
        card_with(json!({
            "waiting_on_you": [{"title": "Welche Farbe?", "source": "ui/theme.css"}],
        })),
        false,
        false,
    )
    .unwrap();
    let id = svc.card("demo").unwrap()["card"]["waiting_on_you"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    svc.state(true).unwrap();
    svc.set_feedback(&id, Some("answered"), Some("Blau"))
        .unwrap();
    assert!(titles(&changes(&svc), "decision_applied").is_empty());
    let after_answer = changes(&svc);
    svc.mark_changes_read(after_answer["token"].as_str().unwrap())
        .unwrap();
    svc.put_card("demo", json!({"waiting_on_you": []}), true, false)
        .unwrap();
    let applied = changes(&svc);
    assert_eq!(
        titles(&applied, "decision_applied"),
        vec!["Antwort in Karte berücksichtigt: Welche Farbe?"]
    );
    assert_eq!(applied["entries"][0]["detail"], "Blau");
    assert!(!applied.to_string().contains("implementiert"));
    svc.mark_changes_read(applied["token"].as_str().unwrap())
        .unwrap();
    svc.put_card(
        "demo",
        json!({"headline": "Alles weiterhin ruhig"}),
        true,
        false,
    )
    .unwrap();
    assert_eq!(
        changes(&svc)["total"],
        0,
        "The same consumed reply appears only once"
    );

    svc.set_feedback(&id, Some("answered"), Some("Grün"))
        .unwrap();
    assert!(
        titles(&changes(&svc), "decision_applied").is_empty(),
        "An edited answer has not been read by the old card"
    );
    svc.put_card("demo", json!({"headline": "Antwort geprüft"}), true, false)
        .unwrap();
    assert_eq!(titles(&changes(&svc), "decision_applied").len(), 1);
    assert_eq!(changes(&svc)["entries"][0]["detail"], "Grün");
}

#[test]
fn risk_changes_require_measurements_and_do_not_follow_warning_acknowledgements() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let base = json!({"ok": true, "is_git": true, "has_remote": true, "dirty": {"files": 0}, "ahead": 0, "ci": {"state": "green"}});
    svc.store()
        .save_facts("demo", "2026-09-30T08:00:00Z", "base", &base)
        .unwrap();
    svc.state(true).unwrap();
    let mut risky = base.clone();
    risky["dirty"]["files"] = json!(2);
    risky["ci"]["state"] = json!("red");
    svc.store()
        .save_facts("demo", "2026-09-30T09:00:00Z", "risky", &risky)
        .unwrap();
    let digest = changes(&svc);
    assert_eq!(titles(&digest, "new").len(), 2);
    svc.mark_changes_read(digest["token"].as_str().unwrap())
        .unwrap();
    let mut unknown = risky.clone();
    unknown["ci"]["state"] = json!("unknown");
    svc.store()
        .save_facts("demo", "2026-09-30T09:01:00Z", "unknown", &unknown)
        .unwrap();
    assert!(
        titles(&changes(&svc), "resolved").is_empty(),
        "Unknown CI never means fixed CI"
    );
    let while_unknown = changes(&svc);
    svc.mark_changes_read(while_unknown["token"].as_str().unwrap())
        .unwrap();
    svc.store()
        .save_facts(
            "demo",
            "2026-09-30T09:02:00Z",
            "failed",
            &json!({"ok": false, "error": "path_missing"}),
        )
        .unwrap();
    assert!(
        titles(&changes(&svc), "resolved").is_empty(),
        "An unreadable repo never means its warnings are fixed"
    );
    let while_unreadable = changes(&svc);
    svc.mark_changes_read(while_unreadable["token"].as_str().unwrap())
        .unwrap();
    svc.store()
        .save_facts("demo", "2026-09-30T09:03:00Z", "base", &base)
        .unwrap();
    assert_eq!(titles(&changes(&svc), "resolved").len(), 2);
    let resolved = changes(&svc);
    svc.mark_changes_read(resolved["token"].as_str().unwrap())
        .unwrap();
    let mut local = base;
    local["has_remote"] = json!(false);
    svc.store()
        .save_facts("demo", "2026-09-30T09:04:00Z", "local", &local)
        .unwrap();
    let before_ack = changes(&svc);
    assert_eq!(titles(&before_ack, "new").len(), 1);
    svc.set_warning_ack("demo", "no_remote", Some("Bleibt absichtlich lokal."))
        .unwrap();
    assert_eq!(
        changes(&svc)["entries"],
        before_ack["entries"],
        "Acknowledging a warning does not change measured facts"
    );
    svc.set_status("demo", "archived", None).unwrap();
    assert_eq!(
        changes(&svc)["total"],
        0,
        "Archiving hides a project without declaring its work completed"
    );
}

#[test]
fn concurrent_writes_cannot_be_silently_included_in_read_acknowledgement() {
    let tmp = tempfile::tempdir().unwrap();
    let (owner, _) = service(tmp.path());
    owner
        .add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    owner.state(true).unwrap();
    let viewed = changes(&owner);
    let (agent, _) = service(tmp.path());
    agent
        .put_card(
            "demo",
            card_with(json!({
                "open_ends": [{"title": "Concurrent new work"}],
            })),
            false,
            false,
        )
        .unwrap();
    assert_eq!(
        owner
            .mark_changes_read(viewed["token"].as_str().unwrap())
            .unwrap_err()
            .code,
        "stale_changes"
    );
    assert_eq!(
        titles(&changes(&owner), "new"),
        vec!["Neu offen: Concurrent new work"]
    );
}
