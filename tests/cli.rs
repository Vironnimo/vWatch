mod common;

use serde_json::{json, Value};

fn run(home: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let mut out = Vec::new();
    let mut argv = vec!["watchtower"];
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
        &["card", "put", "demo", "--json", &common::card().to_string()],
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
            "--json",
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
            "--json",
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
            "--json",
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

    let (code, out) = run(&home, &["card", "put", "demo", "--json", "{broken"]);
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
    let (code, out) = run(&home, &["portfolio", "put", "--json", &brief]);
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
                "--json",
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
            "--json",
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
                "--json",
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
