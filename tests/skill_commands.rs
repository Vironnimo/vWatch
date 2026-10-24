//! Execute the operating Skill's actual recipes against isolated data. A new or
//! changed example is discovered automatically instead of copied into this test.
#![cfg(windows)]

mod common;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use vwatch::VWatch;

const CLI: &str = env!("CARGO_BIN_EXE_vWatch");

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

fn template(home: &Path, repo: &Path) {
    std::fs::create_dir(home).unwrap();
    let service = VWatch::open(&home.join("vwatch.db")).unwrap();
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
    service
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
    service.run_start(false).unwrap();
    drop(service);
    // Copy only a closed, checkpointed database. No recipe shares mutable state
    // or relies on a live WAL file belonging to the template connection.
    let db = rusqlite::Connection::open(home.join("vwatch.db")).unwrap();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(db);
    assert!(!home.join("vwatch.db-wal").exists());
}

fn fixture(home: &Path, template: &Path, repo: &Path, command: &str) -> HashMap<String, String> {
    std::fs::create_dir(home).unwrap();
    std::fs::copy(template.join("vwatch.db"), home.join("vwatch.db")).unwrap();
    let service = VWatch::open(&home.join("vwatch.db")).unwrap();
    let card = common::card_with(json!({
        "waiting_on_you": [{"title":"Welcher Export?", "source":"README", "options": []}]
    }));
    let task = service.tasks(None).unwrap()["tasks"][0].clone();
    let run = service.run_status().unwrap();
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
        ("run-id", run["latest"]["id"].to_string()),
        ("task-id", task["id"].to_string()),
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
    let template_home = tmp.path().join("template");
    template(&template_home, &repo);
    let mut files = Vec::new();
    markdown_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("skill/vwatch"),
        &mut files,
    );
    files.sort();
    let mut count = 0;
    let mut recipes = Vec::new();
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
            if !(line.starts_with("vwatch ")
                || line.starts_with("& ") && line.contains("vWatch.exe")
                || line.starts_with('"') && line.contains("vWatch.exe"))
            {
                continue;
            }
            count += 1;
            let home = tmp.path().join(format!("recipe-{count}"));
            let values = fixture(&home, &template_home, &repo, line);
            let invocation = substitute(line, &values);
            // Both fallback forms are the same native executable with a shell-
            // specific prefix. Run the documented PowerShell form directly; the
            // Git Bash process tests separately cover its encoding and quoting.
            let arguments = if let Some(arguments) = invocation.strip_prefix("vwatch ") {
                arguments
            } else {
                invocation.split_once("vWatch.exe\"").unwrap().1.trim()
            };
            assert!(
                matches!(language, "powershell" | "bash" | ""),
                "{path:?}: unsupported recipe shell {language}"
            );
            recipes.push(json!({
                "home": home, "cwd": tmp.path(), "arguments": arguments,
                "source": format!("{}:{}: {line}", path.display(), number + 1),
                "json": arguments.split_whitespace().any(|arg| arg == "--json")
            }));
        }
    }
    assert!(
        count >= 18,
        "Every common workflow must have an executable recipe; found {count}"
    );
    let input = tmp.path().join("recipes.json");
    std::fs::write(&input, serde_json::to_vec(&recipes).unwrap()).unwrap();
    let results = tmp.path().join("results.json");
    let script = tmp.path().join("recipes.ps1");
    std::fs::write(&script, r#"
param($InputPath, $ResultPath, $Executable)
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$initialEnvironment = @{}
Get-ChildItem Env: | ForEach-Object { $initialEnvironment[$_.Name] = $_.Value }
$results = @()
foreach ($recipe in (Get-Content -LiteralPath $InputPath -Raw -Encoding UTF8 | ConvertFrom-Json)) {
    # Restore shell context for every independent invocation. Command text still
    # passes through PowerShell's parser, including quoted files and fallbacks.
    Get-ChildItem Env: | Where-Object { -not $initialEnvironment.ContainsKey($_.Name) } |
        ForEach-Object { Remove-Item -LiteralPath ('Env:' + $_.Name) }
    foreach ($name in $initialEnvironment.Keys) { Set-Item -LiteralPath ('Env:' + $name) -Value $initialEnvironment[$name] }
    Set-Location -LiteralPath $recipe.cwd
    $env:VWATCH_HOME = $recipe.home
    $env:VWATCH_TEST_EXE = $Executable
    $ErrorActionPreference = 'Continue'
    $Error.Clear()
    $LASTEXITCODE = 0
    $stdout = Join-Path $recipe.home 'stdout.txt'
    $stderr = Join-Path $recipe.home 'stderr.txt'
    try {
        $call = [scriptblock]::Create('& $env:VWATCH_TEST_EXE ' + $recipe.arguments)
        & $call 1> $stdout 2> $stderr
        $code = $LASTEXITCODE
    } catch {
        $code = -1
        $_ | Out-File -LiteralPath $stderr
    }
    $outText = if (Test-Path -LiteralPath $stdout) { [IO.File]::ReadAllText($stdout) } else { '' }
    $errText = if (Test-Path -LiteralPath $stderr) { [IO.File]::ReadAllText($stderr) } else { '' }
    $results += @{ source = $recipe.source; code = $code; stdout = $outText; stderr = $errText }
}
[IO.File]::WriteAllText($ResultPath, (ConvertTo-Json -InputObject @($results) -Depth 8), [Text.UTF8Encoding]::new($false))
"#).unwrap();
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg(&input)
        .arg(&results)
        .arg(CLI)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let results: Vec<Value> = serde_json::from_slice(&std::fs::read(results).unwrap()).unwrap();
    assert_eq!(
        results.len(),
        recipes.len(),
        "every recipe returned a separate outcome"
    );
    let mut failures = Vec::new();
    for (recipe, result) in recipes.iter().zip(&results) {
        assert_eq!(result["source"], recipe["source"]);
        let text = result["stdout"].as_str().unwrap();
        let valid_mode = if recipe["json"] == true {
            serde_json::from_str::<Value>(text).is_ok()
        } else {
            serde_json::from_str::<Value>(text).is_err()
        };
        if result["code"] != 0 || result["stderr"] != "" || text.trim().is_empty() || !valid_mode {
            failures.push(result.to_string());
        }
    }
    assert!(
        failures.is_empty(),
        "Recipe failures:\n{}",
        failures.join("\n")
    );
}
