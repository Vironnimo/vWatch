//! SQLite persistence: one database file shared by the app window and the CLI.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::{Map, Value};

mod history;
mod undo;
use undo::OwnerScope;
pub use undo::OwnerUndo;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS projects(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active', status_note TEXT NOT NULL DEFAULT '',
  status_changed_at TEXT, added_at TEXT NOT NULL, opened_at TEXT, previous_opened_at TEXT,
  goal TEXT NOT NULL DEFAULT '', goal_due TEXT);
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
CREATE TABLE IF NOT EXISTS warning_acks(
  project_id TEXT NOT NULL, kind TEXT NOT NULL, note TEXT NOT NULL, basis TEXT NOT NULL,
  at TEXT NOT NULL, PRIMARY KEY(project_id, kind));
CREATE TABLE IF NOT EXISTS ideas(
  id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL, added_at TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS agent_runs(
  id INTEGER PRIMARY KEY AUTOINCREMENT, started_at TEXT NOT NULL, finished_at TEXT,
  initial TEXT NOT NULL, failures TEXT NOT NULL DEFAULT '{}', result TEXT, outcome TEXT);
CREATE TABLE IF NOT EXISTS tasks(
  id INTEGER PRIMARY KEY AUTOINCREMENT, status TEXT NOT NULL, source_key TEXT,
  revision INTEGER NOT NULL DEFAULT 1, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS document_requests(
  scope TEXT NOT NULL, expected TEXT NOT NULL, request TEXT NOT NULL, after_token TEXT NOT NULL,
  version INTEGER NOT NULL, PRIMARY KEY(scope, expected));
CREATE TABLE IF NOT EXISTS owner_undo(
  token TEXT PRIMARY KEY, scope TEXT NOT NULL, before_data TEXT NOT NULL, after_data TEXT NOT NULL,
  versions TEXT NOT NULL, created_at TEXT NOT NULL, undone_at TEXT, undone_versions TEXT);
CREATE TABLE IF NOT EXISTS owner_field_versions(field TEXT PRIMARY KEY, version INTEGER NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS tasks_active_source ON tasks(source_key)
  WHERE source_key IS NOT NULL AND status IN ('assigned', 'blocked', 'review');
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
    /// What the owner wants to reach with the project, and by when.
    pub goal: String,
    pub goal_due: Option<String>,
    /// Changes only when the registered name or folder changes.
    #[serde(skip_serializing)]
    pub identity_revision: i64,
    pub removed_at: Option<String>,
}

pub enum ProjectRegistration {
    Added,
    Existing,
    IdTaken,
    PathTaken(String),
}

pub enum ProjectUpdate {
    Updated(Value),
    Conflict,
    PathTaken(String),
}

pub(crate) fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        let key = |path: &str| path.replace('\\', "/").trim_end_matches('/').to_lowercase();
        key(a) == key(b)
    } else {
        a == b
    }
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
    /// The owner's instructions and replies read by this card; absent on older cards.
    pub owner_basis: Option<Value>,
    pub data: Value,
}

/// What a new card was written against.
pub struct CardBasis<'a> {
    pub fingerprint: &'a str,
    pub head: Option<&'a str>,
    pub ci: Option<&'a str>,
    pub parts: &'a Value,
    pub owner: Option<&'a Value>,
}

/// An idea the owner parked for agents to assess.
pub struct Idea {
    pub id: i64,
    pub text: String,
    pub added_at: String,
    pub revision: i64,
}

pub struct StoredPortfolio {
    pub version: i64,
    pub created_at: String,
    pub data: Value,
    /// Project state and brief-relevant card content read by this brief.
    pub basis: Option<Value>,
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
    pub snoozed_until: Option<String>,
}

pub struct Store {
    db: Mutex<Connection>,
}

pub enum GuardedWrite {
    Saved(i64, String),
    Replayed(i64, String),
    Conflict,
    ItemConflict(Vec<String>),
}

