//! Execute the operating Skill's actual recipes against isolated data. A new or
//! changed example is discovered automatically instead of copied into this test.
#![cfg(windows)]

mod common;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use watchtower::Watchtower;

const CLI: &str = env!("CARGO_BIN_EXE_Watchtower");

fn markdown_files(path: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            markdown_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            found.push(path);
        }
    }
}

fn fixture(home: &Path, repo: &Path, command: &str) -> HashMap<String, String> {
    std::fs::create_dir(home).unwrap();
    let service = Watchtower::open(&home.join("watchtower.db")).unwrap();
    service
        .add_project(repo.to_str().unwrap(), Some("demo"), None)
        .unwrap();
    let card = common::card_with(json!({
        "waiting_on_you": [{"title":"Welcher Export?", "source":"README", "options": []}]
    }));
    service
        .put_card("demo", card.clone(), false, false)
        .unwrap();
    service
        .put_portfolio(json!({"greeting":"Guten Morgen"}), false, false)
        .unwrap();
    let task = service
        .create_task(json!({
            "projects":["demo"], "source_id":"skill-recipe", "title":"Export prüfen",
            "why":"Ein lesbarer Export", "evidence":"README", "done_when":"Export lässt sich öffnen"
        }))
        .unwrap();
    // An assignment changes the context. The completed-run recipe assumes the
    // preceding card and brief steps have incorporated that assignment.
    let current_card = service.card("demo").unwrap()["card"].clone();
    service
        .put_card("demo", current_card, false, false)
        .unwrap();
    service
        .put_portfolio(json!({"greeting":"Guten Morgen"}), false, false)
        .unwrap();
    let run = service.run_start(false).unwrap();
    let card_file = home.join("card input.json");
    std::fs::write(&card_file, card.to_string()).unwrap();
    let brief_file = home.join("brief input.json");
    std::fs::write(&brief_file, json!({"greeting":"Guten Morgen"}).to_string()).unwrap();
    let report_file = home.join("task report.json");
    std::fs::write(
        &report_file,
        json!({"status":"done", "result":"Export opens successfully", "evidence":"Export file checked"}).to_string(),
    ).unwrap();
    let context = if command.contains("portfolio ") {
        service.portfolio_context(&[]).unwrap()
    } else {
        service.context("demo").unwrap()
    };
    let item_kind = if command.contains("--value later") {
        "suggestions"
    } else {
        "waiting_on_you"
    };
    let item_id = service.card("demo").unwrap()["card"][item_kind][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    [
        ("project-id", "demo".to_owned()),
        ("run-id", run["run_id"].to_string()),
        ("task-id", task["task"]["id"].to_string()),
        ("version", "1".to_owned()),
        (
            "context-token",
            context["context_token"].as_str().unwrap().to_owned(),
        ),
        ("card-file", card_file.to_string_lossy().into_owned()),
        ("brief-file", brief_file.to_string_lossy().into_owned()),
        ("report-file", report_file.to_string_lossy().into_owned()),
        ("display-name", "Renamed project".to_owned()),
        ("existing-folder", repo.to_string_lossy().into_owned()),
        ("YYYY-MM-DD", "2030-10-07".to_owned()),
        ("user's reason", "Other work first".to_owned()),
        ("user's goal", "Provide a readable export".to_owned()),
        ("item-id", item_id),
        ("user's answer", "A local text file".to_owned()),
        ("user's idea", "An export preview".to_owned()),
        ("outcome", "Export opens successfully".to_owned()),
        ("check or file", "Export file checked".to_owned()),
        ("task-title", "Check export".to_owned()),
        ("completion-criterion", "The export opens".to_owned()),
        (
            "cause and next step",
            "File unavailable; restore local access".to_owned(),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect()
}

fn substitute(line: &str, values: &HashMap<String, String>) -> String {
    let mut rest = line;
    let mut result = String::new();
    while let Some(start) = rest.find('<') {
        let end = rest[start..].find('>').expect("closed Skill placeholder") + start;
        result.push_str(&rest[..start]);
        let key = &rest[start + 1..end];
        let value = values
            .get(key)
            .unwrap_or_else(|| panic!("Add a fixture for <{key}> in recipe {line}"));
        let quoted = rest[..start].ends_with('"') && rest[end + 1..].starts_with('"');
        if quoted {
            result.push_str(value);
        } else {
            result.push('\'');
            result.push_str(&value.replace('\'', "''"));
            result.push('\'');
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}

#[test]
fn every_cli_recipe_in_the_operating_skill_runs_with_real_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(tmp.path());
    let mut files = Vec::new();
    markdown_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("skill/watchtower"),
        &mut files,
    );
    files.sort();
    let mut count = 0;
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        let mut fence: Option<&str> = None;
        for (number, line) in source.lines().enumerate() {
            let line = line.trim();
            if let Some(language) = line.strip_prefix("```") {
                fence = if fence.is_some() {
                    None
                } else {
                    Some(language)
                };
                continue;
            }
            let Some(language) = fence else { continue };
            if !(line.starts_with("watchtower ")
                || line.starts_with("& ") && line.contains("Watchtower.exe")
                || line.starts_with('"') && line.contains("Watchtower.exe"))
            {
                continue;
            }
            count += 1;
            let home = tmp.path().join(format!("recipe-{count}"));
            let values = fixture(&home, &repo, line);
            let invocation = substitute(line, &values);
            // Both fallback forms are the same native executable with a shell-
            // specific prefix. Run the documented PowerShell form directly; the
            // Git Bash process tests separately cover its encoding and quoting.
            let arguments = if let Some(arguments) = invocation.strip_prefix("watchtower ") {
                arguments
            } else {
                invocation.split_once("Watchtower.exe\"").unwrap().1.trim()
            };
            assert!(
                matches!(language, "powershell" | "bash" | ""),
                "{path:?}: unsupported recipe shell {language}"
            );
            let output = Command::new("powershell.exe")
                .env("WATCHTOWER_HOME", &home)
                .env("WATCHTOWER_TEST_EXE", CLI)
                .args(["-NoProfile", "-NonInteractive", "-Command", &format!(
                    "[Console]::OutputEncoding = [Text.Encoding]::UTF8; & $env:WATCHTOWER_TEST_EXE {arguments}; exit $LASTEXITCODE"
                )])
                .stdin(Stdio::null())
                .output().unwrap();
            let text = String::from_utf8_lossy(&output.stdout);
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}:{}: {line}\n{text}\n{}",
                path.display(),
                number + 1,
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stderr.is_empty(),
                "{}:{}: {output:?}",
                path.display(),
                number + 1
            );
            assert!(
                !text.trim().is_empty(),
                "{}:{}: empty output",
                path.display(),
                number + 1
            );
            if !arguments.split_whitespace().any(|arg| arg == "--json") {
                assert!(
                    serde_json::from_str::<Value>(&text).is_err(),
                    "{}:{}: unexpected JSON",
                    path.display(),
                    number + 1
                );
            }
        }
    }
    assert!(
        count >= 18,
        "Every common workflow must have an executable recipe; found {count}"
    );
}
