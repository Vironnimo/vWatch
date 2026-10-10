//! Bounded console calls with durable results, without replaying a mutation.
//!
//! The same console executable does the work in a hidden child. A held OS file
//! lock proves that child is alive; process IDs are never used as that proof.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

use crate::service::{Result, WatchtowerError};

const WORKER: &str = "__watchtower_job_worker";
const DEADLINE: Duration = Duration::from_secs(45);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    command: String,
    arguments: Vec<String>,
    working_dir: PathBuf,
    data_dir: PathBuf,
    created_at: String,
    identity: String,
    json: bool,
}

#[derive(Serialize, Deserialize)]
struct Completed {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

// Rust's Windows process launch can otherwise pass on the console parent's
// inherited pipe handles even when the child's three standard streams are NUL.
// Keeping such a spare stdout handle open makes a harness wait for worker EOF
// after the public process has already exited. Only this launcher's streams are
// touched, and their original flags are restored immediately after spawn.
#[cfg(windows)]
struct StreamInheritance(Vec<(*mut std::ffi::c_void, u32)>);

#[cfg(windows)]
impl StreamInheritance {
    fn suspend() -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{
            GetHandleInformation, SetHandleInformation, HANDLE_FLAG_INHERIT,
        };
        let mut saved = Self(Vec::new());
        for handle in [
            std::io::stdin().as_raw_handle(),
            std::io::stdout().as_raw_handle(),
            std::io::stderr().as_raw_handle(),
        ] {
            let mut flags = 0;
            // A closed or absent standard handle has nothing to inherit.
            if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
                continue;
            }
            if flags & HANDLE_FLAG_INHERIT != 0 {
                if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                saved.0.push((handle, flags));
            }
        }
        Ok(saved)
    }
}

#[cfg(windows)]
impl Drop for StreamInheritance {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
        for (handle, flags) in &self.0 {
            unsafe {
                SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, *flags);
            }
        }
    }
}

fn io_error(action: &str, error: std::io::Error) -> WatchtowerError {
    WatchtowerError::new("job_io", format!("Cannot {action} the saved command: {error}. Its outcome is unknown; changes may have happened."))
        .hint("Run `watchtower jobs list` in the same data folder, then inspect the original command's show or status result before repeating a change.")
}

fn prepare_error(action: &str, error: std::io::Error) -> WatchtowerError {
    WatchtowerError::new("job_start_io", format!("Cannot {action} the command job: {error}. This call did not start project work."))
        .hint("Check the selected data folder and access to its jobs folder. After resolving that problem, run `watchtower jobs list` to inspect earlier calls before repeating a change.")
}

fn job_path(data_dir: &Path, token: &str) -> Result<PathBuf> {
    if token.len() != 44
        || !token.starts_with("job_")
        || !token[4..]
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(WatchtowerError::new("invalid_job_token", "The job token must be job_ followed by 40 lowercase hexadecimal characters. No command was repeated.")
            .hint("Copy the complete jobs wait command from the earlier output. The token is not a file path."));
    }
    Ok(data_dir.join("jobs").join(token))
}

fn read_request(data_dir: &Path, token: &str) -> Result<Request> {
    let path = job_path(data_dir, token)?.join("request.json");
    let bytes = fs::read(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            WatchtowerError::new("job_not_found", format!("Saved command {token} was not found in {}. No command was repeated.", data_dir.display()))
                .hint("Use the data folder from the original command. Run `watchtower jobs list` there to see the saved jobs.")
        } else { io_error("read", error) }
    })?;
    serde_json::from_slice(&bytes).map_err(|_| {
        WatchtowerError::new(
            "job_invalid",
            "The saved command request is unreadable. Its outcome is unknown.",
        )
        .hint("Inspect the original command's show or status result before repeating a change.")
    })
}

fn atomic_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let temporary = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

fn lock_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn running(path: &Path) -> Result<bool> {
    let file = lock_file(&path.join("running.lock")).map_err(|e| io_error("inspect", e))?;
    match file.try_lock() {
        Ok(()) => Ok(false),
        Err(TryLockError::WouldBlock) => Ok(true),
        Err(TryLockError::Error(error)) => Err(io_error("inspect", error)),
    }
}

