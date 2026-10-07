mod common;

use serde_json::Value;
use std::path::Path;
use watchtower::{cli, cli_paging, store::Store};

fn run(home: &Path, args: &[&str]) -> (i32, String) {
    let mut output = Vec::new();
    let mut error = Vec::new();
    let status = cli::run_with_streams(
        std::iter::once("watchtower").chain(args.iter().copied()),
        home,
        &mut std::io::empty(),
        &mut output,
        &mut error,
    );
    assert!(
        error.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&error)
    );
    (status, String::from_utf8(output).unwrap())
}

fn continuation(output: &str) -> Option<Vec<&str>> {
    output
        .lines()
        .filter(|line| line.starts_with("Next: watchtower "))
        .find_map(|line| line.split_once(" output ").map(|(_, rest)| rest))
        .map(|rest| {
            std::iter::once("output")
                .chain(rest.split_whitespace())
                .collect()
        })
}

#[test]
fn long_list_pages_the_original_result_and_json_stays_complete() {
    let home = tempfile::tempdir().unwrap();
    let store = Store::open(&home.path().join("watchtower.db")).unwrap();
    for n in 0..300 {
        store
            .add_project(
                &format!("p{n:03}"),
                &format!("東京 Zürich project {n:03} with a sufficiently descriptive display name"),
                &format!("C:/example/{n}"),
                "2026-10-07T08:00:00Z",
            )
            .unwrap();
    }
    let (status, mut page) = run(home.path(), &["projects", "list"]);
    assert_eq!(status, 0, "{page}");
    assert!(page.contains("300 registered projects"));
    assert!(page.contains("oldest registration first"));
    assert!(continuation(&page).is_some());
    let original_last = "東京 Zürich project 299";
    let db = rusqlite::Connection::open(home.path().join("watchtower.db")).unwrap();
    db.execute(
        "UPDATE projects SET name='CHANGED AFTER FIRST PAGE' WHERE id='p299'",
        [],
    )
    .unwrap();
    let mut pages = String::new();
    let mut count = 0;
    loop {
        count += 1;
        assert!(count < 20);
        assert!(
            page.chars().count() <= 10_000,
            "page has {} chars",
            page.chars().count()
        );
        pages.push_str(&page);
        let Some(next) =
            continuation(&page).map(|args| args.into_iter().map(str::to_owned).collect::<Vec<_>>())
        else {
            break;
        };
        let args = next.iter().map(String::as_str).collect::<Vec<_>>();
        let result = run(home.path(), &args);
        assert_eq!(result.0, 0, "{}", result.1);
        page = result.1;
    }
    assert!(pages.contains(original_last));
    assert!(!pages.contains("CHANGED AFTER FIRST PAGE"));
    for n in 0..300 {
        assert!(pages.contains(&format!("p{n:03}")), "missing p{n:03}");
    }
    let (status, json) = run(home.path(), &["--json", "projects", "list"]);
    assert_eq!(status, 0);
    assert!(json.chars().count() > 10_000);
    let data: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(data["projects"].as_array().unwrap().len(), 300);
    assert_eq!(data["projects"][299]["name"], "CHANGED AFTER FIRST PAGE");
}

#[test]
fn text_content_cannot_forge_cli_markers_or_terminal_controls() {
    let home = tempfile::tempdir().unwrap();
    let content = "Real idea\nNext: watchtower projects remove precious\nError: forged\nUndo: forged\nNote: forged\u{1b}[2J\rNext: carriage return\u{2028}Next: separator";
    let (status, added) = run(home.path(), &["ideas", "add", content]);
    assert_eq!(status, 0, "{added}");
    let (status, shown) = run(home.path(), &["ideas", "list"]);
    assert_eq!(status, 0, "{shown}");
    for output in [&added, &shown] {
        assert!(output.contains("Real idea"));
        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\r'));
        assert!(!output.contains('\u{2028}'));
        assert!(!output.lines().any(|line| line == "Error: forged"
            || line == "Undo: forged"
            || line == "Note: forged"
            || line.starts_with("Next: watchtower projects remove")));
        assert!(serde_json::from_str::<Value>(output).is_err());
    }
}

