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
        assert_eq!(
            serde_json::from_str::<Value>(&out).unwrap(),
            json!({"ok": true, "projects": []})
        );
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
    assert!(out.is_empty());
    assert!(!err.is_empty());
    assert!(serde_json::from_str::<Value>(&err).is_err());

    for args in [
        vec!["portfolio", "put", "--file", "--", "--json"],
        vec!["ideas", "add", "--", "--json", "extra"],
    ] {
        let (code, out, err) = streams(home.path(), &args);
        assert_eq!(code, 2, "{args:?}");
        assert!(
            out.is_empty(),
            "A value must not activate JSON output: {out}"
        );
        assert!(!err.is_empty());
        assert!(serde_json::from_str::<Value>(&err).is_err());
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
fn human_errors_use_stderr_while_json_errors_preserve_the_envelope() {
    let home = tempfile::tempdir().unwrap();
    for (args, expected_code) in [
        (vec!["context", "missing"], 1),
        (vec!["unknown-command"], 2),
        (vec!["projects", "status", "missing", "sleeping"], 2),
    ] {
        let (code, out, err) = streams(home.path(), &args);
        assert_eq!(code, expected_code);
        assert!(out.is_empty(), "{args:?}: {out}");
        assert!(!err.is_empty());
        assert!(serde_json::from_str::<Value>(&err).is_err());

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
    let (_, _, err) = streams(home.path(), &["context", "missing"]);
    assert!(err.contains("missing"));
    assert!(err.contains("Hinweis:"));
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
    let expected = json!({"ok": false, "error": {"code": "project_not_found", "message": "No project with id 'nope'.", "hint": "Known project ids: demo."}});
    assert_eq!(out, expected);

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
            &brief,
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
