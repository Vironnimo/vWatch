mod common;

use serde_json::{json, Value};

fn run(home: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let mut out = Vec::new();
    let mut argv = vec!["watchtower", "--json"];
    argv.extend_from_slice(args);
    let code = watchtower::cli::run(argv, home, &mut std::io::empty(), &mut out);
    (code, serde_json::from_slice(&out).unwrap_or(Value::Null))
}

fn card_token(home: &std::path::Path) -> String {
    run(home, &["context", "demo"]).1["context_token"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn streams(home: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let argv = std::iter::once("watchtower").chain(args.iter().copied());
    let code =
        watchtower::cli::run_with_streams(argv, home, &mut std::io::empty(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn human_success(home: &std::path::Path, args: &[&str]) -> String {
    let (code, out, err) = streams(home, args);
    assert_eq!(code, 0, "{args:?}: {err}");
    assert!(err.is_empty(), "{args:?}: {err}");
    assert!(!out.trim().is_empty(), "{args:?} needs readable output");
    assert!(
        serde_json::from_str::<Value>(&out).is_err(),
        "{args:?} must not print JSON by default: {out}"
    );
    assert!(!out.contains("\"ok\":"), "{args:?}: {out}");
    out
}

#[test]
fn ordinary_commands_present_readable_results_without_json() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let repo = common::demo_repo(tmp.path());

    assert!(human_success(&home, &["status"]).contains("Watchtower"));
    human_success(&home, &["projects", "list"]);
    let added = human_success(
        &home,
        &["projects", "add", repo.to_str().unwrap(), "--id", "demo"],
    );
    assert!(added.contains("demo"));
    let updated = human_success(&home, &["projects", "update", "demo", "--name", "vUtility"]);
    assert!(updated.contains("vUtility"));
    assert!(human_success(&home, &["projects", "list"]).contains("vUtility"));
    human_success(&home, &["due", "--no-refresh"]);
    human_success(&home, &["facts", "refresh", "demo", "--no-ci"]);
    assert!(human_success(&home, &["facts", "show", "demo"]).contains("main"));
    human_success(
        &home,
        &[
            "warnings",
            "acknowledge",
            "demo",
            "--key",
            "NO_REMOTE",
            "--note",
            "Local-only project",
        ],
    );
    human_success(&home, &["warnings", "clear", "demo", "--key", "No_Remote"]);

    let saved = human_success(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--input-json",
            &common::card().to_string(),
            "--context-token",
            &card_token(&home),
        ],
    );
    assert!(!saved.contains("\"saved\":"));
    let card = human_success(&home, &["card", "show", "demo"]);
    assert!(card.contains("Alles ruhig"));
    assert!(card.contains("Tests laufen lassen"));
    assert!(human_success(&home, &["context", "demo"]).contains("vUtility"));

    let context = run(&home, &["portfolio", "context"]).1;
    human_success(
        &home,
        &[
            "portfolio",
            "put",
            "--input-json",
            r#"{"greeting":"Guten Morgen"}"#,
            "--context-token",
            context["context_token"].as_str().unwrap(),
        ],
    );

    for args in [
        vec!["runs", "status"],
        vec!["runs", "start", "--no-refresh"],
        vec!["portfolio", "context"],
        vec!["portfolio", "show"],
        vec!["feedback"],
        vec!["ideas", "list"],
        vec!["tasks", "list"],
        vec!["schema", "card"],
        vec!["schema", "portfolio"],
        vec!["schema", "CARD"],
    ] {
        human_success(&home, &args);
    }
}

#[test]
fn json_output_requires_an_explicit_flag_at_any_command_level() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["--json", "projects", "list"],
        vec!["projects", "--json", "list"],
        vec!["projects", "list", "--json"],
    ] {
        let (code, out, err) = streams(home.path(), &args);
        assert_eq!(code, 0, "{args:?}: {err}");
        assert!(err.is_empty());
        let result: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["projects"], json!([]));
    }

    let payload = json!({"greeting": "Die Option --json bleibt Text."}).to_string();
    let args = ["portfolio", "put", "--input-json", &payload, "--dry-run"];
    human_success(home.path(), &args);
    let mut json_args = args.to_vec();
    json_args.push("--json");
    let (code, out, err) = streams(home.path(), &json_args);
    assert_eq!(code, 0, "{err}");
    assert!(err.is_empty());
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["saved"], false);

    // Values that spell the flag are data, including after the option terminator.
    human_success(home.path(), &["ideas", "add", "--", "--json"]);
    let (code, out, err) = streams(
        home.path(),
        &["portfolio", "put", "--input-json=--json", "--dry-run"],
    );
    assert_eq!(code, 1);
    assert!(err.is_empty());
    assert!(out.starts_with("Error:"));
    assert!(serde_json::from_str::<Value>(&out).is_err());

    for args in [
        vec!["portfolio", "put", "--file", "--", "--json"],
        vec!["ideas", "add", "--", "--json", "extra"],
    ] {
        let (code, out, err) = streams(home.path(), &args);
        assert_eq!(code, 2, "{args:?}");
        assert!(err.is_empty(), "{args:?}: {err}");
        assert!(out.starts_with("Error:"));
        assert!(
            serde_json::from_str::<Value>(&out).is_err(),
            "A value must not activate JSON output: {out}"
        );
    }

    // With space-separated options, clap rejects the missing value; --json is
    // still an explicit output option, not a payload beginning with a hyphen.
    for args in [
        vec!["portfolio", "put", "--input-json", "--json"],
        vec!["projects", "update", "demo", "--path", "--json"],
        vec!["card", "put", "demo", "--file", "--json"],
    ] {
        let (code, out, err) = streams(home.path(), &args);
        assert_eq!(code, 2, "{args:?}");
        assert!(err.is_empty(), "{args:?}: {err}");
        let result: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(result["error"]["code"], "invalid_arguments");
    }
}

