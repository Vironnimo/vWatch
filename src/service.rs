//! Lookout's single owner of behavior, used by both the app window and the CLI.
//!
//! Owns project registration, fact refreshes, deciding which projects need a new
//! interpretation, the context an agent reads before writing a card, card and
//! portfolio writes, feedback, and the complete state the window renders.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Local, NaiveDate, Utc};
use serde_json::{json, Map, Value};

use crate::cards::{self, Kind};
use crate::facts;
use crate::store::{Project, Store};

pub const STATUSES: &[&str] = &["active", "parked", "done", "archived"];
pub const FEEDBACK: &[&str] = &["up", "later", "down"];
pub const PREFS: &[&str] = &["style", "hidden_modules"];
pub const SNOOZE_DAYS: i64 = 14;
pub const DEFAULT_MAX_AGE_DAYS: i64 = 14;
const COMMITS_SINCE_LIMIT: usize = 150;
const REFRESH_WORKERS: usize = 4;
const DISCOVER_DEPTH: usize = 2;
/// Repositories with a commit in this many days are recommended when discovering.
const RECENT_DAYS: i64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookoutError {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
}

impl LookoutError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        LookoutError { code: code.into(), message: message.into(), hint: None }
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

impl std::fmt::Display for LookoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LookoutError {}

impl From<rusqlite::Error> for LookoutError {
    fn from(e: rusqlite::Error) -> Self {
        LookoutError::new("storage_error", format!("Lookout's database failed: {e}")).hint("Try again in a moment.")
    }
}

pub type Result<T> = std::result::Result<T, LookoutError>;
pub type Clock = Box<dyn Fn() -> DateTime<Utc> + Send + Sync>;
/// Measures one repository: (path, today, with_ci) -> facts.
pub type Collector = Box<dyn Fn(&str, NaiveDate, bool) -> Value + Send + Sync>;

pub struct Lookout {
    store: Store,
    clock: Clock,
    collect: Collector,
}

fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).map(|t| t.with_timezone(&Utc)).unwrap_or_default()
}

fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) { a.to_lowercase() == b.to_lowercase() } else { a == b }
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

fn valid_project_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=40).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Git repositories below `dir`, at most `depth` levels down; never looks inside a repository.
fn find_repos(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            !name.starts_with('.') && !["node_modules", "target", "$recycle.bin"].contains(&name.as_str())
        })
        .map(|e| e.path())
        .collect();
    children.sort();
    let mut repos = Vec::new();
    for child in children {
        if child.join(".git").exists() {
            repos.push(child);
        } else if depth > 1 {
            repos.extend(find_repos(&child, depth - 1));
        }
    }
    repos
}

/// A CI state firm enough to compare: `green` or `red`. Running, unknown or skipped
/// lookups say nothing about whether a card's CI statement still holds.
fn settled_ci(facts: &Value) -> Option<&str> {
    facts["ci"]["state"].as_str().filter(|s| matches!(*s, "green" | "red"))
}

fn resolve(path: &str) -> PathBuf {
    let expanded = match path.strip_prefix('~') {
        Some(rest) => {
            let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
            PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']))
        }
        None => PathBuf::from(path),
    };
    std::path::absolute(&expanded).unwrap_or(expanded)
}

/// Add the keys of `extra` (an object) to `base` (an object).
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
    Value::Object(keys.iter().map(|k| ((*k).to_owned(), v.get(*k).cloned().unwrap_or(Value::Null))).collect())
}

impl Lookout {
    /// Lookout on a database file, measuring with git and the real clock.
    pub fn open(db: &Path) -> Result<Lookout> {
        Ok(Lookout::with_parts(Store::open(db)?, Box::new(Utc::now), Box::new(facts::collect)))
    }