fn open(path: &Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(15))?;
    // Concurrent first opens can both hold a read lock while switching to WAL.
    // SQLite refuses that lock upgrade immediately, without using busy_timeout.
    // Retry only that startup race, outside a transaction, with a short bounded backoff.
    let started = std::time::Instant::now();
    for attempt in 0..7 {
        match db.pragma_update(None, "journal_mode", "WAL") {
            Ok(()) => break,
            Err(error)
                if attempt < 6
                    && started.elapsed() < Duration::from_millis(500)
                    && matches!(
                        error.sqlite_error_code(),
                        Some(
                            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                        )
                    ) =>
            {
                std::thread::sleep(Duration::from_millis(5 << attempt));
            }
            Err(error) => return Err(error),
        }
    }
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
        goal: row.get("goal")?,
        goal_due: row.get("goal_due")?,
        identity_revision: row.get("identity_revision")?,
        removed_at: row.get("removed_at")?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let db = open(path)?;
        let tx =
            rusqlite::Transaction::new_unchecked(&db, rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        // Columns added after the first release.
        for (table, column, decl) in [
            ("cards", "ci", "TEXT"),
            ("cards", "basis", "TEXT"),
            ("cards", "owner_basis", "TEXT"),
            ("portfolio", "basis", "TEXT"),
            ("feedback", "note", "TEXT NOT NULL DEFAULT ''"),
            ("feedback", "item", "TEXT"),
            ("feedback", "snoozed_until", "TEXT"),
            ("projects", "opened_at", "TEXT"),
            ("projects", "previous_opened_at", "TEXT"),
            ("projects", "goal", "TEXT NOT NULL DEFAULT ''"),
            ("projects", "goal_due", "TEXT"),
            ("projects", "removed_at", "TEXT"),
            ("ideas", "removed_at", "TEXT"),
            ("ideas", "revision", "INTEGER NOT NULL DEFAULT 1"),
            (
                "projects",
                "identity_revision",
                "INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            let present: bool = tx.query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = '{column}'"
                ),
                [],
                |r| r.get(0),
            )?;
            if !present {
                tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
            }
        }
        history::migrate(&tx)?;
        tx.commit()?;
        Ok(Store { db: Mutex::new(db) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        f(&db)
    }

    // projects
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.with(|db| {
            db.prepare("SELECT * FROM projects WHERE removed_at IS NULL ORDER BY added_at, id")?
                .query_map([], project)?
                .collect()
        })
    }

    pub fn project(&self, id: &str) -> Result<Option<Project>> {
        self.with(|db| {
            db.query_row(
                "SELECT * FROM projects WHERE id = ?1 AND removed_at IS NULL",
                [id],
                project,
            )
            .optional()
        })
    }

    pub fn add_project(
        &self,
        id: &str,
        name: &str,
        path: &str,
        at: &str,
    ) -> Result<ProjectRegistration> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let projects = tx
                .prepare("SELECT * FROM projects")?
                .query_map([], project)?
                .collect::<Result<Vec<_>>>()?;
            if let Some(existing) = projects.iter().find(|p| same_path(&p.path, path)) {
                return Ok(
                    if existing.id == id && existing.name == name && existing.removed_at.is_none() {
                        ProjectRegistration::Existing
                    } else {
                        ProjectRegistration::PathTaken(existing.id.clone())
                    },
                );
            }
            if projects.iter().any(|p| p.id == id) {
                return Ok(ProjectRegistration::IdTaken);
            }
            tx.execute(
                "INSERT INTO projects(id,name,path,added_at) VALUES(?1,?2,?3,?4)",
                params![id, name, path, at],
            )?;
            tx.commit()?;
            Ok(ProjectRegistration::Added)
        })
    }

    /// Update identity fields and record their undo in the same transaction.
    pub fn update_project(
        &self,
        before: &Project,
        name: &str,
        path: &str,
        fields: Vec<String>,
        at: &str,
    ) -> Result<ProjectUpdate> {
        let mut outcome = None;
        let receipt = self.owner_change(OwnerScope::Project { id: before.id.clone(), fields }, at, |db| {
            let current = db.query_row("SELECT * FROM projects WHERE id = ?1 AND removed_at IS NULL", [&before.id], project).optional()?;
            if current.is_none_or(|p| p.name != before.name || p.path != before.path || p.identity_revision != before.identity_revision) {
                outcome = Some(ProjectUpdate::Conflict);
                return Ok(());
            }
            let projects = db.prepare("SELECT * FROM projects WHERE id != ?1")?
                .query_map([&before.id], project)?.collect::<Result<Vec<_>>>()?;
            if let Some(other) = projects.iter().find(|p| same_path(&p.path, path)) {
                outcome = Some(ProjectUpdate::PathTaken(other.id.clone()));
                return Ok(());
            }
            db.execute("UPDATE projects SET name = ?2, path = ?3, identity_revision = identity_revision + 1 WHERE id = ?1", params![before.id, name, path])?;
            if before.path != path {
                db.execute("DELETE FROM facts WHERE project_id = ?1", [&before.id])?;
            }
            Ok(())
        })?;
        Ok(outcome.unwrap_or(ProjectUpdate::Updated(receipt)))
    }

    pub fn set_status(
        &self,
        id: &str,
        status: &str,
        note: Option<&str>,
        at: &str,
    ) -> Result<Value> {
        self.owner_change(OwnerScope::Project { id:id.into(), fields: std::iter::once("status".into()).chain(note.map(|_| "status_note".into())).collect() },at, |db| {
            db.execute("UPDATE projects SET status = ?1, status_note = COALESCE(?2,status_note), status_changed_at = ?3 WHERE id = ?4 AND removed_at IS NULL", params![status, note, at, id]).map(drop)
        })
    }

    pub fn set_goal(&self, id: &str, goal: &str, due: Option<&str>, at: &str) -> Result<Value> {
        self.owner_change(OwnerScope::Project { id:id.into(), fields: vec!["goal".into(),"goal_due".into()] },at, |db| {
            db.execute("UPDATE projects SET goal = ?2, goal_due = ?3, status_changed_at = ?4 WHERE id = ?1 AND removed_at IS NULL", params![id, goal, due, at]).map(drop)
        })
    }

    pub fn set_opened(&self, id: &str, at: &str, previous: Option<&str>) -> Result<()> {
        self.with(|db| {
            db.execute(
                "UPDATE projects SET opened_at = ?2, previous_opened_at = ?3 WHERE id = ?1",
                params![id, at, previous],
            )
            .map(drop)
        })
    }

    pub fn project_including_removed(&self, id: &str) -> Result<Option<Project>> {
        self.with(|db| {
            db.query_row("SELECT * FROM projects WHERE id = ?1", [id], project)
                .optional()
        })
    }

    pub fn all_projects(&self) -> Result<Vec<Project>> {
        self.with(|db| {
            db.prepare("SELECT * FROM projects ORDER BY added_at, id")?
                .query_map([], project)?
                .collect()
        })
    }

    /// Removal only changes visibility. All identity and history rows remain intact.
    pub fn set_project_removed(&self, id: &str, at: Option<&str>) -> Result<bool> {
        self.with(|db| db.execute(
            "UPDATE projects SET removed_at = ?2, identity_revision = identity_revision + 1 WHERE id = ?1 AND ((?2 IS NULL AND removed_at IS NOT NULL) OR (?2 IS NOT NULL AND removed_at IS NULL))",
            params![id, at],
        ).map(|n| n > 0))
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

    /// A collector started before a relink must not replace the new folder's facts.
    pub fn save_project_facts(
        &self,
        project: &Project,
        at: &str,
        fingerprint: &str,
        data: &Value,
    ) -> Result<bool> {
        self.with(|db| {
            db.execute(
                "INSERT INTO facts(project_id, collected_at, fingerprint, data)
                 SELECT ?1, ?2, ?3, ?4 WHERE EXISTS(SELECT 1 FROM projects WHERE id = ?1 AND path = ?5 AND identity_revision = ?6 AND removed_at IS NULL)
                 ON CONFLICT(project_id) DO UPDATE SET collected_at = excluded.collected_at,
                 fingerprint = excluded.fingerprint, data = excluded.data",
                params![project.id, at, fingerprint, data.to_string(), project.path, project.identity_revision],
            ).map(|count| count > 0)
        })
    }

    pub fn facts(&self, id: &str) -> Result<Option<StoredFacts>> {
        self.with(|db| {
            db.query_row(
                "SELECT collected_at, fingerprint, data FROM facts WHERE project_id = ?1",
                [id],
                |r| {
                    Ok(StoredFacts {
                        collected_at: r.get(0)?,
                        fingerprint: r.get(1)?,
                        data: json(r.get(2)?),
                    })
                },
            )
            .optional()
        })
    }

    // cards
    pub fn add_card(&self, id: &str, at: &str, basis: &CardBasis, data: &Value) -> Result<i64> {
        self.with(|db| {
            db.execute(
                "INSERT INTO cards(project_id, created_at, fingerprint, head, ci, basis, data, owner_basis) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, at, basis.fingerprint, basis.head, basis.ci, basis.parts.to_string(), data.to_string(), basis.owner.map(Value::to_string)],
            )?;
            db.query_row("SELECT COUNT(*) FROM cards WHERE project_id = ?1", [id], |r| r.get(0))
        })
    }

    pub fn latest_card(&self, id: &str) -> Result<Option<StoredCard>> {
        self.with(|db| {
            db.query_row("SELECT (SELECT COUNT(*) FROM cards WHERE project_id = ?1), created_at, fingerprint, head, ci, basis, data, owner_basis FROM cards WHERE project_id = ?1 ORDER BY id DESC LIMIT 1", [id], |r| {
                Ok(StoredCard {
                    version: r.get(0)?,
                    created_at: r.get(1)?,
                    fingerprint: r.get(2)?,
                    head: r.get(3)?,
                    ci: r.get(4)?,
                    basis: r.get::<_, Option<String>>(5)?.map(json),
                    data: json(r.get(6)?),
                    owner_basis: r.get::<_, Option<String>>(7)?.map(json),
                })
            })
            .optional()
        })
    }

    /// Every card version of a project, oldest first, as (created_at, data).
    pub fn card_history(&self, id: &str) -> Result<Vec<(String, Value)>> {
        self.with(|db| {
            let mut stmt =
                db.prepare("SELECT created_at, data FROM cards WHERE project_id = ?1 ORDER BY id")?;
            let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, json(r.get(1)?))))?;
            rows.collect()
        })
    }

    pub fn card_version(&self, id: &str, version: i64) -> Result<Option<StoredCard>> {
        if version < 1 {
            return Ok(None);
        }
        self.with(|db| db.query_row(
            "SELECT created_at,fingerprint,head,ci,basis,data,owner_basis FROM cards WHERE project_id=?1 ORDER BY id LIMIT 1 OFFSET ?2",
            params![id,version-1], |r| Ok(StoredCard {
                version,created_at:r.get(0)?,fingerprint:r.get(1)?,head:r.get(2)?,ci:r.get(3)?,
                basis:r.get::<_,Option<String>>(4)?.map(json),data:json(r.get(5)?),owner_basis:r.get::<_,Option<String>>(6)?.map(json)
            })).optional())
    }

    pub fn portfolio_history(&self) -> Result<Vec<StoredPortfolio>> {
        self.with(|db| {
            db.prepare("SELECT id,created_at,data,basis FROM portfolio ORDER BY id")?
                .query_map([], |r| {
                    Ok(StoredPortfolio {
                        version: r.get(0)?,
                        created_at: r.get(1)?,
                        data: json(r.get(2)?),
                        basis: r.get::<_, Option<String>>(3)?.map(json),
                    })
                })?
                .collect()
        })
    }

    /// First saved appearances of current app ideas, independent of later edits
    /// or removal and return. Read only the idea lists and stop once all are found.
    pub fn new_app_first_seen(&self, ids: &[&str]) -> Result<Map<String, Value>> {
        let mut dates = Map::new();
        if ids.is_empty() {
            return Ok(dates);
        }
        self.with(|db| {
            let mut stmt = db.prepare(
                "SELECT created_at, json_quote(json_extract(data, '$.new_apps'))
                 FROM portfolio WHERE json_valid(data) ORDER BY id",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let at: String = row.get(0)?;
                let ideas = json(row.get(1)?);
                for idea in ideas.as_array().into_iter().flatten() {
                    if let Some(id) = idea["id"].as_str().filter(|id| ids.contains(id)) {
                        dates
                            .entry(id.to_owned())
                            .or_insert_with(|| Value::String(at.clone()));
                    }
                }
                if dates.len() == ids.len() {
                    break;
                }
            }
            Ok(dates)
        })
    }

    pub fn portfolio_version(&self, version: i64) -> Result<Option<StoredPortfolio>> {
        self.with(|db| {
            db.query_row(
                "SELECT id,created_at,data,basis FROM portfolio WHERE id=?1",
                [version],
                |r| {
                    Ok(StoredPortfolio {
                        version: r.get(0)?,
                        created_at: r.get(1)?,
                        data: json(r.get(2)?),
                        basis: r.get::<_, Option<String>>(3)?.map(json),
                    })
                },
            )
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
        self.with(|db| {
            db.query_row(
                "SELECT COUNT(*) FROM cards WHERE project_id = ?1 AND created_at > ?2",
                params![id, at],
                |r| r.get(0),
            )
        })
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
        self.with(|db| {
            db.execute(
                "UPDATE portfolio SET data = ?1 WHERE id = (SELECT MAX(id) FROM portfolio)",
                [data.to_string()],
            )
            .map(drop)
        })
    }

    // portfolio
    pub fn add_portfolio(&self, at: &str, data: &Value, basis: &Value) -> Result<i64> {
        self.with(|db| {
            db.execute(
                "INSERT INTO portfolio(created_at, data, basis) VALUES(?1, ?2, ?3)",
                params![at, data.to_string(), basis.to_string()],
            )?;
            Ok(db.last_insert_rowid())
        })
    }

    pub fn latest_portfolio(&self) -> Result<Option<StoredPortfolio>> {
        self.with(|db| {
            db.query_row(
                "SELECT id, created_at, data, basis FROM portfolio ORDER BY id DESC LIMIT 1",
                [],
                |r| {
                    Ok(StoredPortfolio {
                        version: r.get(0)?,
                        created_at: r.get(1)?,
                        data: json(r.get(2)?),
                        basis: r.get::<_, Option<String>>(3)?.map(json),
                    })
                },
            )
            .optional()
        })
    }

    // feedback
    /// Resolve omitted fields from the current reaction while holding the write transaction.
    /// A rejected resolution leaves both the reaction and its undo history unchanged.
    pub fn set_feedback(
        &self,
        f: &Feedback,
        resolve: impl FnOnce(Option<Feedback>) -> Option<(String, Option<String>)>,
    ) -> Result<Option<Value>> {
        let mut accepted = true;
        let receipt = self.owner_change(OwnerScope::Feedback { id: f.item_id.clone() }, &f.at, |db| {
            let current = db.query_row("SELECT * FROM feedback WHERE item_id=?1",[&f.item_id], |row| Ok(Feedback {
                item_id: row.get("item_id")?, project_id: row.get("project_id")?, title: row.get("title")?,
                value: row.get("value")?, at: row.get("at")?, note: row.get("note")?,
                item: row.get::<_,Option<String>>("item")?.map(json), snoozed_until: row.get("snoozed_until")?,
            })).optional()?;
            let Some((note, snoozed_until)) = resolve(current) else {
                accepted = false;
                return Ok(());
            };
            db.execute(
                "INSERT INTO feedback(item_id, project_id, title, value, at, note, item, snoozed_until) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(item_id) DO UPDATE SET project_id = excluded.project_id, title = excluded.title, value = excluded.value,
                 at = excluded.at, note = excluded.note, item = excluded.item, snoozed_until = excluded.snoozed_until",
                params![f.item_id, f.project_id, f.title, f.value, f.at, note, f.item.as_ref().map(Value::to_string), snoozed_until],
            ).map(drop)
        })?;
        Ok(accepted.then_some(receipt))
    }

    pub fn delete_feedback(&self, item_id: &str, at: &str) -> Result<Value> {
        self.owner_change(OwnerScope::Feedback { id: item_id.into() }, at, |db| {
            db.execute("DELETE FROM feedback WHERE item_id = ?1", [item_id])
                .map(drop)
        })
    }

    /// All reactions, newest first.
    pub fn feedback(&self) -> Result<Vec<Feedback>> {
        self.with(|db| {
            db.prepare("SELECT item_id, project_id, title, value, at, note, item, snoozed_until FROM feedback ORDER BY at DESC")?
                .query_map([], |r| {
                    Ok(Feedback {
                        item_id: r.get(0)?,
                        project_id: r.get(1)?,
                        title: r.get(2)?,
                        value: r.get(3)?,
                        at: r.get(4)?,
                        note: r.get(5)?,
                        item: r.get::<_, Option<String>>(6)?.map(json),
                        snoozed_until: r.get(7)?,
                    })
                })?
                .collect()
        })
    }

    pub fn warning_acknowledgements(&self, project: &str) -> Result<Value> {
        self.with(|db| {
            let entries: Vec<Value> = db.prepare("SELECT kind, note, basis FROM warning_acks WHERE project_id = ?1 ORDER BY kind")?
                .query_map([project], |r| Ok(serde_json::json!({
                    "key": r.get::<_, String>(0)?, "note": r.get::<_, String>(1)?,
                    "basis": json(r.get(2)?),
                })))?.collect::<Result<_>>()?;
            Ok(Value::Array(entries))
        })
    }

    pub fn set_warning_ack(
        &self,
        project: &str,
        key: &str,
        note: Option<&str>,
        basis: &Value,
        at: &str,
    ) -> Result<Value> {
        self.owner_change(OwnerScope::Warning { id:project.into(), key:key.into() },at, |db| {
            match note {
                Some(note) => db.execute("INSERT INTO warning_acks(project_id,kind,note,basis,at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project_id,kind) DO UPDATE SET note=excluded.note,basis=excluded.basis,at=excluded.at",
                    params![project,key,note,basis.to_string(),at]),
                None => db.execute("DELETE FROM warning_acks WHERE project_id=?1 AND kind=?2", params![project,key]),
            }.map(drop)
        })
    }

    // ideas
    pub fn add_idea(&self, text: &str, at: &str) -> Result<(i64, bool)> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let existing = tx.query_row("SELECT id FROM ideas WHERE text = ?1 AND removed_at IS NULL ORDER BY id LIMIT 1", [text], |r| r.get(0)).optional()?;
            if let Some(id) = existing { return Ok((id, false)); }
            tx.execute("INSERT INTO ideas(text, added_at) VALUES(?1, ?2)", params![text, at])?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            Ok((id, true))
        })
    }

    pub fn idea(&self, id: i64) -> Result<Option<Value>> {
        self.with(|db| db.query_row("SELECT id,text,added_at,removed_at,revision FROM ideas WHERE id=?1", [id], |r| Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?, "text": r.get::<_, String>(1)?,
            "added_at": r.get::<_, String>(2)?, "removed_at": r.get::<_, Option<String>>(3)?, "revision":r.get::<_,i64>(4)?
        }))).optional())
    }

    pub fn set_idea_removed(&self, id: i64, at: Option<&str>) -> Result<bool> {
        self.with(|db| db.execute(
            "UPDATE ideas SET removed_at=?2 WHERE id=?1 AND ((?2 IS NULL AND removed_at IS NOT NULL) OR (?2 IS NOT NULL AND removed_at IS NULL))", params![id,at]
        ).map(|n| n > 0))
    }

    pub fn removed_ideas(&self) -> Result<Vec<Value>> {
        self.with(|db| db.prepare("SELECT id,text,added_at,removed_at FROM ideas WHERE removed_at IS NOT NULL ORDER BY id")?
            .query_map([], |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"text":r.get::<_,String>(1)?,"added_at":r.get::<_,String>(2)?,"removed_at":r.get::<_,String>(3)?})))?.collect())
    }

    /// All parked ideas, oldest first.
    pub fn ideas(&self) -> Result<Vec<Idea>> {
        self.with(|db| {
            db.prepare("SELECT id, text, added_at,revision FROM ideas WHERE removed_at IS NULL ORDER BY id")?
                .query_map([], |r| {
                    Ok(Idea {
                        id: r.get(0)?,
                        text: r.get(1)?,
                        added_at: r.get(2)?,
                        revision: r.get(3)?,
                    })
                })?
                .collect()
        })
    }

    // prefs
    pub fn pref(&self, key: &str) -> Result<Option<Value>> {
        self.with(|db| {
            db.query_row("SELECT value FROM prefs WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
        })
        .map(|v| v.map(json))
    }

    pub fn set_pref(&self, key: &str, value: &Value) -> Result<()> {
        self.with(|db| {
            db.execute("INSERT INTO prefs(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value.to_string()])
                .map(drop)
        })
    }
}

/// A tracked agent run keeps its starting point and final evidence independently of later edits.
pub struct StoredAgentRun {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub initial: Value,
    pub failures: Value,
    pub result: Option<Value>,
}

fn agent_run(row: &Row) -> Result<StoredAgentRun> {
    Ok(StoredAgentRun {
        id: row.get("id")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        initial: json(row.get("initial")?),
        failures: json(row.get("failures")?),
        result: row.get::<_, Option<String>>("result")?.map(json),
    })
}

impl Store {
    pub fn start_agent_run(&self, at: &str, initial: &Value) -> Result<(i64, bool)> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let existing = tx
                .query_row(
                    "SELECT id FROM agent_runs WHERE finished_at IS NULL ORDER BY id DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                return Ok((id, false));
            }
            tx.execute(
                "INSERT INTO agent_runs(started_at, initial) VALUES(?1, ?2)",
                params![at, initial.to_string()],
            )?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            Ok((id, true))
        })
    }

    pub fn agent_run(&self, id: i64) -> Result<Option<StoredAgentRun>> {
        self.with(|db| {
            db.query_row("SELECT * FROM agent_runs WHERE id = ?1", [id], agent_run)
                .optional()
        })
    }

    pub fn agent_runs(&self) -> Result<Vec<StoredAgentRun>> {
        self.with(|db| {
            db.prepare("SELECT * FROM agent_runs ORDER BY id DESC LIMIT 20")?
                .query_map([], agent_run)?
                .collect()
        })
    }

    pub fn last_successful_agent_run(&self) -> Result<Option<StoredAgentRun>> {
        self.with(|db| db.query_row("SELECT * FROM agent_runs WHERE outcome = 'complete' ORDER BY finished_at DESC, id DESC LIMIT 1", [], agent_run).optional())
    }

    pub fn fail_agent_run(&self, id: i64, scope: &str, failure: &Value) -> Result<bool> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let mut failures = json(tx.query_row("SELECT failures FROM agent_runs WHERE id = ?1", [id], |row| row.get::<_, String>(0))?);
            if failures[scope]["error"] == failure["error"] { return Ok(true); }
            let version: i64 = if scope == "@portfolio" {
                tx.query_row("SELECT COALESCE(MAX(id), 0) FROM portfolio", [], |row| row.get(0))?
            } else {
                tx.query_row("SELECT COUNT(*) FROM cards WHERE project_id = ?1", [scope], |row| row.get(0))?
            };
            let mut failure = failure.clone();
            failure["version"] = Value::from(version);
            failures[scope] = failure;
            let changed = tx.execute("UPDATE agent_runs SET failures = ?2 WHERE id = ?1 AND (outcome IS NULL OR outcome != 'complete')", params![id, failures.to_string()])?;
            tx.commit()?;
            Ok(changed > 0)
        })
    }

    pub fn agent_run_token(&self, id: i64) -> Result<String> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let token = agent_run_token(&tx, id)?;
            tx.commit()?;
            Ok(token)
        })
    }

    pub fn finish_agent_run(
        &self,
        id: i64,
        at: &str,
        result: &Value,
        expected: &str,
    ) -> Result<bool> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            if agent_run_token(&tx, id)? != expected {
                return Ok(false);
            }
            tx.execute("UPDATE agent_runs SET finished_at = ?2, result = ?3, outcome = ?4 WHERE id = ?1 AND (outcome IS NULL OR outcome != 'complete')", params![id, at, result.to_string(), result["state"].as_str()])?;
            tx.commit()?;
            Ok(true)
        })
    }
}

