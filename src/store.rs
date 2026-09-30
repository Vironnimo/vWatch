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
  status_changed_at TEXT, added_at TEXT NOT NULL, opened_at TEXT, previous_opened_at TEXT);
CREATE TABLE IF NOT EXISTS facts(
  project_id TEXT PRIMARY KEY, collected_at TEXT NOT NULL, fingerprint TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS cards(
  id INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL, created_at TEXT NOT NULL,
  fingerprint TEXT NOT NULL, head TEXT, data TEXT NOT NULL, ci TEXT);
CREATE INDEX IF NOT EXISTS cards_by_project ON cards(project_id, id);
CREATE TABLE IF NOT EXISTS portfolio(
  id INTEGER PRIMARY KEY AUTOINCREMENT, created_at TEXT NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS feedback(
  item_id TEXT PRIMARY KEY, project_id TEXT, title TEXT NOT NULL, value TEXT NOT NULL, at TEXT NOT NULL,
  note TEXT NOT NULL DEFAULT '', item TEXT);
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
    /// When the owner last opened the project in the window.
    pub opened_at: Option<String>,
    /// The open before that, kept while the owner reopens within the hour.
    pub previous_opened_at: Option<String>,
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
    /// The facts basis the card was written against; missing for older cards.
    pub basis: Option<Value>,
    pub data: Value,
}

/// What a new card was written against.
pub struct CardBasis<'a> {
    pub fingerprint: &'a str,
    pub head: Option<&'a str>,
    pub ci: Option<&'a str>,
    pub parts: &'a Value,
}

pub struct StoredPortfolio {
    pub version: i64,
    pub created_at: String,
    pub data: Value,
}

/// The owner's reaction to an item an agent wrote: a rating or a reply to a question.
pub struct Feedback {
    pub item_id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub value: String,
    pub at: String,
    pub note: String,
    /// The item as it read when the owner reacted, with its `kind`; missing for older reactions.
    pub item: Option<Value>,
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
        opened_at: row.get("opened_at")?,
        previous_opened_at: row.get("previous_opened_at")?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let db = open(path)?;
        db.execute_batch(SCHEMA)?;
        // Columns added after the first release.
        for (table, column, decl) in [
            ("cards", "ci", "TEXT"),
            ("cards", "basis", "TEXT"),
            ("feedback", "note", "TEXT NOT NULL DEFAULT ''"),
            ("feedback", "item", "TEXT"),
            ("projects", "opened_at", "TEXT"),
            ("projects", "previous_opened_at", "TEXT"),
        ] {
            let present: bool =
                db.query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"), [], |r| r.get(0))?;
            if !present {
                db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
            }
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

    pub fn set_opened(&self, id: &str, at: &str, previous: Option<&str>) -> Result<()> {
        self.with(|db| db.execute("UPDATE projects SET opened_at = ?2, previous_opened_at = ?3 WHERE id = ?1", params![id, at, previous]).map(drop))
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
    pub fn add_card(&self, id: &str, at: &str, basis: &CardBasis, data: &Value) -> Result<i64> {
        self.with(|db| {
            db.execute(
                "INSERT INTO cards(project_id, created_at, fingerprint, head, ci, basis, data) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, at, basis.fingerprint, basis.head, basis.ci, basis.parts.to_string(), data.to_string()],
            )?;
            db.query_row("SELECT COUNT(*) FROM cards WHERE project_id = ?1", [id], |r| r.get(0))
        })
    }

    pub fn latest_card(&self, id: &str) -> Result<Option<StoredCard>> {
        self.with(|db| {
            db.query_row("SELECT (SELECT COUNT(*) FROM cards WHERE project_id = ?1), created_at, fingerprint, head, ci, basis, data FROM cards WHERE project_id = ?1 ORDER BY id DESC LIMIT 1", [id], |r| {
                Ok(StoredCard {
                    version: r.get(0)?,
                    created_at: r.get(1)?,
                    fingerprint: r.get(2)?,
                    head: r.get(3)?,
                    ci: r.get(4)?,
                    basis: r.get::<_, Option<String>>(5)?.map(json),
                    data: json(r.get(6)?),
                })
            })
            .optional()
        })
    }

    /// The card the owner could see at `at`: the newest one written by then.
    pub fn card_at(&self, id: &str, at: &str) -> Result<Option<Value>> {
        self.with(|db| {
            db.query_row("SELECT data FROM cards WHERE project_id = ?1 AND created_at <= ?2 ORDER BY id DESC LIMIT 1", params![id, at], |r| r.get(0))
                .optional()
                .map(|d| d.map(json))
        })
    }

    pub fn cards_after(&self, id: &str, at: &str) -> Result<i64> {
        self.with(|db| db.query_row("SELECT COUNT(*) FROM cards WHERE project_id = ?1 AND created_at > ?2", params![id, at], |r| r.get(0)))
    }

    /// Replaces the stored content of a project's latest card, keeping its version and date.
    pub fn rewrite_latest_card(&self, id: &str, data: &Value) -> Result<()> {
        self.with(|db| {
            db.execute(
                "UPDATE cards SET data = ?2 WHERE id = (SELECT MAX(id) FROM cards WHERE project_id = ?1)",
                params![id, data.to_string()],
            )
            .map(drop)
        })
    }

    /// Replaces the stored content of the latest brief, keeping its version and date.
    pub fn rewrite_latest_portfolio(&self, data: &Value) -> Result<()> {
        self.with(|db| db.execute("UPDATE portfolio SET data = ?1 WHERE id = (SELECT MAX(id) FROM portfolio)", [data.to_string()]).map(drop))
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
    pub fn set_feedback(&self, f: &Feedback) -> Result<()> {
        self.with(|db| {
            db.execute(
                "INSERT INTO feedback(item_id, project_id, title, value, at, note, item) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(item_id) DO UPDATE SET project_id = excluded.project_id, title = excluded.title, value = excluded.value,
                 at = excluded.at, note = excluded.note, item = excluded.item",
                params![f.item_id, f.project_id, f.title, f.value, f.at, f.note, f.item.as_ref().map(Value::to_string)],
            )
            .map(drop)
        })
    }

    pub fn delete_feedback(&self, item_id: &str) -> Result<()> {
        self.with(|db| db.execute("DELETE FROM feedback WHERE item_id = ?1", [item_id]).map(drop))
    }

    /// All reactions, newest first.
    pub fn feedback(&self) -> Result<Vec<Feedback>> {
        self.with(|db| {
            db.prepare("SELECT item_id, project_id, title, value, at, note, item FROM feedback ORDER BY at DESC")?
                .query_map([], |r| {
                    Ok(Feedback {
                        item_id: r.get(0)?,
                        project_id: r.get(1)?,
                        title: r.get(2)?,
                        value: r.get(3)?,
                        at: r.get(4)?,
                        note: r.get(5)?,
                        item: r.get::<_, Option<String>>(6)?.map(json),
                    })
                })?
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