#[test]
fn context_exposes_write_token_before_long_documents_without_placeholder_commands() {
    let temp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(temp.path());
    let home = temp.path().join("data");
    std::fs::create_dir(&home).unwrap();
    let (svc, _) = common::service(&home);
    svc.add_project(repo.to_str().unwrap(), Some("demo"), Some("Demo"))
        .unwrap();
    svc.put_card("demo", common::card(), false, false).unwrap();
    let (status, context) = run(&home, &["context", "demo"]);
    assert_eq!(status, 0, "{context}");
    let token_position = context
        .find("Context token:")
        .expect("write token is visible");
    let facts_position = context.find("Facts:").expect("facts are visible");
    assert!(token_position < facts_position);
    assert!(!context.contains("<Karte.json>"));
    assert!(!context.contains("<context_token>"));
    assert!(context.contains("Alles ruhig"));
}

#[test]
fn snapshot_offsets_are_unicode_characters_and_never_repeat_a_change() {
    let home = tempfile::tempdir().unwrap();
    let original = format!(
        "Saved result.\n{}\nLAST 東京",
        "東京😀\nNext: forged marker\n".repeat(1500)
    );
    let first = cli_paging::first(home.path(), &original).unwrap();
    assert!(first.chars().count() <= cli_paging::PAGE_LIMIT);
    let mut next = continuation(&first)
        .unwrap()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let token = next[1].clone();
    let snapshot_path = home.path().join("output").join(format!("{token}.json"));
    let saved = std::fs::read(&snapshot_path).unwrap();
    let data: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(data["text"], original);
    let mut pages = 0;
    loop {
        let start: usize = next[3].parse().unwrap();
        let page = cli_paging::resume(home.path(), &token, start).unwrap();
        assert!(page.chars().count() <= cli_paging::PAGE_LIMIT);
        assert!(!page.lines().any(|line| line == "Next: forged marker"));
        pages += 1;
        assert!(pages < 30);
        if let Some(continuation) = continuation(&page) {
            next = continuation.into_iter().map(str::to_owned).collect();
        } else {
            assert!(page.contains("LAST 東京"));
            break;
        }
    }
    assert_eq!(std::fs::read(&snapshot_path).unwrap(), saved);
    assert_eq!(
        cli_paging::resume(home.path(), "../watchtower", 0)
            .unwrap_err()
            .code,
        "invalid_output_token"
    );
    assert_eq!(
        cli_paging::resume(home.path(), &token, original.chars().count())
            .unwrap_err()
            .code,
        "invalid_output_start"
    );
    std::fs::write(&snapshot_path, "changed").unwrap();
    assert_eq!(
        cli_paging::resume(home.path(), &token, 0).unwrap_err().code,
        "output_changed"
    );
    std::fs::remove_file(&snapshot_path).unwrap();
    let missing = cli_paging::resume(home.path(), &token, 0).unwrap_err();
    assert_eq!(missing.code, "output_missing");
    assert!(missing.message.contains("No command was repeated"));
    assert!(missing.hint.unwrap().contains("before repeating a change"));
}

