#!/usr/bin/env python
"""Run pinned, offline development checks in the current checkout.

No command installs tools, changes source, stages files, or uses the installed
Watchtower data. Cargo dependencies must already be cached for --offline.
"""

from __future__ import annotations

import argparse
import ast
import os
import platform
import subprocess
import sys
import tomllib
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import domain_maps  # noqa: E402


def git(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args], cwd=root, capture_output=True, text=True,
        encoding="utf-8", errors="replace", check=True,
    )


def primary_root(root: Path) -> Path:
    common = git(root, "rev-parse", "--path-format=absolute", "--git-common-dir")
    return Path(common.stdout.strip()).resolve().parent


def rust_version(root: Path) -> str:
    with (root / "rust-toolchain.toml").open("rb") as handle:
        return tomllib.load(handle)["toolchain"]["channel"]


def environment(root: Path, name: str) -> dict[str, str]:
    """Give a run its own scratch and application home, clearing hook-local Git state."""
    scratch = root.resolve() / ".tmp" / name
    scratch.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX",
                "GIT_COMMON_DIR", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                "CARGO_TARGET_DIR"):
        env.pop(key, None)
    env.update({
        "TEMP": str(scratch), "TMP": str(scratch), "TMPDIR": str(scratch),
        "GIT_CEILING_DIRECTORIES": str(root.resolve() / ".tmp"),
        "WATCHTOWER_HOME": str(scratch / "watchtower-home"),
        "CARGO_NET_OFFLINE": "true", "PYTHONDONTWRITEBYTECODE": "1",
    })
    return env


def cargo(root: Path, *args: str) -> list[str]:
    command = ["rustup", "run", rust_version(root), "cargo", *args]
    if args and args[0] in {"build", "check", "clippy", "test", "run", "bench", "doc", "rustc", "rustdoc"}:
        # Options before `--` belong to Cargo; options after it belong to rustc.
        position = command.index("--") if "--" in command else len(command)
        command[position:position] = ["--target-dir", str(primary_root(root) / "target")]
    return command


def run(command: list[str], root: Path, env: dict[str, str] | None = None) -> int:
    print("check: " + subprocess.list2cmdline(command), flush=True)
    try:
        return subprocess.run(command, cwd=root, env=env, check=False).returncode
    except OSError as exc:
        print(f"Could not run {command[0]}: {exc}", file=sys.stderr)
        return 1


def tool_problems(root: Path, *, rust: bool = True, node: bool = True) -> list[str]:
    """Check exact installed runtimes without invoking rustup's installer."""
    problems = []
    expected_python = (root / ".python-version").read_text().strip()
    if platform.python_version() != expected_python:
        problems.append(f"Python {expected_python} required; using {platform.python_version()}.")
    checks = []
    if node:
        checks.append((["node", "--version"], "v" + (root / ".node-version").read_text().strip()))
    if rust:
        version = rust_version(root)
        for command, expected in (("rustc", f"rustc {version} "),
                                  ("rustfmt", "rustfmt "), ("cargo-clippy", "clippy ")):
            checks.append((["rustup", "run", version, command, "--version"], expected))
    for command, expected in checks:
        try:
            result = subprocess.run(command, cwd=root, capture_output=True, text=True,
                                    encoding="utf-8", errors="replace", check=False)
            matches = result.stdout.strip() == expected if command[0] == "node" else result.stdout.startswith(expected)
            if result.returncode or not matches:
                problems.append(f"Required installed tool unavailable: {' '.join(command)}\n"
                                + (result.stdout + result.stderr).strip())
        except OSError as exc:
            problems.append(f"Required installed tool unavailable: {command[0]}: {exc}")
    return problems


def source_files(root: Path) -> list[Path]:
    paths = git(root, "ls-files", "-z", "--cached", "--others", "--exclude-standard").stdout
    return sorted({root / path for path in paths.split("\0") if path and (root / path).is_file()})


def syntax(root: Path, env: dict[str, str]) -> int:
    failed = False
    for path in source_files(root):
        if path.suffix == ".py":
            try:
                ast.parse(path.read_bytes(), filename=str(path))
            except (SyntaxError, ValueError) as exc:
                print(exc, file=sys.stderr)
                failed = True
        elif path.suffix in {".js", ".cjs", ".mjs"}:
            failed |= run(["node", "--check", str(path)], root, env) != 0
    return int(failed)


def static(root: Path, env: dict[str, str]) -> int:
    failures = [syntax(root, env)]
    report = domain_maps.corpus_report(root)
    print(domain_maps.format_report(report))
    failures.append(int(any(finding.blocking for finding in report.findings)))
    failures.append(run(cargo(root, "fmt", "--all", "--", "--check"), root, env))
    for features in ([], ["--no-default-features"]):
        failures.append(run(cargo(root, "clippy", "--locked", "--offline", "--all-targets",
                                  *features, "--", "-D", "warnings"), root, env))
    failures.append(run([sys.executable, "scripts/commit_authorship.py", "history"], root, env))
    return int(any(failures))


def tests(root: Path, env: dict[str, str]) -> int:
    commands = [
        [sys.executable, "scripts/test.py", "--suite", "all"],
        [sys.executable, "scripts/test.py", "--suite", "rust", "--no-default-features"],
        [sys.executable, "-m", "unittest", "discover", "-s", "tests", "-p", "test_development.py"],
    ]
    return int(any([run(command, root, env) for command in commands]))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("mode", choices=("tools", "static", "test", "all"), nargs="?", default="all")
    args = parser.parse_args(argv)
    problems = tool_problems(ROOT)
    if problems:
        print("\n".join(problems), file=sys.stderr)
        print("Install the versions in rust-toolchain.toml, .node-version and .python-version; "
              "checks never install packages.", file=sys.stderr)
        return 1
    if args.mode == "tools":
        print("Pinned Rust, rustfmt, clippy, Node and Python are available.")
        return 0
    env = environment(ROOT, f"checks-{uuid.uuid4().hex[:12]}")
    failures = []
    if args.mode in {"all", "static"}:
        failures.append(static(ROOT, env))
    if args.mode in {"all", "test"}:
        failures.append(tests(ROOT, env))
    return int(any(failures))


if __name__ == "__main__":
    sys.exit(main())
