//! Contracts at the actual process boundary, including Windows shell waiting.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const CLI: &str = env!("CARGO_BIN_EXE_Watchtower");

fn command(home: &Path) -> Command {
    let mut command = Command::new(CLI);
    command.env("WATCHTOWER_HOME", home).stdin(Stdio::null());
    command
}

fn run(home: &Path, args: &[&str]) -> Output {
    timed_output(command(home).args(args))
}

fn timed_output(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI or shell starts");
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Test command did not finish with closed stdin within 10 seconds: {command:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    Output {
        status,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}

fn json_output(output: &Output, code: i32) -> Value {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout)
        .expect("stdout contains exactly one complete JSON object")
}

#[test]
fn executable_reports_json_exit_codes_and_usage_on_the_right_streams() {
    let home = tempfile::tempdir().unwrap();
    let result = json_output(&run(home.path(), &["--json", "projects", "list"]), 0);
    assert_eq!(result["ok"], true);
    assert_eq!(result["projects"], json!([]));

    let result = json_output(&run(home.path(), &["context", "missing", "--json"]), 1);
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"]["code"], "project_not_found");
    assert!(result["error"]["hint"].is_string());

    let output = run(home.path(), &["projects", "list"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(!output.stdout.is_empty());
    assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());

    let output = run(home.path(), &["context", "missing"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.starts_with("Error:"));
    assert!(text.contains("missing"));
    assert!(text.contains("projects"));
    assert!(serde_json::from_slice::<Value>(&output.stdout).is_err());

    for args in [&[][..], &["unknown-command"][..]] {
        let output = run(home.path(), args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.starts_with("Error:"));
        assert!(text.contains("Usage:"));
        assert!(text.contains("Usage:"));
    }

    for args in [
        vec!["--help"],
        vec!["--json", "--help"],
        vec!["projects", "update", "--help"],
    ] {
        let output = run(home.path(), &args);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
        assert!(String::from_utf8_lossy(&output.stdout).contains("--json"));
    }
    let output = run(home.path(), &["--version"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));

    let result = json_output(&run(home.path(), &["--json", "unknown-command"]), 2);
    assert_eq!(result["ok"], false);
    assert_eq!(result["error"]["code"], "invalid_arguments");

    let absent_home = home.path().join("not-created-by-usage");
    for (args, code) in [
        (vec!["--help"], 0),
        (vec!["--version"], 0),
        (vec!["unknown-command"], 2),
        (vec!["--json", "unknown-command"], 2),
        (vec!["schema", "card"], 0),
        (vec!["--json", "schema", "portfolio"], 0),
    ] {
        let output = run(&absent_home, &args);
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        assert!(!absent_home.exists(), "{args:?} must not create app data");
    }
}

#[test]
fn executable_reads_unicode_json_from_stdin_and_closes_with_complete_output() {
    let home = tempfile::tempdir().unwrap();
    let context = json_output(&run(home.path(), &["--json", "portfolio", "context"]), 0);
    let token = context["context_token"].as_str().unwrap();
    let greeting = "Grüße aus Köln – 日本語 🗼";
    let mut child = command(home.path())
        .args([
            "--json",
            "portfolio",
            "put",
            "--file",
            "-",
            "--context-token",
            token,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Closing stdin is part of the protocol: the CLI must observe EOF and finish.
    child
        .stdin
        .take()
        .unwrap()
        .write_all(json!({"greeting": greeting}).to_string().as_bytes())
        .unwrap();
    let saved = json_output(&child.wait_with_output().unwrap(), 0);
    assert_eq!(saved["saved"], true);
    let result = json_output(&run(home.path(), &["--json", "portfolio", "show"]), 0);
    assert_eq!(result["portfolio"]["greeting"], greeting);

    let output = run(home.path(), &["portfolio", "show"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout).unwrap().contains(greeting));
}

#[test]
fn closed_stdin_and_missing_input_fail_without_prompting_or_saving() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["portfolio", "put", "--file", "-", "--dry-run"],
        vec!["portfolio", "put", "--dry-run"],
    ] {
        let output = run(home.path(), &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.starts_with("Error:"), "{text}");
        assert!(text.contains("--file"), "{text}");
    }
    let result = json_output(&run(home.path(), &["--json", "portfolio", "context"]), 0);
    assert!(result["previous_portfolio"].is_null());
}

#[test]
fn owner_and_recovery_commands_return_with_closed_stdin() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["overview"],
        vec!["projects", "show", "missing"],
        vec!["projects", "visit", "missing"],
        vec!["projects", "trash"],
        vec!["projects", "restore", "missing"],
        vec!["projects", "list", "--query", "missing"],
        vec!["feedback", "list"],
        vec!["feedback", "set", "missing", "--value", "up"],
        vec!["feedback", "clear", "missing"],
        vec!["feedback", "restore", "missing", "--file", "-"],
        vec!["warnings", "list", "missing"],
        vec![
            "warnings",
            "acknowledge",
            "missing",
            "--key",
            "no_remote",
            "--note",
            "Local project",
        ],
        vec!["warnings", "clear", "missing", "--key", "no_remote"],
        vec!["changes", "show"],
        vec!["changes", "read", "--token", "missing"],
        vec!["settings", "show"],
        vec!["settings", "update"],
        vec!["ideas", "trash"],
        vec!["ideas", "restore", "999"],
        vec!["tasks", "show", "999"],
        vec!["tasks", "create", "--file", "-"],
        vec!["tasks", "confirm", "999", "--revision", "1"],
        vec!["tasks", "reopen", "999", "--revision", "1"],
        vec!["tasks", "cancel", "999", "--revision", "1"],
        vec!["card", "history", "missing"],
        vec![
            "card",
            "restore",
            "missing",
            "--version",
            "1",
            "--context-token",
            "missing",
        ],
        vec!["portfolio", "history"],
        vec![
            "portfolio",
            "restore",
            "--version",
            "1",
            "--context-token",
            "missing",
        ],
        vec!["output", "missing"],
        vec!["jobs", "list"],
        vec!["jobs", "status", "missing"],
        vec!["jobs", "wait", "missing"],
        vec!["usage"],
        vec!["activity"],
        vec!["undo", "missing"],
    ] {
        let output = run(home.path(), &args);
        assert!(
            matches!(output.status.code(), Some(0..=2)),
            "{args:?}: {output:?}"
        );
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.trim().is_empty(), "{args:?}");
        assert!(
            serde_json::from_str::<Value>(&text).is_err(),
            "{args:?}: {text}"
        );
        if !output.status.success() {
            assert!(text.starts_with("Error:"), "{args:?}: {text}");
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::process::CommandExt;

    fn on_path(name: &str) -> Option<std::path::PathBuf> {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|directory| directory.join(name))
            .find(|path| path.is_file())
    }

    #[test]
    fn unicode_round_trips_through_powershell_files_arguments_and_json() {
        let greeting = "Zürich 東京 🗼";
        let mut tested = 0;
        for shell in ["powershell.exe", "pwsh.exe"] {
            let Some(executable) = on_path(shell) else {
                continue;
            };
            for encoding in ["Unicode", "UTF8"] {
                let home = tempfile::tempdir().unwrap();
                let context =
                    json_output(&run(home.path(), &["--json", "portfolio", "context"]), 0);
                let script = format!(
                    r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$file = Join-Path $env:WATCHTOWER_HOME 'brief with spaces.json'
@{{ greeting = $env:WATCHTOWER_TEST_TEXT }} | ConvertTo-Json | Set-Content -LiteralPath $file -Encoding {encoding}
& $env:WATCHTOWER_TEST_EXE --json portfolio put --file $file --context-token $env:WATCHTOWER_TEST_TOKEN | Out-Null
if ($LASTEXITCODE -ne 0) {{ exit 21 }}
& $env:WATCHTOWER_TEST_EXE ideas add $env:WATCHTOWER_TEST_TEXT | Out-Null
if ($LASTEXITCODE -ne 0) {{ exit 22 }}
# Program-readable output must survive a legacy console code page.
[Console]::OutputEncoding = [Text.Encoding]::GetEncoding(437)
$brief = & $env:WATCHTOWER_TEST_EXE --json portfolio show | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $brief.portfolio.greeting -cne $env:WATCHTOWER_TEST_TEXT) {{ exit 23 }}
$ideas = & $env:WATCHTOWER_TEST_EXE --json ideas list | ConvertFrom-Json
if ($LASTEXITCODE -ne 0 -or $ideas.ideas[0].text -cne $env:WATCHTOWER_TEST_TEXT) {{ exit 24 }}
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$text = (& $env:WATCHTOWER_TEST_EXE portfolio show) -join "`n"
if ($LASTEXITCODE -ne 0 -or -not $text.Contains($env:WATCHTOWER_TEST_TEXT)) {{ exit 25 }}
[Console]::Out.WriteLine('ROUNDTRIP_OK')
"#
                );
                let output = timed_output(
                    Command::new(&executable)
                        .env("WATCHTOWER_HOME", home.path())
                        .env("WATCHTOWER_TEST_EXE", CLI)
                        .env("WATCHTOWER_TEST_TEXT", greeting)
                        .env(
                            "WATCHTOWER_TEST_TOKEN",
                            context["context_token"].as_str().unwrap(),
                        )
                        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                        .stdin(Stdio::null()),
                );
                assert_eq!(
                    output.status.code(),
                    Some(0),
                    "{shell} {encoding}: {output:?}"
                );
                assert!(output.stderr.is_empty(), "{shell} {encoding}: {output:?}");
                assert!(String::from_utf8_lossy(&output.stdout).contains("ROUNDTRIP_OK"));
                tested += 1;
            }
        }
        assert!(
            tested >= 2,
            "Windows PowerShell is required for the shell contract"
        );
    }

    #[test]
    fn unicode_round_trips_through_git_bash_when_available() {
        let Some(git) = on_path("git.exe") else {
            return;
        };
        let Some(root) = git.parent().and_then(Path::parent) else {
            return;
        };
        let bash = root.join("bin/bash.exe");
        if !bash.is_file() {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let context = json_output(&run(home.path(), &["--json", "portfolio", "context"]), 0);
        let greeting = "Zürich 東京 🗼";
        let file = home.path().join("brief with spaces.json");
        std::fs::write(&file, json!({"greeting": greeting}).to_string()).unwrap();
        let output = timed_output(
            Command::new(bash)
                .env("WATCHTOWER_HOME", home.path())
                .env("WATCHTOWER_TEST_EXE", CLI.replace('\\', "/"))
                .env("WATCHTOWER_TEST_FILE", file.to_string_lossy().replace('\\', "/"))
                .env("WATCHTOWER_TEST_TEXT", greeting)
                .env("WATCHTOWER_TEST_TOKEN", context["context_token"].as_str().unwrap())
                .args(["--noprofile", "--norc", "-c", r#"
set -eu
"$WATCHTOWER_TEST_EXE" portfolio put --file "$WATCHTOWER_TEST_FILE" --context-token "$WATCHTOWER_TEST_TOKEN" >/dev/null
"$WATCHTOWER_TEST_EXE" ideas add "$WATCHTOWER_TEST_TEXT" >/dev/null
"$WATCHTOWER_TEST_EXE" --json portfolio show
"#])
                .stdin(Stdio::null()),
        );
        assert_eq!(json_output(&output, 0)["portfolio"]["greeting"], greeting);
        let result = json_output(&run(home.path(), &["--json", "ideas", "list"]), 0);
        assert_eq!(result["ideas"][0]["text"], greeting);
    }

    fn subsystem(exe: &str) -> u16 {
        let bytes = std::fs::read(exe).unwrap();
        assert_eq!(&bytes[..2], b"MZ");
        let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
        assert_eq!(&bytes[pe..pe + 4], b"PE\0\0");
        // Subsystem has the same offset in PE32 and PE32+ optional headers.
        let offset = pe + 4 + 20 + 68;
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
    }

    #[test]
    fn cli_is_a_console_executable_and_desktop_is_a_gui_executable() {
        assert_eq!(
            subsystem(CLI),
            3,
            "CLI must use IMAGE_SUBSYSTEM_WINDOWS_CUI"
        );
        #[cfg(feature = "desktop")]
        assert_eq!(
            subsystem(env!("CARGO_BIN_EXE_Watchtower-UI")),
            2,
            "desktop must use IMAGE_SUBSYSTEM_WINDOWS_GUI"
        );
    }

    fn powershell(home: &Path, invocation: &str) -> Command {
        let mut command = Command::new("powershell.exe");
        command
            .env("WATCHTOWER_HOME", home)
            .env("WATCHTOWER_TEST_EXE", CLI)
            .env("WATCHTOWER_TEST_STARTED", home.join("shell-started"))
            .env("WATCHTOWER_TEST_FINISHED", home.join("shell-finished"))
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "[IO.File]::WriteAllText($env:WATCHTOWER_TEST_STARTED, 'started'); \
                     & $env:WATCHTOWER_TEST_EXE {invocation}; \
                     $code = $LASTEXITCODE; \
                     [IO.File]::WriteAllText($env:WATCHTOWER_TEST_FINISHED, 'finished'); \
                     [Console]::Out.WriteLine('WATCHTOWER_DONE:' + $code); exit $code"
                ),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn shell_json(output: &Output, code: i32) -> Value {
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let text = std::str::from_utf8(&output.stdout).unwrap();
        let suffix = format!("WATCHTOWER_DONE:{code}");
        let document = text.trim_end().strip_suffix(&suffix).unwrap_or_else(|| {
            panic!("Shell must observe the exit code and emit its marker after all JSON: {text:?}")
        });
        serde_json::from_str(document).expect("complete JSON must precede shell completion")
    }

    #[test]
    fn powershell_waits_for_work_and_receives_json_before_continuing() {
        let home = tempfile::tempdir().unwrap();
        json_output(&run(home.path(), &["--json", "projects", "list"]), 0);
        let db = rusqlite::Connection::open(home.path().join("watchtower.db")).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();

        // Keep the CLI busy deterministically. A GUI-subsystem entry lets PowerShell
        // continue and exit here, even though the CLI is still waiting for this lock.
        // No pipeline/redirection in the script may accidentally make PowerShell wait.
        let mut child = powershell(home.path(), "--json runs start --no-refresh")
            .spawn()
            .unwrap();
        let started = Instant::now();
        while !home.path().join("shell-started").exists()
            && started.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let shell_started = home.path().join("shell-started").exists();
        std::thread::sleep(Duration::from_millis(500));
        let continued_while_locked = home.path().join("shell-finished").exists();
        let exited_while_locked = child.try_wait().unwrap().is_some();
        db.execute_batch("ROLLBACK").unwrap();
        let output = child.wait_with_output().unwrap();

        assert!(shell_started, "PowerShell did not reach the CLI invocation");
        assert!(
            !continued_while_locked,
            "PowerShell continued before the CLI finished"
        );
        assert!(
            !exited_while_locked,
            "PowerShell exited before the CLI finished"
        );
        assert_eq!(shell_json(&output, 0)["ok"], true);

        let output = powershell(home.path(), "--json context missing")
            .output()
            .unwrap();
        assert_eq!(shell_json(&output, 1)["error"]["code"], "project_not_found");

        let output = powershell(home.path(), "projects list").output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty());
        let text = std::str::from_utf8(&output.stdout).unwrap();
        let human = text.trim_end().strip_suffix("WATCHTOWER_DONE:0").unwrap();
        assert!(!human.trim().is_empty());
        assert!(serde_json::from_str::<Value>(human).is_err());
    }

    #[test]
    fn cmd_receives_complete_json_and_native_exit_codes() {
        let home = tempfile::tempdir().unwrap();
        for (invocation, code, json) in [
            ("--json projects list", 0, true),
            ("context missing --json", 1, true),
            ("projects list", 0, false),
        ] {
            let output = Command::new("cmd.exe")
                .env("WATCHTOWER_HOME", home.path())
                .env("WATCHTOWER_TEST_EXE", CLI)
                .args(["/D", "/V:ON", "/S", "/C"])
                // cmd parses its own quoting; the normal Windows argv escaping
                // would introduce literal backslashes before these quotes.
                .raw_arg(format!(
                    "\"\"%WATCHTOWER_TEST_EXE%\" {invocation} & \
                     set \"watchtower_exit=!errorlevel!\" & \
                     echo WATCHTOWER_DONE:!watchtower_exit! & exit /b !watchtower_exit!\""
                ))
                .output()
                .unwrap();
            if json {
                assert_eq!(shell_json(&output, code)["ok"], code == 0);
            } else {
                assert_eq!(output.status.code(), Some(code), "{output:?}");
                assert!(output.stderr.is_empty());
                let text = std::str::from_utf8(&output.stdout).unwrap();
                let human = text.trim_end().strip_suffix("WATCHTOWER_DONE:0").unwrap();
                assert!(!human.trim().is_empty());
                assert!(serde_json::from_str::<Value>(human).is_err());
            }
        }
    }
}
