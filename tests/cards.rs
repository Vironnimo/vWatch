use serde_json::{json, Value};
use watchtower::cards::{json_schema, validate, Kind};

#[test]
fn proposal_categories_preserve_legacy_meaning_and_reserve_room_for_both() {
    let mut doc = json!({"one_liner":"Test", "phase":"active", "headline":"Bereit", "reentry":{"next_step":"Weiter"},
    "suggestions":[
        {"id":"sg-feature","type":"feature","title":"Vorschau","why":"Verständlicher"},
        {"id":"sg-release","type":"next","title":"Release veröffentlichen","why":"Bereit"},
        {"id":"sg-idea","type":"next","category":"idea","title":"Einfacherer Ablauf","why":"Weniger Schritte"}
    ]});
    assert!(Kind::Card
        .missing_fields(&doc)
        .contains(&"suggestions[].category".to_owned()));
    let (clean, _) = validate(Kind::Card, &doc, &[]).unwrap();
    assert_eq!(clean["suggestions"][0]["category"], "idea");
    assert_eq!(clean["suggestions"][1]["category"], "recommendation");
    assert_eq!(clean["suggestions"][2]["category"], "idea");
    assert_eq!(
        Kind::Card.current_view(&doc)["suggestions"][0]["category"],
        "idea"
    );
    assert!(doc["suggestions"][0].get("category").is_none());
    doc["suggestions"][0]["category"] = json!("recommendation");
    assert_eq!(
        validate(Kind::Card, &doc, &[]).unwrap().0["suggestions"][0]["category"],
        "recommendation"
    );
    doc["suggestions"][0]["category"] = json!("unknown");
    assert!(validate(Kind::Card, &doc, &[])
        .unwrap_err()
        .iter()
        .any(|e| e.starts_with("suggestions[0].category")));
    doc["suggestions"] = json!((0..8).map(|i| json!({"type":"next","category":if i < 4 {"idea"} else {"recommendation"}, "title":"Vorschlag", "why":"Nützlich"})).collect::<Vec<_>>());
    assert!(validate(Kind::Card, &doc, &[]).is_ok());
    doc["suggestions"][4]["category"] = json!("idea");
    assert!(validate(Kind::Card, &doc, &[])
        .unwrap_err()
        .iter()
        .any(|e| e.contains("limit is 4 per category")));
    let brief = json!({"greeting":"Hallo","synergies":[{"title":"Gemeinsames Aufräumen","why":"Platz"},{"title":"Gemeinsame Suche","why":"Leichter finden","category":"idea"}]});
    let clean = validate(Kind::Portfolio, &brief, &[]).unwrap().0;
    assert_eq!(clean["synergies"][0]["category"], "recommendation");
    assert_eq!(clean["synergies"][1]["category"], "idea");
}

#[test]
fn documents_need_no_effort_estimates_and_drop_legacy_estimates() {
    let card = json!({
        "one_liner": "Ein Testprojekt",
        "phase": "active",
        "headline": "Export ist bereit",
        "reentry": {"next_step": "Export prüfen", "done_when": "Die Datei lässt sich öffnen."},
        "suggestions": [{"id": "suggestion-export", "type": "next", "title": "Export anbieten", "why": "Daten weiterverwenden", "done_when": "Der Export ist erreichbar."}]
    });
    let portfolio = json!({
        "greeting": "Moin",
        "focus": [{"id": "focus-export", "project": "demo", "title": "Export prüfen", "why": "Datei bestätigen", "done_when": "Die Datei lässt sich öffnen."}],
        "synergies": [{"id": "synergy-format", "title": "Format teilen", "why": "Daten austauschen", "projects": ["demo", "other"], "evidence": "Beide lesen CSV."}],
        "new_apps": [{"id": "app-viewer", "title": "Dateibetrachter", "pitch": "Exportierte Daten lesen", "because": "Dateien direkt prüfen"}],
        "idea_checks": [{"idea": 42, "verdict": "fold_in", "fits": ["demo"], "why": "Passt zum Export."}]
    });
    for (kind, document, locations) in [
        (
            Kind::Card,
            card,
            vec![
                ("/reentry", "reentry.effort", "half-day"),
                ("/suggestions/0", "suggestions[0].effort", "S"),
            ],
        ),
        (
            Kind::Portfolio,
            portfolio,
            vec![
                ("/focus/0", "focus[0].effort", "1h"),
                ("/synergies/0", "synergies[0].effort", "M"),
                ("/new_apps/0", "new_apps[0].effort", "L"),
                ("/idea_checks/0", "idea_checks[0].effort", "S"),
            ],
        ),
    ] {
        let (clean, warnings) = validate(kind, &document, &[]).unwrap();
        assert!(warnings.is_empty(), "{kind:?}: {warnings:?}");
        assert!(kind.missing_fields(&clean).is_empty());
        assert!(!json_schema(kind).to_string().contains("\"effort\""));

        let mut legacy = document.clone();
        for (pointer, _, estimate) in &locations {
            legacy.pointer_mut(pointer).unwrap()["effort"] = json!(estimate);
        }
        let (without_estimates, warnings) = validate(kind, &legacy, &[]).unwrap();
        assert_eq!(
            without_estimates, clean,
            "{kind:?}: preserve content and IDs"
        );
        assert_eq!(warnings.len(), locations.len());
        for (_, field, _) in locations {
            assert!(
                warnings.contains(&format!("ignored unknown field: {field}")),
                "{kind:?}: {warnings:?}"
            );
        }
        // A later list item can still lack a field even when the first is current.
        let list = if kind == Kind::Card {
            "suggestions"
        } else {
            "focus"
        };
        let mut partial = clean.clone();
        let mut incomplete = partial[list][0].clone();
        incomplete.as_object_mut().unwrap().remove("done_when");
        partial[list]
            .as_array_mut()
            .unwrap()
            .extend([incomplete.clone(), incomplete]);
        assert_eq!(
            kind.missing_fields(&partial),
            vec![format!("{list}[].done_when")]
        );
    }
}

#[test]
fn integer_validation_preserves_values_without_overflow() {
    let brief = |idea: Value| {
        json!({
            "greeting": "Moin",
            "idea_checks": [{"idea": idea, "verdict": "later", "why": "Später prüfen"}]
        })
    };
    for (given, expected) in [(json!(1), 1), (json!(3.0), 3), (json!(i64::MAX), i64::MAX)] {
        let (clean, _) = validate(Kind::Portfolio, &brief(given), &[]).unwrap();
        assert_eq!(clean["idea_checks"][0]["idea"], expected);
    }
    for invalid in [
        json!(u64::MAX),
        json!(i64::MAX as u64 + 1),
        json!(1e30),
        json!(2.5),
    ] {
        let errors = validate(Kind::Portfolio, &brief(invalid.clone()), &[])
            .expect_err("an invalid idea id must never turn into a different id");
        assert!(
            errors.iter().any(|e| e.starts_with("idea_checks[0].idea:")),
            "{invalid}: {errors:?}"
        );
    }
}
