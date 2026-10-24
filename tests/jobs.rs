//! Real-process deadline, result replay, liveness and instance contracts.

mod common;

use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const CLI: &str = env!("CARGO_BIN_EXE_vWatch");

fn command(home: &Path, deadline_ms: u64) -> Command {
    let mut command = Command::new(CLI);
    command
        .env("VWATCH_HOME", home)
        .env("VWATCH_TEST_INSTANCE", "1")
        .env("VWATCH_TEST_JOB_DEADLINE_MS", deadline_ms.to_string())
        .stdin(Stdio::null());
    command
}

fn json(output: &Output) -> Value {
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("one complete JSON object")
}

fn token(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .find(|part| part.starts_with("job_"))
        .expect("saved job token")
        .to_owned()
}

#[test]
fn deadline_returns_a_durable_job_and_wait_replays_complete_result_in_requested_format() {
    let home = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let first = command(home.path(), 70)
        .env("VWATCH_TEST_JOB_BLOCK", "1")
        .args(["overview", "--refresh"])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(1));
    assert!(first.stderr.is_empty());
    let text = String::from_utf8_lossy(&first.stdout);
    assert!(text.starts_with("Not finished:"), "{text}");
    assert!(text.contains("Next: vwatch --instance"), "{text}");
    assert!(start.elapsed() < Duration::from_secs(2));
    let job = token(&first);

    // An identical call while work is active joins the same job, never starts it twice.
    let repeated = command(home.path(), 70)
        .env("VWATCH_TEST_JOB_BLOCK", "1")
        .args(["--json", "overview", "--refresh"])
        .output()
        .unwrap();
    assert_eq!(json(&repeated)["job"]["token"], job);

    let activity = command(home.path(), 70)
        .args(["--json", "activity"])
        .output()
        .unwrap();
    let activity = json(&activity);
    let calls = activity["activity"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["command"] == "overview")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    assert!(
        calls.iter().all(|call| call["outcome"] == "not_finished"),
        "a returned continuation must not be logged as a failed operation"
    );

    std::fs::write(home.path().join(".release-test-job"), "release").unwrap();

    // Continue from a fresh shell whose environment selects a different instance.
    let wrong = tempfile::tempdir().unwrap();
    let completed = command(wrong.path(), 2_000)
        .args([
            "--data-dir",
            home.path().to_str().unwrap(),
            "--json",
            "jobs",
            "wait",
            &job,
        ])
        .output()
        .unwrap();
    assert_eq!(completed.status.code(), Some(0), "{completed:?}");
    let result = json(&completed);
    assert_eq!(result["ok"], true);
    assert_eq!(result["refreshed"], serde_json::json!([]));

    let replay = command(home.path(), 2_000)
        .args(["jobs", "wait", &job])
        .output()
        .unwrap();
    assert_eq!(replay.status.code(), Some(0));
    assert!(replay.stderr.is_empty());
    assert!(
        serde_json::from_slice::<Value>(&replay.stdout).is_err(),
        "ordinary wait must render text even though capture was JSON"
    );
    assert!(!String::from_utf8_lossy(&replay.stdout).contains("Not finished:"));
    let jobs = command(home.path(), 2_000)
        .args(["--json", "jobs", "list"])
        .output()
        .unwrap();
    let jobs = json(&jobs);
    assert_eq!(jobs["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(jobs["jobs"][0]["state"], "completed");
}

#[test]
fn stopped_worker_reports_unknown_outcome_and_never_claims_rollback() {
    let home = tempfile::tempdir().unwrap();
    let first = command(home.path(), 60)
        .env("VWATCH_TEST_JOB_DELAY_MS", "180")
        .env("VWATCH_TEST_JOB_CRASH", "1")
        .args(["--json", "facts", "refresh", "--no-ci"])
        .output()
        .unwrap();
    let first = json(&first);
    assert_eq!(first["finished"], false);
    let job = first["job"]["token"].as_str().unwrap();
    let result = command(home.path(), 2_000)
        .args(["--json", "jobs", "wait", job])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    let result = json(&result);
    assert_eq!(result["job"]["state"], "unknown");
    assert!(result["next"]
        .as_str()
        .unwrap()
        .contains("due --no-refresh"));
    let human = command(home.path(), 2_000)
        .args(["jobs", "status", job])
        .output()
        .unwrap();
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(human.contains("unknown"), "{human}");
    assert!(!human.contains("Nothing was changed"), "{human}");
}

#[test]
fn job_tokens_are_opaque_and_help_and_invalid_calls_start_nothing() {
    let home = tempfile::tempdir().unwrap();
    let unused = home.path().join("unused");
    for args in [vec!["--help"], vec!["facts", "refresh", "--unknown"]] {
        let output = command(&unused, 30).args(args).output().unwrap();
        assert!(output.stderr.is_empty());
        assert!(!unused.exists());
    }
    let invalid = command(home.path(), 30)
        .args(["--json", "jobs", "wait", "../../outside"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert_eq!(json(&invalid)["error"]["code"], "invalid_job_token");
    assert!(!home.path().join("jobs").exists());
}

#[test]
fn fast_worker_returns_the_complete_error_in_one_call() {
    let home = tempfile::tempdir().unwrap();
    let output = command(home.path(), 2_000)
        .args(["--json", "context", "missing"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let body = json(&output);
    assert_eq!(body["error"]["code"], "project_not_found");
    assert!(body["error"]["hint"].is_string());
    assert!(
        body.get("job").is_none(),
        "already finished work needs no wait call"
    );
}

#[test]
fn stopping_the_call_does_not_lose_the_worker_or_replay_the_operation() {
    let home = tempfile::tempdir().unwrap();
    let mut caller = command(home.path(), 5_000)
        .env("VWATCH_TEST_JOB_BLOCK", "1")
        .args(["--json", "facts", "refresh", "--no-ci"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let job = loop {
        let active = std::fs::read_dir(home.path().join("jobs"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(std::result::Result::ok)
            .find(|entry| entry.path().join("started.json").exists());
        if let Some(entry) = active {
            break entry.file_name().to_string_lossy().into_owned();
        }
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "worker starts promptly"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    caller.kill().unwrap();
    let _ = caller.wait_with_output().unwrap();
    let pending = command(home.path(), 50)
        .args(["--json", "jobs", "status", &job])
        .output()
        .unwrap();
    assert_eq!(json(&pending)["job"]["state"], "running");
    std::fs::write(home.path().join(".release-test-job"), "release").unwrap();
    let finished = command(home.path(), 2_000)
        .args(["--json", "jobs", "wait", &job])
        .output()
        .unwrap();
    assert_eq!(finished.status.code(), Some(0));
    assert_eq!(json(&finished)["refreshed"], serde_json::json!([]));
}

#[test]
fn only_reads_that_can_measure_repositories_need_a_worker() {
    for args in [
        vec!["vwatch", "overview", "--refresh"],
        vec!["vwatch", "state", "--refresh"],
        vec!["vwatch", "facts", "show", "demo"],
        vec!["vwatch", "projects", "visit", "demo"],
    ] {
        let args = args
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert!(vwatch::cli::job_plan(&args).is_some(), "{args:?}");
    }
    for args in [
        vec!["vwatch", "overview"],
        vec!["vwatch", "projects", "show", "demo"],
        vec!["vwatch", "projects", "list", "--query", "demo"],
        vec!["vwatch", "portfolio", "context"],
    ] {
        let args = args
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert!(vwatch::cli::job_plan(&args).is_none(), "{args:?}");
    }
}

#[test]
fn job_storage_failure_before_launch_reports_that_no_work_started() {
    let home = tempfile::tempdir().unwrap();
    let blocked = home.path().join("a-file");
    std::fs::write(&blocked, "not a directory").unwrap();
    let output = command(&blocked, 70)
        .args(["--json", "overview", "--refresh"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let output = json(&output);
    assert_eq!(output["error"]["code"], "job_start_io");
    let message = output["error"]["message"].as_str().unwrap();
    assert!(message.contains("did not start"), "{message}");
    assert!(!message.contains("outcome is unknown"), "{message}");
}

#[test]
fn a_relative_repository_job_keeps_its_request_directory_across_continuations() {
    let fixture = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(fixture.path());
    let home = fixture.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let first = command(&home, 70)
        .current_dir(fixture.path())
        .env("VWATCH_TEST_JOB_BLOCK", "1")
        .args(["--json", "projects", "add", "demo", "--id", "relative"])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(1), "{first:?}");
    let first_result = json(&first);
    assert_eq!(first_result["finished"], false);
    let job = first_result["job"]["token"].as_str().unwrap().to_owned();
    std::fs::write(home.join(".release-test-job"), "release").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let wait_started = Instant::now();
    let mut next = first_result["next"].as_str().unwrap().to_owned();
    let result = loop {
        let finished = command(&home, 2_000)
            .current_dir(elsewhere.path())
            .args(next.split_whitespace().skip(1))
            .output()
            .unwrap();
        let result = json(&finished);
        if finished.status.code() == Some(0) {
            break result;
        }
        assert_eq!(finished.status.code(), Some(1), "{result}");
        assert_eq!(result["finished"], false, "{result}");
        assert_eq!(result["job"]["state"], "running", "{result}");
        assert_eq!(result["job"]["token"], job, "{result}");
        assert!(
            wait_started.elapsed() < Duration::from_secs(15),
            "job did not finish through bounded continuations: {result}"
        );
        next = result["next"].as_str().unwrap().to_owned();
    };
    let project = result["project"].clone();
    assert_eq!(project["id"], "relative");
    assert_eq!(
        std::fs::canonicalize(project["path"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(repo).unwrap()
    );
    let replay = command(&home, 2_000)
        .current_dir(elsewhere.path())
        .args(["--json", "jobs", "wait", &job])
        .output()
        .unwrap();
    assert_eq!(json(&replay)["project"], project);
    let service = vwatch::VWatch::open(&home.join("vwatch.db")).unwrap();
    assert_eq!(
        service.list_projects().unwrap().as_array().unwrap().len(),
        1
    );
}
