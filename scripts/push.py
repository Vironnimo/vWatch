#!/usr/bin/env python
"""Check an immutable commit completely, then push it to origin's main.

The default is main. --no-push only checks; --ref checks another commit, such as
a task commit before landing it, and implies --no-push. Each run owns a unique
detached checkout and persistent logs under the primary checkout's
.tmp/full-checks/. Only the checked commit is pushed, and only as a fast-forward
of origin's main; nothing is fetched or pulled.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import check  # noqa: E402
import worktree  # noqa: E402


@dataclass(frozen=True)
class Step:
    name: str
    command: list[str]


STEPS = [
    Step("tools", [sys.executable, "scripts/check.py", "tools"]),
    Step("static", [sys.executable, "scripts/check.py", "static"]),
    Step("rust-default", [sys.executable, "scripts/test.py", "--suite", "rust"]),
    Step("rust-core", [sys.executable, "scripts/test.py", "--suite", "rust", "--no-default-features"]),
    Step("ui", [sys.executable, "scripts/test.py", "--suite", "ui"]),
    Step("development", [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-p", "test_development.py"]),
]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--no-push", action="store_true", help="check only; do not push")
    parser.add_argument("--ref", default="main", help="local commit to check only (default: main)")
    args = parser.parse_args(argv)
    if args.ref != "main" and not args.no_push:
        parser.error("--ref checks without pushing; add --no-push, or land the work and push main.")
    root = check.primary_root(ROOT)
    commit = check.git(root, "rev-parse", "--verify", "--end-of-options", args.ref + "^{commit}").stdout.strip()
    # Fail before the long checks when there is nowhere to push.
    if not args.no_push and subprocess.run(["git", "remote", "get-url", "origin"], cwd=root,
                                           capture_output=True, check=False).returncode != 0:
        parser.error("there is no origin remote to push to; add --no-push to check only.")
    stamp = time.strftime("%Y%m%d-%H%M%S") + "-" + uuid.uuid4().hex[:12]
    checkout = root / ".worktrees" / (".check-" + stamp)
    logs = root / ".tmp" / "full-checks" / stamp
    logs.mkdir(parents=True)
    if checkout.parent.is_symlink() or checkout.parent.is_junction():
        raise worktree.WorktreeError("private worktree directory cannot be a link")
    check.git(root, "worktree", "add", "--detach", str(checkout), commit)
    print(f"Checking {commit} in {checkout.as_posix()}\nLogs: {logs.as_posix()}", flush=True)
    results: dict[str, bool] = {}
    try:
        env = check.environment(checkout, "full-check")
        for step in STEPS:
            if step.name != "tools" and not results["tools"]:
                print(f"SKIP {step.name}: pinned tools unavailable", flush=True)
                results[step.name] = False
                continue
            started = time.monotonic()
            log = logs / (step.name + ".log")
            with log.open("w", encoding="utf-8") as handle:
                try:
                    result = subprocess.run(step.command, cwd=checkout, env=env, stdout=handle,
                                            stderr=subprocess.STDOUT, check=False)
                    results[step.name] = result.returncode == 0
                except OSError as exc:
                    handle.write(str(exc))
                    results[step.name] = False
            print(f"{'PASS' if results[step.name] else 'FAIL'} {step.name} "
                  f"({time.monotonic() - started:.1f}s)", flush=True)
            if not results[step.name]:
                print("\n".join(log.read_text(encoding="utf-8", errors="replace").splitlines()[-40:]))
    finally:
        reports = checkout / ".tmp" / "test-runs"
        if reports.is_dir():
            worktree.ensure_no_directory_links(reports)
            shutil.copytree(reports, logs / "checkout-test-reports")
        worktree.remove_private_checkout(root, checkout)
    if not all(results.values()):
        print(f"Local check failed for {commit[:12]}; logs retained at {logs.as_posix()}.")
        return 1
    if args.no_push:
        print(f"Every check passed for {commit[:12]}. Nothing pushed (--no-push).")
        return 0
    # The pre-push hook refuses AI attribution; Git refuses a non-fast-forward.
    pushed = subprocess.run(["git", "push", "origin", f"{commit}:refs/heads/main"], cwd=root, check=False)
    if pushed.returncode != 0:
        print(f"Every check passed for {commit[:12]}, but the push failed.")
        return 1
    print(f"Every check passed for {commit[:12]}; pushed to origin/main.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, subprocess.CalledProcessError, worktree.WorktreeError) as exc:
        print(f"Local check failed: {exc}", file=sys.stderr)
        sys.exit(1)