/// Finish against the same project evidence and failures that were inspected.
fn agent_run_token(db: &Connection, id: i64) -> Result<String> {
    use sha1::{Digest, Sha1};
    let run = db.query_row("SELECT * FROM agent_runs WHERE id = ?1", [id], agent_run)?;
    let basis = serde_json::json!([
        context_token(db, None)?,
        run.initial,
        run.failures,
        run.result,
        run.finished_at
    ]);
    Ok(format!(
        "run1-{:x}",
        Sha1::digest(basis.to_string().as_bytes())
    ))
}

// Context tokens cover the values agents read, independent of wall-clock precision.
fn token_rows(db: &Connection, sql: &str, scope: Option<&str>) -> Result<Value> {
    let mut stmt = db.prepare(sql)?;
    let columns = stmt.column_count();
    let mut rows = stmt.query([scope])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut values = Vec::new();
        for i in 0..columns {
            values.push(match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(n) => Value::from(n),
                rusqlite::types::ValueRef::Real(n) => Value::from(n),
                rusqlite::types::ValueRef::Text(s) => {
                    Value::from(String::from_utf8_lossy(s).into_owned())
                }
                rusqlite::types::ValueRef::Blob(s) => Value::from(format!("{s:?}")),
            });
        }
        out.push(Value::Array(values));
    }
    Ok(Value::Array(out))
}

