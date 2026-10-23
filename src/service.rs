//! Watchtower's single owner of behavior, used by both the app window and the CLI.
//!
//! Owns project registration, fact refreshes, deciding which projects need a new
//! interpretation, the context an agent reads before writing a card, card and
//! portfolio writes, feedback, and the complete state the window renders.
use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
mod context;
mod documents;
mod freshness;
mod ideas;
mod owner_state;
mod projects;
mod reading;
mod runs;
mod tasks;
use documents::document_request;
use freshness::{card_outdated, owner_basis, settled_ci, task_basis};
use reading::{digest_text, document_item, DocumentIndex};
use tasks::{active_task, new_task, record_task_transition, task_source_key};
mod signals;
pub use freshness::DEFAULT_MAX_AGE_DAYS;
use ideas::{idea_json, without_effort};
pub use owner_state::PREFS;
pub use owner_state::RATINGS;
pub use owner_state::SNOOZE_DAYS;
use owner_state::{feedback_context, feedback_summary, kind_of, snoozed_until, title_of};
pub use projects::STATUSES;
use signals::acknowledged_warnings;
mod statistics;
mod undo;
use crate::cards::{self, Kind};
use crate::facts;
use crate::store::{
    same_path, CardBasis, Feedback, Idea, Project, ProjectRegistration, ProjectUpdate, Store,
    StoredCard, StoredFacts, StoredPortfolio,
};
use undo::with_receipt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchtowerError {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
}

impl WatchtowerError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        WatchtowerError {
            code: code.into(),
            message: message.into(),
            hint: None,
        }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn to_json(&self) -> Value {
        let mut err = json!({"code": self.code, "message": self.message});
        if let Some(hint) = &self.hint {
            err["hint"] = json!(hint);
        }
        err
    }
}

impl std::fmt::Display for WatchtowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WatchtowerError {}

impl From<rusqlite::Error> for WatchtowerError {
    fn from(e: rusqlite::Error) -> Self {
        use rusqlite::ErrorCode;
        let recovery = match e.sqlite_error_code() {
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) =>
                "Wait for the other database operation to finish. Read the current state before retrying a write; this error does not confirm that earlier steps were undone.",
            Some(ErrorCode::ReadOnly | ErrorCode::PermissionDenied | ErrorCode::CannotOpen | ErrorCode::AuthorizationForStatementDenied) =>
                "Check WATCHTOWER_HOME and access to its data folder and database. Restore the intended access, then read the current state before retrying a write. Do not create a replacement database.",
            Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase) =>
                "Stop writing and preserve the database and its -wal/-shm files. Check WATCHTOWER_HOME points to the intended data folder and report this error for recovery; do not delete or recreate the database.",
            Some(ErrorCode::DiskFull | ErrorCode::SystemIoFailure) =>
                "Check available space and access to the disk containing WATCHTOWER_HOME. Resolve the storage problem, then read the current state before retrying a write; this error does not confirm that earlier steps were undone.",
            _ =>
                "Read the current state before repeating a write; this error does not confirm that earlier steps were undone. If the error persists, report it with the failing command and preserve the database. Do not delete or recreate it.",
        };
        WatchtowerError::new(
            "storage_error",
            format!("Watchtower's database failed: {e}"),
        )
        .hint(recovery)
    }
}

pub type Result<T> = std::result::Result<T, WatchtowerError>;

pub type Clock = Box<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// Measures one repository: (path, today, with_ci, the folder's previous facts) -> facts.
pub type Collector = Box<dyn Fn(&str, NaiveDate, bool, Option<&Value>) -> Value + Send + Sync>;

/// Deterministic synchronization points for run-completion integration contracts.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunCompletionStage {
    DueEvaluated,
    BeforeSave,
}

pub struct Watchtower {
    store: Store,
    clock: Clock,
    collect: Collector,
    /// Item appearances read so far, extended by later saves.
    ages: Mutex<signals::AgeIndex>,
    documents: Mutex<DocumentIndex>,
    completion_observer: Option<Box<dyn Fn(RunCompletionStage) + Send + Sync>>,
}

fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_default()
}

fn extend(mut base: Value, extra: Value) -> Value {
    if let (Some(b), Value::Object(e)) = (base.as_object_mut(), extra) {
        b.extend(e);
    }
    base
}

fn project_json(p: &Project) -> Value {
    serde_json::to_value(p).unwrap_or(Value::Null)
}

fn pick(v: &Value, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .map(|k| ((*k).to_owned(), v.get(*k).cloned().unwrap_or(Value::Null)))
            .collect(),
    )
}

impl Watchtower {
    /// Watchtower on a database file, measuring with git and the real clock.
    pub fn open(db: &Path) -> Result<Watchtower> {
        let svc = Watchtower::with_parts(
            Store::open(db)?,
            Box::new(Utc::now),
            Box::new(facts::collect_reusing),
        );
        svc.give_items_ids()?;
        Ok(svc)
    }

    pub fn with_parts(store: Store, clock: Clock, collect: Collector) -> Watchtower {
        Watchtower {
            store,
            clock,
            collect,
            ages: Mutex::default(),
            documents: Mutex::default(),
            completion_observer: None,
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }
    // ---------- time ----------
    pub fn now(&self) -> String {
        stamp((self.clock)())
    }

    pub fn today(&self) -> NaiveDate {
        (self.clock)().with_timezone(&Local).date_naive()
    }
}
