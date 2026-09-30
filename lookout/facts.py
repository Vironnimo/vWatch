"""Exact repository facts, computed from git without any model.

Everything here is read-only: no fetch, no checkout, nothing that changes a
repository. The GitHub CI lookup is optional and uses the `gh` CLI when it is
installed and the repository has a github.com remote.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Any

WEEKS = 26
_GIT_TIMEOUT = 30
_GH_TIMEOUT = 20
_MAX_UNTRACKED_FILES = 20_000
_MAX_UNTRACKED_BYTES = 2_000_000
_DOC_FILES = (
    "README.md", "AGENTS.md", "TODO.md", "ROADMAP.md", "VISION.md", "IDEAS.md", "CHANGELOG.md",
    ".vorch/PROJECT.md", ".vorch/FLAGGED.md",
)
_GITHUB = re.compile(r"github\.com[:/]+([^/\s]+)/([^/\s]+?)(?:\.git)?/?$")
_NO_WINDOW = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0


def _run(cmd: list[str], cwd: str, timeout: int) -> tuple[int, str]:
    try:
        proc = subprocess.run(
            cmd, cwd=cwd, capture_output=True, encoding="utf-8", errors="replace",
            timeout=timeout, creationflags=_NO_WINDOW, stdin=subprocess.DEVNULL,
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        return 1, str(exc)
    return proc.returncode, proc.stdout if proc.returncode == 0 else proc.stderr


def _git(repo: str, *args: str) -> str | None:
    code, out = _run(["git", "-c", "core.quotepath=off", "-c", "i18n.logOutputEncoding=utf-8", *args], repo, _GIT_TIMEOUT)
    return out if code == 0 else None


def _iso(ts: float) -> str:
    return datetime.fromtimestamp(ts, tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _local_date(ts: float) -> str:
    return datetime.fromtimestamp(ts).date().isoformat()


def _monday(d: date) -> date:
    return d - timedelta(days=d.weekday())


def _commits(repo: str, today: date) -> dict[str, Any]:
    out = _git(repo, "log", "--format=%cs", "HEAD")
    dates = [date.fromisoformat(line) for line in (out or "").split() if line]
    weekly = [0] * WEEKS
    this_monday = _monday(today)
    for d in dates:
        weeks_ago = (this_monday - _monday(d)).days // 7
        if 0 <= weeks_ago < WEEKS:
            weekly[WEEKS - 1 - weeks_ago] += 1
    return {
        "total": len(dates),
        "first": min(dates).isoformat() if dates else None,
        "last": max(dates).isoformat() if dates else None,
        "last_7d": sum(1 for d in dates if (today - d).days < 7),
        "last_30d": sum(1 for d in dates if (today - d).days < 30),
        "last_90d": sum(1 for d in dates if (today - d).days < 90),
        "weekly": weekly,
    }


def _remotes(repo: str) -> list[dict[str, str]]:
    seen: dict[str, str] = {}
    for line in (_git(repo, "remote", "-v") or "").splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[0] not in seen:
            seen[parts[0]] = re.sub(r"//[^@/]+@", "//", parts[1])
    return [{"name": name, "url": url} for name, url in seen.items()]


def _upstream(repo: str) -> dict[str, Any]:
    upstream = _git(repo, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}")
    if not upstream:
        return {"upstream": None, "ahead": None, "behind": None}
    counts = (_git(repo, "rev-list", "--left-right", "--count", "@{upstream}...HEAD") or "").split()
    behind, ahead = (int(counts[0]), int(counts[1])) if len(counts) == 2 else (None, None)
    return {"upstream": upstream.strip(), "ahead": ahead, "behind": behind}


def _dirty(repo: str, has_head: bool) -> dict[str, Any]:
    raw = _git(repo, "status", "--porcelain=v1", "-z", "--untracked-files=all") or ""
    tokens = raw.split("\0")
    modified = untracked = deleted = 0
    paths: list[str] = []
    untracked_paths: list[str] = []
    i = 0
    while i < len(tokens):
        token = tokens[i]
        i += 1
        if len(token) < 4:
            continue
        xy, path = token[:2], token[3:]
        if "R" in xy or "C" in xy:
            i += 1  # the original path of a rename/copy follows
        if xy == "??":
            untracked += 1
            untracked_paths.append(path)
        elif "D" in xy:
            deleted += 1
        else:
            modified += 1
        paths.append(path)

    added = removed = 0
    if has_head:
        for line in (_git(repo, "diff", "HEAD", "--numstat") or "").splitlines():
            parts = line.split("\t")
            if len(parts) >= 3 and parts[0].isdigit() and parts[1].isdigit():
                added += int(parts[0])
                removed += int(parts[1])
    for path in untracked_paths[:_MAX_UNTRACKED_FILES]:
        try:
            full = Path(repo, path)
            if full.stat().st_size > _MAX_UNTRACKED_BYTES:
                continue
            data = full.read_bytes()
        except OSError:
            continue
        if b"\0" not in data:
            added += data.count(b"\n")

    mtimes = []
    for path in paths:
        try:
            mtimes.append(Path(repo, path).stat().st_mtime)
        except OSError:
            pass
    return {
        "files": len(paths), "modified": modified, "untracked": untracked, "deleted": deleted,
        "lines_added": added, "lines_removed": removed,
        "oldest_change": _iso(min(mtimes)) if mtimes else None,
        "newest_change": _iso(max(mtimes)) if mtimes else None,
        "_newest_ts": max(mtimes) if mtimes else None,
    }


def _worktrees(repo: str) -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    for line in (_git(repo, "worktree", "list", "--porcelain") or "").splitlines():
        if line.startswith("worktree "):
            current = {"path": line[9:], "branch": None, "prunable": False}
            items.append(current)
        elif current is not None and line.startswith("branch "):
            current["branch"] = line[7:].removeprefix("refs/heads/")
        elif current is not None and line.startswith("prunable"):
            current["prunable"] = True
    extra = items[1:]  # the first entry is the main checkout
    return {"count": len(extra), "prunable": sum(1 for w in extra if w["prunable"]), "items": extra}


def _branches(repo: str, current: str | None) -> dict[str, Any]:
    fmt = "--format=%(refname:short)\t%(committerdate:short)"
    merged = [ln.split("\t")[0] for ln in (_git(repo, "branch", "--merged", "HEAD", fmt) or "").splitlines() if ln]
    unmerged = []
    for line in (_git(repo, "branch", "--no-merged", "HEAD", fmt) or "").splitlines():
        name, _, last = line.partition("\t")
        if name:
            unmerged.append({"name": name, "last": last or None})
    return {"merged_count": sum(1 for b in merged if b != current), "unmerged": unmerged}


def _ci(remotes: list[dict[str, str]], branch: str | None) -> dict[str, Any]:
    slug = None
    for remote in sorted(remotes, key=lambda r: r["name"] != "origin"):
        match = _GITHUB.search(remote["url"])
        if match:
            slug = f"{match.group(1)}/{match.group(2)}"
            break
    if slug is None:
        return {"state": "none"}
    cmd = ["gh", "run", "list", "-R", slug, "-L", "1", "--json", "status,conclusion,createdAt,displayTitle,workflowName,url"]
    if branch and branch != "HEAD":
        cmd[5:5] = ["-b", branch]
    code, out = _run(cmd, os.getcwd(), _GH_TIMEOUT)
    if code != 0:
        return {"state": "unknown", "repo": slug, "error": out.strip()[:200]}
    try:
        runs = json.loads(out)
    except json.JSONDecodeError:
        return {"state": "unknown", "repo": slug}
    if not runs:
        return {"state": "none", "repo": slug}
    run = runs[0]
    if run.get("status") != "completed":
        state = "running"
    elif run.get("conclusion") == "success":
        state = "green"
    elif run.get("conclusion") in ("failure", "cancelled", "timed_out", "startup_failure"):
        state = "red"
    else:
        state = "unknown"
    return {
        "state": state, "repo": slug, "workflow": run.get("workflowName"), "title": run.get("displayTitle"),
        "at": run.get("createdAt"), "url": run.get("url"),
    }


def _docs_mtime(repo: str) -> float | None:
    stamps = []
    for rel in _DOC_FILES:
        try:
            stamps.append(Path(repo, rel).stat().st_mtime)
        except OSError:
            pass
    for plan in Path(repo, ".vorch", "plans").glob("*.md"):
        try:
            stamps.append(plan.stat().st_mtime)
        except OSError:
            pass
    return max(stamps) if stamps else None


def collect(path: str, today: date, with_ci: bool = True) -> dict[str, Any]:
    """Collect the facts of one repository. Never raises for repository problems."""
    if not Path(path).is_dir():
        return {"ok": False, "error": "path_missing", "is_git": False}
    top = _git(path, "rev-parse", "--show-toplevel")
    if top is None:
        return {"ok": False, "error": "not_a_git_repository", "is_git": False}
    head = (_git(path, "rev-parse", "HEAD") or "").strip() or None
    branch = (_git(path, "rev-parse", "--abbrev-ref", "HEAD") or "").strip() or None
    commits = _commits(path, today) if head else {"total": 0, "first": None, "last": None, "last_7d": 0, "last_30d": 0, "last_90d": 0, "weekly": [0] * WEEKS}
    remotes = _remotes(path)
    dirty = _dirty(path, head is not None)
    docs = _docs_mtime(path)
    newest_ts = dirty.pop("_newest_ts")
    last_work = max(filter(None, [commits["last"], _local_date(newest_ts) if newest_ts else None]), default=None)
    return {
        "ok": True, "is_git": True, "head": head, "branch": branch,
        "commits": commits, "remotes": remotes, "has_remote": bool(remotes), **_upstream(path),
        "dirty": dirty, "worktrees": _worktrees(path), "branches": _branches(path, branch),
        "ci": _ci(remotes, branch) if with_ci else {"state": "skipped"},
        "docs_mtime": _iso(docs) if docs else None,
        "last_work": last_work,
    }


def commits_since(path: str, rev: str, limit: int = 150) -> list[str] | None:
    """Commit subjects after `rev`, newest first, as "YYYY-MM-DD subject"; None when rev is unknown."""
    out = _git(path, "log", f"--max-count={limit}", "--format=%cs %s", f"{rev}..HEAD")
    return None if out is None else [line for line in out.splitlines() if line]


def count_since(path: str, rev: str) -> int | None:
    out = _git(path, "rev-list", "--count", f"{rev}..HEAD")
    return int(out.strip()) if out and out.strip().isdigit() else None


def is_git_repo(path: str) -> bool:
    return Path(path).is_dir() and _git(path, "rev-parse", "--show-toplevel") is not None


def last_commit_date(path: str) -> str | None:
    out = _git(path, "log", "-1", "--format=%cs")
    return out.strip() if out and out.strip() else None


def fingerprint(facts: dict[str, Any]) -> str:
    """What must change before a project needs a fresh interpretation."""
    dirty = facts.get("dirty") or {}
    key = {
        "head": facts.get("head"),
        "dirty": [dirty.get("files"), dirty.get("lines_added"), dirty.get("lines_removed")],
        "ahead": facts.get("ahead"),
        "docs": facts.get("docs_mtime"),
        "ok": facts.get("ok"),
    }
    return hashlib.sha1(json.dumps(key, sort_keys=True).encode()).hexdigest()[:16]