fn context_token(db: &Connection, scope: Option<&str>) -> Result<String> {
    use sha1::{Digest, Sha1};
    let mut basis = serde_json::json!({"scope": scope});
    basis["projects"] = token_rows(db,
        "SELECT id,name,path,status,status_note,goal,goal_due,added_at,identity_revision FROM projects WHERE removed_at IS NULL AND ((?1 IS NULL AND status != 'archived') OR id = ?1) ORDER BY id", scope)?;
    basis["cards"] = token_rows(db,
        "SELECT c.project_id,c.id,c.data FROM cards c JOIN projects p ON p.id=c.project_id WHERE c.id=(SELECT MAX(id) FROM cards WHERE project_id=c.project_id) AND p.removed_at IS NULL AND ((?1 IS NULL AND p.status != 'archived') OR p.id=?1) ORDER BY p.id", scope)?;
    basis["facts"] = token_rows(db,
        "SELECT f.project_id,f.fingerprint,f.data FROM facts f JOIN projects p ON p.id=f.project_id WHERE p.removed_at IS NULL AND ((?1 IS NULL AND p.status != 'archived') OR p.id=?1) ORDER BY p.id", scope)?;
    basis["feedback"] = token_rows(db,
        "SELECT item_id,project_id,value,note,item,at,snoozed_until FROM feedback WHERE ?1 IS NULL OR project_id=?1 OR value IN ('up','later','down') ORDER BY item_id", scope)?;
    if scope.is_none() {
        basis["portfolio"] = token_rows(
            db,
            "SELECT id,data FROM portfolio WHERE ?1 IS NULL ORDER BY id DESC LIMIT 1",
            scope,
        )?;
        basis["ideas"] = token_rows(
            db,
            "SELECT id,text FROM ideas WHERE ?1 IS NULL AND removed_at IS NULL ORDER BY id",
            scope,
        )?;
        // The brief only needs to know whether the owner opened Watchtower since
        // its previous version. Later visits do not change that interpretation.
        basis["previous_portfolio_opened"] = Value::Bool(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM prefs WHERE key='last_visit' AND json_extract(value, '$') >= (SELECT created_at FROM portfolio ORDER BY id DESC LIMIT 1))",
            [],
            |row| row.get(0),
        )?);
    }
    // These owner-facing features are absent in databases from before their migration.
    for (table, query) in [
        ("tasks", "SELECT id,status,revision,data FROM tasks WHERE ?1 IS NULL OR EXISTS(SELECT 1 FROM json_each(data,'$.projects') WHERE value=?1) ORDER BY id"),
        ("warning_acks", "SELECT project_id,kind,note,basis FROM warning_acks WHERE ?1 IS NULL OR project_id=?1 ORDER BY project_id,kind"),
    ] {
        let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |r| r.get(0))?;
        if exists { basis[table] = token_rows(db, query, scope)?; }
    }
    Ok(format!(
        "ct1-{:x}",
        Sha1::digest(basis.to_string().as_bytes())
    ))
}

