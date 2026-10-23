#!/usr/bin/env python
"""Static checks of the staged tree; never format, stage, install, or run tests.

An index export keeps partially staged content independent of the working tree.
Every error in this candidate commit blocks, including errors in callers that
were not staged. Rust diagnostics use Cargo's structured output.
"""

from __future__ import annotations

import ast
import json
import os
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import check  # noqa: E402
import domain_maps  # noqa: E402


def paths(root: Path, *args: str) -> set[str]:
    return {path for path in check.git(root, *args).stdout.split("\0") if path}


def index_snapshot(root: Path, destination: Path) -> None:
    destination.mkdir(parents=True)
    check.git(root, "checkout-index", "--all", "--prefix", destination.as_posix() + "/")


def remove_snapshot(root: Path, snapshot: Path) -> None:
    """Remove only this run's temporary tree; never traverse directory links."""
    scratch = (root / ".tmp").resolve()
    if not snapshot.resolve().is_relative_to(scratch) or snapshot.resolve() == scratch:
        raise ValueError(f"Unsafe snapshot path: {snapshot}")
    for current, dirs, _files in os.walk(snapshot, followlinks=False):
        for name in dirs:
            path = Path(current) / name
            if path.is_symlink() or path.is_junction():
                raise ValueError(f"Snapshot contains a directory link; kept: {snapshot}")
    shutil.rmtree(snapshot)


def diagnostic_paths(message: dict, root: Path) -> set[str]:
    """Use primary spans, including macro call sites, never line-oriented guesses."""
    spans = message.get("spans", [])
    primary = [span for span in spans if span.get("is_primary")] or spans
    found = set()
    for span in primary:
        while span:
            raw = span.get("file_name", "").replace("\\", "/")
            if raw and not raw.startswith("<"):
                path = Path(raw)
                if path.is_absolute():
                    try:
                        raw = path.resolve().relative_to(root.resolve()).as_posix()
                    except ValueError:
                        raw = path.as_posix()
                found.add(raw.removeprefix("./"))
            span = (span.get("expansion") or {}).get("span")
    return found


def classify_rust(output: str, stderr: str, code: int, root: Path) -> list[str]:
    blocking = []
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("reason") != "compiler-message":
            continue
        message = event["message"]
        if message.get("level") not in {"error", "warning", "failure-note"}:
            continue
        # rustc's final count is not a diagnostic with a source owner.
        if message.get("level") == "failure-note":
            continue
        owners = diagnostic_paths(message, root)
        rendered = message.get("rendered")
        if not rendered:
            rendered = (", ".join(sorted(owners)) + ": " if owners else "") + message.get("message", "")
        blocking.append(rendered.strip())
    if code and not blocking:
        blocking.append((stderr or output or "clippy failed without diagnostics").strip())
    return blocking


def captured(command: list[str], root: Path, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, cwd=root, env=env, capture_output=True, text=True,
                          encoding="utf-8", errors="replace", check=False)


def map_findings(root: Path, snapshot: Path, staged: set[str]) -> tuple[list[str], list[str]]:
    blocking, reported = [], []
    for path in sorted(staged):
        if not domain_maps.is_checked(path):
            continue
        if not (snapshot / path).is_file():
            continue
        text = (snapshot / path).read_text(encoding="utf-8")
        if domain_maps.is_map(path):
            lines = domain_maps.prose_non_ascii(text)
            if lines:
                blocking.append(f"{path}: non-ASCII prose on lines {', '.join(map(str, lines[:10]))}")
            blocking.extend(str(finding) for finding in domain_maps.broken_references(snapshot, path, text))
        budget = domain_maps.budget_for(path)
        if budget is not None and len(text) > budget:
            before = subprocess.run(["git", "show", f"HEAD:{path}"], cwd=root,
                                    capture_output=True, text=True, encoding="utf-8",
                                    errors="replace", check=False)
            if before.returncode or len(text) > len(before.stdout):
                reported.append(f"{path}: grows beyond its {budget}-character budget ({len(text)}).")
    return blocking, reported


def inspect(root: Path, snapshot: Path, staged: set[str]) -> tuple[list[str], list[str]]:
    rust = any(Path(path).suffix == ".rs" or Path(path).name in
               {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "config.toml"} for path in staged)
    javascript = any(Path(path).suffix in {".js", ".cjs", ".mjs"} for path in staged)
    blocking = check.tool_problems(snapshot, rust=rust, node=javascript)
    if blocking:
        return blocking, []
    reported = []
    env = check.environment(snapshot, "commit-check")
    version = check.rust_version(snapshot) if rust else ""
    for path in sorted(staged):
        source = snapshot / path
        if not source.is_file():
            continue
        if source.suffix == ".py":
            try:
                ast.parse(source.read_bytes(), filename=path)
            except (SyntaxError, ValueError) as exc:
                blocking.append(str(exc))
        command = None
        if source.suffix in {".js", ".cjs", ".mjs"}:
            command = ["node", "--check", str(source)]
        elif source.suffix == ".rs":
            command = ["rustup", "run", version, "rustfmt", "--check", "--edition", "2021",
                       "--config", "skip_children=true", str(source)]
        if command:
            result = captured(command, snapshot, env)
            if result.returncode:
                blocking.append((result.stdout + result.stderr).strip())
    if rust:
        for features in ([], ["--no-default-features"]):
            command = ["rustup", "run", version, "cargo", "clippy", "--locked", "--offline",
                       "--all-targets", *features, "--message-format=json",
                       "--target-dir", str(check.primary_root(root) / "target"), "--", "-D", "warnings"]
            result = captured(command, snapshot, env)
            blocking.extend(classify_rust(result.stdout, result.stderr, result.returncode, snapshot))
    blocks, reports = map_findings(root, snapshot, staged)
    return blocking + blocks, reported + reports


def main() -> int:
    snapshot = ROOT / ".tmp" / f"commit-index-{uuid.uuid4().hex}"
    try:
        staged = paths(ROOT, "diff", "--cached", "--name-only", "--diff-filter=ACMRD", "-z")
        print(f"Commit check: {len(staged)} staged files (static checks, index snapshot)", flush=True)
        if not staged:
            return 0
        index_snapshot(ROOT, snapshot)
        blocking, reported = inspect(ROOT, snapshot, staged)
        for problem in reported:
            print(f"Reported, not blocking: {problem}")
        if blocking:
            print("Commit blocked:\n" + "\n".join(blocking))
            print("Fix and stage the intended changes, then commit again. No files were changed or staged by this check.")
            return 1
        print("Commit check passed.")
        return 0
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        print(f"Commit check failed: {exc}", file=sys.stderr)
        return 1
    finally:
        if snapshot.exists():
            try:
                remove_snapshot(ROOT, snapshot)
            except (OSError, ValueError) as exc:
                print(f"Snapshot kept: {exc}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
