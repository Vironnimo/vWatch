//! Bounded subprocess execution and read-only Git command configuration.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Keep every descendant in a private job, including after its parent exits.
/// Suspending before assignment closes the race with a fast-spawning child.
#[cfg(windows)]
fn spawn_contained(
    cmd: &mut Command,
) -> std::io::Result<(std::process::Child, std::os::windows::io::OwnedHandle)> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Diagnostics::ProcessSnapshotting::{
        PssCaptureSnapshot, PssFreeSnapshot, PssWalkMarkerCreate, PssWalkMarkerFree,
        PssWalkSnapshot, HPSS, HPSSWALK, PSS_CAPTURE_THREADS, PSS_THREAD_ENTRY, PSS_WALK_THREADS,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
        THREAD_SUSPEND_RESUME,
    };

    // PSS descriptors require their own cleanup APIs, not CloseHandle. The
    // snapshot belongs to this caller even though it describes the child.
    struct ThreadSnapshot {
        snapshot: HPSS,
        marker: HPSSWALK,
    }
    impl Drop for ThreadSnapshot {
        fn drop(&mut self) {
            unsafe {
                if !self.marker.is_null() {
                    PssWalkMarkerFree(self.marker);
                }
                if !self.snapshot.is_null() {
                    PssFreeSnapshot(GetCurrentProcess(), self.snapshot);
                }
            }
        }
    }

    // All handles are owned locally and non-inheritable. Closing the last job
    // handle terminates its processes on success, timeout and setup failure.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let job = unsafe { OwnedHandle::from_raw_handle(job) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let mut child = cmd
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
        .spawn()?;
    let resume = || -> std::io::Result<()> {
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // Stable Child does not expose its primary thread handle. Capture only
        // this still-suspended process, avoiding a scan of every system thread.
        let mut snapshot = ThreadSnapshot {
            snapshot: std::ptr::null_mut(),
            marker: std::ptr::null_mut(),
        };
        let status = unsafe {
            PssCaptureSnapshot(
                child.as_raw_handle(),
                PSS_CAPTURE_THREADS,
                0,
                &mut snapshot.snapshot,
            )
        };
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
        let status = unsafe { PssWalkMarkerCreate(std::ptr::null(), &mut snapshot.marker) };
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
        let mut entry: PSS_THREAD_ENTRY = unsafe { std::mem::zeroed() };
        let status = unsafe {
            PssWalkSnapshot(
                snapshot.snapshot,
                PSS_WALK_THREADS,
                snapshot.marker,
                (&mut entry as *mut PSS_THREAD_ENTRY).cast(),
                std::mem::size_of_val(&entry) as u32,
            )
        };
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
        let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.ThreadId) };
        if thread.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    };
    if let Err(error) = resume() {
        // Assignment itself may have failed, leaving the child outside the
        // job. It is still suspended and cannot have spawned descendants.
        let _ = child.kill();
        let _ = child.try_wait();
        return Err(error);
    }
    Ok((child, job))
}

/// Run a program without a console window. Returns (success, stdout or stderr).
pub(super) fn run(program: &str, args: &[&str], cwd: &Path, timeout: Duration) -> (bool, String) {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .envs([
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GCM_INTERACTIVE", "never"),
            ("GIT_PAGER", "cat"),
            ("PAGER", "cat"),
            ("GH_PROMPT_DISABLED", "1"),
            ("LC_ALL", "C"),
            ("LANG", "C"),
            ("LANGUAGE", "C"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    let (mut child, _job) = match spawn_contained(&mut cmd) {
        Ok(spawned) => spawned,
        Err(e) => return (false, crate::system::io_message(&e)),
    };
    #[cfg(not(windows))]
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, crate::system::io_message(&e)),
    };
    let reader = |mut pipe: Box<dyn Read + Send>| {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            let _ = send.send(buf);
        });
        receive
    };
    let out = reader(Box::new(child.stdout.take().expect("piped stdout")));
    let err = reader(Box::new(child.stderr.take().expect("piped stderr")));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => break None,
        }
    };
    // A hook or other descendant may keep the pipes open after its parent exits.
    // The deadline therefore covers both the process and its output readers.
    if let Some(status) = status {
        if let (Ok(out), Ok(err)) = (
            out.recv_timeout(deadline.saturating_duration_since(Instant::now())),
            err.recv_timeout(deadline.saturating_duration_since(Instant::now())),
        ) {
            return if status.success() {
                (true, String::from_utf8_lossy(&out).into_owned())
            } else {
                (false, String::from_utf8_lossy(&err).into_owned())
            };
        }
    }
    if child.kill().is_ok() {
        #[cfg(windows)]
        let _ = child.try_wait();
        #[cfg(not(windows))]
        let _ = child.wait();
    }
    (false, format!("{program} timed out"))
}

pub(super) fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let mut full = vec![
        "--no-optional-locks",
        "-c",
        "core.quotepath=off",
        "-c",
        "diff.autoRefreshIndex=false",
        "-c",
        "i18n.logOutputEncoding=utf-8",
    ];
    full.extend_from_slice(args);
    match run("git", &full, repo, GIT_TIMEOUT) {
        (true, out) => Some(out),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn command_timeout_includes_descendant_output_pipes() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };

        let dir = tempfile::tempdir().unwrap();
        // A unique executable name identifies only this test's descendants,
        // even when the intermediary cmd.exe has already exited.
        let name = format!("vwatch-timeout-{}.exe", std::process::id());
        let system = std::env::var_os("SystemRoot").unwrap();
        std::fs::copy(
            Path::new(&system).join("System32/ping.exe"),
            dir.path().join(&name),
        )
        .unwrap();
        let running = || {
            let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
            assert_ne!(snapshot, INVALID_HANDLE_VALUE);
            let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
            let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut more = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
            while more != 0 {
                let end = entry.szExeFile.iter().position(|c| *c == 0).unwrap();
                if String::from_utf16_lossy(&entry.szExeFile[..end]) == name {
                    return true;
                }
                more = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
            }
            false
        };
        // An explicit .\ path keeps working when NoDefaultCurrentDirectoryInExePath
        // stops cmd.exe from searching the working directory.
        for command in [
            format!(".\\{name} -n 4 127.0.0.1"),
            format!("start \"\" /B .\\{name} -n 4 127.0.0.1"),
        ] {
            let started = Instant::now();
            let result = run(
                "cmd.exe",
                &["/D", "/C", &command],
                dir.path(),
                Duration::from_millis(100),
            );
            assert_eq!(result, (false, "cmd.exe timed out".to_owned()));
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "the output pipes exceeded the command's timeout: {command}"
            );
            let settled_by = Instant::now() + Duration::from_millis(250);
            while running() && Instant::now() < settled_by {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(!running(), "a descendant outlived the command: {command}");
        }
    }
}