fn item_conflicts(
    db: &Connection,
    kind: crate::cards::Kind,
    project: Option<&str>,
    doc: &Value,
) -> Result<Vec<String>> {
    use crate::cards::Kind;
    let mut owners: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    let mut add = |kind: Kind, project: Option<String>, doc: Value| {
        for (list, _, item_kind) in kind.items() {
            for item in doc[*list].as_array().into_iter().flatten() {
                if let Some(id) = item["id"].as_str() {
                    owners.push((
                        id.to_owned(),
                        project.clone(),
                        Some((*item_kind).to_owned()),
                    ));
                }
            }
        }
    };
    let mut stmt = db.prepare("SELECT project_id,data FROM cards c WHERE id=(SELECT MAX(id) FROM cards WHERE project_id=c.project_id)")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, json(r.get(1)?))))? {
        let (project, data) = row?;
        add(Kind::Card, Some(project), data);
    }
    if let Some(data) = db
        .query_row(
            "SELECT data FROM portfolio ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        add(Kind::Portfolio, None, json(data));
    }
    let mut stmt = db.prepare("SELECT id,NULLIF(scope,'@portfolio'),kind FROM proposals")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })? {
        owners.push(row?);
    }
    let mut stmt = db.prepare("SELECT item_id,project_id,item FROM feedback")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?.map(json),
        ))
    })? {
        let (id, project, item) = row?;
        let item_kind = item.as_ref().and_then(|i| i["kind"].as_str());
        let scope = match item_kind {
            Some("suggestion" | "question") | None => project,
            Some(_) => None,
        };
        owners.push((id, scope, item_kind.map(str::to_owned)));
    }
    let mut conflicts = Vec::new();
    for (list, _, item_kind) in kind.items() {
        for (index, item) in doc[*list].as_array().into_iter().flatten().enumerate() {
            let Some(id) = item["id"].as_str() else {
                continue;
            };
            if owners.iter().any(|(other, scope, other_kind)| {
                other == id
                    && (scope.as_deref() != project
                        || other_kind.as_deref().is_some_and(|k| k != *item_kind))
            }) {
                conflicts.push(format!(
                    "{list}[{index}].id: '{id}' belongs to another card, brief or item kind"
                ));
            }
        }
    }
    Ok(conflicts)
}

