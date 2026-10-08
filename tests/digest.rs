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
fn measured_fact_changes_stay_in_project_facts_without_invalidating_the_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    let base = json!({
        "ok": true, "is_git": true, "has_remote": true, "head": "first",
        "dirty": {"files": 0}, "ahead": 0, "ci": {"state": "green"},
        "branches": {"unmerged": []}, "worktrees": {"items": []},
    });
    svc.store()
        .save_facts("demo", "2026-09-30T08:00:00Z", "base", &base)
        .unwrap();
    svc.state(true).unwrap();
    svc.put_card(
        "demo",
        card_with(json!({"open_ends": [{"title": "Export verschluckt Zeilen"}]})),
        false,
        false,
    )
    .unwrap();
    let viewed = changes(&svc);
    assert_eq!(viewed["total"], 1);
    let busy = json!({
        "ok": true, "is_git": true, "has_remote": false, "head": "second",
        "dirty": {"files": 3}, "ahead": 6, "ci": {"state": "red"},
        "branches": {"unmerged": [{"name": "feature"}]},
        "worktrees": {"items": [{"path": "C:/work/feature", "head": "topic", "dirty_files": 2, "prunable": true}]},
    });
    for (label, facts) in [
        ("busy", busy.clone()),
        ("no_ci", {
            let mut f = busy.clone();
            f["ci"]["state"] = json!("none");
            f
        }),
        ("unknown", {
            let mut f = busy.clone();
            f["ci"]["state"] = json!("unknown");
            f
        }),
        ("failed", json!({"ok": false, "error": "path_missing"})),
        ("clean", base),
        ("busy_again", busy),
    ] {
        svc.store()
            .save_facts("demo", "2026-09-30T09:00:00Z", label, &facts)
            .unwrap();
        let state = svc.state(false).unwrap();
        assert_eq!(
            state["projects"][0]["facts"], facts,
            "{label}: measured facts stay visible"
        );
        assert_eq!(
            state["changes"], viewed,
            "{label}: only meaningful card changes belong in the digest"
        );
    }
    svc.set_warning_ack("demo", "no_remote", Some("Bleibt absichtlich lokal."))
        .unwrap();
    assert_eq!(
        changes(&svc),
        viewed,
        "Acknowledging a warning does not affect the digest"
    );
    assert_eq!(
        svc.mark_changes_read(viewed["token"].as_str().unwrap())
            .unwrap()["changes"]["total"],
        0,
        "Facts refreshed after reading must not prevent acknowledging the viewed changes"
    );

    svc.put_card(
        "demo",
        json!({"open_ends": [{"title": "Weiterer Befund"}]}),
        true,
        false,
    )
    .unwrap();
    svc.set_status("demo", "archived", None).unwrap();
    assert_eq!(
        changes(&svc)["total"],
        0,
        "Archiving hides a project without declaring its work completed"
    );
}

#[test]
fn legacy_fact_snapshots_keep_the_read_baseline_and_only_compare_card_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = service(tmp.path());
    svc.add_project(demo_repo(tmp.path()).to_str().unwrap(), None, None)
        .unwrap();
    svc.put_card(
        "demo",
        card_with(json!({"open_ends": [{"title": "Alten Export prüfen"}]})),
        false,
        false,
    )
    .unwrap();
    svc.state(true).unwrap();
    let mut legacy = svc.store().pref("changes_read").unwrap().unwrap();
    legacy["at"] = json!("2026-09-29T08:00:00Z");
    legacy["snapshot"]["projects"][0]["facts"] = json!({
        "ok": true, "has_remote": false, "dirty": {"files": 3}, "ahead": 6,
        "ci": {"state": "red"}, "branches": {"unmerged": [{"name": "feature"}]},
        "worktrees": {"items": [{"path": "C:/work/feature"}]},
    });
    svc.store().set_pref("changes_read", &legacy).unwrap();
    svc.put_card(
        "demo",
        json!({"recent_changes": [{
            "date": "2026-09-30", "title": "Suche ergänzt", "detail": "Ergebnisse jetzt im Fenster."
        }]}),
        true,
        false,
    )
    .unwrap();

    let digest = svc.state(true).unwrap()["changes"].clone();
    assert_eq!(digest["since"], legacy["at"]);
    assert_eq!(digest["total"], 1);
    assert_eq!(titles(&digest, "new"), vec!["Neu: Suche ergänzt"]);
    assert!(titles(&digest, "resolved").is_empty());
    assert_eq!(svc.store().pref("changes_read").unwrap().unwrap(), legacy);
    svc.mark_changes_read(digest["token"].as_str().unwrap())
        .unwrap();
    assert_eq!(changes(&svc)["total"], 0);
    assert!(
        svc.store().pref("changes_read").unwrap().unwrap()["snapshot"]["projects"][0]
            .get("facts")
            .is_none()
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