    pub fn with_parts(store: Store, clock: Clock, collect: Collector) -> Lookout {
        Lookout { store, clock, collect }
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

    // ---------- projects ----------
    fn project(&self, id: &str) -> Result<Project> {
        if let Some(p) = self.store.project(id)? {
            return Ok(p);
        }
        let known: Vec<String> = self.store.projects()?.into_iter().map(|p| p.id).collect();
        let known = if known.is_empty() { "none yet".to_owned() } else { known.join(", ") };
        Err(LookoutError::new("project_not_found", format!("No project with id '{id}'.")).hint(format!("Known project ids: {known}.")))
    }

    pub fn list_projects(&self) -> Result<Value> {
        let mut out = Vec::new();
        for p in self.store.projects()? {
            let card = self.store.latest_card(&p.id)?;
            let stored = self.store.facts(&p.id)?;
            out.push(extend(
                project_json(&p),
                json!({
                    "has_card": card.is_some(),
                    "card_updated_at": card.map(|c| c.created_at),
                    "facts_collected_at": stored.map(|s| s.collected_at),
                }),
            ));
        }
        Ok(Value::Array(out))
    }

    fn register(&self, path: &str, id: Option<&str>, name: Option<&str>) -> Result<Project> {
        let resolved = resolve(path);
        let resolved_str = resolved.to_string_lossy().into_owned();
        if !resolved.is_dir() {
            return Err(LookoutError::new("path_missing", format!("Folder does not exist: {resolved_str}")));
        }
        if let Some(p) = self.store.projects()?.into_iter().find(|p| same_path(&p.path, &resolved_str)) {
            return Err(LookoutError::new("already_registered", format!("This folder is already registered as '{}'.", p.id)));
        }
        let folder = resolved.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| resolved_str.clone());
        let id = match id {
            Some(id) => id.to_owned(),
            None => {
                let s: String = slug(&folder).chars().take(40).collect();
                let s = s.trim_end_matches('-').to_owned();
                if s.is_empty() { "project".to_owned() } else { s }
            }
        };
        if !valid_project_id(&id) {
            return Err(LookoutError::new("invalid_id", format!("Invalid project id '{id}'."))
                .hint("Use lowercase letters, digits and hyphens, at most 40 characters."));
        }
        if self.store.project(&id)?.is_some() {
            return Err(LookoutError::new("id_taken", format!("The id '{id}' is already used.")).hint("Pass another id with --id."));
        }
        self.store.add_project(&id, name.unwrap_or(&folder), &resolved_str, &self.now())?;
        self.project(&id)
    }

    pub fn add_project(&self, path: &str, id: Option<&str>, name: Option<&str>) -> Result<Value> {
        let project = self.register(path, id, name)?;
        let refreshed = self.refresh_facts(Some(std::slice::from_ref(&project.id)), true)?;
        let mut result = extend(project_json(&project), json!({"facts": refreshed[0]}));
        if !facts::is_git_repo(&project.path) {
            result["warning"] = json!("The folder is not a git repository; only a card written by an agent can describe it.");
        }
        Ok(result)
    }

    /// Register several folders at once and measure them in parallel.
    pub fn add_projects(&self, paths: &[String]) -> Result<Value> {
        let mut added = Vec::new();
        let mut failed = Vec::new();
        for path in paths {
            match self.register(path, None, None) {
                Ok(p) => added.push(p),
                Err(e) => failed.push(json!({"path": path, "error": e.to_json()})),
            }
        }
        let ids: Vec<String> = added.iter().map(|p| p.id.clone()).collect();
        if !ids.is_empty() {
            self.refresh_facts(Some(&ids), true)?;
        }
        Ok(json!({"added": added.iter().map(project_json).collect::<Vec<_>>(), "failed": failed}))
    }

    pub fn remove_project(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        self.store.remove_project(id)?;
        Ok(json!({"removed": id}))
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<Value> {
        self.project(id)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(LookoutError::new("invalid_name", "The name must not be empty."));
        }
        self.store.rename_project(id, name)?;
        Ok(project_json(&self.project(id)?))
    }