/// Receipts and the written document share one transaction. A receipt is usable
/// only while every context input still equals the state immediately after that write.
fn replay_document(
    db: &Connection,
    scope: Option<&str>,
    expected: &str,
    request: &str,
) -> Result<Option<(i64, String)>> {
    let receipt = db.query_row(
        "SELECT version,after_token FROM document_requests WHERE scope=?1 AND expected=?2 AND request=?3",
        params![scope.unwrap_or("@portfolio"), expected, request],
        |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)),
    ).optional()?;
    match receipt {
        Some((version, token)) if context_token(db, scope)? == token => Ok(Some((version, token))),
        _ => Ok(None),
    }
}

fn record_document_request(
    db: &Connection,
    scope: Option<&str>,
    expected: &str,
    request: &str,
    version: i64,
) -> Result<String> {
    let token = context_token(db, scope)?;
    db.execute("INSERT OR REPLACE INTO document_requests(scope,expected,request,after_token,version) VALUES(?1,?2,?3,?4,?5)",
        params![scope.unwrap_or("@portfolio"), expected,request,token,version])?;
    Ok(token)
}

impl Store {
    pub fn document_replay(
        &self,
        scope: Option<&str>,
        expected: &str,
        request: &str,
    ) -> Result<Option<(i64, String)>> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let result = replay_document(&tx, scope, expected, request)?;
            tx.commit()?;
            Ok(result)
        })
    }

    pub fn context_token(&self, project: Option<&str>) -> Result<String> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let token = context_token(&tx, project)?;
            tx.commit()?;
            Ok(token)
        })
    }

    pub fn add_card_guarded(
        &self,
        id: &str,
        at: &str,
        basis: &CardBasis,
        data: &Value,
        expected: &str,
        request: &str,
    ) -> Result<GuardedWrite> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            if context_token(&tx, Some(id))? != expected {
                return Ok(match replay_document(&tx, Some(id), expected, request)? {
                    Some((version, token)) => GuardedWrite::Replayed(version, token),
                    None => GuardedWrite::Conflict,
                });
            }
            let conflicts = item_conflicts(&tx, crate::cards::Kind::Card, Some(id), data)?;
            if !conflicts.is_empty() { return Ok(GuardedWrite::ItemConflict(conflicts)); }
            tx.execute("INSERT INTO cards(project_id,created_at,fingerprint,head,ci,basis,data,owner_basis) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![id,at,basis.fingerprint,basis.head,basis.ci,basis.parts.to_string(),data.to_string(),basis.owner.map(Value::to_string)])?;
            let version = tx.query_row("SELECT COUNT(*) FROM cards WHERE project_id=?1", [id], |r| r.get(0))?;
            let after = record_document_request(&tx, Some(id), expected, request, version)?;
            tx.commit()?;
            Ok(GuardedWrite::Saved(version, after))
        })
    }

    pub fn add_portfolio_guarded(
        &self,
        at: &str,
        data: &Value,
        basis: &Value,
        expected: &str,
        request: &str,
    ) -> Result<GuardedWrite> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            if context_token(&tx, None)? != expected {
                return Ok(match replay_document(&tx, None, expected, request)? {
                    Some((version, token)) => GuardedWrite::Replayed(version, token),
                    None => GuardedWrite::Conflict,
                });
            }
            let conflicts = item_conflicts(&tx, crate::cards::Kind::Portfolio, None, data)?;
            if !conflicts.is_empty() {
                return Ok(GuardedWrite::ItemConflict(conflicts));
            }
            tx.execute(
                "INSERT INTO portfolio(created_at,data,basis) VALUES(?1,?2,?3)",
                params![at, data.to_string(), basis.to_string()],
            )?;
            let version = tx.last_insert_rowid();
            let after = record_document_request(&tx, None, expected, request, version)?;
            tx.commit()?;
            Ok(GuardedWrite::Saved(version, after))
        })
    }
}