fn prefix(data_dir: &Path, json_mode: bool) -> String {
    // Pass a suffix so scoped_command can encode an explicit non-default target.
    crate::cli::scoped_command(
        data_dir,
        if json_mode {
            "watchtower --json"
        } else {
            "watchtower "
        },
    )
    .trim_end()
    .to_owned()
}

fn recovery(request: &Request) -> String {
    let action = match request.command.as_str() {
        "runs.start" => "runs status",
        "projects.add" | "projects.update" | "projects.discover" => "projects list",
        _ => "due --no-refresh",
    };
    format!("{} {action}", prefix(&request.data_dir, request.json))
}

fn inspect(data_dir: &Path, token: &str) -> Result<Value> {
    let request = read_request(data_dir, token)?;
    if let Some(result) = completed_result(data_dir, token, &request)? {
        return Ok(result);
    }
    inspect_pending(data_dir, token, &request)
}

fn completed_result(data_dir: &Path, token: &str, request: &Request) -> Result<Option<Value>> {
    let path = job_path(data_dir, token)?;
    let wait = format!("{} jobs wait {token}", prefix(data_dir, request.json));
    match fs::read(path.join("result.json")) {
        Ok(bytes) => {
            let result: Completed = serde_json::from_slice(&bytes).map_err(|_| WatchtowerError::new("job_result_invalid", "The saved result is unreadable. The command may have changed data; its outcome is unknown.")
                .hint(format!("Inspect the current state before retrying: {}", recovery(request))))?;
            Ok(Some(
                json!({"finished":true,"job":{"token":token,"state":"completed","command":request.command,"created_at":request.created_at,"next":wait},"result":result,"next":wait}),
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error("read", error)),
    }
}

fn inspect_pending(data_dir: &Path, token: &str, request: &Request) -> Result<Value> {
    let path = job_path(data_dir, token)?;
    let active = running(&path)?;
    // The worker can save its result and release the lock after the initial
    // result read. Once the lock is free, recheck that durable result before
    // describing the outcome as unknown.
    if !active {
        if let Some(result) = completed_result(data_dir, token, request)? {
            return Ok(result);
        }
    }
    // A newly spawned child takes the lock before writing started.json.
    // Concurrent readers can briefly observe this immutable request first.
    let starting = !path.join("started.json").exists()
        && fs::metadata(path.join("request.json"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(3));
    let state = if active || starting {
        "running"
    } else {
        "unknown"
    };
    let next = if state == "running" {
        format!("{} jobs wait {token}", prefix(data_dir, request.json))
    } else {
        recovery(request)
    };
    Ok(
        json!({"finished":false,"job":{"token":token,"state":state,"command":request.command,"created_at":request.created_at,"next":next},"next":next}),
    )
}

fn deadline(data_dir: &Path) -> Duration {
    test_fault(data_dir, "WATCHTOWER_TEST_JOB_DEADLINE_MS")
        .map(|ms| Duration::from_millis(ms.clamp(20, 45_000)))
        .unwrap_or(DEADLINE)
}

fn test_fault(data_dir: &Path, name: &str) -> Option<u64> {
    // Fault injection is opt-in twice and can never affect the default instance.
    let default = crate::system::app_dir().join("data");
    let selected = fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_owned());
    let default = fs::canonicalize(&default).unwrap_or(default);
    if std::env::var("WATCHTOWER_TEST_INSTANCE").ok().as_deref() != Some("1") || selected == default
    {
        return None;
    }
    std::env::var(name).ok()?.parse().ok()
}

fn wait(data_dir: &Path, token: &str, budget: Duration) -> Result<Value> {
    let started = Instant::now();
    loop {
        let status = inspect(data_dir, token)?;
        if status["job"]["state"] != "running" || started.elapsed() >= budget {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

// Rendering and instance selectors are not a different operation. Parsing has
// already validated these arguments; preserve positional-only values after --.
fn operation_arguments(arguments: &[String]) -> Vec<&str> {
    let mut result = Vec::new();
    let mut options = true;
    let mut skip_value = false;
    for argument in arguments.iter().skip(1) {
        if skip_value {
            skip_value = false;
            continue;
        }
        if argument == "--" {
            options = false;
            result.push(argument.as_str());
            continue;
        }
        if options && argument == "--json" {
            continue;
        }
        if options
            && ["--format", "--output", "-o", "--data-dir", "--instance"]
                .contains(&argument.as_str())
        {
            skip_value = true;
            continue;
        }
        if options
            && ["--format=", "--output=", "--data-dir=", "--instance="]
                .iter()
                .any(|prefix| argument.starts_with(prefix))
        {
            continue;
        }
        if options && argument.starts_with("-o") && argument.len() > 2 {
            continue;
        }
        result.push(argument.as_str());
    }
    result
}

/// Inspect/await a saved request. Completed wait results are rendered by the CLI
/// in the requested format, from captured output, without executing again.
pub fn command(action: &str, token: Option<&str>, data_dir: &Path) -> Result<Value> {
    if action == "list" {
        let mut jobs = Vec::new();
        let directory = data_dir.join("jobs");
        if directory.exists() {
            for entry in fs::read_dir(directory).map_err(|e| io_error("list", e))? {
                let entry = entry.map_err(|e| io_error("list", e))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !entry.file_type().map_err(|e| io_error("list", e))?.is_dir()
                    || job_path(data_dir, &name).is_err()
                {
                    continue;
                }
                match inspect(data_dir, &name) {
                    Ok(status) => jobs.push(status["job"].clone()),
                    Err(error) => jobs.push(json!({"token":name,"state":"unknown","command":"unknown","error":error.message,"next":crate::cli::scoped_command(data_dir, &format!("watchtower jobs status {name}"))})),
                }
            }
        }
        jobs.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
        return Ok(json!({"jobs":jobs}));
    }
    let token = token.ok_or_else(|| {
        WatchtowerError::new(
            "job_token_required",
            "Supply the token of the saved command. No command was repeated.",
        )
        .hint("Run `watchtower jobs list` to see saved jobs.")
    })?;
    if action == "wait" {
        wait(data_dir, token, deadline(data_dir))
    } else {
        inspect(data_dir, token)
    }
}

fn start(args: &[OsString], data_dir: &Path, plan: &crate::cli::JobPlan) -> Result<Value> {
    let data_dir = std::path::absolute(data_dir).map_err(|e| prepare_error("resolve", e))?;
    let working_dir = std::env::current_dir().map_err(|e| prepare_error("resolve", e))?;
    let arguments = args
        .iter()
        .map(|arg| {
            arg.to_str().map(str::to_owned).ok_or_else(|| {
                WatchtowerError::new(
                    "invalid_encoding",
                    "The command contains a non-Unicode argument. Nothing was started.",
                )
                .hint("Use a Unicode path or text value and run the command again.")
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let identity = format!(
        "{:x}",
        Sha1::digest(
            serde_json::to_vec(&(operation_arguments(&arguments), &working_dir, &data_dir))
                .expect("request identity serializes")
        )
    );
    let directory = data_dir.join("jobs");
    fs::create_dir_all(&directory).map_err(|e| prepare_error("create", e))?;
    let launch_lock =
        lock_file(&directory.join("launch.lock")).map_err(|e| prepare_error("coordinate", e))?;
    let deadline_started = Instant::now();
    loop {
        match launch_lock.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if deadline_started.elapsed() < Duration::from_secs(2) => std::thread::sleep(Duration::from_millis(10)),
            Err(TryLockError::WouldBlock) => return Err(WatchtowerError::new("job_launch_busy", "Another command is recording its job. This call did not start work.")
                .hint("Run `watchtower jobs list` to find the already running command before repeating a change.")),
            Err(TryLockError::Error(error)) => return Err(prepare_error("coordinate", error)),
        }
    }
    for entry in fs::read_dir(&directory).map_err(|e| prepare_error("list", e))? {
        let entry = entry.map_err(|e| prepare_error("list", e))?;
        let token = entry.file_name().to_string_lossy().into_owned();
        if !entry
            .file_type()
            .map_err(|e| prepare_error("list", e))?
            .is_dir()
            || job_path(&data_dir, &token).is_err()
        {
            continue;
        }
        let Ok(existing) = read_request(&data_dir, &token) else {
            continue;
        };
        if existing.identity == identity {
            // An unreadable result is not evidence that this operation is safe
            // to launch again. Return that uncertainty instead of duplicating it.
            let existing_status = inspect(&data_dir, &token)?;
            if existing_status["job"]["state"] == "running" {
                drop(launch_lock);
                return wait(
                    &data_dir,
                    &token,
                    deadline(&data_dir).saturating_sub(deadline_started.elapsed()),
                );
            }
        }
    }
    let token = format!(
        "job_{:x}",
        Sha1::digest(format!(
            "{identity}:{}:{}:{}",
            Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    );
    let request = Request {
        token: token.clone(),
        command: plan.command.into(),
        arguments,
        working_dir,
        data_dir: data_dir.clone(),
        created_at: Utc::now().to_rfc3339(),
        identity,
        json: plan.json,
    };
    let path = job_path(&data_dir, &token)?;
    fs::create_dir(&path).map_err(|e| prepare_error("create", e))?;
    atomic_json(&path.join("request.json"), &request).map_err(|e| prepare_error("save", e))?;
    let mut child = Command::new(crate::system::exe_path());
    child
        .args([
            OsString::from(WORKER),
            data_dir.as_os_str().to_owned(),
            OsString::from(&token),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("WATCHTOWER_INTERNAL_JOB_WORKER", "1");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(windows)]
    let stream_inheritance =
        StreamInheritance::suspend().map_err(|e| prepare_error("isolate worker streams for", e))?;
    let spawned = child.spawn();
    #[cfg(windows)]
    drop(stream_inheritance);
    let mut child = match spawned {
        Ok(child) => child,
        Err(cause) => {
            let error = WatchtowerError::new("job_start_failed", format!("The command worker could not start: {cause}. No work was started."))
                .hint("Check that Watchtower.exe is available and executable, then repeat the original command.");
            let result = Completed {
                exit_code: 1,
                stdout: json!({"ok":false,"error":error.to_json()}).to_string(),
                stderr: String::new(),
            };
            let _ = atomic_json(&path.join("result.json"), &result);
            return Err(error);
        }
    };
    // Keep launch coordination only until the worker holds its own liveness lock.
    while !path.join("started.json").exists() {
        if child
            .try_wait()
            .map_err(|e| io_error("inspect", e))?
            .is_some()
        {
            break;
        }
        if deadline_started.elapsed() >= Duration::from_secs(2) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(launch_lock);
    let result = wait(
        &data_dir,
        &token,
        deadline(&data_dir).saturating_sub(deadline_started.elapsed()),
    )?;
    let _ = child.try_wait(); // Reap a completed child on platforms that require it.
    Ok(result)
}

fn worker(data_dir: &Path, token: &str) -> i32 {
    let work = || -> Result<i32> {
        let request = read_request(data_dir, token)?;
        let path = job_path(data_dir, token)?;
        let alive = lock_file(&path.join("running.lock")).map_err(|e| io_error("lock", e))?;
        match alive.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(1),
            Err(TryLockError::Error(error)) => return Err(io_error("lock", error)),
        }
        if path.join("result.json").exists() {
            return Ok(0);
        }
        atomic_json(
            &path.join("started.json"),
            &json!({"started_at":Utc::now().to_rfc3339()}),
        )
        .map_err(|e| io_error("save", e))?;
        std::env::set_current_dir(&request.working_dir)
            .map_err(|e| io_error("restore working directory for", e))?;
        if let Some(ms) = test_fault(data_dir, "WATCHTOWER_TEST_JOB_DELAY_MS") {
            std::thread::sleep(Duration::from_millis(ms.min(120_000)));
        }
        if test_fault(data_dir, "WATCHTOWER_TEST_JOB_BLOCK") == Some(1) {
            let began = Instant::now();
            while !data_dir.join(".release-test-job").exists()
                && began.elapsed() < Duration::from_secs(30)
            {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if test_fault(data_dir, "WATCHTOWER_TEST_JOB_CRASH") == Some(1) {
            return Ok(91);
        }
        let mut arguments: Vec<OsString> = request.arguments.into_iter().map(Into::into).collect();
        if !request.json {
            arguments.insert(1, "--json".into());
        }
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit_code = crate::cli::run_with_streams(
            arguments,
            &request.data_dir,
            &mut std::io::empty(),
            &mut stdout,
            &mut stderr,
        );
        let result = Completed {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        };
        atomic_json(&path.join("result.json"), &result).map_err(|e| io_error("save", e))?;
        drop(alive);
        Ok(exit_code)
    };
    work().unwrap_or(1)
}

/// Actual process entry point. Fast/help/invalid calls stay in-process and never
/// create a job. Hidden workers only execute the already parsed request once.
pub fn main(args: Vec<OsString>) -> i32 {
    if args.get(1).is_some_and(|arg| arg == WORKER) {
        return if args.len() == 4 {
            worker(Path::new(&args[2]), &args[3].to_string_lossy())
        } else {
            2
        };
    }
    let Some(plan) = crate::cli::job_plan(&args) else {
        return crate::cli::main(args);
    };
    let data_dir = plan
        .data_dir
        .clone()
        .unwrap_or_else(crate::system::data_path);
    // The public call is logged once with its original flags. JSON used by the
    // worker is an internal transport, not a second user command.
    let _ = fs::create_dir_all(&data_dir);
    let metadata = crate::cli::runtime_metadata(&args);
    let ticket =
        crate::cli_usage::begin(&data_dir, plan.command, &metadata.flags, metadata.targets);
    let result = start(&args, &data_dir, &plan);
    let outcome = match result
        .as_ref()
        .ok()
        .and_then(|result| result["job"]["state"].as_str())
    {
        Some("running") => Some("not_finished"),
        Some("unknown") => Some("unknown"),
        _ if result.as_ref().is_err_and(|error| {
            matches!(
                error.code.as_str(),
                "job_io" | "job_invalid" | "job_result_invalid"
            )
        }) =>
        {
            Some("unknown")
        }
        _ => None,
    };
    let mut out = Vec::new();
    let mut err = std::io::stderr().lock();
    let code = match result {
        Ok(value) if value["job"]["state"] == "completed" => crate::cli::render_completed_job(
            plan.command,
            &value["result"],
            plan.json,
            &data_dir,
            &mut out,
            &mut err,
        ),
        Ok(value) => crate::cli::write_result("jobs.wait", value, plan.json, &data_dir, &mut out),
        Err(error) => {
            crate::cli::write_error(&mut out, &error, plan.json, &data_dir);
            1
        }
    };
    crate::cli_usage::finish_outcome(
        ticket,
        code,
        String::from_utf8_lossy(&out).chars().count(),
        outcome,
    );
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(&out);
    let _ = stdout.flush();
    let _ = err.flush();
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_between_result_and_liveness_reads_is_not_unknown() {
        let home = tempfile::tempdir().unwrap();
        let token = format!("job_{}", "a".repeat(40));
        let request = Request {
            token: token.clone(),
            command: "runs.start".into(),
            arguments: Vec::new(),
            working_dir: home.path().into(),
            data_dir: home.path().into(),
            created_at: "2026-10-10T08:00:00Z".into(),
            identity: "fixture".into(),
            json: false,
        };
        let path = job_path(home.path(), &token).unwrap();
        fs::create_dir_all(&path).unwrap();
        atomic_json(&path.join("request.json"), &request).unwrap();
        atomic_json(
            &path.join("started.json"),
            &json!({"started_at":request.created_at}),
        )
        .unwrap();
        assert!(completed_result(home.path(), &token, &request)
            .unwrap()
            .is_none());

        // The first probe saw no result. The worker now saves it and exits
        // before the reader can inspect the liveness lock.
        let result = Completed {
            exit_code: 0,
            stdout: json!({"ok":true,"run_id":7}).to_string(),
            stderr: String::new(),
        };
        atomic_json(&path.join("result.json"), &result).unwrap();
        let status = inspect_pending(home.path(), &token, &request).unwrap();
        assert_eq!(status["job"]["state"], "completed");
        assert_eq!(status["result"]["stdout"], result.stdout);
        assert_eq!(status["result"]["exit_code"], 0);

        // A file appearing in the same window is not automatically success.
        fs::write(path.join("result.json"), "incomplete").unwrap();
        assert_eq!(
            inspect_pending(home.path(), &token, &request)
                .unwrap_err()
                .code,
            "job_result_invalid"
        );
        fs::remove_file(path.join("result.json")).unwrap();
        assert_eq!(
            inspect_pending(home.path(), &token, &request).unwrap()["job"]["state"],
            "unknown"
        );
    }
}