    pub fn set_status(&self, id: &str, status: &str, note: Option<&str>) -> Result<Value> {
        let project = self.project(id)?;
        if !STATUSES.contains(&status) {
            return Err(LookoutError::new("invalid_status", format!("Unknown status '{status}'."))
                .hint(format!("Use one of: {}.", STATUSES.join(", "))));
        }
        self.store.set_status(id, status, note.unwrap_or(&project.status_note), &self.now())?;
        Ok(project_json(&self.project(id)?))
    }

    /// Git repositories directly inside `root` that are not registered yet, most recently committed first.
    /// Git repositories an owner may want to watch. A folder that is itself a repository
    /// comes first, followed by its neighbours; any other folder is searched two levels deep.
    /// Recently active repositories (and a chosen repository itself) are `recommended`.
    pub fn discover(&self, root: &str) -> Result<Value> {
        let base = resolve(root);
        if !base.is_dir() {
            return Err(LookoutError::new("path_missing", format!("Folder does not exist: {}", base.to_string_lossy())));
        }
        let folder_is_repo = base.join(".git").exists();
        let mut repos: Vec<PathBuf> = Vec::new();
        if folder_is_repo {
            repos.push(base.clone());
            if let Some(parent) = base.parent() {
                repos.extend(find_repos(parent, 1).into_iter().filter(|p| p != &base));
            }
        } else {
            std::fs::read_dir(&base).map_err(|e| LookoutError::new("path_unreadable", format!("Cannot read {}: {e}", base.to_string_lossy())))?;
            repos = find_repos(&base, DISCOVER_DEPTH);
        }
        let registered: Vec<String> = self.store.projects()?.into_iter().map(|p| p.path).collect();
        let today = self.today();
        let mut already_watched = 0;
        let mut found: Vec<(bool, Option<String>, Value)> = Vec::new();
        for repo in repos {
            let path = repo.to_string_lossy().into_owned();
            if registered.iter().any(|r| same_path(r, &path)) {
                already_watched += 1;
                continue;
            }
            let chosen = repo == base;
            let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let last = facts::last_commit_date(&path);
            let recent = last
                .as_deref()
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                .is_some_and(|d| (today - d).num_days() <= RECENT_DAYS);
            let recommended = if folder_is_repo { chosen } else { recent };
            found.push((chosen, last.clone(), json!({"path": path, "name": name, "suggested_id": slug(&name), "last_commit": last, "recommended": recommended})));
        }
        found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        Ok(json!({
            "folder": base.to_string_lossy(),
            "folder_is_repo": folder_is_repo,
            "candidates": found.into_iter().map(|(_, _, v)| v).collect::<Vec<_>>(),
            "already_watched": already_watched,
        }))
    }

