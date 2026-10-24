"""Development workflow contracts: staged content and local work survive checks."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import check  # noqa: E402
import commit_check  # noqa: E402
import worktree  # noqa: E402
import test as test_runner  # noqa: E402


class Development(unittest.TestCase):
    def setUp(self):
        fixtures = check.primary_root(ROOT).parent / ".vorch-test-data" / ROOT.name
        fixtures.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="development-", dir=fixtures)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        home = self.base / "home"
        home.mkdir()
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env["RUSTUP_HOME"] = os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
        env["CARGO_HOME"] = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))
        env.update({"HOME": str(home), "USERPROFILE": str(home), "GIT_CONFIG_NOSYSTEM": "1",
                    "GIT_CONFIG_GLOBAL": os.devnull, "GIT_AUTHOR_NAME": "Julian B.",
                    "GIT_AUTHOR_EMAIL": "virojb@web.de", "GIT_COMMITTER_NAME": "Julian B.",
                    "GIT_COMMITTER_EMAIL": "virojb@web.de", "PYTHONDONTWRITEBYTECODE": "1"})
        self.environment = patch.dict(os.environ, env, clear=True)
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.setup_commands = patch.object(worktree, "SETUP_COMMANDS", [])
        self.setup_commands.start()
        self.addCleanup(self.setup_commands.stop)
        self.git("init", "-b", "main")
        self.git("config", "core.autocrlf", "false")
        self.git("config", "commit.gpgsign", "false")
        (self.repo / ".gitignore").write_text(".tmp/\n.worktrees/\n.vorch-worktree\ntarget/\n", encoding="utf-8")
        (self.repo / "scripts").mkdir()
        for name in ("check.py", "commit_check.py", "domain_maps.py", "install.ps1"):
            shutil.copy2(ROOT / "scripts" / name, self.repo / "scripts" / name)
        for name in (".python-version", ".node-version", "rust-toolchain.toml"):
            shutil.copy2(ROOT / name, self.repo / name)
        (self.repo / "a.txt").write_text("base\n")
        (self.repo / "b.txt").write_text("base\n")
        hooks = self.repo / ".githooks"
        hooks.mkdir()
        for name in ("pre-commit", "commit-msg"):
            (hooks / name).write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            (hooks / name).chmod(0o755)
        self.git("add", ".")
        self.git("commit", "-m", "test: fixture")
        self.git("config", "core.hooksPath", ".githooks")
        self.trees = worktree.Worktrees(self.repo)

    def git(self, *args, cwd=None):
        return subprocess.run(["git", *args], cwd=cwd or self.repo, capture_output=True,
                              text=True, encoding="utf-8", errors="replace", check=True).stdout.strip()

    def create(self):
        worktree.create(self.trees, argparse.Namespace(name="task"))
        return self.repo / ".worktrees" / "task"

    def commit_probe(self, staged, working):
        path = self.repo / "probe.py"
        path.write_text(staged, encoding="utf-8")
        self.git("add", "probe.py")
        path.write_text(working, encoding="utf-8")
        index_before = self.git("write-tree")
        result = subprocess.run([sys.executable, "scripts/commit_check.py"], cwd=self.repo,
                                capture_output=True, text=True, encoding="utf-8", errors="replace")
        self.assertEqual(self.git("write-tree"), index_before)
        self.assertEqual(path.read_text(encoding="utf-8"), working)
        return result

    def test_checks_staged_content_and_preserves_partially_staged_work(self):
        bad_staged = self.commit_probe("def broken(:\n", "value = 1\n")
        self.assertNotEqual(bad_staged.returncode, 0, bad_staged.stdout + bad_staged.stderr)
        good_staged = self.commit_probe("value = 1\n", "def unfinished(:\n")
        self.assertEqual(good_staged.returncode, 0, good_staged.stdout + good_staged.stderr)

    @unittest.skipUnless(os.name == "nt", "installer uses Windows PowerShell")
    def test_installer_installs_from_the_primary_target_or_its_package(self):
        task = self.create()
        probe = self.base / "installer-probe.ps1"
        # Stop after observing the two copy sources. No copy, process shutdown,
        # application execution, registry access or Start menu change occurs.
        probe.write_text(r'''
param([string]$Installer, [string]$Destination)
$ErrorActionPreference = 'Stop'
$global:copies = @()
function cargo {
    $global:build = @{ args = @($args); cwd = (Get-Location).Path }
    $global:LASTEXITCODE = 0
}
function Get-Process { @() }
function Copy-Item {
    param([switch]$Force, [string]$LiteralPath, [string]$Destination)
    $global:copies += @{ source = $LiteralPath; destination = $Destination }
    if ($global:copies.Count -eq 2) { throw 'probe complete' }
}
try { & $Installer -Target $Destination; throw 'installer did not stop' }
catch { if ($_.Exception.Message -ne 'probe complete') { throw } }
@{ build = $global:build; copies = $global:copies } | ConvertTo-Json -Depth 5 -Compress
''', encoding="utf-8")

        def install(installer, env):
            result = subprocess.run(
                ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(probe),
                 "-Installer", str(installer), "-Destination", str(self.base / "installation")],
                cwd=self.base, env=env, capture_output=True, text=True, timeout=15,
            )
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            return json.loads(result.stdout)

        def assert_copied_from(observed, source):
            for copied, binary in zip(observed["copies"], ("vWatch.exe", "vWatch-UI.exe"), strict=True):
                self.assertEqual(Path(copied["source"]), source / binary)
                self.assertEqual(Path(copied["destination"]), self.base / "installation" / binary)

        for checkout in (self.repo, task):
            for override in (False, True):
                with self.subTest(checkout=checkout.name, override=override):
                    env = dict(os.environ)
                    if override:
                        env["CARGO_TARGET_DIR"] = str(self.base / "wrong-target")
                        env["CARGO_BUILD_TARGET_DIR"] = str(self.base / "also-wrong")
                    observed = install(checkout / "scripts" / "install.ps1", env)
                    self.assertEqual(Path(observed["build"]["cwd"]), checkout)
                    self.assertEqual(observed["build"]["args"],
                                     ["build", "--release", "--bins", "--target-dir", str(self.repo / "target")])
                    assert_copied_from(observed, self.repo / "target" / "release")
                    self.assertFalse((checkout / "target").exists())
        # An extracted release package installs its own executables without building.
        with self.subTest(checkout="package"):
            package = self.base / "package"
            package.mkdir()
            shutil.copy2(self.repo / "scripts" / "install.ps1", package / "install.ps1")
            for binary in ("vWatch.exe", "vWatch-UI.exe"):
                (package / binary).write_bytes(b"")
            observed = install(package / "install.ps1", dict(os.environ))
            self.assertIsNone(observed["build"])
            assert_copied_from(observed, package)

    def test_delete_preserves_ignored_data_and_unmerged_commits(self):
        task = self.create()
        data = task / ".tmp" / "vwatch.db"
        data.parent.mkdir()
        data.write_bytes(b"owner data")
        with self.assertRaises(worktree.WorktreeError):
            worktree.delete(self.trees, argparse.Namespace(name="task", force=False))
        self.assertEqual(data.read_bytes(), b"owner data")
        data.unlink()
        (task / "a.txt").write_text("new commit\n")
        self.git("add", "a.txt", cwd=task)
        self.git("commit", "-m", "test: task", cwd=task)
        with self.assertRaises(worktree.WorktreeError):
            worktree.delete(self.trees, argparse.Namespace(name="task", force=False))
        self.assertEqual(self.git("branch", "--show-current", cwd=task), "codex/task")

    def test_landing_keeps_primary_edits_task_data_and_previous_runs(self):
        task = self.create()
        old = self.repo / ".worktrees" / ".landing-task"
        old.mkdir()
        (old / "retained.txt").write_text("previous run")
        (task / "a.txt").write_text("task change\n")
        self.git("add", "a.txt", cwd=task)
        self.git("commit", "-m", "test: task", cwd=task)
        (self.repo / "b.txt").write_text("unfinished owner work\n")
        data = task / ".tmp" / "vwatch.db"
        data.parent.mkdir()
        data.write_bytes(b"keep")
        result = worktree.merge(self.trees, argparse.Namespace(name="task", message=None, wait_timeout=1))
        self.assertEqual(result, 0)
        self.assertEqual((self.repo / "a.txt").read_text(), "task change\n")
        self.assertEqual((self.repo / "b.txt").read_text(), "unfinished owner work\n")
        self.assertEqual(data.read_bytes(), b"keep")
        self.assertEqual((old / "retained.txt").read_text(), "previous run")

    def test_failed_merge_hook_leaves_main_unchanged(self):
        task = self.create()
        hook = task / ".githooks" / "pre-commit"
        hook.write_text("#!/bin/sh\nexit 1\n", encoding="utf-8")
        self.git("config", "core.hooksPath", ".githooks-fixture-disabled")
        self.git("add", ".githooks/pre-commit", cwd=task)
        self.git("commit", "-m", "test: refusing hook", cwd=task)
        self.git("config", "core.hooksPath", ".githooks")
        before = self.git("rev-parse", "main")
        with self.assertRaises(worktree.WorktreeError):
            worktree.merge(self.trees, argparse.Namespace(name="task", message=None, wait_timeout=1))
        self.assertEqual(self.git("rev-parse", "main"), before)
        self.assertTrue(task.is_dir())

    def test_landing_refuses_overlap_without_autostashing_owner_work(self):
        task = self.create()
        (task / "a.txt").write_text("task change\n")
        self.git("add", "a.txt", cwd=task)
        self.git("commit", "-m", "test: task", cwd=task)
        (self.repo / "a.txt").write_text("unfinished owner work\n")
        self.git("config", "merge.autostash", "true")
        before_main = self.git("rev-parse", "main")
        before_status = self.git("status", "--porcelain")
        before_file = (self.repo / "a.txt").read_bytes()
        with self.assertRaises(worktree.WorktreeError):
            worktree.merge(self.trees, argparse.Namespace(name="task", message=None, wait_timeout=1))
        self.assertEqual(self.git("rev-parse", "main"), before_main)
        self.assertEqual(self.git("status", "--porcelain"), before_status)
        self.assertEqual((self.repo / "a.txt").read_bytes(), before_file)
        self.assertEqual(self.git("stash", "list"), "")
        self.assertEqual(self.git("config", "--get", "merge.autostash"), "true")
        self.assertTrue(task.is_dir())
        self.assertEqual(self.git("branch", "--show-current", cwd=task), "codex/task")

    def test_staged_rust_api_break_blocks_even_when_its_caller_has_unstaged_work(self):
        source = self.repo / "src"
        source.mkdir()
        (self.repo / "Cargo.toml").write_text(
            '[package]\nname = "hook-fixture"\nversion = "0.1.0"\nedition = "2021"\n'
        )
        library = source / "lib.rs"
        caller = source / "caller.rs"
        library.write_text("pub mod caller;\n\npub fn value() -> u8 {\n    1\n}\n")
        caller.write_text("pub fn call() -> u8 {\n    crate::value()\n}\n")
        subprocess.run(check.cargo(self.repo, "generate-lockfile", "--offline"), cwd=self.repo,
                       check=True, capture_output=True)
        self.git("add", "Cargo.toml", "Cargo.lock", "src")
        self.git("commit", "-m", "test: rust fixture")
        library.write_text("pub mod caller;\n\npub fn new_value() -> u8 {\n    1\n}\n")
        self.git("add", "src/lib.rs")
        caller.write_text(caller.read_text() + "// unfinished work\n")
        before_index = self.git("write-tree")
        before_caller = caller.read_bytes()
        result = subprocess.run([sys.executable, "scripts/commit_check.py"], cwd=self.repo,
                                capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=30)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("src/caller.rs", result.stdout.replace("\\", "/"))
        self.assertEqual(self.git("write-tree"), before_index)
        self.assertEqual(caller.read_bytes(), before_caller)

    def test_rust_diagnostics_report_source_errors_but_ignore_notes(self):
        def diagnostic(path, level="error"):
            return json.dumps({"reason": "compiler-message", "message": {
                "level": level, "message": "broken", "spans": [
                    {"file_name": str(self.repo / path), "is_primary": True}],
            }})
        output = "\n".join([diagnostic("src/staged.rs"), diagnostic("src/wip.rs"),
                            diagnostic("src/stable.rs"), diagnostic("src/stable.rs", "note")])
        blocking = commit_check.classify_rust(output, "", 1, self.repo)
        self.assertEqual(len(blocking), 3)
        self.assertTrue(commit_check.classify_rust("", "tool failed", 1, self.repo))


class TestRunner(unittest.TestCase):
    """Safety boundaries require real disposable processes, not application builds."""

    def setUp(self):
        fixtures = test_runner.fixture_root(check.primary_root(ROOT))
        fixtures.mkdir(parents=True, exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="runner-contract-", dir=fixtures)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)

    def test_isolation_removes_inherited_owner_config_and_rejects_forbidden_roots(self):
        hostile = dict(os.environ, HOME="owner-home", USERPROFILE="owner-profile",
                       GIT_INDEX_FILE="owner-index", GIT_AUTHOR_NAME="owner-name",
                       VWATCH_FAULT_INJECT="1", NODE_OPTIONS="--require owner-code",
                       NODE_PATH="owner-libraries", EMAIL="owner@example.invalid",
                       XDG_CONFIG_HOME="owner-config")
        env = test_runner.isolated_environment(hostile, self.base / "case")
        for key in ("GIT_INDEX_FILE", "GIT_AUTHOR_NAME", "VWATCH_FAULT_INJECT",
                    "NODE_OPTIONS", "NODE_PATH", "EMAIL", "XDG_CONFIG_HOME"):
            self.assertNotIn(key, env)
        for key in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "TMPDIR",
                    "VWATCH_HOME", "GIT_CONFIG_GLOBAL", "GIT_CEILING_DIRECTORIES"):
            self.assertTrue(Path(env[key]).is_relative_to(self.base))
        self.assertEqual(env["VWATCH_TEST_INSTANCE"], "1")
        self.assertEqual(env["GIT_CONFIG_NOSYSTEM"], "1")
        self.assertEqual(env["GIT_TERMINAL_PROMPT"], "0")
        subprocess.run(["git", "init", "--template=", str(self.base)],
                       env=env, check=True, capture_output=True, timeout=10)
        # Use an actual child to establish that both inherited config and Git's
        # parent repository discovery are absent at the process boundary.
        child = subprocess.run(
            [sys.executable, "-c", "import json,os,subprocess; print(json.dumps({'home':os.environ['HOME'],"
             "'git':subprocess.run(['git','rev-parse','--show-toplevel'],cwd=os.environ['TEMP'],"
             "capture_output=True).returncode}))"],
            env=env, cwd=self.base, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(child.returncode, 0, child.stderr)
        self.assertEqual(json.loads(child.stdout), {"home": env["HOME"], "git": 128})
        primary, owner = self.base / "primary", self.base / "owner"
        with patch.dict(os.environ, {"HOME": str(owner), "USERPROFILE": str(owner / "profile")}):
            for forbidden in (ROOT, primary, owner, owner / "profile"):
                for candidate in (forbidden, forbidden / "nested"):
                    with self.subTest(root=candidate), patch.dict(os.environ, {"VORCH_TEST_ROOT": str(candidate)}):
                        with self.assertRaisesRegex(RuntimeError, "inside a repository or home"):
                            test_runner.fixture_root(primary)
            allowed = self.base / "outside"
            with patch.dict(os.environ, {"VORCH_TEST_ROOT": str(allowed)}):
                self.assertEqual(test_runner.fixture_root(primary), allowed)
                self.assertFalse(allowed.exists())

    @unittest.skipUnless(os.name == "nt", "Windows Job Object descendant contract")
    def test_timeout_cancellation_and_leaked_descendants_terminate_the_owned_tree(self):
        import ctypes
        from ctypes import wintypes

        api = ctypes.WinDLL("kernel32", use_last_error=True)
        api.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        api.OpenProcess.restype = wintypes.HANDLE
        api.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
        api.GetExitCodeProcess.restype = wintypes.BOOL
        api.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
        api.WaitForSingleObject.restype = wintypes.DWORD
        api.CloseHandle.argtypes = [wintypes.HANDLE]
        api.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
        for mode in ("timeout", "cancelled", "leak", "build_helper"):
            with self.subTest(mode=mode):
                folder = self.base / mode
                marker = folder / "child.pid"
                # The child is detached, so terminating just the launcher or
                # ordinary process group would leave it alive on Windows.
                child = "import os,time; from pathlib import Path; Path(" + repr(str(marker)) + ").write_text(str(os.getpid())); time.sleep(20)"
                parent = (
                    "import subprocess,sys,time; from pathlib import Path; "
                    f"p=subprocess.Popen([sys.executable,'-c',{child!r}],creationflags=0x208); "
                    f"marker=Path({str(marker)!r}); "
                    "deadline=time.monotonic()+5; "
                    "exec('while not marker.exists() and time.monotonic()<deadline: time.sleep(0.01)'); "
                    + ("sys.exit(0)" if mode in ("leak", "build_helper") else "p.wait()")
                )
                cancel = threading.Event()
                ready = []

                def request_cancel():
                    deadline = time.monotonic() + 5
                    while not marker.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    ready.append(marker.exists())
                    cancel.set()

                trigger = threading.Thread(target=request_cancel) if mode == "cancelled" else None
                if trigger:
                    trigger.start()
                try:
                    result = test_runner.run_process(
                        [sys.executable, "-c", parent],
                        test_runner.isolated_environment(dict(os.environ), folder),
                        folder / "logs", 1 if mode == "timeout" else 8, cancel,
                        check_descendants=mode != "build_helper",
                    )
                finally:
                    cancel.set()
                    if trigger:
                        trigger.join(timeout=6)
                self.assertTrue(marker.exists(), result)
                pid = int(marker.read_text())
                handle = api.OpenProcess(0x101001, False, pid)  # sync, terminate, query limited
                if handle:
                    try:
                        status = wintypes.DWORD()
                        self.assertEqual(api.WaitForSingleObject(handle, 5000), 0, result)
                        self.assertTrue(api.GetExitCodeProcess(handle, ctypes.byref(status)))
                        self.assertNotEqual(status.value, 259, result)
                    finally:
                        # Only this case's PID is eligible for emergency teardown
                        # if containment regresses; wait before fixture cleanup.
                        api.TerminateProcess(handle, 1)
                        api.WaitForSingleObject(handle, 5000)
                        api.CloseHandle(handle)
                else:
                    self.assertEqual(ctypes.get_last_error(), 87, "could not inspect owned child")
                expected = {"leak":"failed", "build_helper":"passed"}.get(mode, mode)
                self.assertEqual(result["status"], expected, result)
                self.assertIn("terminated", result["detail"])
                if trigger:
                    self.assertEqual(ready, [True])

    def test_reports_preserve_failed_fixtures_and_cleanup_only_the_successful_unique_run(self):
        repository = self.base / "repo"
        repository.mkdir()
        shutil.copy2(ROOT / ".python-version", repository / ".python-version")
        external = self.base / "external"
        external.mkdir()
        sentinel = external / "previous-run" / "keep.txt"
        sentinel.parent.mkdir()
        sentinel.write_text("unrelated retained data")
        marker = self.base / "active"

        for mode in ("passed", "failed", "cancelled"):
            with self.subTest(mode=mode):
                if marker.exists():
                    marker.unlink()
                body = (f"from pathlib import Path; import time; Path({str(marker)!r}).touch(); time.sleep(20)"
                        if mode == "cancelled" else
                        "print('1 passed; 0 failed; 0 ignored')" if mode == "passed" else "raise SystemExit(2)")
                cases = [{"suite": "rust", "name": f"synthetic::{i}", "command": [sys.executable, "-c", body]}
                         for i in range(2 if mode == "cancelled" else 1)]

                def interrupt_when_started(*args, **kwargs):
                    deadline = time.monotonic() + 5
                    while not marker.exists() and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(marker.exists(), "synthetic job never started")
                    raise KeyboardInterrupt

                with patch.object(test_runner, "ROOT", repository), patch.object(check, "primary_root", return_value=repository), \
                        patch.object(test_runner, "rust_cases", return_value=cases), patch.object(test_runner, "WORKERS", 1), \
                        patch.dict(os.environ, {"VORCH_TEST_ROOT": str(external)}), patch.object(sys, "argv", ["test.py", "--suite", "rust"]):
                    if mode == "cancelled":
                        with patch.object(test_runner, "wait", side_effect=interrupt_when_started):
                            code = test_runner.main()
                    else:
                        code = test_runner.main()
                reports = sorted((repository / ".tmp" / "test-runs").glob("*/report.json"), key=lambda path: path.stat().st_mtime_ns)
                report = json.loads(reports[-1].read_text())
                self.assertEqual(code, {"passed": 0, "failed": 1, "cancelled": 130}[mode])
                self.assertEqual(report["status"], "passed" if mode == "passed" else "failed")
                self.assertEqual([item["status"] for item in report["tests"]], [mode] * len(cases))
                self.assertEqual(Path(report["fixtures"]).exists(), mode != "passed")
                self.assertEqual(report.get("fixtures_removed", False), mode == "passed")
                self.assertEqual(sentinel.read_text(), "unrelated retained data")


if __name__ == "__main__":
    unittest.main()
