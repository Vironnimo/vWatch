use serde_json::{json, Value};
use watchtower::cards::{validate, Kind};

#[test]
fn integer_validation_preserves_values_without_overflow() {
    let brief = |idea: Value| {
        json!({
            "greeting": "Moin",
            "idea_checks": [{"idea": idea, "verdict": "later", "why": "Später prüfen", "effort": "S"}]
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
