//! SQLite persistence: one database file shared by the app window and the CLI.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS projects(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active', status_note TEXT NOT NULL DEFAULT '',
  status_changed_at TEXT, added_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS facts(
  project_id TEXT PRIMARY KEY, collected_at TEXT NOT NULL, fingerprint TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS cards(
  id INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL, created_at TEXT NOT NULL,
  fingerprint TEXT NOT NULL, head TEXT, data TEXT NOT NULL, ci TEXT);
CREATE INDEX IF NOT EXISTS cards_by_project ON cards(project_id, id);
CREATE TABLE IF NOT EXISTS portfolio(
  id INTEGER PRIMARY KEY AUTOINCREMENT, created_at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS feedback(
  item_id TEXT PRIMARY KEY, project_id TEXT, title TEXT NOT NULL, value TEXT NOT NULL, at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS prefs(key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

pub type Result<T> = rusqlite::Result<T>;

#[derive(Clone, Debug, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub status: String,
    pub status_note: String,
    pub status_changed_at: Option<String>,
    pub added_at: String,
}

pub struct StoredFacts {
    pub collected_at: String,
    pub fingerprint: String,
    pub data: Value,
}

pub struct StoredCard {
    /// Counts this project's cards: its first card is version 1.
    pub version: i64,
    pub created_at: String,
    pub fingerprint: String,
    pub head: Option<String>,
    /// The CI state (`green` or `red`) the card was written against, if it was known.
    pub ci: Option<String>,
    pub data: Value,
}

pub struct StoredPortfolio {
    pub version: i64,
    pub created_at: String,
    pub data: Value,
}

pub struct Feedback {
    pub item_id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub value: String,
    pub at: String,
}

pub struct Store {
    db: Mutex<Connection>,
}

fn open(path: &Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(15))?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    Ok(db)
}

fn json(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

fn project(row: &Row) -> Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        path: row.get("path")?,
        status: row.get("status")?,
        status_note: row.get("status_note")?,
        status_changed_at: row.get("status_changed_at")?,
        added_at: row.get("added_at")?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let db = open(path)?;
        db.execute_batch(SCHEMA)?;
        // Databases from before cards remembered their CI state.
        let has_ci: bool = db.query_row("SELECT COUNT(*) FROM pragma_table_info('cards') WHERE name = 'ci'", [], |r| r.get(0))?;
        if !has_ci {
            db.execute_batch("ALTER TABLE cards ADD COLUMN ci TEXT")?;
        }
        Ok(Store { db: Mutex::new(db) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        f(&db)
    }

    // projects
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.with(|db| db.prepare("SELECT * FROM projects ORDER BY added_at, id")?.query_map([], project)?.collect())
    }

    pub fn project(&self, id: &str) -> Result<Option<Project>> {
        self.with(|db| db.query_row("SELECT * FROM projects WHERE id = ?1", [id], project).optional())
    }

    pub fn add_project(&self, id: &str, name: &str, path: &str, at: &str) -> Result<()> {
        self.with(|db| db.execute("INSERT INTO projects(id, name, path, added_at) VALUES(?1, ?2, ?3, ?4)", params![id, name, path, at]).map(drop))
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<()> {
        self.with(|db| db.execute("UPDATE projects SET name = ?1 WHERE id = ?2", params![name, id]).map(drop))
    }

    pub fn set_status(&self, id: &str, status: &str, note: &str, at: &str) -> Result<()> {
        self.with(|db| {
            db.execute("UPDATE projects SET status = ?1, status_note = ?2, status_changed_at = ?3 WHERE id = ?4", params![status, note, at, id]).map(drop)
        })
    }

    pub fn remove_project(&self, id: &str) -> Result<()> {
        self.with(|db| {
            db.execute_batch("BEGIN")?;
            for sql in ["DELETE FROM projects WHERE id = ?1", "DELETE FROM facts WHERE project_id = ?1", "DELETE FROM cards WHERE project_id = ?1"] {
                db.execute(sql, [id])?;
            }
            db.execute_batch("COMMIT")
        })
    }

    // facts
    pub fn save_facts(&self, id: &str, at: &str, fingerprint: &str, data: &Value) -> Result<()> {
        self.with(|db| {
            db.execute(
                "INSERT INTO facts(project_id, collected_at, fingerprint, data) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(project_id) DO UPDATE SET collected_at = excluded.collected_at,
                 fingerprint = excluded.fingerprint, data = excluded.data",
                params![id, at, fingerprint, data.to_string()],
            )
            .map(drop)
        })
    }

    pub fn facts(&self, id: &str) -> Result<Option<StoredFacts>> {
        self.with(|db| {
            db.query_row("SELECT collected_at, fingerprint, data FROM facts WHERE project_id = ?1", [id], |r| {
                Ok(StoredFacts { collected_at: r.get(0)?, fingerprint: r.get(1)?, data: json(r.get(2)?) })
            })
            .optional()
        })
    }

    // cards
    pub fn add_card(&self, id: &str, at: &str, fingerprint: &str, head: Option<&str>, ci: Option<&str>, data: &Value) -> Result<i64> {
        self.with(|db| {
            db.execute(
                "INSERT INTO cards(project_id, created_at, fingerprint, head, ci, data) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, at, fingerprint, head, ci, data.to_string()],
            )?;
            db.query_row("SELECT COUNT(*) FROM cards WHERE project_id = ?1", [id], |r| r.get(0))
        })
    }

    pub fn latest_card(&self, id: &str) -> Result<Option<StoredCard>> {
        self.with(|db| {
            db.query_row("SELECT (SELECT COUNT(*) FROM cards WHERE project_id = ?1), created_at, fingerprint, head, ci, data FROM cards WHERE project_id = ?1 ORDER BY id DESC LIMIT 1", [id], |r| {
                Ok(StoredCard { version: r.get(0)?, created_at: r.get(1)?, fingerprint: r.get(2)?, head: r.get(3)?, ci: r.get(4)?, data: json(r.get(5)?) })
            })
            .optional()
        })
    }

    // portfolio
    pub fn add_portfolio(&self, at: &str, data: &Value) -> Result<i64> {
        self.with(|db| {
            db.execute("INSERT INTO portfolio(created_at, data) VALUES(?1, ?2)", params![at, data.to_string()])?;
            Ok(db.last_insert_rowid())
        })
    }

    pub fn latest_portfolio(&self) -> Result<Option<StoredPortfolio>> {
        self.with(|db| {
            db.query_row("SELECT id, created_at, data FROM portfolio ORDER BY id DESC LIMIT 1", [], |r| {
                Ok(StoredPortfolio { version: r.get(0)?, created_at: r.get(1)?, data: json(r.get(2)?) })
            })
            .optional()
        })
    }

    // feedback
    pub fn set_feedback(&self, item_id: &str, project_id: Option<&str>, title: &str, value: Option<&str>, at: &str) -> Result<()> {
        self.with(|db| match value {
            None => db.execute("DELETE FROM feedback WHERE item_id = ?1", [item_id]).map(drop),
            Some(value) => db
                .execute(
                    "INSERT INTO feedback(item_id, project_id, title, value, at) VALUES(?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(item_id) DO UPDATE SET value = excluded.value, at = excluded.at, title = excluded.title",
                    params![item_id, project_id, title, value, at],
                )
                .map(drop),
        })
    }

    pub fn feedback(&self) -> Result<Vec<Feedback>> {
        self.with(|db| {
            db.prepare("SELECT item_id, project_id, title, value, at FROM feedback ORDER BY at DESC")?
                .query_map([], |r| Ok(Feedback { item_id: r.get(0)?, project_id: r.get(1)?, title: r.get(2)?, value: r.get(3)?, at: r.get(4)? }))?
                .collect()
        })
    }

    // prefs
    pub fn pref(&self, key: &str) -> Result<Option<Value>> {
        self.with(|db| db.query_row("SELECT value FROM prefs WHERE key = ?1", [key], |r| r.get::<_, String>(0)).optional())
            .map(|v| v.map(json))
    }

    pub fn set_pref(&self, key: &str, value: &Value) -> Result<()> {
        self.with(|db| {
            db.execute("INSERT INTO prefs(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value.to_string()])
                .map(drop)
        })
    }
}

/// Notices writes from any connection, including other processes such as the CLI.
pub struct ChangeWatch {
    db: Mutex<Connection>,
}

impl ChangeWatch {
    pub fn open(path: &Path) -> Result<ChangeWatch> {
        Ok(ChangeWatch { db: Mutex::new(open(path)?) })
    }

    /// A number that changes whenever another connection commits a write.
    pub fn version(&self) -> Result<i64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("PRAGMA data_version", [], |r| r.get(0))
    }
}
