use serde_json::{json, Value};

fn command(home: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = watchtower::cli::run_with_streams(
        ["watchtower", "--json"]
            .into_iter()
            .chain(args.iter().copied()),
        home,
        &mut std::io::empty(),
        &mut out,
        &mut err,
    );
    assert!(err.is_empty());
    (code, serde_json::from_slice(&out).unwrap())
}

#[test]
fn command_metadata_never_contains_ids_values_or_document_text() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(
        command(home.path(), &["ideas", "add", "private-value-42"]).0,
        0
    );
    assert_eq!(
        command(home.path(), &["context", "private-project-99"]).0,
        1
    );
    assert_eq!(
        command(
            home.path(),
            &[
                "portfolio",
                "put",
                "--input-json",
                "{private-broken-payload}"
            ]
        )
        .0,
        1
    );
    assert_eq!(
        command(
            home.path(),
            &["invalid-private-command", "--unknown-private=secret"]
        )
        .0,
        2
    );
    let log = std::fs::read_to_string(watchtower::cli_usage::path(home.path())).unwrap();
    for secret in [
        "private-value-42",
        "private-project-99",
        "private-broken-payload",
        "invalid-private-command",
        "secret",
    ] {
        assert!(!log.contains(secret), "metadata leaked {secret}");
    }
    assert!(log.contains("--input-json"));
    let (code, usage) = command(home.path(), &["usage"]);
    assert_eq!(code, 0);
    assert!(usage["calls"].as_u64().unwrap() >= 4);
    assert!(usage["commands"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["command"] == "invalid_arguments" && row["invalid"] == 1));
}

#[test]
fn an_unfinished_metadata_record_reports_unknown_not_unchanged() {
    let home = tempfile::tempdir().unwrap();
    let ticket =
        watchtower::cli_usage::begin(home.path(), "projects.update", &["--name".into()], 1);
    assert!(ticket.is_some());
    drop(ticket);
    let result = watchtower::cli_usage::activity(home.path()).unwrap();
    assert_eq!(result["activity"][0]["outcome"], "unknown");
    assert!(result["note"].as_str().unwrap().contains("never proves"));
    assert_eq!(result["next"], "watchtower jobs list");

    let (code, activity) = command(home.path(), &["activity"]);
    assert_eq!(code, 0);
    assert_eq!(activity["activity"].as_array().unwrap().len(), 1);
    assert_eq!(activity["activity"][0]["command"], "projects.update");
    assert_eq!(activity["activity"][0]["outcome"], "unknown");
    let (code, usage) = command(home.path(), &["usage"]);
    assert_eq!(code, 0);
    assert_eq!(usage["calls"], 2);
    assert!(usage["commands"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["command"] != "usage"));
    let recorded = watchtower::cli_usage::activity(home.path()).unwrap();
    assert_eq!(recorded["activity"].as_array().unwrap().len(), 3);
    assert!(recorded["activity"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["command"] == "activity" && row["outcome"] == "returned_success"));
}

#[test]
fn parser_owned_metadata_ignores_flag_shaped_values() {
    let args: Vec<_> = [
        "watchtower",
        "projects",
        "update",
        "hidden-target",
        "--name=--json",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    let metadata = watchtower::cli::runtime_metadata(&args);
    assert_eq!(metadata.command, "projects.update");
    assert_eq!(metadata.flags, ["--name"]);
    assert!(
        !serde_json::to_string(&json!({"command":metadata.command,"flags":metadata.flags}))
            .unwrap()
            .contains("hidden-target")
    );
}