#[test]
fn output_aliases_are_explicit_and_help_remains_readable() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["projects", "list", "--format", "json"],
        vec!["projects", "list", "--output", "json"],
        vec!["-o", "json", "project", "ls"],
    ] {
        let (code, out, err) = streams(home.path(), &args);
        assert_eq!(code, 0, "{args:?}: {out}");
        assert!(err.is_empty(), "{args:?}: {err}");
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap()["projects"],
            json!([])
        );
    }
    human_success(home.path(), &["projects", "list", "--format", "text"]);
    for args in [
        vec!["--help"],
        vec!["--format", "json", "--help"],
        vec!["--json", "projects", "update", "--help"],
        vec!["tasks", "report", "--help"],
        vec!["--output", "json", "--version"],
    ] {
        human_success(home.path(), &args);
    }

    // Every entry in the parser-generated command index has independently
    // usable help, including the output/exit contract, within the CLI budget.
    let help = human_success(home.path(), &["--help"]);
    assert!(help.chars().count() <= 10_000);
    let index = help.split("Complete command index:\n").nth(1).unwrap();
    let commands: Vec<_> = index
        .lines()
        .take_while(|line| !line.is_empty())
        .map(|line| line.trim().split_once(':').unwrap().0)
        .collect();
    assert!(commands.len() > 50);
    for command in commands {
        let mut args: Vec<_> = command.split_whitespace().collect();
        args.push("--help");
        let help = human_success(home.path(), &args);
        assert!(help.contains("Exit codes:"), "{command}: {help}");
        assert!(help.chars().count() <= 10_000, "{command}: {help}");
    }
}

