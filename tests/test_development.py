"""Development workflow contracts: staged content and local work survive checks."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import check  # noqa: E402
import commit_check  # noqa: E402
import worktree  # noqa: E402


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
    def test_installer_builds_and_copies_from_the_primary_target(self):
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
        for checkout in (self.repo, task):
            for override in (False, True):
                with self.subTest(checkout=checkout.name, override=override):
                    env = dict(os.environ)
                    if override:
                        env["CARGO_TARGET_DIR"] = str(self.base / "wrong-target")
                        env["CARGO_BUILD_TARGET_DIR"] = str(self.base / "also-wrong")
                    result = subprocess.run(
                        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(probe),
                         "-Installer", str(checkout / "scripts" / "install.ps1"),
                         "-Destination", str(self.base / "installation")],
                        cwd=self.base, env=env, capture_output=True, text=True, timeout=15,
                    )
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                    observed = json.loads(result.stdout)
                    self.assertEqual(Path(observed["build"]["cwd"]), checkout)
                    self.assertEqual(observed["build"]["args"],
                                     ["build", "--release", "--bins", "--target-dir", str(self.repo / "target")])
                    for copied, binary in zip(observed["copies"], ("Watchtower.exe", "Watchtower-UI.exe"), strict=True):
                        self.assertEqual(Path(copied["source"]), self.repo / "target" / "release" / binary)
                        self.assertEqual(Path(copied["destination"]), self.base / "installation" / binary)
                    self.assertFalse((checkout / "target").exists())

    def test_delete_preserves_ignored_data_and_unmerged_commits(self):
        task = self.create()
        data = task / ".tmp" / "watchtower.db"
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
        data = task / ".tmp" / "watchtower.db"
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


if __name__ == "__main__":
    unittest.main()
