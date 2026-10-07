mod common;

use std::path::Path;

use serde_json::{json, Value};

fn call(home: &Path, args: &[&str]) -> (i32, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = watchtower::cli::run_with_streams(
        std::iter::once("watchtower").chain(args.iter().copied()),
        home,
        &mut std::io::empty(),
        &mut out,
        &mut err,
    );
    assert!(
        err.is_empty(),
        "Command errors belong to stdout: {args:?}: {err:?}"
    );
    (code, String::from_utf8(out).unwrap())
}

fn machine(home: &Path, args: &[&str]) -> (i32, Value) {
    let mut flags = vec!["--json"];
    flags.extend_from_slice(args);
    let (code, text) = call(home, &flags);
    assert!(
        text.is_ascii(),
        "JSON pipe output must preserve Unicode in PowerShell 5.1"
    );
    (
        code,
        serde_json::from_str(&text).unwrap_or_else(|_| panic!("{args:?}: {text}")),
    )
}

fn ok(home: &Path, args: &[&str]) -> Value {
    let (code, body) = machine(home, args);
    assert_eq!(code, 0, "{args:?}: {body}");
    assert_eq!(body["ok"], true, "{args:?}: {body}");
    body
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("data");
    std::fs::create_dir(&home).unwrap();
    let repo = common::demo_repo(temp.path());
    let (svc, _) = common::service(&home);
    svc.add_project(repo.to_str().unwrap(), Some("demo"), Some("Demo"))
        .unwrap();
    svc.put_card("demo", common::card(), false, false).unwrap();
    (temp, home)
}

