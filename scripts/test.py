#!/usr/bin/env python3
"""Run vWatch's Rust and UI contracts with bounded, isolated processes."""

from __future__ import annotations

import argparse
from concurrent.futures import FIRST_COMPLETED, CancelledError, ThreadPoolExecutor, wait
import ctypes
from ctypes import wintypes
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import threading
import time
import uuid


ROOT = Path(__file__).resolve().parents[1]
WORKERS = 4
TEST_TIMEOUT = 30
# This single libtest case runs all documented recipes and shell forms in series.
AGGREGATE_RECIPE = "skill_commands::every_cli_recipe_in_the_operating_skill_runs_with_real_arguments"
# The launcher cannot start a test until its process belongs to our Windows job.
LAUNCHER = (
    "import json,subprocess,sys; command=json.load(sys.stdin); "
    "sys.exit(subprocess.call(command,stdin=subprocess.DEVNULL))"
)


class WindowsJob:
    """Contain even detached CLI workers; closing the handle kills descendants."""

    def __init__(self) -> None:
        class BasicLimits(ctypes.Structure):
            _fields_ = [
                ("process_time", ctypes.c_longlong), ("job_time", ctypes.c_longlong),
                ("flags", wintypes.DWORD), ("min_working_set", ctypes.c_size_t),
                ("max_working_set", ctypes.c_size_t), ("active_limit", wintypes.DWORD),
                ("affinity", ctypes.c_size_t), ("priority", wintypes.DWORD),
                ("scheduling", wintypes.DWORD),
            ]

        class ExtendedLimits(ctypes.Structure):
            _fields_ = [
                ("basic", BasicLimits), ("io", ctypes.c_ulonglong * 6),
                ("process_memory", ctypes.c_size_t), ("job_memory", ctypes.c_size_t),
                ("peak_process_memory", ctypes.c_size_t), ("peak_job_memory", ctypes.c_size_t),
            ]

        self.api = ctypes.WinDLL("kernel32", use_last_error=True)
        self.api.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        self.api.CreateJobObjectW.restype = wintypes.HANDLE
        self.api.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        self.api.SetInformationJobObject.restype = wintypes.BOOL
        self.api.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        self.api.AssignProcessToJobObject.restype = wintypes.BOOL
        self.api.QueryInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.c_void_p]
        self.api.QueryInformationJobObject.restype = wintypes.BOOL
        self.api.CloseHandle.argtypes = [wintypes.HANDLE]
        self.api.CloseHandle.restype = wintypes.BOOL
        self.handle = self.api.CreateJobObjectW(None, None)
        if not self.handle:
            raise ctypes.WinError(ctypes.get_last_error())
        limits = ExtendedLimits()
        limits.basic.flags = 0x2000  # JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not self.api.SetInformationJobObject(self.handle, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            error = ctypes.WinError(ctypes.get_last_error())
            self.close()
            raise error

    def assign(self, process: subprocess.Popen) -> None:
        if not self.api.AssignProcessToJobObject(self.handle, wintypes.HANDLE(int(process._handle))):
            raise ctypes.WinError(ctypes.get_last_error())

    def active(self) -> int:
        class Accounting(ctypes.Structure):
            _fields_ = [
                ("times", ctypes.c_longlong * 4), ("faults", wintypes.DWORD),
                ("total", wintypes.DWORD), ("active", wintypes.DWORD),
                ("terminated", wintypes.DWORD),
            ]

        info = Accounting()
        if not self.api.QueryInformationJobObject(self.handle, 1, ctypes.byref(info), ctypes.sizeof(info), None):
            raise ctypes.WinError(ctypes.get_last_error())
        return info.active

    def close(self) -> None:
        if self.handle:
            self.api.CloseHandle(self.handle)
            self.handle = None


