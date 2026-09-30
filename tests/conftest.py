import os
import subprocess
from datetime import datetime, timezone
from pathlib import Path

import pytest

from lookout.service import Lookout
from lookout.store import Store

NOW = datetime(2026, 9, 30, 8, 0, tzinfo=timezone.utc)


def git(repo: Path, *args: str, date: str = "2026-09-29T10:00:00") -> None:
    env = {**os.environ, "GIT_AUTHOR_DATE": date, "GIT_COMMITTER_DATE": date}
    subprocess.run(["git", *args], cwd=repo, check=True, capture_output=True, env=env)


@pytest.fixture
def make_repo(tmp_path):
    def make(name: str = "demo", commits: tuple[str, ...] = ("2026-09-29T10:00:00",)) -> Path:
        repo = tmp_path / name
        repo.mkdir()
        git(repo, "init", "-q", "-b", "main")
        git(repo, "config", "user.email", "t@example.com")
        git(repo, "config", "user.name", "Test")
        for i, date in enumerate(commits):
            (repo / f"f{i}.txt").write_text(f"line {i}\n")
            git(repo, "add", ".")
            git(repo, "commit", "-q", "-m", f"commit {i}", date=date)
        return repo
    return make


class Clock:
    def __init__(self, now: datetime = NOW):
        self.now = now

    def __call__(self) -> datetime:
        return self.now


@pytest.fixture
def clock():
    return Clock()


@pytest.fixture
def service(tmp_path, clock):
    from lookout import facts

    def collect(path, today, with_ci=True):
        return facts.collect(path, today, with_ci=False)

    return Lookout(Store(tmp_path / "lookout.db"), now=clock, collect=collect)


def card(**overrides):
    base = {
        "one_liner": "Ein Testprojekt",
        "phase": "active",
        "headline": "Alles ruhig",
        "reentry": {"next_step": "Tests laufen lassen", "effort": "15min"},
        "suggestions": [{"type": "feature", "title": "Dark Mode", "why": "Weil nachts.", "effort": "S"}],
    }
    base.update(overrides)
    return base