#[test]
fn overview_project_details_search_visits_and_settings_are_available_without_the_window() {
    let (_temp, home) = fixture();
    let status = ok(&home, &["status"]);
    assert!(chrono::DateTime::parse_from_rfc3339(status["now"].as_str().unwrap()).is_ok());
    assert_eq!(
        std::path::Path::new(status["logs"]["usage"].as_str().unwrap()),
        home.join("cli-usage.jsonl")
    );
    let state = ok(&home, &["overview"]);
    assert!(state["projects"][0]["history"].is_array());
    assert!(state["changes"]["token"].is_string());
    assert!(state["run_status"].is_object());
    let project = ok(&home, &["project", "get", "demo"]);
    assert_eq!(project["project"]["card"]["headline"], "Alles ruhig");
    assert_eq!(project["project"]["history"].as_array().unwrap().len(), 1);
    assert!(project["project"]["warnings"].is_array());
    let results = ok(&home, &["projects", "ls", "--search", "TESTPROJEKT"]);
    assert_eq!(results["projects"].as_array().unwrap().len(), 1);
    assert_eq!(
        ok(&home, &["projects", "search", "Testprojekt"])["projects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        ok(&home, &["projects", "list", "--query", "one_liner"])["projects"],
        json!([]),
        "search text values, not serialized field names"
    );
    assert_eq!(
        ok(&home, &["projects", "list", "--query", "absent"])["projects"],
        json!([])
    );
    let (code, error) = machine(&home, &["projects", "show", "missing"]);
    assert_eq!(code, 1);
    assert_eq!(error["error"]["code"], "project_not_found");
    assert!(error["error"]["hint"].is_string());

    let (svc, _) = common::service(&home);
    assert_eq!(svc.store().pref("last_visit").unwrap(), None);
    ok(&home, &["projects", "visit", "demo"]);
    assert!(svc
        .store()
        .project("demo")
        .unwrap()
        .unwrap()
        .opened_at
        .is_some());
    ok(&home, &["overview", "--visit"]);
    assert!(svc.store().pref("last_visit").unwrap().is_some());

    let before = ok(&home, &["settings", "show"]);
    assert!(before["settings"]["agent_access"].is_boolean());
    let updated = ok(
        &home,
        &[
            "settings",
            "update",
            "--style",
            "BOARD",
            "--hidden-modules",
            "ideas,week",
        ],
    );
    assert_eq!(updated["prefs"]["style"], "board");
    assert_eq!(updated["prefs"]["hidden_modules"], json!(["ideas", "week"]));
    assert_eq!(
        updated["settings"]["autostart"],
        before["settings"]["autostart"]
    );
    assert_eq!(
        updated["settings"]["agent_access"],
        before["settings"]["agent_access"]
    );
    let (code, invalid) = machine(
        &home,
        &[
            "settings",
            "update",
            "--style",
            "handover",
            "--hidden-modules",
            "unknown-module",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(invalid["error"]["code"], "invalid_pref");
    assert_eq!(
        ok(&home, &["settings", "show"])["prefs"],
        updated["prefs"],
        "validate all fields before changing one"
    );
}

#[test]
fn owner_feedback_warning_and_digest_commands_preserve_explicit_intent() {
    let (_temp, home) = fixture();
    let card = ok(&home, &["card", "show", "demo"]);
    let item = card["card"]["suggestions"][0]["id"].as_str().unwrap();
    let reaction = ok(
        &home,
        &[
            "feedback",
            "set",
            item,
            "--value",
            "LATER",
            "--note",
            "Morgen prüfen",
            "--until",
            "2099-12-31",
        ],
    );
    assert_eq!(reaction["value"], "later");
    assert_eq!(reaction["note"], "Morgen prüfen");
    assert_eq!(
        ok(&home, &["feedback", "list", "--project", "demo"])["feedback"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let snapshot =
        json!({"value":"later", "note":"Morgen prüfen", "snoozed_until":"2099-12-31"}).to_string();
    ok(&home, &["feedback", "clear", item]);
    assert_eq!(ok(&home, &["feedback"])["feedback"], json!([]));
    ok(
        &home,
        &["feedback", "restore", item, "--input-json", &snapshot],
    );
    assert_eq!(
        ok(&home, &["feedback"])["feedback"][0]["note"],
        "Morgen prüfen"
    );
    let (code, invalid) = machine(
        &home,
        &[
            "feedback",
            "--project",
            "other",
            "set",
            item,
            "--value",
            "down",
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(invalid["error"]["code"], "invalid_arguments");
    assert_eq!(ok(&home, &["feedback"])["feedback"][0]["value"], "later");

    let warnings = ok(&home, &["warnings", "list", "demo"]);
    assert!(warnings["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["key"] == "no_remote"));
    let ack = ok(
        &home,
        &[
            "warnings",
            "acknowledge",
            "demo",
            "--key",
            "no_remote",
            "--note",
            "Bewusst lokal",
        ],
    );
    assert!(ack["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["key"] == "no_remote" && w["acknowledged"] == true));
    ok(&home, &["warnings", "clear", "demo", "--key", "no_remote"]);
    assert_eq!(
        ok(&home, &["warnings", "list", "demo"])["acknowledgements"],
        json!([])
    );
    let (code, invalid) = machine(
        &home,
        &[
            "warnings",
            "acknowledge",
            "demo",
            "--key",
            "worktrees",
            "--note",
            "Intentional",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(invalid["error"]["code"], "warning_not_present");

    let changes = ok(&home, &["changes", "show"]);
    let token = changes["changes"]["token"].as_str().unwrap();
    let read = ok(&home, &["changes", "read", "--token", token]);
    assert_eq!(read["changes"]["has_baseline"], true);
    let (code, stale) = machine(
        &home,
        &["changes", "read", "--token", "not-a-current-snapshot"],
    );
    assert_eq!(code, 1);
    assert_eq!(stale["error"]["code"], "stale_changes");
}

#[test]
fn tasks_support_assignment_reporting_review_and_reopening_through_cli() {
    let (temp, home) = fixture();
    let created = ok(
        &home,
        &[
            "tasks",
            "create",
            "--project",
            "demo",
            "--title",
            "Prüfe Übergabe 🦊",
            "--done-when",
            "Tests bestehen",
            "--source-id",
            "task-fixture",
        ],
    );
    let id = created["task"]["id"].to_string();
    let repeated = ok(
        &home,
        &[
            "tasks",
            "create",
            "--project",
            "demo",
            "--title",
            "Prüfe Übergabe 🦊",
            "--done-when",
            "Tests bestehen",
            "--source-id",
            "task-fixture",
        ],
    );
    assert_eq!(repeated["created"], false);
    assert_eq!(repeated["task"]["id"], created["task"]["id"]);
    let shown = ok(&home, &["tasks", "show", &id]);
    assert_eq!(shown["task"]["title"], "Prüfe Übergabe 🦊");
    let before_revision = shown["task"]["revision"].to_string();
    let reported = ok(
        &home,
        &[
            "tasks",
            "report",
            &id,
            "--status",
            "DONE",
            "--result",
            "Erledigt",
            "--evidence",
            "Alle Tests bestehen",
        ],
    );
    assert_eq!(reported["task"]["status"], "review");
    let (code, changed) = machine(
        &home,
        &["tasks", "confirm", &id, "--revision", &before_revision],
    );
    assert_eq!(code, 1);
    assert_eq!(changed["error"]["code"], "task_changed");
    let confirmed = ok(
        &home,
        &[
            "tasks",
            "confirm",
            &id,
            "--revision",
            &reported["task"]["revision"].to_string(),
        ],
    );
    assert_eq!(confirmed["task"]["status"], "done");
    let reopened = ok(
        &home,
        &[
            "tasks",
            "reopen",
            &id,
            "--revision",
            &confirmed["task"]["revision"].to_string(),
        ],
    );
    assert_eq!(reopened["task"]["status"], "assigned");
    let cancelled = ok(
        &home,
        &[
            "tasks",
            "cancel",
            &id,
            "--revision",
            &reopened["task"]["revision"].to_string(),
        ],
    );
    assert_eq!(cancelled["task"]["status"], "cancelled");
    assert_eq!(cancelled["task"]["history"].as_array().unwrap().len(), 5);
    let (code, missing) = machine(
        &home,
        &[
            "tasks", "report", "99999", "--status", "done", "--result", "done",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(
        missing["error"]["code"], "unknown_task",
        "target recovery precedes missing evidence"
    );

    let path = temp.path().join("task.json");
    std::fs::write(
        &path,
        json!({"projects":["demo"],"title":"Long document", "why":"line one\nline two"})
            .to_string(),
    )
    .unwrap();
    let imported = ok(
        &home,
        &["tasks", "create", "--file", path.to_str().unwrap()],
    );
    assert_eq!(imported["task"]["why"], "line one\nline two");
}

#[test]
fn removed_projects_ideas_and_saved_documents_can_be_restored() {
    let (_temp, home) = fixture();
    let before = ok(&home, &["card", "show", "demo"]);
    let removed = ok(&home, &["projects", "remove", "demo"]);
    assert!(removed["undo"]
        .as_str()
        .unwrap()
        .ends_with("projects restore demo"));
    assert_eq!(ok(&home, &["projects", "list"])["projects"], json!([]));
    assert_eq!(
        ok(&home, &["projects", "trash"])["projects"][0]["id"],
        "demo"
    );
    assert_eq!(
        ok(&home, &["projects", "remove", "demo"])["already_removed"],
        true
    );
    ok(&home, &["projects", "restore", "demo"]);
    assert_eq!(ok(&home, &["card", "show", "demo"])["card"], before["card"]);

    let token = ok(&home, &["context", "demo"])["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let saved = ok(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--merge",
            "--input-json",
            "{\"headline\":\"Changed\"}",
            "--context-token",
            &token,
        ],
    );
    let undo = saved["undo"].as_str().unwrap();
    let undo_args: Vec<&str> = undo.split_whitespace().skip(1).collect();
    ok(&home, &undo_args);
    assert_eq!(ok(&home, &["card", "show", "demo"])["card"], before["card"]);
    let history = ok(&home, &["card", "history", "demo"]);
    assert_eq!(history["versions"].as_array().unwrap().len(), 3);
    assert_eq!(
        ok(&home, &["card", "show", "demo", "--version", "2"])["card"]["headline"],
        "Changed"
    );

    let idea = ok(&home, &["ideas", "add", "Archive fixture"]);
    let id = idea["idea"]["id"].to_string();
    ok(&home, &["ideas", "remove", &id]);
    assert_eq!(
        ok(&home, &["ideas", "trash"])["ideas"][0]["id"],
        idea["idea"]["id"]
    );
    ok(&home, &["ideas", "restore", &id]);
    assert_eq!(
        ok(&home, &["ideas", "list"])["ideas"][0]["text"],
        "Archive fixture"
    );

    let token = ok(&home, &["portfolio", "context"])["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        &home,
        &[
            "portfolio",
            "put",
            "--input-json",
            "{\"greeting\":\"First\"}",
            "--context-token",
            &token,
        ],
    );
    let token = ok(&home, &["portfolio", "context"])["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let saved = ok(
        &home,
        &[
            "portfolio",
            "put",
            "--input-json",
            "{\"greeting\":\"Second\"}",
            "--context-token",
            &token,
        ],
    );
    let undo_args: Vec<&str> = saved["undo"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .skip(1)
        .collect();
    ok(&home, &undo_args);
    assert_eq!(
        ok(&home, &["portfolio", "show"])["portfolio"]["greeting"],
        "First"
    );
    assert_eq!(
        ok(&home, &["portfolio", "history"])["versions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        ok(&home, &["portfolio", "show", "--version", "2"])["portfolio"]["greeting"],
        "Second"
    );
}

#[test]
fn all_added_commands_have_readable_output_and_errors_do_not_require_help() {
    let (_temp, home) = fixture();
    for args in [
        vec!["overview"],
        vec!["projects", "show", "demo"],
        vec!["projects", "list", "--query", "demo"],
        vec!["warnings", "list", "demo"],
        vec!["changes", "show"],
        vec!["settings", "show"],
        vec!["feedback", "list"],
        vec!["card", "history", "demo"],
        vec!["portfolio", "history"],
        vec!["projects", "trash"],
        vec!["ideas", "trash"],
    ] {
        let (code, output) = call(&home, &args);
        assert_eq!(code, 0, "{args:?}: {output}");
        assert!(!output.trim().is_empty());
        assert!(
            serde_json::from_str::<Value>(&output).is_err(),
            "{args:?} must be readable text"
        );
        assert!(
            output.chars().count() <= 10000,
            "{args:?}: {}",
            output.chars().count()
        );
    }
    for args in [
        vec!["card", "put", "demo", "--file", "missing-document.json"],
        vec!["card", "put", "demo", "--file", "-"],
        vec!["card", "put", "demo", "--input-json", "{"],
        vec!["card", "put", "demo"],
    ] {
        let (code, error) = call(&home, &args);
        assert_eq!(code, 1, "{args:?}: {error}");
        assert!(error.starts_with("Error:"), "{error}");
        assert!(error.contains("Nothing was saved"), "{error}");
        assert!(error.contains("--file"), "{error}");
    }
    let (code, error) = call(
        &home,
        &["tasks", "report", "1", "--status", "typo", "--result", "x"],
    );
    assert_eq!(code, 2);
    assert!(
        error.contains("done")
            && error.contains("blocked")
            && error.contains("Nothing was changed")
    );
    let (code, help) = call(&home, &["--help"]);
    assert_eq!(code, 0);
    assert!(
        help.chars().count() <= 10000,
        "Generated command index must fit one help call: {} characters",
        help.chars().count()
    );
    for command in [
        "feedback restore",
        "tasks confirm",
        "warnings acknowledge",
        "changes read",
        "settings update",
        "card history",
        "projects restore",
    ] {
        assert!(
            help.contains(command),
            "missing {command} from generated help"
        );
    }
}

#[test]
fn output_aliases_and_explicit_instance_selection_do_not_guess_targets() {
    let (temp, home) = fixture();
    let unused = temp.path().join("unused");
    for format in ["--format", "--output", "-o"] {
        let (code, output) = call(
            &unused,
            &[
                "--data-dir",
                home.to_str().unwrap(),
                format,
                "JSON",
                "project",
                "get",
                "demo",
            ],
        );
        assert_eq!(code, 0, "{output}");
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap()["project"]["id"],
            "demo"
        );
    }
    assert!(
        !unused.exists(),
        "per-call instance selection must not touch the fallback"
    );
    let (code, output) = call(
        &home,
        &[
            "--output",
            "json",
            "projects",
            "list",
            "--searchall",
            "demo",
        ],
    );
    assert_eq!(code, 2);
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap()["error"]["code"],
        "invalid_arguments"
    );
    let (code, output) = call(
        &home,
        &["projects", "update", "dem", "--name", "Wrong target"],
    );
    assert_eq!(code, 1);
    assert!(output.contains("dem"));
    assert_eq!(
        ok(&home, &["projects", "show", "demo"])["project"]["name"],
        "Demo"
    );
}

#[test]
fn batch_registration_preserves_successes_and_reports_partial_failure() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("data");
    let first = common::make_repo(temp.path(), "first", &["2026-09-29T10:00:00"]);
    let second = common::make_repo(temp.path(), "second", &["2026-09-29T10:00:00"]);
    let missing = temp.path().join("does-not-exist");
    let (code, batch) = machine(
        &home,
        &[
            "projects",
            "add",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            missing.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(batch["ok"], false);
    assert_eq!(batch["added"].as_array().unwrap().len(), 2);
    assert_eq!(batch["failed"].as_array().unwrap().len(), 1);
    assert_eq!(batch["refreshed"].as_array().unwrap().len(), 2);
    assert_eq!(
        ok(&home, &["projects", "list"])["projects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let repeat = ok(
        &home,
        &[
            "projects",
            "add",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ],
    );
    assert_eq!(repeat["already_registered"].as_array().unwrap().len(), 2);
    let (code, rejected) = machine(
        &home,
        &[
            "projects",
            "add",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "--name",
            "ambiguous",
        ],
    );
    assert_ne!(code, 0);
    assert_eq!(rejected["error"]["code"], "invalid_arguments");
    assert_eq!(
        ok(&home, &["projects", "show", "first"])["project"]["name"],
        "first"
    );
}

#[test]
fn document_file_failures_name_the_location_and_keep_existing_cards() {
    let (temp, home) = fixture();
    let before = ok(&home, &["card", "show", "demo"]);
    let broken = temp.path().join("invalid-encoding.json");
    std::fs::write(&broken, [0xff, 0xfe, 0x3d]).unwrap();
    for path in [temp.path(), broken.as_path()] {
        let (code, error) = call(
            &home,
            &["card", "put", "demo", "--file", path.to_str().unwrap()],
        );
        assert_eq!(code, 1);
        assert!(
            error.contains(path.file_name().unwrap().to_str().unwrap()),
            "{error}"
        );
        assert!(error.contains("Nothing was saved"), "{error}");
        assert!(
            error.contains("--file") || error.contains("UTF-8"),
            "{error}"
        );
    }
    assert_eq!(ok(&home, &["card", "show", "demo"])["card"], before["card"]);
}

#[test]
fn owner_changes_return_an_executable_undo_without_overwriting_newer_changes() {
    let (_temp, home) = fixture();
    let before = ok(&home, &["settings", "show"])["prefs"].clone();
    let changed = ok(
        &home,
        &[
            "settings",
            "update",
            "--style",
            "board",
            "--hidden-modules",
            "ideas",
        ],
    );
    let undo_args: Vec<&str> = changed["undo"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .skip(1)
        .collect();
    let undone = ok(&home, &undo_args);
    assert_eq!(undone["restored"], true);
    assert_eq!(ok(&home, &["settings", "show"])["prefs"], before);
    assert_eq!(
        ok(&home, &undo_args)["changed"],
        false,
        "retry of undo is safe"
    );

    let changed = ok(&home, &["projects", "update", "demo", "--name", "Earlier"]);
    ok(&home, &["projects", "update", "demo", "--name", "Newer"]);
    let undo_args: Vec<&str> = changed["undo"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .skip(1)
        .collect();
    let (code, rejected) = machine(&home, &undo_args);
    assert_eq!(code, 1);
    assert!(rejected["error"]["hint"].is_string());
    assert_eq!(
        ok(&home, &["projects", "show", "demo"])["project"]["name"],
        "Newer"
    );
}