#[test]
fn task_reports_accept_document_files_and_reject_mixed_or_invalid_input() {
    let home = tempfile::tempdir().unwrap();
    let (code, created) = run(
        home.path(),
        &["tasks", "create", "--title", "Export prüfen"],
    );
    assert_eq!(code, 0, "{created}");
    let id = created["task"]["id"].to_string();
    let file = home.path().join("task report.json");
    let payload = json!({
        "status": "DONE",
        "result": "Zürich und 東京\nExport enthält \"quoted text\".",
        "evidence": "Geprüft: C:/output/report.txt"
    });
    let utf16: Vec<_> = [0xff, 0xfe]
        .into_iter()
        .chain(
            payload
                .to_string()
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        )
        .collect();
    std::fs::write(&file, utf16).unwrap();
    let args = ["tasks", "report", &id, "--file", file.to_str().unwrap()];
    let (code, reported) = run(home.path(), &args);
    assert_eq!(code, 0, "{reported}");
    assert_eq!(reported["task"]["status"], "review");
    assert_eq!(reported["task"]["result"], payload["result"]);
    assert_eq!(reported["task"]["result_evidence"], payload["evidence"]);
    assert_eq!(run(home.path(), &args).1["already_reported"], true);

    for extra in [
        vec!["--status", "blocked"],
        vec!["--result", "Ignored result"],
        vec!["--evidence", "Ignored evidence"],
        vec!["--input-json", "{}"],
    ] {
        let mut mixed = args.to_vec();
        mixed.extend(extra);
        let (code, result) = run(home.path(), &mixed);
        assert_eq!(code, 2, "{mixed:?}: {result}");
        assert_eq!(result["error"]["code"], "invalid_arguments");
    }
    let (_, text, _) = streams(home.path(), &["tasks", "report", &id, "--bogus"]);
    assert!(!text.contains("--input-json"), "{text}");

    for bad in [
        "{broken",
        "null",
        r#"{"status":"done","result":"Missing evidence"}"#,
    ] {
        std::fs::write(&file, bad).unwrap();
        assert_eq!(run(home.path(), &args).0, 1);
        assert_eq!(
            run(home.path(), &["tasks", "show", &id]).1["task"],
            reported["task"]
        );
    }

    let blocked = br#"{"status":"blocked","result":"Awaiting local test data"}"#;
    let mut out = Vec::new();
    let code = watchtower::cli::run(
        [
            "watchtower",
            "--json",
            "tasks",
            "report",
            &id,
            "--file",
            "-",
        ],
        home.path(),
        &mut blocked.as_slice(),
        &mut out,
    );
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&out));
    let result: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(result["task"]["status"], "blocked");
    assert_eq!(result["task"]["result_evidence"], "");
}

#[test]
fn invalid_flags_or_unrecognized_targets_cannot_mutate_a_nearby_project() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let repo = common::demo_repo(tmp.path());
    assert_eq!(
        run(
            &home,
            &["projects", "add", repo.to_str().unwrap(), "--id", "demo"]
        )
        .0,
        0
    );
    let before = run(&home, &["projects", "list"]).1["projects"].clone();
    for args in [
        vec![
            "projects",
            "update",
            "demo",
            "--name",
            "Must not save",
            "--include-everything",
        ],
        vec!["projects", "update", "dem", "--name", "Must not save"],
        vec!["projects", "status", "<project-id>", "parked"],
        vec!["projects", "remove", "dem"],
        vec!["projects", "remove", "demo", "--include-everything"],
        vec![
            "warnings",
            "acknowledge",
            "demo",
            "--key",
            "no_remot",
            "--note",
            "Must not save",
        ],
        vec![
            "projects",
            "goal",
            "demo",
            "Must not save",
            "--due",
            "09/10/2026",
        ],
    ] {
        let (code, out, err) = streams(&home, &args);
        assert!(matches!(code, 1 | 2), "{args:?}: {out}");
        assert!(err.is_empty(), "{args:?}: {err}");
        assert!(out.starts_with("Error:"), "{args:?}: {out}");
        assert!(
            out.lines()
                .skip(1)
                .any(|line| !line.trim().is_empty() && !line.starts_with("Note: non-default")),
            "Recovery guidance must follow the outcome: {args:?}: {out}"
        );
        assert_eq!(
            run(&home, &["projects", "list"]).1["projects"],
            before,
            "{args:?}"
        );
    }
}