/// Notices writes from any connection, including other processes such as the CLI.
pub struct ChangeWatch {
    db: Mutex<Connection>,
}

/// A consistent copy of the inputs used by the owner's unread-change digest.
pub(crate) struct ChangesSnapshot {
    pub current: Value,
    pub baseline: Option<Value>,
    pub token: String,
}

fn changes_source(db: &Connection) -> Result<Value> {
    let mut statement = db.prepare(
        "SELECT p.id, p.name, p.status, c.id, c.data, c.owner_basis
         FROM projects p
         LEFT JOIN cards c ON c.id = (SELECT MAX(id) FROM cards WHERE project_id = p.id)
         WHERE p.removed_at IS NULL ORDER BY p.id",
    )?;
    let projects: Vec<Value> = statement
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "card_version": row.get::<_, Option<i64>>(3)?,
                "card": row.get::<_, Option<String>>(4)?.map(json),
                "owner_basis": row.get::<_, Option<String>>(5)?.map(json),
            }))
        })?
        .collect::<Result<_>>()?;
    let mut statement = db.prepare(
        "SELECT item_id, project_id, title, value, note, item FROM feedback ORDER BY item_id",
    )?;
    let feedback: Vec<Value> = statement
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?, "project": row.get::<_, Option<String>>(1)?,
                "title": row.get::<_, String>(2)?, "value": row.get::<_, String>(3)?,
                "note": row.get::<_, String>(4)?,
                "item": row.get::<_, Option<String>>(5)?.map(json),
            }))
        })?
        .collect::<Result<_>>()?;
    Ok(serde_json::json!({"projects": projects, "feedback": feedback}))
}

