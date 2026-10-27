//! SQLite persistence: one database file shared by the app window and the CLI.
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
mod documents;
mod owner_state;
mod projects;
mod reading;
mod runs;
mod tasks;
use documents::context_token;
pub use runs::StoredAgentRun;
pub(crate) use tasks::{IdeaChange, IdeaSnapshot, IdeaTaskChange};
mod history;
mod undo;
use undo::OwnerScope;
pub use undo::{OwnerReceipt, OwnerUndo};
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
CREATE TABLE IF NOT EXISTS idea_archives(
  item_id TEXT PRIMARY KEY, project_id TEXT, title TEXT NOT NULL,
  archived INTEGER NOT NULL, revision INTEGER NOT NULL, at TEXT NOT NULL);
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
CREATE INDEX IF NOT EXISTS owner_undo_active_scope ON owner_undo(scope) WHERE undone_at IS NULL;
CREATE TABLE IF NOT EXISTS owner_field_versions(field TEXT PRIMARY KEY, version INTEGER NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS tasks_active_source ON tasks(source_key)
  WHERE source_key IS NOT NULL AND status IN ('assigned', 'blocked', 'review');
CREATE INDEX IF NOT EXISTS tasks_by_source_id ON tasks(json_extract(data,'$.source_id'),id);
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
    Updated(Box<OwnerReceipt>),
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
    pub revision: i64,
    pub collected_at: String,
    pub fingerprint: String,
    pub data: Value,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FactsSave {
    Saved,
    ProjectChanged,
    FactsChanged,
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
/// One saved card or brief version as stored; `project_id` is empty for briefs.
pub struct DocumentRow {
    pub id: i64,
    pub project_id: Option<String>,
    pub created_at: String,
    pub data: String,
}

pub struct DocumentRows {
    pub cards: Vec<DocumentRow>,
    pub briefs: Vec<DocumentRow>,
    /// The newest card row of every project.
    pub latest_cards: Vec<i64>,
    pub latest_brief: Option<i64>,
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
/// One measurement of the facts that make up a persisting risk; `None` is unknown.
pub struct RiskObservation {
    pub at: String,
    pub ok: bool,
    pub dirty_files: Option<i64>,
    pub lines_added: Option<i64>,
    pub ahead: Option<i64>,
    pub ci: Option<String>,
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
            ("facts", "revision", "INTEGER NOT NULL DEFAULT 0"),
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

    /// A number that changes whenever another connection, such as the CLI in another
    /// process, commits a write. Writes through this store leave it unchanged.
    pub fn data_version(&self) -> Result<i64> {
        self.with(|db| db.query_row("PRAGMA data_version", [], |r| r.get(0)))
    }
    // projects
}
