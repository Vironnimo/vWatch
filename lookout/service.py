"""Lookout's single entry point for the CLI and the web app.

Owns project registration, fact refreshes, deciding which projects need a new
interpretation, the context an agent reads before writing a card, card and
portfolio writes, feedback, and the complete state the web page renders.
"""

from __future__ import annotations

import re
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable

from lookout import facts as factlib
from lookout.cards import Card, CardError, Portfolio, validate
from lookout.store import Store

STATUSES = ("active", "parked", "done", "archived")
FEEDBACK = ("up", "later", "down")
SNOOZE_DAYS = 14
DEFAULT_MAX_AGE_DAYS = 14
_ID = re.compile(r"^[a-z0-9][a-z0-9-]{0,39}$")


class LookoutError(Exception):
    def __init__(self, code: str, message: str, hint: str | None = None):
        super().__init__(message)
        self.code, self.message, self.hint = code, message, hint

    def as_dict(self) -> dict[str, Any]:
        err = {"code": self.code, "message": self.message}
        if self.hint:
            err["hint"] = self.hint
        return err


def _utcnow() -> datetime:
    return datetime.now(timezone.utc)


def _stamp(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _parse(stamp: str) -> datetime:
    return datetime.fromisoformat(stamp.replace("Z", "+00:00"))


def _same_path(a: str, b: str) -> bool:
    return a.casefold() == b.casefold() if sys.platform == "win32" else a == b


class Lookout:
    def __init__(self, store: Store, now: Callable[[], datetime] = _utcnow, collect: Callable[..., dict[str, Any]] = factlib.collect):
        self.store = store
        self._now = now
        self._collect = collect

    # ---------- time ----------
    def now(self) -> str:
        return _stamp(self._now())

    def today(self) -> date:
        return self._now().astimezone().date()

    # ---------- projects ----------
    def _project(self, project_id: str) -> dict[str, Any]:
        project = self.store.project(project_id)
        if project is None:
            known = ", ".join(p["id"] for p in self.store.projects()) or "none yet"
            raise LookoutError("project_not_found", f"No project with id '{project_id}'.", f"Known project ids: {known}.")
        return project

    def list_projects(self) -> list[dict[str, Any]]:
        out = []
        for p in self.store.projects():
            card = self.store.latest_card(p["id"])
            stored = self.store.facts(p["id"])
            out.append({**p, "has_card": card is not None, "card_updated_at": card["created_at"] if card else None,
                        "facts_collected_at": stored["collected_at"] if stored else None})
        return out

    def add_project(self, path: str, project_id: str | None = None, name: str | None = None) -> dict[str, Any]:
        resolved = str(Path(path).expanduser().resolve())
        if not Path(resolved).is_dir():
            raise LookoutError("path_missing", f"Folder does not exist: {resolved}")
        for p in self.store.projects():
            if _same_path(p["path"], resolved):
                raise LookoutError("already_registered", f"This folder is already registered as '{p['id']}'.")
        folder = Path(resolved).name
        project_id = project_id or re.sub(r"[^a-z0-9]+", "-", folder.lower()).strip("-")[:40] or "project"
        if not _ID.match(project_id):
            raise LookoutError("invalid_id", f"Invalid project id '{project_id}'.", "Use lowercase letters, digits and hyphens, at most 40 characters.")
        if self.store.project(project_id):
            raise LookoutError("id_taken", f"The id '{project_id}' is already used.", "Pass another id with --id.")
        self.store.add_project(project_id, name or folder, resolved, self.now())
        refreshed = self.refresh_facts([project_id])
        result = {**self._project(project_id), "facts": refreshed[0]}
        if not factlib.is_git_repo(resolved):
            result["warning"] = "The folder is not a git repository; only a card written by an agent can describe it."
        return result

    def remove_project(self, project_id: str) -> dict[str, Any]:
        self._project(project_id)
        self.store.remove_project(project_id)
        return {"removed": project_id}

    def rename_project(self, project_id: str, name: str) -> dict[str, Any]:
        self._project(project_id)
        self.store.update_project(project_id, name=name)
        return self._project(project_id)

    def set_status(self, project_id: str, status: str, note: str | None = None) -> dict[str, Any]:
        project = self._project(project_id)
        if status not in STATUSES:
            raise LookoutError("invalid_status", f"Unknown status '{status}'.", f"Use one of: {', '.join(STATUSES)}.")
        fields: dict[str, Any] = {"status": status, "status_changed_at": self.now()}
        fields["status_note"] = project["status_note"] if note is None else note
        self.store.update_project(project_id, **fields)
        return self._project(project_id)

    def discover(self, root: str) -> list[dict[str, Any]]:
        base = Path(root).expanduser().resolve()
        if not base.is_dir():
            raise LookoutError("path_missing", f"Folder does not exist: {base}")
        registered = [p["path"] for p in self.store.projects()]
        found = []
        for child in sorted(base.iterdir()):
            if not child.is_dir() or not (child / ".git").exists():
                continue
            if any(_same_path(str(child), r) for r in registered):
                continue
            found.append({"path": str(child), "suggested_id": re.sub(r"[^a-z0-9]+", "-", child.name.lower()).strip("-"),
                          "last_commit": factlib.last_commit_date(str(child))})
        found.sort(key=lambda f: f["last_commit"] or "", reverse=True)
        return found

    # ---------- facts ----------
    def refresh_facts(self, project_ids: list[str] | None = None, with_ci: bool = True) -> list[dict[str, Any]]:
        if project_ids:
            projects = [self._project(pid) for pid in project_ids]
        else:
            projects = [p for p in self.store.projects() if p["status"] != "archived"]
        today = self.today()

        def one(project: dict[str, Any]) -> dict[str, Any]:
            before = self.store.facts(project["id"])
            data = self._collect(project["path"], today, with_ci=with_ci)
            if not with_ci and before and "ci" in before["data"]:
                data["ci"] = before["data"]["ci"]
            fp = factlib.fingerprint(data)
            self.store.save_facts(project["id"], self.now(), fp, data)
            return {"id": project["id"], "ok": data.get("ok", False), "error": data.get("error"),
                    "changed": before is None or before["fingerprint"] != fp}

        with ThreadPoolExecutor(max_workers=4) as pool:
            return list(pool.map(one, projects))

    def facts(self, project_id: str) -> dict[str, Any]:
        self._project(project_id)
        stored = self.store.facts(project_id)
        if stored is None:
            self.refresh_facts([project_id])
            stored = self.store.facts(project_id)
        return {"project": project_id, "collected_at": stored["collected_at"], **stored["data"]}

    # ---------- the agent's morning run ----------
    def due(self, max_age_days: int = DEFAULT_MAX_AGE_DAYS, refresh: bool = True) -> dict[str, Any]:
        if refresh:
            self.refresh_facts()
        now = self._now()
        projects, problems, up_to_date = [], [], []
        newest_card = None
        for p in self.store.projects():
            if p["status"] == "archived":
                continue
            stored = self.store.facts(p["id"])
            card = self.store.latest_card(p["id"])
            if card and (newest_card is None or card["created_at"] > newest_card):
                newest_card = card["created_at"]
            if stored and not stored["data"].get("ok", False) and stored["data"].get("error") == "path_missing":
                problems.append({"id": p["id"], "problem": "path_missing", "path": p["path"]})
                continue
            reason = None
            if card is None:
                reason = "no_card"
            elif stored and card["fingerprint"] != stored["fingerprint"]:
                reason = "changed"
            elif p["status_changed_at"] and p["status_changed_at"] > card["created_at"]:
                reason = "status_changed"
            elif (now - _parse(card["created_at"])).days >= max_age_days:
                reason = "stale"
            if reason:
                projects.append({"id": p["id"], "name": p["name"], "reason": reason})
            else:
                up_to_date.append(p["id"])
        portfolio = self.store.latest_portfolio()
        if portfolio is None:
            p_reason = "missing"
        elif _parse(portfolio["created_at"]).astimezone().date() != self.today():
            p_reason = "new_day"
        elif newest_card and newest_card > portfolio["created_at"]:
            p_reason = "cards_changed"
        else:
            p_reason = None
        return {
            "today": self.today().isoformat(),
            "projects": projects,
            "up_to_date": up_to_date,
            "problems": problems,
            "portfolio": {"due": p_reason is not None or bool(projects), "reason": p_reason or ("cards_will_change" if projects else None)},
            "next": ("For each project listed: `lookout context <id>`, study the repository, then `lookout card put <id> --file <card.json>`. "
                     "Afterwards: `lookout portfolio context` and `lookout portfolio put --file <portfolio.json>`.")
            if projects or p_reason else "Nothing is due. Every card and the portfolio brief are current.",
        }

    def context(self, project_id: str) -> dict[str, Any]:
        project = self._project(project_id)
        stored = self.store.facts(project_id)
        if stored is None:
            self.refresh_facts([project_id])
            stored = self.store.facts(project_id)
        card = self.store.latest_card(project_id)
        since = None
        if card:
            since = {"days": (self._now() - _parse(card["created_at"])).days, "commit_count": None, "commits": None}
            if card["head"]:
                since["commit_count"] = factlib.count_since(project["path"], card["head"])
                since["commits"] = factlib.commits_since(project["path"], card["head"])
        feedback = [{"id": f["item_id"], "title": f["title"], "value": f["value"], "at": f["at"]}
                    for f in self.store.feedback() if f["project_id"] == project_id]
        return {
            "today": self.today().isoformat(),
            "project": {k: project[k] for k in ("id", "name", "path", "status", "status_note")},
            "facts": {"collected_at": stored["collected_at"], **stored["data"]},
            "previous_card": {"version": card["version"], "created_at": card["created_at"], "card": card["data"]} if card else None,
            "since_previous_card": since,
            "feedback": feedback,
            "next": (f"Write the card with `lookout card put {project_id} --file <card.json>`"
                     + (" (add --merge to send only the fields that changed)." if card else ".")),
        }

    def put_card(self, project_id: str, payload: Any, merge: bool = False) -> dict[str, Any]:
        self._project(project_id)
        previous = self.store.latest_card(project_id)
        if merge and previous is None:
            raise LookoutError("nothing_to_merge", "There is no previous card to merge into.", "Send a complete card without --merge.")
        if merge and isinstance(payload, dict):
            payload = {**previous["data"], **payload}
        try:
            clean, warnings = validate(Card, payload)
        except CardError as exc:
            raise LookoutError("invalid_card", "The card was not saved: " + "; ".join(exc.problems),
                               "Fix the listed fields and send the card again. `lookout schema card` prints the full schema.") from None
        stored = self.store.facts(project_id)
        if stored is None:
            self.refresh_facts([project_id])
            stored = self.store.facts(project_id)
        version = self.store.add_card(project_id, self.now(), stored["fingerprint"], stored["data"].get("head"), clean)
        return {"saved": True, "project": project_id, "version": version, "warnings": warnings,
                "rated_items": [{"id": s["id"], "title": s["title"]} for s in clean["suggestions"]]}

    def card(self, project_id: str) -> dict[str, Any]:
        self._project(project_id)
        card = self.store.latest_card(project_id)
        if card is None:
            raise LookoutError("no_card", f"Project '{project_id}' has no card yet.", f"Run `lookout context {project_id}` and write one.")
        return {"project": project_id, "version": card["version"], "created_at": card["created_at"], "card": card["data"]}

    def portfolio_context(self) -> dict[str, Any]:
        rows = []
        for p in self.store.projects():
            if p["status"] == "archived":
                continue
            stored = self.store.facts(p["id"])
            f = stored["data"] if stored else {}
            card = self.store.latest_card(p["id"])
            c = card["data"] if card else None
            rows.append({
                "id": p["id"], "name": p["name"], "status": p["status"], "status_note": p["status_note"],
                "facts": {
                    "last_work": f.get("last_work"), "commits": {k: (f.get("commits") or {}).get(k) for k in ("total", "last_7d", "last_30d", "last")},
                    "has_remote": f.get("has_remote"), "ahead": f.get("ahead"),
                    "dirty": {k: (f.get("dirty") or {}).get(k) for k in ("files", "lines_added", "lines_removed", "oldest_change")},
                    "ci": (f.get("ci") or {}).get("state"), "unmerged_branches": len((f.get("branches") or {}).get("unmerged", [])),
                    "extra_worktrees": (f.get("worktrees") or {}).get("count"),
                },
                "card": None if c is None else {
                    "updated_at": card["created_at"], "one_liner": c["one_liner"], "phase": c["phase"], "headline": c["headline"],
                    "reentry": c["reentry"], "waiting_on_you": [q["title"] for q in c["waiting_on_you"]],
                    "open_ends": [o["title"] for o in c["open_ends"]], "suggestions": [{"id": s["id"], "type": s["type"], "title": s["title"]} for s in c["suggestions"]],
                    "cross_project_hints": c["cross_project_hints"],
                },
            })
        previous = self.store.latest_portfolio()
        return {
            "today": self.today().isoformat(),
            "projects": rows,
            "previous_portfolio": {"version": previous["version"], "created_at": previous["created_at"], "portfolio": previous["data"]} if previous else None,
            "feedback": [{"id": f["item_id"], "project": f["project_id"], "title": f["title"], "value": f["value"], "at": f["at"]} for f in self.store.feedback()],
            "next": "Write the brief with `lookout portfolio put --file <portfolio.json>`.",
        }

    def put_portfolio(self, payload: Any, merge: bool = False) -> dict[str, Any]:
        previous = self.store.latest_portfolio()
        if merge and previous is None:
            raise LookoutError("nothing_to_merge", "There is no previous portfolio brief to merge into.", "Send a complete brief without --merge.")
        if merge and isinstance(payload, dict):
            payload = {**previous["data"], **payload}
        try:
            clean, warnings = validate(Portfolio, payload)
        except CardError as exc:
            raise LookoutError("invalid_portfolio", "The brief was not saved: " + "; ".join(exc.problems),
                               "Fix the listed fields and send it again. `lookout schema portfolio` prints the full schema.") from None
        known = {p["id"] for p in self.store.projects()}
        refs = [f["project"] for f in clean["focus"]] + [v["project"] for v in clean["park_or_revive"]]
        refs += [pid for item in clean["patterns"] + clean["synergies"] for pid in item["projects"]]
        unknown = sorted(set(refs) - known)
        if unknown:
            raise LookoutError("unknown_project", f"The brief names unknown project ids: {', '.join(unknown)}.",
                               f"Use the ids from `lookout portfolio context`: {', '.join(sorted(known))}.")
        version = self.store.add_portfolio(self.now(), clean)
        rated = [{"id": i["id"], "title": i.get("title") or i.get("project")} for k in ("patterns", "synergies", "new_apps", "park_or_revive") for i in clean[k]]
        return {"saved": True, "version": version, "warnings": warnings, "rated_items": rated}

    def portfolio(self) -> dict[str, Any]:
        previous = self.store.latest_portfolio()
        if previous is None:
            raise LookoutError("no_portfolio", "No portfolio brief has been written yet.", "Run `lookout portfolio context` and write one.")
        return {"version": previous["version"], "created_at": previous["created_at"], "portfolio": previous["data"]}

    # ---------- feedback and preferences ----------
    def set_feedback(self, item_id: str, value: str | None, project_id: str | None = None, title: str = "") -> dict[str, Any]:
        if value is not None and value not in FEEDBACK:
            raise LookoutError("invalid_feedback", f"Unknown rating '{value}'.", f"Use one of: {', '.join(FEEDBACK)}.")
        self.store.set_feedback(item_id, project_id, title, value, self.now())
        return {"item_id": item_id, "value": value}

    def feedback(self, project_id: str | None = None) -> list[dict[str, Any]]:
        rows = self.store.feedback()
        if project_id:
            rows = [r for r in rows if r["project_id"] == project_id]
        return [{"id": r["item_id"], "project": r["project_id"], "title": r["title"], "value": r["value"], "at": r["at"]} for r in rows]

    def set_pref(self, key: str, value: Any) -> None:
        if key not in ("style", "hidden_modules"):
            raise LookoutError("invalid_pref", f"Unknown preference '{key}'.")
        self.store.set_pref(key, value)

    # ---------- web page ----------
    def state(self, visit: bool = False) -> dict[str, Any]:
        now = self._now()
        last = self.store.pref("last_visit")
        previous = self.store.pref("previous_visit", last)
        if visit:
            if last is None or (now - _parse(last)) > timedelta(hours=1):
                previous = last
                self.store.set_pref("previous_visit", last)
            self.store.set_pref("last_visit", _stamp(now))
        projects = []
        for p in self.store.projects():
            stored = self.store.facts(p["id"])
            card = self.store.latest_card(p["id"])
            projects.append({
                **{k: p[k] for k in ("id", "name", "path", "status", "status_note")},
                "facts": stored["data"] if stored else None,
                "facts_collected_at": stored["collected_at"] if stored else None,
                "card": card["data"] if card else None,
                "card_updated_at": card["created_at"] if card else None,
                "card_outdated": bool(card and stored and card["fingerprint"] != stored["fingerprint"]),
            })
        portfolio = self.store.latest_portfolio()
        return {
            "today": self.today().isoformat(),
            "now": _stamp(now),
            "previous_visit": previous,
            "projects": projects,
            "portfolio": portfolio["data"] if portfolio else None,
            "portfolio_updated_at": portfolio["created_at"] if portfolio else None,
            "feedback": {f["item_id"]: {"value": f["value"], "at": f["at"]} for f in self.store.feedback()},
            "snooze_days": SNOOZE_DAYS,
            "prefs": {"style": self.store.pref("style", "handover"), "hidden_modules": self.store.pref("hidden_modules", [])},
        }