def run_process(command: list[str], env: dict[str, str], folder: Path, timeout: float, cancel: threading.Event | None = None, *, check_descendants: bool = True) -> dict:
    """Capture output to disk so a blocked descendant cannot keep a pipe alive."""
    folder.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    status = "passed"
    code = None
    detail = ""
    job = WindowsJob() if os.name == "nt" else None
    process = None
    try:
        with (folder / "stdout.txt").open("wb") as stdout, (folder / "stderr.txt").open("wb") as stderr:
            process = subprocess.Popen(
                [sys.executable, "-c", LAUNCHER], cwd=ROOT, env=env,
                stdin=subprocess.PIPE, stdout=stdout, stderr=stderr,
                start_new_session=os.name != "nt",
                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
            )
            if job:
                job.assign(process)
            if not cancel or not cancel.is_set():
                process.stdin.write(json.dumps(command).encode("utf-8"))
            process.stdin.close()
            deadline = time.monotonic() + timeout
            while True:
                if cancel and cancel.is_set():
                    status, detail = "cancelled", "Run interrupted; the runner terminated this process tree."
                    break
                try:
                    code = process.wait(timeout=min(0.1, max(0.001, deadline - time.monotonic())))
                    status = "passed" if code == 0 else "failed"
                    # Windows can signal exit before updating job accounting.
                    settled_by = time.monotonic() + 0.2
                    while job and job.active() and time.monotonic() < settled_by:
                        time.sleep(0.01)
                    if job and job.active():
                        if check_descendants:
                            status, detail = "failed", "Test left child processes running; the runner terminated them."
                        else:
                            detail = "Build helper processes were terminated after the compiler exited."
                    break
                except subprocess.TimeoutExpired:
                    if time.monotonic() >= deadline:
                        status, detail = "timeout", f"Process exceeded {timeout:g}s; the runner terminated its process tree."
                        break
    finally:
        if job:
            job.close()
        elif process:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        if process:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
    return {
        "status": status, "duration_seconds": round(time.monotonic() - started, 6),
        "exit_code": code, "detail": detail, "logs": str(folder),
    }


def isolated_environment(base: dict[str, str], folder: Path) -> dict[str, str]:
    # Hooks and shell config must not leak an index, identity or failure injection.
    env = {
        key: value for key, value in base.items()
        if not key.upper().startswith(("GIT_", "VWATCH_"))
        and key.upper() not in {"XDG_CONFIG_HOME", "NODE_OPTIONS", "NODE_PATH", "EMAIL"}
    }
    home, temp = folder / "home", folder / "tmp"
    home.mkdir(parents=True, exist_ok=True)
    temp.mkdir(parents=True, exist_ok=True)
    config = home / "empty.gitconfig"
    config.touch()
    env.update({
        "HOME": str(home), "USERPROFILE": str(home),
        "APPDATA": str(home / "AppData" / "Roaming"),
        "LOCALAPPDATA": str(home / "AppData" / "Local"),
        "TEMP": str(temp), "TMP": str(temp), "TMPDIR": str(temp),
        "GIT_CONFIG_GLOBAL": str(config), "GIT_CONFIG_NOSYSTEM": "1",
        # Git ignores a ceiling equal to its starting directory. Use the case
        # parent so commands started directly in TEMP or HOME stay contained.
        "GIT_CEILING_DIRECTORIES": str(folder), "GIT_TERMINAL_PROMPT": "0",
        "VWATCH_HOME": str(folder / "vwatch"), "VWATCH_TEST_INSTANCE": "1",
    })
    return env


def output(folder: Path, name: str = "stdout.txt") -> str:
    return (folder / name).read_text(encoding="utf-8", errors="replace")


