mod common;

use serde_json::{json, Value};
use std::path::Path;
use watchtower::Watchtower;

fn run(home: &Path, args: &[&str]) -> Value {
    run_expecting(home, args, 0)
}

fn run_expecting(home: &Path, args: &[&str], expected_exit: i32) -> Value {
    let mut out = Vec::new();
    let argv = ["watchtower", "--json"]
        .into_iter()
        .chain(args.iter().copied());
    let code = watchtower::cli::run(argv, home, &mut std::io::empty(), &mut out);
    let value: Value = serde_json::from_slice(&out).expect("CLI JSON");
    assert_eq!(code, expected_exit, "{args:?}: {value}");
    value
}

#[test]
fn tracked_cli_run_and_reported_task_complete_only_after_the_required_steps() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let repo = common::demo_repo(tmp.path());
    run(&home, &["projects", "add", repo.to_str().unwrap()]);
    assert_eq!(run(&home, &["runs", "status"])["state"], "unknown");

    let start = run(&home, &["runs", "start", "--no-refresh"]);
    let id = start["run_id"].to_string();
    assert_eq!(start["due"]["projects"][0]["id"], "demo");
    let failed = run(
        &home,
        &[
            "runs",
            "fail",
            &id,
            "--project",
            "demo",
            "--error",
            "Repository konnte nicht gelesen werden",
        ],
    );
    assert_eq!(failed["run"]["projects"][0]["status"], "failed");
    assert_eq!(
        run_expecting(&home, &["runs", "finish", &id], 1)["state"],
        "failed"
    );

    let context = run(&home, &["context", "demo"]);
    run(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--context-token",
            context["context_token"].as_str().unwrap(),
            "--input-json",
            &common::card().to_string(),
        ],
    );
    assert_eq!(
        run_expecting(&home, &["runs", "finish", &id], 1)["state"],
        "incomplete"
    );
    let context = run(&home, &["portfolio", "context"]);
    run(
        &home,
        &[
            "portfolio",
            "put",
            "--context-token",
            context["context_token"].as_str().unwrap(),
            "--input-json",
            &json!({"greeting":"Guten Morgen"}).to_string(),
        ],
    );
    let finished = run(&home, &["runs", "finish", &id]);
    assert_eq!(finished["run"]["state"], "complete");
    assert_eq!(finished["last_successful"]["id"].to_string(), id);

    let owner = Watchtower::open(&home.join("watchtower.db")).unwrap();
    let task = owner.create_task(json!({"projects":["demo"],"source_id":"workflow-test","title":"Export prüfen","why":"Der Export muss lesbar sein","evidence":"README","done_when":"Eine exportierte Datei lässt sich wieder öffnen"})).unwrap();
    let task_id = task["task"]["id"].as_i64().unwrap();
    assert_eq!(
        run(&home, &["tasks", "list", "--project", "demo"])["tasks"][0]["status"],
        "assigned"
    );
    let report = run(
        &home,
        &[
            "tasks",
            "report",
            &task_id.to_string(),
            "--status",
            "done",
            "--result",
            "Export lässt sich wieder öffnen",
            "--evidence",
            "cargo test export_roundtrip bestanden",
        ],
    );
    assert_eq!(report["task"]["status"], "review");
    assert_eq!(
        run(&home, &["tasks", "list"])["tasks"][0]["status"],
        "review"
    );
    owner.set_task_status(task_id, "done").unwrap();
    assert_eq!(run(&home, &["tasks", "list"])["tasks"][0]["status"], "done");
}
