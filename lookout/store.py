"""SQLite persistence. One file under the data directory, shared by CLI and web app."""

from __future__ import annotations

import json
import sqlite3
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Iterator

_SCHEMA = """
CREATE TABLE IF NOT EXISTS projects(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active', status_note TEXT NOT NULL DEFAULT '',
  status_changed_at TEXT, added_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS facts(
  project_id TEXT PRIMARY KEY, collected_at TEXT NOT NULL, fingerprint TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS cards(
  id INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL, created_at TEXT NOT NULL,
  fingerprint TEXT NOT NULL, head TEXT, data TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS cards_by_project ON cards(project_id, id);
CREATE TABLE IF NOT EXISTS portfolio(
  id INTEGER PRIMARY KEY AUTOINCREMENT, created_at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS feedback(
  item_id TEXT PRIMARY KEY, project_id TEXT, title TEXT NOT NULL, value TEXT NOT NULL, at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS prefs(key TEXT PRIMARY KEY, value TEXT NOT NULL);
"""


class Store:
    def __init__(self, path: Path):
        self.path = path
        with self._db() as db:
            db.executescript(_SCHEMA)

    @contextmanager
    def _db(self) -> Iterator[sqlite3.Connection]:
        db = sqlite3.connect(self.path, timeout=15)
        db.row_factory = sqlite3.Row
        try:
            db.execute("PRAGMA journal_mode=WAL")
            yield db
            db.commit()
        finally:
            db.close()

    # projects
    def projects(self) -> list[dict[str, Any]]:
        with self._db() as db:
            return [dict(r) for r in db.execute("SELECT * FROM projects ORDER BY added_at, id")]

    def project(self, project_id: str) -> dict[str, Any] | None:
        with self._db() as db:
            row = db.execute("SELECT * FROM projects WHERE id = ?", (project_id,)).fetchone()
            return dict(row) if row else None

    def add_project(self, project_id: str, name: str, path: str, at: str) -> None:
        with self._db() as db:
            db.execute("INSERT INTO projects(id, name, path, added_at) VALUES(?, ?, ?, ?)", (project_id, name, path, at))

    def update_project(self, project_id: str, **fields: Any) -> None:
        cols = ", ".join(f"{k} = ?" for k in fields)
        with self._db() as db:
            db.execute(f"UPDATE projects SET {cols} WHERE id = ?", (*fields.values(), project_id))

    def remove_project(self, project_id: str) -> None:
        with self._db() as db:
            for table, col in (("projects", "id"), ("facts", "project_id"), ("cards", "project_id")):
                db.execute(f"DELETE FROM {table} WHERE {col} = ?", (project_id,))

    # facts
    def save_facts(self, project_id: str, at: str, fingerprint: str, data: dict[str, Any]) -> None:
        with self._db() as db:
            db.execute(
                "INSERT INTO facts(project_id, collected_at, fingerprint, data) VALUES(?, ?, ?, ?) "
                "ON CONFLICT(project_id) DO UPDATE SET collected_at = excluded.collected_at, "
                "fingerprint = excluded.fingerprint, data = excluded.data",
                (project_id, at, fingerprint, json.dumps(data)),
            )

    def facts(self, project_id: str) -> dict[str, Any] | None:
        with self._db() as db:
            row = db.execute("SELECT * FROM facts WHERE project_id = ?", (project_id,)).fetchone()
        return {"collected_at": row["collected_at"], "fingerprint": row["fingerprint"], "data": json.loads(row["data"])} if row else None

    # cards
    def add_card(self, project_id: str, at: str, fingerprint: str, head: str | None, data: dict[str, Any]) -> int:
        with self._db() as db:
            cur = db.execute(
                "INSERT INTO cards(project_id, created_at, fingerprint, head, data) VALUES(?, ?, ?, ?, ?)",
                (project_id, at, fingerprint, head, json.dumps(data, ensure_ascii=False)),
            )
            return int(cur.lastrowid)

    def latest_card(self, project_id: str) -> dict[str, Any] | None:
        with self._db() as db:
            row = db.execute("SELECT * FROM cards WHERE project_id = ? ORDER BY id DESC LIMIT 1", (project_id,)).fetchone()
        if not row:
            return None
        return {"version": row["id"], "created_at": row["created_at"], "fingerprint": row["fingerprint"],
                "head": row["head"], "data": json.loads(row["data"])}

    # portfolio
    def add_portfolio(self, at: str, data: dict[str, Any]) -> int:
        with self._db() as db:
            cur = db.execute("INSERT INTO portfolio(created_at, data) VALUES(?, ?)", (at, json.dumps(data, ensure_ascii=False)))
            return int(cur.lastrowid)

    def latest_portfolio(self) -> dict[str, Any] | None:
        with self._db() as db:
            row = db.execute("SELECT * FROM portfolio ORDER BY id DESC LIMIT 1").fetchone()
        return {"version": row["id"], "created_at": row["created_at"], "data": json.loads(row["data"])} if row else None

    # feedback
    def set_feedback(self, item_id: str, project_id: str | None, title: str, value: str | None, at: str) -> None:
        with self._db() as db:
            if value is None:
                db.execute("DELETE FROM feedback WHERE item_id = ?", (item_id,))
            else:
                db.execute(
                    "INSERT INTO feedback(item_id, project_id, title, value, at) VALUES(?, ?, ?, ?, ?) "
                    "ON CONFLICT(item_id) DO UPDATE SET value = excluded.value, at = excluded.at, title = excluded.title",
                    (item_id, project_id, title, value, at),
                )

    def feedback(self) -> list[dict[str, Any]]:
        with self._db() as db:
            return [dict(r) for r in db.execute("SELECT * FROM feedback ORDER BY at DESC")]

    # prefs
    def pref(self, key: str, default: Any = None) -> Any:
        with self._db() as db:
            row = db.execute("SELECT value FROM prefs WHERE key = ?", (key,)).fetchone()
        return json.loads(row["value"]) if row else default

    def set_pref(self, key: str, value: Any) -> None:
        with self._db() as db:
            db.execute(
                "INSERT INTO prefs(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (key, json.dumps(value)),
            )