fn changes_token(snapshot: &Value) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(snapshot.to_string().as_bytes()))
}

impl Store {
    /// Reading or visiting never creates or moves the explicit read baseline.
    pub(crate) fn changes_snapshot(&self) -> Result<ChangesSnapshot> {
        self.with(|db| {
            let transaction =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Deferred)?;
            let current = changes_source(&transaction)?;
            let token = changes_token(&current);
            let baseline = transaction
                .query_row(
                    "SELECT value FROM prefs WHERE key = 'changes_read'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(json);
            transaction.commit()?;
            Ok(ChangesSnapshot {
                current,
                baseline,
                token,
            })
        })
    }

    /// The comparison and acknowledgement share a write transaction, so another
    /// process cannot sneak an unread card into the acknowledged baseline.
    pub(crate) fn acknowledge_changes(
        &self,
        token: &str,
        snapshot: &Value,
        at: &str,
    ) -> Result<bool> {
        self.with(|db| {
            let transaction =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let current = changes_source(&transaction)?;
            if changes_token(&current) != token {
                return Ok(false);
            }
            let baseline = serde_json::json!({"at": at, "snapshot": snapshot});
            transaction.execute(
                "INSERT INTO prefs(key, value) VALUES('changes_read', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [baseline.to_string()],
            )?;
            transaction.commit()?;
            Ok(true)
        })
    }
}

fn tracked_task(row: &Row) -> Result<Value> {
    let mut task = json(row.get("data")?);
    task["id"] = serde_json::json!(row.get::<_, i64>("id")?);
    task["status"] = serde_json::json!(row.get::<_, String>("status")?);
    task["revision"] = serde_json::json!(row.get::<_, i64>("revision")?);
    Ok(task)
}

impl Store {
    pub fn tasks(&self) -> Result<Vec<Value>> {
        self.with(|db| {
            db.prepare("SELECT id, status, revision, data FROM tasks ORDER BY id DESC")?
                .query_map([], tracked_task)?
                .collect()
        })
    }

    pub fn task(&self, id: i64) -> Result<Option<Value>> {
        self.with(|db| {
            db.query_row(
                "SELECT id, status, revision, data FROM tasks WHERE id = ?1",
                [id],
                tracked_task,
            )
            .optional()
        })
    }

    /// The partial unique index makes repeated assignment atomic across processes.
    pub fn create_tracked_task(
        &self,
        source_key: Option<&str>,
        data: &Value,
    ) -> Result<(Value, bool)> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let created = tx.execute(
                "INSERT INTO tasks(status, source_key, data) VALUES('assigned', ?1, ?2) ON CONFLICT DO NOTHING",
                params![source_key, data.to_string()],
            )? > 0;
            let task = if created {
                tx.query_row(
                    "SELECT id, status, revision, data FROM tasks WHERE id = ?1",
                    [tx.last_insert_rowid()], tracked_task,
                )?
            } else {
                tx.query_row(
                    "SELECT id, status, revision, data FROM tasks WHERE source_key = ?1 AND status IN ('assigned', 'blocked', 'review')",
                    [source_key], tracked_task,
                )?
            };
            tx.commit()?;
            Ok((task, created))
        })
    }

    /// A stale agent report must not overwrite a simultaneous owner decision.
    pub fn replace_task(&self, id: i64, revision: i64, status: &str, data: &Value) -> Result<bool> {
        self.with(|db| {
            db.execute(
                "UPDATE tasks SET status = ?1, revision = revision + 1, data = ?2 WHERE id = ?3 AND revision = ?4",
                params![status, data.to_string(), id, revision],
            ).map(|count| count > 0)
        })
    }
}

impl ChangeWatch {
    pub fn open(path: &Path) -> Result<ChangeWatch> {
        Ok(ChangeWatch {
            db: Mutex::new(open(path)?),
        })
    }

    /// A number that changes whenever another connection commits a write.
    pub fn version(&self) -> Result<i64> {
        let db = self.db.lock().unwrap_or_else(|e| e.into_inner());
        db.query_row("PRAGMA data_version", [], |r| r.get(0))
    }
}