#[test]
fn expected_errors_keep_recovery_on_stdout_in_both_output_formats() {
    let home = tempfile::tempdir().unwrap();
    for (args, expected_code) in [
        (vec!["context", "missing"], 1),
        (vec!["unknown-command"], 2),
        (vec!["projects", "status", "missing", "sleeping"], 2),
    ] {
        let (code, out, err) = streams(home.path(), &args);
        assert_eq!(code, expected_code);
        assert!(err.is_empty(), "{args:?}: {err}");
        assert!(out.starts_with("Error:"), "{args:?}: {out}");
        assert!(
            out.contains("Usage:") || out.contains("projects") || out.contains("possible values:"),
            "The recovery must identify accepted arguments or an available project operation: {args:?}: {out}"
        );
        assert!(serde_json::from_str::<Value>(&out).is_err());

        for position in [0, args.len()] {
            let mut json_args = args.clone();
            json_args.insert(position, "--json");
            let (code, out, err) = streams(home.path(), &json_args);
            assert_eq!(code, expected_code);
            assert!(err.is_empty(), "{args:?}: {err}");
            let result: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(result["ok"], false);
            assert!(result["error"]["code"].is_string());
            assert!(result["error"]["message"].is_string());
            assert!(result["error"]["hint"].is_string());
        }
    }
    let (_, out, err) = streams(home.path(), &["context", "missing"]);
    assert!(err.is_empty());
    assert!(out.contains("missing"));
    assert!(out.contains("projects"));
}

