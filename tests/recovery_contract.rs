use serde_json::Value;
use std::path::Path;
use watchtower::{cli, cli_usage, store::Store};

fn call(home: &Path, args: &[&str]) -> (i32, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = cli::run_with_streams(
        std::iter::once("watchtower").chain(args.iter().copied()),
        home,
        &mut std::io::empty(),
        &mut out,
        &mut err,
    );
    assert!(err.is_empty());
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn errors_and_nested_receipts_preserve_the_selected_database() {
    let home = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let store = Store::open(&home.path().join("watchtower.db")).unwrap();
    store
        .add_project("demo", "Demo", "C:/example", "2026-10-07T08:00:00Z")
        .unwrap();
    assert_eq!(call(home.path(), &["projects", "remove", "demo"]).0, 0);
    let (_, text) = call(home.path(), &["--json", "projects", "show", "demo"]);
    let body: Value = serde_json::from_str(&text).unwrap();
    let hint = body["error"]["hint"].as_str().unwrap();
    assert!(hint.contains("watchtower --instance p_"), "{hint}");
    assert!(!hint.contains("watchtower projects restore"));
    let (_, text) = call(home.path(), &["--json", "ideas", "add", "Keep history"]);
    let body: Value = serde_json::from_str(&text).unwrap();
    let command = body["idea"]["undo"].as_str().unwrap();
    assert!(command.starts_with("watchtower --instance p_"), "{command}");
    let args: Vec<_> = command.split_whitespace().skip(1).collect();
    let (code, text) = call(other.path(), &args);
    assert_eq!(code, 0, "{text}");
    assert!(!other.path().join("watchtower.db").exists());
    let (_, text) = call(home.path(), &["--json", "ideas", "list"]);
    assert!(serde_json::from_str::<Value>(&text).unwrap()["ideas"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn long_errors_page_without_repeating_the_failed_call_and_json_is_complete() {
    let home = tempfile::tempdir().unwrap();
    let unknown = format!("--{}", "z".repeat(25_000));
    let (code, text) = call(home.path(), &["projects", "list", &unknown]);
    assert_eq!(code, 2);
    assert!(text.starts_with("Error:"));
    assert!(text.chars().count() <= 10_000);
    let next = text
        .lines()
        .find_map(|line| line.strip_prefix("Next: watchtower "))
        .unwrap();
    let args: Vec<_> = next.split_whitespace().collect();
    let (code, text) = call(home.path(), &args);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("continuing does not repeat"));
    assert!(!home.path().join("watchtower.db").exists());
    let (code, text) = call(home.path(), &["--json", "projects", "list", &unknown]);
    assert_eq!(code, 2);
    let body: Value = serde_json::from_str(&text).unwrap();
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains(&unknown));
}

#[test]
fn invalid_recovery_explains_document_flags_and_names_the_requested_instance() {
    let home = tempfile::tempdir().unwrap();
    let fallback = tempfile::tempdir().unwrap();
    let (code, text) = call(
        fallback.path(),
        &[
            "--data-dir",
            home.path().to_str().unwrap(),
            "card",
            "put",
            "demo",
            "--unknown",
        ],
    );
    assert_eq!(code, 2);
    assert!(text.contains(home.path().to_str().unwrap()), "{text}");
    assert!(text.contains("--input-json"), "{text}");
    assert!(
        text.contains("--json only selects the output format"),
        "{text}"
    );
    assert!(!home.path().join("watchtower.db").exists());
    assert!(!fallback.path().join("watchtower.db").exists());

    let locator = format!(
        "p_{}",
        home.path()
            .to_str()
            .unwrap()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let attached = format!("--data-dir={}", home.path().display());
    for selector in [
        vec!["--data-dir", home.path().to_str().unwrap()],
        vec!["--instance", locator.as_str()],
        vec![attached.as_str()],
    ] {
        let mut args = vec!["--json", "projects", "list", "--unknown"];
        args.extend(selector);
        let (code, text) = call(fallback.path(), &args);
        assert_eq!(code, 2, "{text}");
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["instance"]["data_dir"], home.path().to_str().unwrap());
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_Watchtower"))
            .env("WATCHTOWER_HOME", fallback.path())
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stderr.is_empty());
        let body: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["instance"]["data_dir"], home.path().to_str().unwrap());
    }
    assert!(cli_usage::path(home.path()).exists());
    assert!(!cli_usage::path(fallback.path()).exists());
    assert!(!home.path().join("watchtower.db").exists());

    // Instance-looking payload text stays data after the option terminator.
    let (code, text) = call(
        fallback.path(),
        &[
            "--json",
            "ideas",
            "add",
            "--",
            "--data-dir",
            home.path().to_str().unwrap(),
        ],
    );
    assert_eq!(code, 2, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        body["instance"]["data_dir"],
        fallback.path().to_str().unwrap()
    );
}

#[test]
fn a_running_job_is_not_recorded_as_a_failed_operation() {
    let home = tempfile::tempdir().unwrap();
    let ticket = cli_usage::begin(home.path(), "facts.refresh", &[], 1);
    cli_usage::finish_outcome(ticket, 1, 150, Some("not_finished"));
    let report = cli_usage::report(home.path(), None).unwrap();
    assert_eq!(report["commands"][0]["failed"], 0);
    assert_eq!(report["commands"][0]["not_finished"], 1);
    assert_eq!(
        cli_usage::activity(home.path()).unwrap()["activity"][0]["outcome"],
        "not_finished"
    );
}
