#!/usr/bin/env python
"""Build the demo instance for the README screenshots.

Creates the fictional repositories of demo_projects.py with back-dated commits
under <root>/projects, registers them in a separate vWatch data folder
<root>/home and, with --replay, saves the recorded cards, brief and owner
reactions from demo-state/. Never touches the installed vWatch data.

    python scripts/readme-assets/seed.py --replay

To record new demo state, seed without --replay, let an agent do a daily run
on the demo instance with the operating Skill, then save its documents with
--export and add the owner's reactions to demo-state/owner.json.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHECKOUT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(CHECKOUT / "scripts"))
import check  # noqa: E402
import demo_projects as demo  # noqa: E402

MARKER = ".readme-demo"
AUTHOR = {"GIT_AUTHOR_NAME": "Sam Rivera", "GIT_AUTHOR_EMAIL": "sam@example.com",
          "GIT_COMMITTER_NAME": "Sam Rivera", "GIT_COMMITTER_EMAIL": "sam@example.com"}


def git(repo: Path, *args: str, when: datetime | None = None) -> str:
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(AUTHOR, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
               GIT_CEILING_DIRECTORIES=str(repo.parent))
    if when:
        stamp = when.strftime("%Y-%m-%dT%H:%M:%S%z")
        env.update(GIT_AUTHOR_DATE=stamp, GIT_COMMITTER_DATE=stamp)
    return subprocess.run(["git", *args], cwd=repo, env=env, check=True, capture_output=True,
                          text=True, encoding="utf-8").stdout.strip()


def write(repo: Path, files: dict[str, str | None]) -> None:
    for name, content in files.items():
        path = repo / name
        if content is None:
            path.unlink()
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8", newline="\n")


def build_repositories(projects: Path, now: datetime) -> list[Path]:
    repos = []
    for name, commits in demo.PROJECTS.items():
        repo = projects / name
        repo.mkdir(parents=True)
        git(repo, "init", "-q", "-b", "main")
        git(repo, "config", "core.autocrlf", "false")
        git(repo, "config", "commit.gpgsign", "false")
        shas = []
        for days, message, files in commits:
            # Spread commits over the working day so activity curves look natural.
            when = (now - timedelta(days=days)).replace(hour=9 + len(shas) % 9, minute=17 * len(shas) % 60)
            write(repo, files)
            git(repo, "add", "-A")
            git(repo, "commit", "-q", "-m", message, when=when)
            shas.append(git(repo, "rev-parse", "HEAD"))
        if name in demo.PUSHED:
            origin = projects.parent / "remotes" / f"{name}.git"
            origin.mkdir(parents=True)
            git(origin, "init", "-q", "--bare", "-b", "main")
            git(repo, "remote", "add", "origin", origin.as_posix())
            git(repo, "push", "-q", "origin", f"{shas[demo.PUSHED[name] - 1]}:refs/heads/main")
            git(repo, "fetch", "-q", "origin")
            git(repo, "branch", "-q", "--set-upstream-to=origin/main", "main")
        if name in demo.TAGS:
            tag, index = demo.TAGS[name]
            git(repo, "tag", tag, shas[index])
        if name in demo.BRANCHES:
            branch, days, message, files = demo.BRANCHES[name]
            git(repo, "switch", "-q", "-c", branch)
            write(repo, files)
            git(repo, "add", "-A")
            git(repo, "commit", "-q", "-m", message, when=now - timedelta(days=days))
            git(repo, "switch", "-q", "main")
        if name in demo.WORKTREES:
            folder, branch = demo.WORKTREES[name]
            git(repo, "worktree", "add", "-q", "-b", branch, (projects / folder).as_posix())
        write(repo, demo.WORKING.get(name, {}))
        repos.append(repo)
    return repos


def vwatch(cli: Path, home: Path, *args: str) -> dict:
    result = subprocess.run([str(cli), "--data-dir", str(home), "--json", *args], capture_output=True,
                            text=True, encoding="utf-8", check=False)
    if result.returncode != 0:
        raise SystemExit(f"vwatch {' '.join(args)} failed:\n{result.stdout}\n{result.stderr}")
    return json.loads(result.stdout)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=CHECKOUT / ".tmp" / "readme-demo")
    parser.add_argument("--cli", type=Path,
                        default=Path(check.git(CHECKOUT, "rev-parse", "--path-format=absolute",
                                               "--git-common-dir").stdout.strip()).parent
                        / "target" / "debug" / "vWatch.exe")
    parser.add_argument("--replay", action="store_true", help="save the recorded cards and reactions")
    parser.add_argument("--export", action="store_true",
                        help="record the existing demo instance's cards and brief in demo-state/")
    args = parser.parse_args()
    root = args.root.resolve()
    home = root / "home"
    if args.export:
        import replay  # noqa: PLC0415

        replay.export(lambda *call: vwatch(args.cli, home, *call), HERE / "demo-state")
        return 0
    if root.exists():
        if not (root / MARKER).is_file():
            raise SystemExit(f"{root} exists and is not a demo folder; choose another --root")
        shutil.rmtree(root)
    root.mkdir(parents=True)
    (root / MARKER).write_text("Created by scripts/readme-assets/seed.py\n", encoding="utf-8")
    now = datetime.now(timezone.utc).astimezone()
    repos = build_repositories(root / "projects", now)
    vwatch(args.cli, home, "projects", "add", *[repo.as_posix() for repo in repos])
    if args.replay:
        import replay  # noqa: PLC0415

        replay.run(lambda *call: vwatch(args.cli, home, *call), HERE / "demo-state")
    print(f"Demo instance: {home}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
