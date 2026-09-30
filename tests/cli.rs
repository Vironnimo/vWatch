mod common;

use serde_json::{json, Value};

fn run(home: &std::path::Path, args: &[&str]) -> (i32, Value) {
    let mut out = Vec::new();
    let mut argv = vec!["watchtower"];
    argv.extend_from_slice(args);
    let code = watchtower::cli::run(argv, home, &mut std::io::empty(), &mut out);
    (code, serde_json::from_slice(&out).unwrap_or(Value::Null))
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
    assert_eq!((code, out["saved"].as_bool()), (0, Some(true)));

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
                "--merge"
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
        ],
    );
    assert_eq!(
        run(&home, &["card", "show", "demo"]).1["card"]["headline"],
        "Grüße"
    );

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
}