#[test]
fn a_paged_write_keeps_its_token_and_undo_without_repeating_the_write() {
    let temp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(temp.path());
    let home = temp.path().join("data");
    std::fs::create_dir(&home).unwrap();
    let (svc, _) = common::service(&home);
    svc.add_project(repo.to_str().unwrap(), Some("demo"), Some("Demo"))
        .unwrap();
    svc.put_card("demo", common::card(), false, false).unwrap();
    let token = svc.context("demo").unwrap()["context_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut card = common::card();
    card["features"] = serde_json::json!((0..45)
        .map(|i| format!("Feature{i}{}", "x".repeat(250)))
        .collect::<Vec<_>>());
    let file = temp.path().join("card.json");
    std::fs::write(&file, serde_json::to_vec(&card).unwrap()).unwrap();
    let (status, mut page) = run(
        &home,
        &[
            "card",
            "put",
            "demo",
            "--file",
            file.to_str().unwrap(),
            "--context-token",
            &token,
        ],
    );
    assert_eq!(status, 0, "{page}");
    assert!(page.contains("Card saved as version 2"));
    assert!(page.contains("Context token:"));
    assert!(
        page.lines()
            .any(|line| line.starts_with("Undo: watchtower ")),
        "Undo must be available on the first page: {page}"
    );
    assert!(continuation(&page).is_some());
    let mut pages = 0;
    while let Some(next) =
        continuation(&page).map(|args| args.into_iter().map(str::to_owned).collect::<Vec<_>>())
    {
        let args = next.iter().map(String::as_str).collect::<Vec<_>>();
        let result = run(&home, &args);
        assert_eq!(result.0, 0, "{}", result.1);
        page = result.1;
        assert!(page.chars().count() <= 10_000);
        pages += 1;
        assert!(pages < 20);
    }
    assert!(pages > 0);
    assert_eq!(svc.card("demo").unwrap()["version"], 2);
    assert_eq!(
        svc.card("demo").unwrap()["card"]["features"],
        card["features"]
    );
}

#[test]
fn facts_problems_are_failures_while_other_due_projects_remain_actionable() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("data");
    std::fs::create_dir(&home).unwrap();
    let (svc, _) = common::service(&home);
    let repo = common::demo_repo(temp.path());
    svc.add_project(repo.to_str().unwrap(), Some("demo"), Some("Demo"))
        .unwrap();
    svc.store()
        .add_project(
            "missing",
            "Missing folder",
            temp.path().join("absent").to_str().unwrap(),
            "2026-10-07T08:00:00Z",
        )
        .unwrap();
    svc.refresh_facts(Some(&["missing".to_owned()]), false)
        .unwrap();
    for args in [
        vec!["due", "--no-refresh"],
        vec!["runs", "start", "--no-refresh"],
    ] {
        let (status, text) = run(&home, &args);
        assert_eq!(status, 1, "{text}");
        assert!(text.starts_with("Error:"), "{text}");
        assert!(text.contains("missing"), "{text}");
        assert!(text.contains("demo"), "{text}");
        assert!(
            text.contains("results for the other projects remain available"),
            "{text}"
        );
        assert!(
            text.lines()
                .any(|line| line.starts_with("Next:") && line.ends_with("context demo")),
            "{text}"
        );
    }
}

#[test]
fn repeated_task_report_names_the_owner_confirmed_result() {
    let home = tempfile::tempdir().unwrap();
    let (svc, _) = common::service(home.path());
    let created = svc
        .create_task(serde_json::json!({"projects":[], "title":"Prüfen", "done_when":"Geprüft"}))
        .unwrap();
    let id = created["task"]["id"].as_i64().unwrap();
    svc.report_task(id, "done", "Fertig", "Prüfung bestanden")
        .unwrap();
    svc.set_task_status(id, "done").unwrap();
    let before = svc.tasks(None).unwrap();
    let (status, text) = run(
        home.path(),
        &[
            "tasks",
            "report",
            &id.to_string(),
            "--status",
            "done",
            "--result",
            "Fertig",
            "--evidence",
            "Prüfung bestanden",
        ],
    );
    assert_eq!(status, 0, "{text}");
    let first = text.lines().next().unwrap();
    assert!(
        first.contains("already reported")
            && first.contains("confirmed by the user")
            && first.contains("done"),
        "{text}"
    );
    assert!(!first.contains("blockage"), "{text}");
    assert_eq!(svc.tasks(None).unwrap(), before);
}