#[test]
fn cli_prints_json_for_success_and_expected_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let repo = common::demo_repo(tmp.path());

    let (code, out) = run(
        &home,
        &["projects", "add", repo.to_str().unwrap(), "--id", "demo"],
    );
    assert_eq!(
        (code, out["ok"].as_bool(), out["project"]["id"].as_str()),
        (0, Some(true), Some("demo"))
    );

    let (code, out) = run(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--input-json",
            &common::card().to_string(),
        ],
    );
    assert_eq!(
        (code, out["error"]["code"].as_str()),
        (1, Some("context_required"))
    );
    let token = card_token(&home);
    let (code, out) = run(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--input-json",
            &common::card().to_string(),
            "--context-token",
            &token,
        ],
    );
    assert_eq!((code, out["saved"].as_bool()), (0, Some(true)));
    // Replaying the identical accepted write may be idempotent. A stale token
    // must still reject a different document rather than overwrite new work.
    let stale_edit = common::card_with(json!({"headline": "Veralteter anderer Entwurf"}));
    let (code, out) = run(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--input-json",
            &stale_edit.to_string(),
            "--context-token",
            &token,
        ],
    );
    assert_eq!(
        (code, out["error"]["code"].as_str()),
        (1, Some("version_conflict"))
    );
    assert_eq!(run(&home, &["card", "show", "demo"]).1["version"], 1);
    let (code, out) = run(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--input-json",
            &common::card().to_string(),
            "--dry-run",
        ],
    );
    assert_eq!((code, out["saved"].as_bool()), (0, Some(false)));

    // UTF-8 with BOM and UTF-16 (Windows PowerShell's default) both read.
    let patch = tmp.path().join("patch.json");
    std::fs::write(
        &patch,
        [b"\xEF\xBB\xBF".as_slice(), br#"{"headline": "Aus Datei"}"#].concat(),
    )
    .unwrap();
    assert_eq!(
        run(
            &home,
            &[
                "card",
                "put",
                "demo",
                "--file",
                patch.to_str().unwrap(),
                "--merge",
                "--context-token",
                &card_token(&home),
            ]
        )
        .1["version"],
        2
    );
    let utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(r#"{"headline": "Grüße"}"#.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    std::fs::write(&patch, utf16).unwrap();
    run(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--file",
            patch.to_str().unwrap(),
            "--merge",
            "--context-token",
            &card_token(&home),
        ],
    );
    assert_eq!(
        run(&home, &["card", "show", "demo"]).1["card"]["headline"],
        "Grüße"
    );

    // Malformed text must never be saved after silent character replacement or truncation.
    let version = run(&home, &["card", "show", "demo"]).1["version"].clone();
    let mut truncated_utf16: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(r#"{"headline":"Changed"}"#.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    truncated_utf16.push(0);
    let mut invalid_utf16 = vec![0xFF, 0xFE];
    invalid_utf16.extend(r#"{"headline":""#.encode_utf16().flat_map(u16::to_le_bytes));
    invalid_utf16.extend(0xD800u16.to_le_bytes());
    invalid_utf16.extend("\"}".encode_utf16().flat_map(u16::to_le_bytes));
    for bytes in [
        b"{\"headline\":\"Bad \xff text\"}".to_vec(),
        truncated_utf16,
        invalid_utf16,
    ] {
        std::fs::write(&patch, &bytes).unwrap();
        let (code, out) = run(
            &home,
            &[
                "card",
                "put",
                "demo",
                "--file",
                patch.to_str().unwrap(),
                "--merge",
            ],
        );
        assert_eq!(
            (code, out["error"]["code"].as_str()),
            (1, Some("invalid_json"))
        );
        assert_eq!(run(&home, &["card", "show", "demo"]).1["version"], version);
        assert_eq!(
            run(&home, &["card", "show", "demo"]).1["card"]["headline"],
            "Grüße"
        );
    }

    let (code, out) = run(&home, &["context", "nope"]);
    assert_eq!(code, 1);
    assert_eq!(out["ok"], false);
    assert_eq!(out["error"]["code"], "project_not_found");
    assert!(out["error"]["message"].as_str().unwrap().contains("nope"));
    assert!(out["error"]["hint"].as_str().unwrap().contains("demo"));

    let (code, out) = run(&home, &["card", "put", "demo", "--input-json", "{broken"]);
    assert_eq!(
        (code, out["error"]["code"].as_str()),
        (1, Some("invalid_json"))
    );

    let (code, out) = run(&home, &["schema", "card"]);
    assert_eq!(
        (code, out["schema"]["required"].clone()),
        (0, json!(["one_liner", "phase", "headline", "reentry"]))
    );

    assert_eq!(run(&home, &["projects", "status", "demo", "sleeping"]).0, 2);

    let brief = json!({"greeting":"Moin"}).to_string();
    let (code, out) = run(&home, &["portfolio", "put", "--input-json", &brief]);
    assert_eq!(
        (code, out["error"]["code"].as_str()),
        (1, Some("context_required"))
    );
    let token = run(&home, &["portfolio", "context"]).1["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        run(
            &home,
            &[
                "portfolio",
                "put",
                "--input-json",
                &brief,
                "--context-token",
                &token
            ]
        )
        .0,
        0
    );
    let (code, out) = run(
        &home,
        &[
            "portfolio",
            "put",
            "--input-json",
            &json!({"greeting":"Veralteter anderer Entwurf"}).to_string(),
            "--context-token",
            &token,
        ],
    );
    assert_eq!(
        (code, out["error"]["code"].as_str()),
        (1, Some("version_conflict"))
    );
}

#[test]
fn project_update_cli_preserves_the_registered_id_and_card_history() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let repo = common::demo_repo(tmp.path());
    assert_eq!(
        run(&home, &["projects", "add", repo.to_str().unwrap()]).0,
        0
    );
    assert_eq!(
        run(
            &home,
            &[
                "card",
                "put",
                "demo",
                "--input-json",
                &common::card().to_string(),
                "--context-token",
                &card_token(&home)
            ]
        )
        .0,
        0
    );
    let moved = tmp.path().join("vUtility");
    std::fs::rename(repo, &moved).unwrap();
    let (code, updated) = run(
        &home,
        &[
            "projects",
            "update",
            "demo",
            "--name",
            "vUtility",
            "--path",
            moved.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(updated["project"]["id"], "demo");
    assert_eq!(updated["project"]["name"], "vUtility");
    assert_eq!(updated["changed"], true);
    assert_eq!(updated["facts_refresh"]["ok"], true);
    assert_eq!(run(&home, &["card", "show", "demo"]).1["version"], 1);
    assert_eq!(
        run(&home, &["projects", "update", "demo", "--name", "vUtility"]).1["changed"],
        false
    );
    assert_eq!(
        run(&home, &["projects", "rename", "demo", "Another Name"]).1["project"]["name"],
        "Another Name"
    );
    for args in [
        vec!["projects", "update", "demo"],
        vec!["projects", "update", "demo", "--path", ""],
    ] {
        let (code, rejected) = run(&home, &args);
        assert_eq!(code, 1);
        assert!(rejected["error"]["hint"].is_string());
    }
    assert_eq!(run(&home, &["card", "show", "demo"]).1["version"], 1);
}
