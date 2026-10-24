#!/usr/bin/env python
"""Check an immutable local commit completely; this repository never pushes.

Use --no-push (required). The default is main; --ref checks a task commit before
landing it. Each run owns a unique detached checkout and persistent logs under
the primary checkout's .tmp/full-checks/. No fetch, pull or push is performed.
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
    parser.add_argument("--no-push", action="store_true", help="required: perform a local check only")
    parser.add_argument("--ref", default="main", help="local commit to check (default: main)")
    args = parser.parse_args(argv)
    if not args.no_push:
        parser.error("vWatch is local-only. Use --no-push; network publishing is disabled.")
    root = check.primary_root(ROOT)
    commit = check.git(root, "rev-parse", "--verify", "--end-of-options", args.ref + "^{commit}").stdout.strip()
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
    print(f"Every check passed for {commit[:12]}. Local-only; nothing pushed.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, subprocess.CalledProcessError, worktree.WorktreeError) as exc:
        print(f"Local check failed: {exc}", file=sys.stderr)
        sys.exit(1)