    // ---------- facts ----------
    pub fn refresh_facts(&self, ids: Option<&[String]>, with_ci: bool) -> Result<Vec<Value>> {
        let projects: Vec<Project> = match ids {
            Some(ids) if !ids.is_empty() => ids.iter().map(|id| self.project(id)).collect::<Result<_>>()?,
            _ => self.store.projects()?.into_iter().filter(|p| p.status != "archived").collect(),
        };
        let today = self.today();
        let next = AtomicUsize::new(0);
        let results: Mutex<Vec<Option<Result<Value>>>> = Mutex::new(vec![None; projects.len()]);
        std::thread::scope(|s| {
            for _ in 0..REFRESH_WORKERS.min(projects.len()) {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(project) = projects.get(i) else { break };
                    let result = self.refresh_one(project, today, with_ci);
                    results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(result);
                });
            }
        });
        results.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().flatten().collect()
    }

    fn refresh_one(&self, project: &Project, today: NaiveDate, with_ci: bool) -> Result<Value> {
        let before = self.store.facts(&project.id)?;
        let mut data = (self.collect)(&project.path, today, with_ci);
        if !with_ci {
            if let Some(ci) = before.as_ref().and_then(|b| b.data.get("ci")).filter(|ci| ci["state"] != "skipped") {
                data["ci"] = ci.clone();
            }
        }
        let fp = facts::fingerprint(&data);
        self.store.save_facts(&project.id, &self.now(), &fp, &data)?;
        Ok(json!({
            "id": project.id,
            "ok": data["ok"].as_bool().unwrap_or(false),
            "error": data.get("error").cloned().unwrap_or(Value::Null),
            "changed": before.is_none_or(|b| b.fingerprint != fp),
        }))
    }

    fn stored_facts(&self, id: &str) -> Result<crate::store::StoredFacts> {
        if let Some(f) = self.store.facts(id)? {
            return Ok(f);
        }
        self.refresh_facts(Some(&[id.to_owned()]), true)?;
        self.store.facts(id)?.ok_or_else(|| LookoutError::new("facts_missing", format!("Could not measure '{id}'.")))
    }

    pub fn facts(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        let stored = self.stored_facts(id)?;
        Ok(extend(json!({"project": id, "collected_at": stored.collected_at}), stored.data))
    }

    // ---------- the agent's daily run ----------
    pub fn due(&self, max_age_days: i64, refresh: bool) -> Result<Value> {
        if refresh {
            self.refresh_facts(None, true)?;
        }
        let now = (self.clock)();
        let (mut projects, mut problems, mut up_to_date) = (Vec::new(), Vec::new(), Vec::new());
        let mut newest_card: Option<String> = None;
        for p in self.store.projects()? {
            if p.status == "archived" {
                continue;
            }
            let stored = self.store.facts(&p.id)?;
            let card = self.store.latest_card(&p.id)?;
            if let Some(c) = &card {
                if newest_card.as_ref().is_none_or(|n| c.created_at > *n) {
                    newest_card = Some(c.created_at.clone());
                }
            }
            if stored.as_ref().is_some_and(|s| s.data["error"] == "path_missing") {
                problems.push(json!({"id": p.id, "problem": "path_missing", "path": p.path}));
                continue;
            }
            let reason = match &card {
                None => Some("no_card"),
                Some(c) if stored.as_ref().is_some_and(|s| s.fingerprint != c.fingerprint) => Some("changed"),
                Some(c) if stored.as_ref().and_then(|s| settled_ci(&s.data)).is_some_and(|now| c.ci.as_deref().is_some_and(|then| then != now)) => {
                    Some("ci_changed")
                }
                Some(c) if p.status_changed_at.as_ref().is_some_and(|at| *at > c.created_at) => Some("status_changed"),
                Some(c) if (now - parse(&c.created_at)).num_days() >= max_age_days => Some("stale"),
                Some(_) => None,
            };
            match reason {
                Some(reason) => projects.push(json!({"id": p.id, "name": p.name, "reason": reason})),
                None => up_to_date.push(json!(p.id)),
            }
        }
        let portfolio_reason = match self.store.latest_portfolio()? {
            None => Some("missing"),
            Some(pf) if parse(&pf.created_at).with_timezone(&Local).date_naive() != self.today() => Some("new_day"),
            Some(pf) if newest_card.as_ref().is_some_and(|n| *n > pf.created_at) => Some("cards_changed"),
            Some(_) => None,
        };
        let any_due = !projects.is_empty();
        let next = if any_due || portfolio_reason.is_some() {
            "For each project listed: `lookout context <id>`, study the repository, then `lookout card put <id> --file <card.json>`. \
             Afterwards: `lookout portfolio context` and `lookout portfolio put --file <portfolio.json>`."
        } else {
            "Nothing is due. Every card and the portfolio brief are current."
        };
        Ok(json!({
            "today": self.today().to_string(),
            "projects": projects,
            "up_to_date": up_to_date,
            "problems": problems,
            "portfolio": {
                "due": portfolio_reason.is_some() || any_due,
                "reason": portfolio_reason.or(any_due.then_some("cards_will_change")),
            },
            "next": next,
        }))
    }

    pub fn context(&self, id: &str) -> Result<Value> {
        let project = self.project(id)?;
        let stored = self.stored_facts(id)?;
        let card = self.store.latest_card(id)?;
        let since = card.as_ref().map(|c| {
            let (count, commits) = match &c.head {
                Some(head) => (
                    json!(facts::count_since(&project.path, head)),
                    json!(facts::commits_since(&project.path, head, COMMITS_SINCE_LIMIT)),
                ),
                None => (Value::Null, Value::Null),
            };
            json!({"days": ((self.clock)() - parse(&c.created_at)).num_days(), "commit_count": count, "commits": commits})
        });
        let feedback: Vec<Value> = self
            .store
            .feedback()?
            .into_iter()
            .filter(|f| f.project_id.as_deref() == Some(id))
            .map(|f| json!({"id": f.item_id, "title": f.title, "value": f.value, "at": f.at}))
            .collect();
        let next = format!(
            "Write the card with `lookout card put {id} --file <card.json>`{}",
            if card.is_some() { " (add --merge to send only the fields that changed)." } else { "." }
        );
        Ok(json!({
            "today": self.today().to_string(),
            "project": pick(&project_json(&project), &["id", "name", "path", "status", "status_note"]),
            "facts": extend(json!({"collected_at": stored.collected_at}), stored.data),
            "previous_card": card.map(|c| json!({"version": c.version, "created_at": c.created_at, "card": c.data})),
            "since_previous_card": since,
            "feedback": feedback,
            "next": next,
        }))
    }

    fn merged(previous: Option<&Value>, payload: Value, merge: bool, what: &str) -> Result<Value> {
        if !merge {
            return Ok(payload);
        }
        let Some(previous) = previous else {
            return Err(LookoutError::new("nothing_to_merge", format!("There is no previous {what} to merge into."))
                .hint(format!("Send a complete {what} without --merge.")));
        };
        Ok(if payload.is_object() { extend(previous.clone(), payload) } else { payload })
    }

    pub fn put_card(&self, id: &str, payload: Value, merge: bool) -> Result<Value> {
        self.project(id)?;
        let previous = self.store.latest_card(id)?;
        let payload = Self::merged(previous.as_ref().map(|c| &c.data), payload, merge, "card")?;
        let (clean, warnings) = cards::validate(Kind::Card, &payload).map_err(|problems| {
            LookoutError::new("invalid_card", format!("The card was not saved: {}", problems.join("; ")))
                .hint("Fix the listed fields and send the card again. `lookout schema card` prints the full schema.")
        })?;
        let stored = self.stored_facts(id)?;
        let version = self.store.add_card(id, &self.now(), &stored.fingerprint, stored.data["head"].as_str(), settled_ci(&stored.data), &clean)?;
        let rated: Vec<Value> = clean["suggestions"].as_array().into_iter().flatten().map(|s| pick(s, &["id", "title"])).collect();
        Ok(json!({"saved": true, "project": id, "version": version, "warnings": warnings, "rated_items": rated}))
    }

    pub fn card(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        let card = self.store.latest_card(id)?.ok_or_else(|| {
            LookoutError::new("no_card", format!("Project '{id}' has no card yet.")).hint(format!("Run `lookout context {id}` and write one."))
        })?;
        Ok(json!({"project": id, "version": card.version, "created_at": card.created_at, "card": card.data}))
    }

    pub fn portfolio_context(&self) -> Result<Value> {
        let mut rows = Vec::new();
        for p in self.store.projects()? {
            if p.status == "archived" {
                continue;
            }
            let f = self.store.facts(&p.id)?.map(|s| s.data).unwrap_or_else(|| json!({}));
            let card = self.store.latest_card(&p.id)?;
            let card_json = card.map(|card| {
                let c = &card.data;
                let titles = |key: &str| -> Vec<Value> { c[key].as_array().into_iter().flatten().map(|x| x["title"].clone()).collect() };
                json!({
                    "updated_at": card.created_at, "one_liner": c["one_liner"], "phase": c["phase"], "headline": c["headline"],
                    "reentry": c["reentry"], "waiting_on_you": titles("waiting_on_you"), "open_ends": titles("open_ends"),
                    "suggestions": c["suggestions"].as_array().into_iter().flatten().map(|s| pick(s, &["id", "type", "title"])).collect::<Vec<_>>(),
                    "cross_project_hints": c["cross_project_hints"],
                })
            });
            rows.push(json!({
                "id": p.id, "name": p.name, "status": p.status, "status_note": p.status_note,
                "facts": {
                    "last_work": f["last_work"],
                    "commits": pick(&f["commits"], &["total", "last_7d", "last_30d", "last"]),
                    "has_remote": f["has_remote"], "ahead": f["ahead"],
                    "dirty": pick(&f["dirty"], &["files", "lines_added", "lines_removed", "oldest_change"]),
                    "ci": f["ci"]["state"],
                    "unmerged_branches": f["branches"]["unmerged"].as_array().map_or(0, Vec::len),
                    "extra_worktrees": f["worktrees"]["count"],
                },
                "card": card_json,
            }));
        }
        let previous = self.store.latest_portfolio()?;
        let feedback: Vec<Value> = self
            .store
            .feedback()?
            .into_iter()
            .map(|f| json!({"id": f.item_id, "project": f.project_id, "title": f.title, "value": f.value, "at": f.at}))
            .collect();
        Ok(json!({
            "today": self.today().to_string(),
            "projects": rows,
            "previous_portfolio": previous.map(|p| json!({"version": p.version, "created_at": p.created_at, "portfolio": p.data})),
            "feedback": feedback,
            "next": "Write the brief with `lookout portfolio put --file <portfolio.json>`.",
        }))
    }

    pub fn put_portfolio(&self, payload: Value, merge: bool) -> Result<Value> {
        let previous = self.store.latest_portfolio()?;
        let payload = Self::merged(previous.as_ref().map(|p| &p.data), payload, merge, "portfolio brief")?;
        let (clean, warnings) = cards::validate(Kind::Portfolio, &payload).map_err(|problems| {
            LookoutError::new("invalid_portfolio", format!("The brief was not saved: {}", problems.join("; ")))
                .hint("Fix the listed fields and send it again. `lookout schema portfolio` prints the full schema.")
        })?;
        let known: Vec<String> = self.store.projects()?.into_iter().map(|p| p.id).collect();
        let items = |key: &str| clean[key].as_array().cloned().unwrap_or_default();
        let mut refs: Vec<String> = Vec::new();
        for item in items("focus").iter().chain(items("park_or_revive").iter()) {
            refs.extend(item["project"].as_str().map(str::to_owned));
        }
        for item in items("patterns").iter().chain(items("synergies").iter()) {
            refs.extend(item["projects"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)));
        }
        let mut unknown: Vec<String> = refs.into_iter().filter(|r| !known.contains(r)).collect();
        unknown.sort();
        unknown.dedup();
        if !unknown.is_empty() {
            let mut known = known;
            known.sort();
            return Err(LookoutError::new("unknown_project", format!("The brief names unknown project ids: {}.", unknown.join(", ")))
                .hint(format!("Use the ids from `lookout portfolio context`: {}.", known.join(", "))));
        }
        let version = self.store.add_portfolio(&self.now(), &clean)?;
        let mut rated = Vec::new();
        for (key, _) in Kind::Portfolio.rated() {
            for item in items(key) {
                let title = if item["title"].is_string() { item["title"].clone() } else { item["project"].clone() };
                rated.push(json!({"id": item["id"], "title": title}));
            }
        }
        Ok(json!({"saved": true, "version": version, "warnings": warnings, "rated_items": rated}))
    }

    pub fn portfolio(&self) -> Result<Value> {
        let p = self.store.latest_portfolio()?.ok_or_else(|| {
            LookoutError::new("no_portfolio", "No portfolio brief has been written yet.").hint("Run `lookout portfolio context` and write one.")
        })?;
        Ok(json!({"version": p.version, "created_at": p.created_at, "portfolio": p.data}))
    }

    // ---------- feedback and preferences ----------
    pub fn set_feedback(&self, item_id: &str, value: Option<&str>, project_id: Option<&str>, title: &str) -> Result<Value> {
        if let Some(v) = value {
            if !FEEDBACK.contains(&v) {
                return Err(LookoutError::new("invalid_feedback", format!("Unknown rating '{v}'."))
                    .hint(format!("Use one of: {}.", FEEDBACK.join(", "))));
            }
        }
        self.store.set_feedback(item_id, project_id, title, value, &self.now())?;
        Ok(json!({"item_id": item_id, "value": value}))
    }

    pub fn feedback(&self, project_id: Option<&str>) -> Result<Value> {
        Ok(Value::Array(
            self.store
                .feedback()?
                .into_iter()
                .filter(|f| project_id.is_none() || f.project_id.as_deref() == project_id)
                .map(|f| json!({"id": f.item_id, "project": f.project_id, "title": f.title, "value": f.value, "at": f.at}))
                .collect(),
        ))
    }

    pub fn set_pref(&self, key: &str, value: &Value) -> Result<()> {
        if !PREFS.contains(&key) {
            return Err(LookoutError::new("invalid_pref", format!("Unknown preference '{key}'.")));
        }
        Ok(self.store.set_pref(key, value)?)
    }

    // ---------- the window ----------
    /// Everything the window renders. A visit more than an hour after the last one
    /// moves the "new since your last visit" line forward.
    pub fn state(&self, visit: bool) -> Result<Value> {
        let now = (self.clock)();
        let last = self.store.pref("last_visit")?.filter(|v| v.is_string());
        let mut previous = match self.store.pref("previous_visit")? {
            Some(v) => v,
            None => last.clone().unwrap_or(Value::Null),
        };
        if visit {
            let long_ago = last.as_ref().and_then(Value::as_str).is_none_or(|l| now - parse(l) > chrono::Duration::hours(1));
            if long_ago {
                previous = last.clone().unwrap_or(Value::Null);
                self.store.set_pref("previous_visit", &previous)?;
            }
            self.store.set_pref("last_visit", &json!(stamp(now)))?;
        }
        let mut projects = Vec::new();
        for p in self.store.projects()? {
            let stored = self.store.facts(&p.id)?;
            let card = self.store.latest_card(&p.id)?;
            let outdated = matches!((&card, &stored), (Some(c), Some(s)) if c.fingerprint != s.fingerprint);
            projects.push(extend(
                pick(&project_json(&p), &["id", "name", "path", "status", "status_note"]),
                json!({
                    "facts": stored.as_ref().map(|s| s.data.clone()),
                    "facts_collected_at": stored.map(|s| s.collected_at),
                    "card": card.as_ref().map(|c| c.data.clone()),
                    "card_updated_at": card.map(|c| c.created_at),
                    "card_outdated": outdated,
                }),
            ));
        }
        let portfolio = self.store.latest_portfolio()?;
        let feedback: Map<String, Value> =
            self.store.feedback()?.into_iter().map(|f| (f.item_id, json!({"value": f.value, "at": f.at}))).collect();
        Ok(json!({
            "today": self.today().to_string(),
            "now": stamp(now),
            "previous_visit": previous,
            "projects": projects,
            "portfolio": portfolio.as_ref().map(|p| p.data.clone()),
            "portfolio_updated_at": portfolio.map(|p| p.created_at),
            "feedback": feedback,
            "snooze_days": SNOOZE_DAYS,
            "prefs": {
                "style": self.store.pref("style")?.unwrap_or_else(|| json!("handover")),
                "hidden_modules": self.store.pref("hidden_modules")?.unwrap_or_else(|| json!([])),
            },
        }))
    }
}