def checked(command: list[str], env: dict[str, str], folder: Path, timeout: float = 30, *, check_descendants: bool = True) -> str:
    result = run_process(command, env, folder, timeout, check_descendants=check_descendants)
    (folder / "process.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
    if result["status"] != "passed":
        raise RuntimeError(
            f"Command failed: {subprocess.list2cmdline(command)}\n"
            f"{result['detail']}\n{output(folder)[-4000:]}\n{output(folder, 'stderr.txt')[-4000:]}\n"
            f"Logs: {folder}"
        )
    return output(folder)


def rust_cases(check, env: dict[str, str], folder: Path, scratch: Path, no_default: bool, stress: bool, name_filter: str) -> list[dict]:
    arguments = ["test", "--locked", "--no-run", "--message-format=json"]
    if no_default:
        arguments.append("--no-default-features")
    print("Building Rust test executables (locked dependencies)...", flush=True)
    # MSVC can retain compiler helpers such as vctip.exe after a successful link. The build
    # job still terminates all its descendants; only actual test/discovery leaks are failures.
    messages = checked(check.cargo(ROOT, *arguments), env, folder / "build", timeout=1200, check_descendants=False)
    artifacts = {}
    for line in messages.splitlines():
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get("reason") == "compiler-artifact" and value.get("profile", {}).get("test") and value.get("executable"):
            artifacts[value["executable"]] = value["target"]["name"]
    if not artifacts:
        raise RuntimeError("Cargo produced no test executables.")
    cases = []
    for index, (exe, target) in enumerate(sorted(artifacts.items())):
        discovery = folder / f"discover-{index:03d}"
        isolated = isolated_environment(env, scratch / discovery.name)
        listing = checked([exe, "--list", "--format", "terse"], isolated, discovery / "all")
        ignored_listing = checked([exe, "--list", "--ignored", "--format", "terse"], isolated, discovery / "ignored")
        names = [line.removesuffix(": test") for line in listing.splitlines() if line.endswith(": test")]
        ignored = {line.removesuffix(": test") for line in ignored_listing.splitlines() if line.endswith(": test")}
        for name in names:
            is_stress = name.rsplit("::", 1)[-1].startswith("stress_")
            if is_stress != (name in ignored):
                raise RuntimeError(
                    f"{target}::{name}: ignored tests must use the stress_ name prefix, "
                    'and stress_ tests must have #[ignore = "stress"]. No tests were silently omitted.'
                )
            command = [exe, name, "--exact", "--nocapture", "--test-threads=1"]
            if is_stress:
                command.append("--include-ignored")
            if name_filter in f"{target}::{name}":
                cases.append({"suite": "rust", "name": f"{target}::{name}", "command": command, "skip": is_stress and not stress})
    return cases


def tap_cases(text: str, target: str, logs: str, test_file: str = "") -> list[dict]:
    """Read Node's TAP case results and YAML duration fields, including subtests."""
    cases = []
    current = None
    for line in text.splitlines():
        match = re.match(r"\s*(ok|not ok) \d+ - (.*)", line)
        if match:
            name = match[2]
            # Node reports the file itself as a passed test if no names matched.
            if test_file and os.path.normcase(name.replace("\\\\", "\\")) == os.path.normcase(test_file):
                current = None
                continue
            skipped = " # SKIP" in name
            current = {
                "suite": "ui", "name": f"{target}::{name.split(' # SKIP', 1)[0]}",
                "status": "skipped" if skipped else ("passed" if match[1] == "ok" else "failed"),
                "duration_seconds": 0.0, "logs": logs,
            }
            cases.append(current)
        elif current and (match := re.match(r"\s*duration_ms: ([\d.]+)", line)):
            current["duration_seconds"] = round(float(match[1]) / 1000, 6)
    return cases


def cancelled_case(case: dict) -> list[dict]:
    return [{"suite": case["suite"], "name": case["name"], "status": "cancelled", "duration_seconds": 0.0, "detail": "Run interrupted before this case started."}]


def run_case(case: dict, base_env: dict[str, str], folder: Path, scratch: Path, cancel: threading.Event | None = None) -> list[dict]:
    if cancel and cancel.is_set():
        return cancelled_case(case)
    if case.get("skip"):
        return [{"suite": case["suite"], "name": case["name"], "status": "skipped", "duration_seconds": 0.0}]
    timeout = 120 if case["suite"] == "ui" or case["name"] == AGGREGATE_RECIPE else TEST_TIMEOUT
    result = run_process(case["command"], isolated_environment(base_env, scratch), folder, timeout, cancel)
    result.update(suite=case["suite"], name=case["name"])
    if case["suite"] == "ui":
        cases = tap_cases(output(folder), case["name"], result["logs"], case["command"][-1])
        if result["status"] != "passed" or (not cases and not case.get("filtered")):
            if result["status"] == "passed":
                result.update(status="failed", detail="Node produced no TAP test results.")
            cases.append(result)
        return cases
    if result["status"] == "passed" and "1 passed; 0 failed; 0 ignored" not in output(folder):
        result.update(status="failed", detail="The exact Rust test did not report one executed test.")
    return [result]


def future_results(future, case: dict) -> list[dict]:
    try:
        return future.result()
    except CancelledError:
        return cancelled_case(case)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        return [{"suite": case["suite"], "name": case["name"], "status": "failed", "duration_seconds": 0.0, "detail": str(error)}]


def fixture_root(primary: Path) -> Path:
    """Fixtures must not inherit repository rules or use the owner's profile."""
    override = os.environ.get("VORCH_TEST_ROOT")
    root = Path(os.path.expandvars(override)).expanduser() if override else primary.parent / ".vorch-test-data" / primary.name
    root = root.resolve()
    forbidden = [ROOT.resolve(), primary.resolve()]
    for key in ("HOME", "USERPROFILE"):
        if os.environ.get(key):
            forbidden.append(Path(os.environ[key]).resolve())
    if any(root == path or root.is_relative_to(path) for path in forbidden):
        raise RuntimeError(f"Test fixture root {root} is inside a repository or home directory. Set VORCH_TEST_ROOT to an outside directory.")
    return root


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--suite", choices=("all", "rust", "ui"), default="all", help="select Rust, UI, or both (default: all)")
    parser.add_argument("--no-default-features", action="store_true", help="build Rust core and CLI without desktop features")
    parser.add_argument("--stress", action="store_true", help="also run explicitly marked stress cases")
    parser.add_argument("--filter", default="", metavar="TEXT", help="only run tests whose names contain this case-sensitive text")
    args = parser.parse_args()
    if args.suite == "ui" and args.no_default_features:
        parser.error("--no-default-features only applies to Rust")
    expected = (ROOT / ".python-version").read_text().strip()
    actual = ".".join(map(str, sys.version_info[:3]))
    if actual != expected:
        parser.error(f"Python {expected} is required; current interpreter is {actual}.")
    import check

    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    folder = ROOT / ".tmp" / "test-runs" / f"{stamp}-{uuid.uuid4().hex[:8]}"
    folder.mkdir(parents=True)
    env = check.environment(ROOT, str(folder.relative_to(ROOT / ".tmp")))
    report = {"started_at": stamp, "suite": args.suite, "no_default_features": args.no_default_features, "stress": args.stress, "filter": args.filter, "workers": WORKERS, "tests": []}
    started = time.monotonic()
    failed = False
    completed = False
    interrupted = False
    scratch = None
    try:
        external_root = fixture_root(check.primary_root(ROOT))
        scratch = external_root / folder.name
        scratch.mkdir(parents=True, exist_ok=False)
        report["fixtures"] = str(scratch)
        cases = []
        if args.suite in ("all", "rust"):
            cases.extend(rust_cases(check, env, folder, scratch, args.no_default_features, args.stress, args.filter))
        if args.suite in ("all", "ui"):
            expected_node = (ROOT / ".node-version").read_text().strip().removeprefix("v")
            actual_node = checked(["node", "--version"], env, folder / "node-version").strip().removeprefix("v")
            if actual_node != expected_node:
                raise RuntimeError(f"Node {expected_node} is required; current node is {actual_node}.")
            for path in sorted((ROOT / "tests").glob("ui*.cjs")):
                if path.name == "ui-helpers.cjs":
                    continue
                command = ["node", "--experimental-vm-modules", "--test", "--test-reporter=tap", "--test-concurrency=1", f"--test-timeout={TEST_TIMEOUT * 1000}"]
                if not args.stress:
                    command.append("--test-skip-pattern=^stress: ")
                if args.filter:
                    command.append(f"--test-name-pattern={re.escape(args.filter)}")
                cases.append({"suite": "ui", "name": path.name, "command": command + [str(path)], "filtered": bool(args.filter)})
        print(f"Running {len(cases)} isolated Rust cases / UI files, at most {WORKERS} workers...", flush=True)
        cancel = threading.Event()
        pool = ThreadPoolExecutor(max_workers=WORKERS)
        futures = {}
        collected = set()
        try:
            for index, case in enumerate(cases):
                future = pool.submit(run_case, case, env, folder / f"case-{index:04d}", scratch / f"case-{index:04d}", cancel)
                futures[future] = case
            pending = set(futures)
            count = 0
            while pending:
                done, pending = wait(pending, timeout=0.1, return_when=FIRST_COMPLETED)
                for future in done:
                    results = future_results(future, futures[future])
                    report["tests"].extend(results)
                    collected.add(future)
                    for result in results:
                        if result["status"] not in ("passed", "skipped"):
                            failed = True
                            print(f"FAIL {result['name']}: {result.get('detail', '')}\n  Logs: {result.get('logs', '')}", flush=True)
                    count += 1
                    if count % 50 == 0 or count == len(cases):
                        print(f"Completed {count}/{len(cases)} cases / files.", flush=True)
        except KeyboardInterrupt:
            interrupted = failed = True
            report["error"] = "Interrupted; queued cases cancelled and active process trees terminated."
            print(report["error"], file=sys.stderr, flush=True)
            cancel.set()
            for future in futures:
                future.cancel()
        finally:
            pool.shutdown(wait=True, cancel_futures=interrupted)
        for future, case in futures.items():
            if future not in collected:
                report["tests"].extend(future_results(future, case))
        if interrupted:
            for case in cases[len(futures):]:
                report["tests"].extend(cancelled_case(case))
        if not any(item["status"] != "skipped" for item in report["tests"]):
            raise RuntimeError("No tests matched the requested selection.")
        if not failed:
            # Only this uniquely created run is removable; reports stay in .tmp.
            resolved = scratch.resolve()
            if resolved.parent != external_root or resolved.name != folder.name:
                raise RuntimeError(f"Refusing to remove changed fixture path {resolved}.")
            shutil.rmtree(resolved)
            report["fixtures_removed"] = True
        completed = True
    except KeyboardInterrupt:
        interrupted = failed = True
        report["error"] = "Interrupted during build or discovery; the active process tree was terminated."
        print(report["error"], file=sys.stderr)
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        failed = True
        report["error"] = str(error)
        print(str(error), file=sys.stderr)
    finally:
        report["tests"].sort(key=lambda item: (item["suite"], item["name"]))
        report["duration_seconds"] = round(time.monotonic() - started, 6)
        report["status"] = "passed" if completed and not failed else "failed"
        (folder / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    counts = {status: sum(item["status"] == status for item in report["tests"]) for status in ("passed", "failed", "timeout", "skipped", "cancelled")}
    print(f"Results: {counts}; total {report['duration_seconds']:.2f}s")
    print("Slowest 20 tests (Rust: process wall time; UI: Node test duration):")
    for result in sorted(report["tests"], key=lambda item: item["duration_seconds"], reverse=True)[:20]:
        print(f"  {result['duration_seconds']:7.3f}s {result['name']} [{result['status']}]")
    print(f"Report: {folder / 'report.json'}")
    return 130 if interrupted else (1 if failed else 0)


if __name__ == "__main__":
    raise SystemExit(main())
