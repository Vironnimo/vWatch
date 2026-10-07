//! Watchtower's single owner of behavior, used by both the app window and the CLI.
//!
//! Owns project registration, fact refreshes, deciding which projects need a new
//! interpretation, the context an agent reads before writing a card, card and
//! portfolio writes, feedback, and the complete state the window renders.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use serde_json::{json, Map, Value};

use crate::cards::{self, Kind};
use crate::facts;
use crate::store::{
    CardBasis, Feedback, Idea, Project, Store, StoredCard, StoredFacts, StoredPortfolio,
};

pub const STATUSES: &[&str] = &["active", "parked", "done", "archived"];
/// How the owner rates suggestions, patterns, synergies, app ideas and verdicts.
pub const RATINGS: &[&str] = &["up", "later", "down"];
pub const PREFS: &[&str] = &["style", "hidden_modules"];
pub const SNOOZE_DAYS: i64 = 14;
pub const DEFAULT_MAX_AGE_DAYS: i64 = 14;
/// Opening a project at least this many days after the last time shows what happened since.
const SINCE_DAYS: i64 = 7;
/// Longest parked idea, in characters.
const IDEA_MAX: usize = 600;
/// The card fields the portfolio brief builds on; changes elsewhere (the handbook, features) leave it current.
const BRIEF_FIELDS: &[&str] = &[
    "one_liner",
    "phase",
    "headline",
    "reentry",
    "waiting_on_you",
    "open_ends",
    "health_notes",
    "decisions",
    "suggestions",
    "cross_project_hints",
];
const COMMITS_SINCE_LIMIT: usize = 150;
const REFRESH_WORKERS: usize = 4;
const DISCOVER_DEPTH: usize = 2;
/// Repositories with a commit in this many days are recommended when discovering.
const RECENT_DAYS: i64 = 120;

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
        WatchtowerError::new(
            "storage_error",
            format!("Watchtower's database failed: {e}"),
        )
        .hint("Try again in a moment.")
    }
}

pub type Result<T> = std::result::Result<T, WatchtowerError>;
pub type Clock = Box<dyn Fn() -> DateTime<Utc> + Send + Sync>;
/// Measures one repository: (path, today, with_ci) -> facts.
pub type Collector = Box<dyn Fn(&str, NaiveDate, bool) -> Value + Send + Sync>;

pub struct Watchtower {
    store: Store,
    clock: Clock,
    collect: Collector,
}

/// Semantic identities keep regenerated item ids and advancing age counters out
/// of the owner's unread list. The visible text remains exactly as authored.
fn digest_text(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn digest_identity(category: &str, item: &Value) -> String {
    format!(
        "{category}:{}:{}",
        digest_text(&item["title"]),
        digest_text(&item["source"])
    )
}

/// An unavailable observation cannot erase the last known risk when the owner
/// acknowledges other changes. The window still renders the actual facts.
fn digest_measurements(current: &Value, baseline: Option<&Value>) -> Value {
    let mut current = current.clone();
    let Some(previous) = baseline.and_then(|b| b["snapshot"]["projects"].as_array()) else {
        return current;
    };
    for project in current["projects"].as_array_mut().into_iter().flatten() {
        let Some(old) = previous.iter().find(|p| p["id"] == project["id"]) else {
            continue;
        };
        if project["facts"]["ok"] != true {
            project["facts"] = old["facts"].clone();
        } else if !matches!(
            project["facts"]["ci"]["state"].as_str(),
            Some("green" | "red" | "none")
        ) {
            project["facts"]["ci"] = old["facts"]["ci"].clone();
        }
    }
    current
}

struct DigestItem {
    category: &'static str,
    title: String,
    detail: String,
    next_step: String,
    comparison: Value,
}

fn digest_items(
    project: &Value,
    feedback: &[Value],
) -> std::collections::BTreeMap<String, DigestItem> {
    let mut items = std::collections::BTreeMap::new();
    let facts = &project["facts"];
    for warning in fact_warnings(facts) {
        let key = warning["key"].as_str().unwrap_or("");
        let title = warning["title"].as_str().unwrap_or("").to_owned();
        // A new HEAD is not a new warning. Worktree identities and dirtiness are
        // meaningful; elapsed age and collection times never are.
        let comparison = if key == "worktrees" {
            warning["basis"].clone()
        } else {
            json!(title)
        };
        items.insert(
            format!("risk:{key}"),
            DigestItem {
                category: "risk",
                title,
                detail: String::new(),
                next_step: "Projekt prüfen und nächsten Schritt festlegen.".into(),
                comparison,
            },
        );
    }
    for (key, count, title, step) in [
        (
            "dirty",
            facts["dirty"]["files"].as_u64().unwrap_or(0),
            "Dateien nicht committet",
            "Offene Arbeit prüfen und einen Auftrag daraus machen.",
        ),
        (
            "ahead",
            facts["ahead"].as_u64().unwrap_or(0),
            "Commits nicht gepusht",
            "Lokale Commits prüfen und bei Bedarf sichern lassen.",
        ),
        (
            "branches",
            facts["branches"]["unmerged"]
                .as_array()
                .map_or(0, |v| v.len() as u64),
            "Branches nicht gemergt",
            "Offene Branches prüfen und das weitere Vorgehen festlegen.",
        ),
    ] {
        if count > 0 {
            items.insert(
                format!("risk:{key}"),
                DigestItem {
                    category: "risk",
                    title: format!("{count} {title}"),
                    detail: String::new(),
                    next_step: step.into(),
                    comparison: json!(count),
                },
            );
        }
    }
    let card = &project["card"];
    for item in card["open_ends"].as_array().into_iter().flatten() {
        let detail = item["detail"].as_str().unwrap_or("").to_owned();
        items.insert(
            digest_identity("open", item),
            DigestItem {
                category: "open",
                title: item["title"].as_str().unwrap_or("").into(),
                detail,
                next_step: "Offenes Ende im Projekt prüfen oder als Auftrag übernehmen.".into(),
                comparison: json!({"kind": item["kind"], "detail": digest_text(&item["detail"])}),
            },
        );
    }
    for item in card["waiting_on_you"].as_array().into_iter().flatten() {
        let answered = feedback.iter().any(|f| {
            f["project"] == project["id"]
                && f["id"] == item["id"]
                && matches!(f["value"].as_str(), Some("answered" | "done"))
        });
        if !answered {
            items.insert(
                digest_identity("question", item),
                DigestItem {
                    category: "question",
                    title: item["title"].as_str().unwrap_or("").into(),
                    detail: item["source"].as_str().unwrap_or("").into(),
                    next_step: "Frage im Projekt beantworten.".into(),
                    comparison: item["options"].clone(),
                },
            );
        }
    }
    for item in card["recent_changes"].as_array().into_iter().flatten() {
        items.insert(
            digest_identity("change", item),
            DigestItem {
                category: "change",
                title: item["title"].as_str().unwrap_or("").into(),
                detail: item["detail"].as_str().unwrap_or("").into(),
                next_step: String::new(),
                comparison: json!(digest_text(&item["detail"])),
            },
        );
    }
    for decision in card["decisions"].as_array().into_iter().flatten() {
        items.insert(
            format!("decision:{}", digest_text(decision)),
            DigestItem {
                category: "decision",
                title: decision.as_str().unwrap_or("").into(),
                detail: "In der aktuellen Karte dokumentiert.".into(),
                next_step: String::new(),
                comparison: Value::Null,
            },
        );
    }
    for feedback in feedback.iter().filter(|f| {
        f["project"] == project["id"] && f["value"] == "answered" && f["item"]["kind"] == "question"
    }) {
        let Some(id) = feedback["id"].as_str() else {
            continue;
        };
        let applied = &project["owner_basis"]["questions"][id];
        if applied["value"] != feedback["value"] || applied["note"] != feedback["note"] {
            continue;
        }
        let key = format!(
            "applied:{}:{}:{}",
            digest_text(&feedback["title"]),
            digest_text(&feedback["item"]["source"]),
            digest_text(&feedback["note"])
        );
        items.insert(
            key,
            DigestItem {
                category: "applied",
                title: feedback["title"].as_str().unwrap_or("").into(),
                detail: feedback["note"].as_str().unwrap_or("").into(),
                next_step: "Aktuelle Karte lesen.".into(),
                comparison: Value::Null,
            },
        );
    }
    items
}

fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn parse(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_default()
}

fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
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
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Git repositories below `dir`, at most `depth` levels down; never looks inside a repository.
fn find_repos(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            !name.starts_with('.')
                && !["node_modules", "target", "$recycle.bin"].contains(&name.as_str())
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

/// The reactions the owner can give to an item of this kind.
fn reactions_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "question" => &["answered", "done"],
        "focus" => &["done"],
        _ => RATINGS,
    }
}

fn kind_of(f: &Feedback) -> &str {
    f.item
        .as_ref()
        .and_then(|i| i["kind"].as_str())
        .unwrap_or("")
}

/// A short label for an item: its title, or for a verdict the project and the verdict.
fn title_of(item: &Value) -> String {
    match item["title"].as_str() {
        Some(t) => t.to_owned(),
        None => format!(
            "{}: {}",
            item["project"].as_str().unwrap_or(""),
            item["verdict"].as_str().unwrap_or("")
        ),
    }
}

/// The ids Watchtower keeps for a saved document's items, for the agent's reply.
/// The items of a dated list (`date`, `title`) from `from` on.
fn dated_since(list: &Value, from: &str) -> Vec<Value> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["date"].as_str().is_some_and(|d| d >= from))
        .map(|i| pick(i, &["date", "title"]))
        .collect()
}

/// Whether a handbook command reads like something other than what the owner types:
/// commands quoted in a sentence, a URL behind a command, or a shortcut to click.
fn not_typed(how: &str) -> bool {
    let lower = how.to_lowercase();
    how.contains(['`', '„', '“'])
        || how.contains("; http")
        || [
            "startmen",
            "start-men",
            "start menu",
            "doppelklick",
            "double-click",
        ]
        .iter()
        .any(|w| lower.contains(w))
}

fn idea_json(i: &Idea) -> Value {
    json!({"id": i.id, "text": i.text, "added_at": i.added_at})
}

fn item_ids(kind: Kind, doc: &Value) -> Vec<Value> {
    kind.items()
        .iter()
        .flat_map(|(list, _, item_kind)| {
            doc[*list]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |i| json!({"id": i["id"], "kind": item_kind, "title": title_of(i)}))
        })
        .collect()
}

/// The top-level fields whose content differs from the previous version (all of them for a first version).
fn changed_fields(kind: Kind, previous: Option<&Value>, clean: &Value) -> Vec<String> {
    // compare against the previous version with today's defaults filled in, so fields the schema
    // gained since do not count as changes
    let previous = previous
        .map(|p| cards::validate(kind, p, &top_fields(p)).map_or_else(|_| p.clone(), |(c, _)| c));
    let previous = previous.as_ref();
    let map = clean.as_object().into_iter().flatten();
    map.filter(|(k, v)| previous.is_none_or(|p| p.get(k.as_str()) != Some(v)))
        .map(|(k, _)| k.clone())
        .collect()
}

fn top_fields(doc: &Value) -> Vec<String> {
    doc.as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// With --merge, the previous version's fields the agent did not send: they are kept unchanged.
fn kept_fields(previous: Option<&Value>, sent: Option<&[String]>) -> Vec<String> {
    match (previous, sent) {
        (Some(previous), Some(sent)) => top_fields(previous)
            .into_iter()
            .filter(|f| !sent.contains(f))
            .collect(),
        _ => Vec::new(),
    }
}

/// With --merge, the top-level fields the agent actually sent.
fn sent_fields(payload: &Value, merge: bool) -> Option<Vec<String>> {
    merge.then(|| {
        payload
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    })
}

/// Validation problems, each marked when its field came from the previous version rather than the agent.
fn mark_inherited(problems: &[String], sent: Option<&[String]>) -> String {
    problems
        .iter()
        .map(|p| {
            let field = p.split(['.', '[', ':']).next().unwrap_or("");
            match sent {
                Some(sent) if !sent.iter().any(|s| s == field) => {
                    format!("{p} (from the previous card)")
                }
                _ => p.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A dry run's items: new items show `id: null`, because their id is only assigned when saved.
fn dry_items(kind: Kind, sent: &Value, clean: &Value) -> Vec<Value> {
    let given: Vec<&Value> = kind
        .items()
        .iter()
        .flat_map(|(list, _, _)| {
            sent[*list]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| &i["id"])
        })
        .collect();
    let mut seen = Vec::new();
    item_ids(kind, clean)
        .into_iter()
        .map(|mut item| {
            let kept = given.contains(&&item["id"]) && !seen.contains(&item["id"]);
            seen.push(item["id"].clone());
            if !kept {
                item["id"] = Value::Null;
            }
            item
        })
        .collect()
}

fn feedback_json(f: &Feedback) -> Value {
    json!({
        "id": f.item_id, "project": f.project_id, "value": f.value, "note": f.note, "at": f.at,
        "snoozed_until": snoozed_until(f),
        "item": f.item.clone().unwrap_or_else(|| json!({"title": f.title})),
    })
}

fn snoozed_until(f: &Feedback) -> Option<String> {
    (f.value == "later").then(|| {
        f.snoozed_until.clone().unwrap_or_else(|| {
            (parse(&f.at).with_timezone(&Local).date_naive() + chrono::Duration::days(SNOOZE_DAYS))
                .to_string()
        })
    })
}

/// Measured warnings independent of the owner's decisions or the observation time.
pub(crate) fn fact_warnings(facts: &Value) -> Vec<Value> {
    if facts["ok"] != true {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    let warning = |key: &str, severity: &str, title: &str, basis: Value, can_acknowledge: bool| {
        json!({"key": key, "kind": key, "severity": severity, "title": title, "basis": basis,
            "can_acknowledge": can_acknowledge})
    };
    if facts["has_remote"] == false {
        warnings.push(warning(
            "no_remote",
            "risk",
            "Arbeit liegt nur lokal",
            json!({
                "head": facts["head"], "commits": facts["commits"]["total"],
                "dirty": pick(&facts["dirty"], &["files", "lines_added", "lines_removed"]),
            }),
            true,
        ));
    }
    if facts["ci"]["state"] == "none" {
        warnings.push(warning(
            "ci_none",
            "info",
            "Keine CI eingerichtet",
            json!({"state": "none"}),
            true,
        ));
    } else if facts["ci"]["state"] == "red" {
        warnings.push(warning(
            "ci_red",
            "risk",
            "CI ist fehlgeschlagen",
            json!({
                "state": "red", "failing": facts["ci"]["failing"],
            }),
            false,
        ));
    }
    let mut worktrees: Vec<Value> = facts["worktrees"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            pick(
                item,
                &[
                    "path",
                    "head",
                    "dirty_files",
                    "dirty_fingerprint",
                    "prunable",
                ],
            )
        })
        .collect();
    if !worktrees.is_empty() {
        worktrees.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        let severity = if worktrees.iter().any(|w| w["prunable"] == true) {
            "warn"
        } else {
            "info"
        };
        warnings.push(warning(
            "worktrees",
            severity,
            "Weitere Arbeitsverzeichnisse vorhanden",
            json!({"items": worktrees}),
            true,
        ));
    }
    warnings
}

fn warning_ack_applies(key: &str, before: &Value, now: &Value) -> bool {
    match key {
        "no_remote" => {
            before["head"] == now["head"]
                && now["commits"].as_u64().unwrap_or(0) <= before["commits"].as_u64().unwrap_or(0)
                && ["files", "lines_added", "lines_removed"]
                    .iter()
                    .all(|field| {
                        now["dirty"][*field].as_u64().unwrap_or(0)
                            <= before["dirty"][*field].as_u64().unwrap_or(0)
                    })
        }
        "worktrees" => now["items"]
            .as_array()
            .into_iter()
            .flatten()
            .all(|current| {
                before["items"].as_array().into_iter().flatten().any(|old| {
                    ["path", "head", "dirty_files", "dirty_fingerprint"]
                        .iter()
                        .all(|field| old[*field] == current[*field])
                        && (current["prunable"] != true || old["prunable"] == true)
                })
            }),
        "ci_none" => before == now,
        _ => false,
    }
}

/// The owner's ratings counted per kind and type of item: which directions they tend to take up or reject.
fn feedback_summary(reactions: &[Feedback]) -> Value {
    let mut counts: std::collections::BTreeMap<(String, String), [u32; 3]> = Default::default();
    for f in reactions {
        let Some(slot) = RATINGS.iter().position(|r| *r == f.value) else {
            continue;
        };
        let item = f.item.clone().unwrap_or_default();
        let ty = item["type"]
            .as_str()
            .or(item["verdict"].as_str())
            .unwrap_or("")
            .to_owned();
        counts.entry((kind_of(f).to_owned(), ty)).or_default()[slot] += 1;
    }
    let text = |s: String| {
        if s.is_empty() {
            Value::Null
        } else {
            Value::String(s)
        }
    };
    counts
        .into_iter()
        .map(|((kind, ty), [up, later, down])| json!({"kind": text(kind), "type": text(ty), "up": up, "later": later, "down": down}))
        .collect()
}

/// A CI state firm enough to compare: `green` or `red`. Running, unknown or skipped
/// lookups say nothing about whether a card's CI statement still holds.
fn settled_ci(facts: &Value) -> Option<&str> {
    facts["ci"]["state"]
        .as_str()
        .filter(|s| matches!(*s, "green" | "red"))
}

fn ci_changed(card: &StoredCard, facts: &Value) -> bool {
    settled_ci(facts).is_some_and(|now| match card.ci.as_deref() {
        Some(then) => then != now,
        None => now == "red",
    })
}

fn owner_changes(
    card: &StoredCard,
    project: &Project,
    reactions: &[Feedback],
    warning_acks: &Value,
    tasks: &Value,
) -> (bool, bool) {
    match &card.owner_basis {
        Some(before) => {
            let owner = owner_basis(project, reactions, warning_acks, tasks);
            (
                before["questions"] != owner["questions"],
                before["project"] != owner["project"],
            )
        }
        None => (
            reactions.iter().any(|f| {
                f.project_id.as_deref() == Some(&project.id)
                    && kind_of(f) == "question"
                    && f.at > card.created_at
            }),
            project
                .status_changed_at
                .as_ref()
                .is_some_and(|at| *at > card.created_at)
                || warning_acks.as_array().is_some_and(|acks| !acks.is_empty())
                || tasks.as_array().is_some_and(|tasks| !tasks.is_empty()),
        ),
    }
}

fn card_outdated(
    card: Option<&StoredCard>,
    facts: Option<&StoredFacts>,
    project: &Project,
    reactions: &[Feedback],
    warning_acks: &Value,
    tasks: &Value,
) -> bool {
    card.is_some_and(|card| {
        let (answered, status_changed) =
            owner_changes(card, project, reactions, warning_acks, tasks);
        answered
            || status_changed
            || facts.is_some_and(|facts| {
                facts.fingerprint != card.fingerprint || ci_changed(card, &facts.data)
            })
    })
}

fn resolve(path: &str) -> PathBuf {
    let expanded = match path.strip_prefix('~') {
        Some(rest) => {
            let home = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .unwrap_or_default();
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

/// Content, rather than timestamps, keeps owner edits within one clock tick visible.
fn owner_basis(p: &Project, reactions: &[Feedback], warning_acks: &Value, tasks: &Value) -> Value {
    let questions: Map<String, Value> = reactions
        .iter()
        .filter(|f| f.project_id.as_deref() == Some(&p.id) && kind_of(f) == "question")
        .map(|f| (f.item_id.clone(), json!({"value": f.value, "note": f.note})))
        .collect();
    let mut project = pick(
        &project_json(p),
        &["status", "status_note", "goal", "goal_due"],
    );
    if warning_acks.as_array().is_some_and(|acks| !acks.is_empty()) {
        project["warning_acks"] = warning_acks.clone();
    }
    if tasks.as_array().is_some_and(|tasks| !tasks.is_empty()) {
        project["tasks"] = tasks.clone();
    }
    json!({
        "project": project,
        "questions": questions,
    })
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
            Box::new(facts::collect),
        );
        svc.give_items_ids()?;
        Ok(svc)
    }

    /// Cards and briefs written before questions and focus items had ids get them,
    /// so the owner can answer those items too.
    fn give_items_ids(&self) -> Result<()> {
        let missing = |kind: Kind, doc: &Value| {
            kind.items().iter().any(|(list, _, _)| {
                doc[*list]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|i| !i["id"].is_string()))
            })
        };
        for p in self.store.projects()? {
            if let Some(card) = self
                .store
                .latest_card(&p.id)?
                .filter(|c| missing(Kind::Card, &c.data))
            {
                if let Ok((clean, _)) =
                    cards::validate(Kind::Card, &card.data, &top_fields(&card.data))
                {
                    self.store.rewrite_latest_card(&p.id, &clean)?;
                }
            }
        }
        if let Some(pf) = self
            .store
            .latest_portfolio()?
            .filter(|p| missing(Kind::Portfolio, &p.data))
        {
            if let Ok((clean, _)) =
                cards::validate(Kind::Portfolio, &pf.data, &top_fields(&pf.data))
            {
                self.store.rewrite_latest_portfolio(&clean)?;
            }
        }
        Ok(())
    }

    pub fn with_parts(store: Store, clock: Clock, collect: Collector) -> Watchtower {
        Watchtower {
            store,
            clock,
            collect,
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

    // ---------- projects ----------
    fn project(&self, id: &str) -> Result<Project> {
        if let Some(p) = self.store.project(id)? {
            return Ok(p);
        }
        let known: Vec<String> = self.store.projects()?.into_iter().map(|p| p.id).collect();
        let known = if known.is_empty() {
            "none yet".to_owned()
        } else {
            known.join(", ")
        };
        Err(
            WatchtowerError::new("project_not_found", format!("No project with id '{id}'."))
                .hint(format!("Known project ids: {known}.")),
        )
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
            return Err(WatchtowerError::new(
                "path_missing",
                format!("Folder does not exist: {resolved_str}"),
            ));
        }
        if let Some(p) = self
            .store
            .projects()?
            .into_iter()
            .find(|p| same_path(&p.path, &resolved_str))
        {
            return Err(WatchtowerError::new(
                "already_registered",
                format!("This folder is already registered as '{}'.", p.id),
            ));
        }
        let folder = resolved
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| resolved_str.clone());
        let id = match id {
            Some(id) => id.to_owned(),
            None => {
                let s: String = slug(&folder).chars().take(40).collect();
                let s = s.trim_end_matches('-').to_owned();
                if s.is_empty() {
                    "project".to_owned()
                } else {
                    s
                }
            }
        };
        if !valid_project_id(&id) {
            return Err(
                WatchtowerError::new("invalid_id", format!("Invalid project id '{id}'."))
                    .hint("Use lowercase letters, digits and hyphens, at most 40 characters."),
            );
        }
        if self.store.project(&id)?.is_some() {
            return Err(WatchtowerError::new(
                "id_taken",
                format!("The id '{id}' is already used."),
            )
            .hint("Pass another id with --id."));
        }
        self.store
            .add_project(&id, name.unwrap_or(&folder), &resolved_str, &self.now())?;
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
            return Err(WatchtowerError::new(
                "invalid_name",
                "The name must not be empty.",
            ));
        }
        self.store.rename_project(id, name)?;
        Ok(project_json(&self.project(id)?))
    }

    /// The owner's goal for a project; an empty goal clears it. Like a status change, it makes the card due.
    pub fn set_goal(&self, id: &str, goal: &str, due: Option<&str>) -> Result<Value> {
        self.project(id)?;
        let due = due.filter(|d| !d.is_empty());
        if let Some(d) = due {
            if NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
                return Err(
                    WatchtowerError::new("invalid_date", format!("'{d}' is not a date."))
                        .hint("Write the due date as YYYY-MM-DD."),
                );
            }
        }
        let goal = goal.trim();
        self.store.set_goal(
            id,
            goal,
            if goal.is_empty() { None } else { due },
            &self.now(),
        )?;
        Ok(project_json(&self.project(id)?))
    }

    pub fn set_status(&self, id: &str, status: &str, note: Option<&str>) -> Result<Value> {
        let project = self.project(id)?;
        if !STATUSES.contains(&status) {
            return Err(WatchtowerError::new(
                "invalid_status",
                format!("Unknown status '{status}'."),
            )
            .hint(format!("Use one of: {}.", STATUSES.join(", "))));
        }
        self.store.set_status(
            id,
            status,
            note.unwrap_or(&project.status_note),
            &self.now(),
        )?;
        Ok(project_json(&self.project(id)?))
    }

    /// Git repositories directly inside `root` that are not registered yet, most recently committed first.
    /// Git repositories an owner may want to watch. A folder that is itself a repository
    /// comes first, followed by its neighbours; any other folder is searched two levels deep.
    /// Recently active repositories (and a chosen repository itself) are `recommended`.
    pub fn discover(&self, root: &str) -> Result<Value> {
        let base = resolve(root);
        if !base.is_dir() {
            return Err(WatchtowerError::new(
                "path_missing",
                format!("Folder does not exist: {}", base.to_string_lossy()),
            ));
        }
        let folder_is_repo = base.join(".git").exists();
        let mut repos: Vec<PathBuf> = Vec::new();
        if folder_is_repo {
            repos.push(base.clone());
            if let Some(parent) = base.parent() {
                repos.extend(find_repos(parent, 1).into_iter().filter(|p| p != &base));
            }
        } else {
            std::fs::read_dir(&base).map_err(|e| {
                WatchtowerError::new(
                    "path_unreadable",
                    format!("Cannot read {}: {e}", base.to_string_lossy()),
                )
            })?;
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
            let name = repo
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
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
            Some(ids) if !ids.is_empty() => ids
                .iter()
                .map(|id| self.project(id))
                .collect::<Result<_>>()?,
            _ => self
                .store
                .projects()?
                .into_iter()
                .filter(|p| p.status != "archived")
                .collect(),
        };
        let today = self.today();
        let next = AtomicUsize::new(0);
        let results: Mutex<Vec<Option<Result<Value>>>> = Mutex::new(vec![None; projects.len()]);
        std::thread::scope(|s| {
            for _ in 0..REFRESH_WORKERS.min(projects.len()) {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(project) = projects.get(i) else {
                        break;
                    };
                    let result = self.refresh_one(project, today, with_ci);
                    results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(result);
                });
            }
        });
        results
            .into_inner()
            .unwrap_or_else(|e| e.into_inner())
            .into_iter()
            .flatten()
            .collect()
    }

    fn refresh_one(&self, project: &Project, today: NaiveDate, with_ci: bool) -> Result<Value> {
        let before = self.store.facts(&project.id)?;
        let mut data = (self.collect)(&project.path, today, with_ci);
        if !with_ci {
            if let Some(ci) = before
                .as_ref()
                .and_then(|b| b.data.get("ci"))
                .filter(|ci| ci["state"] != "skipped")
            {
                data["ci"] = ci.clone();
            }
        }
        if data["ok"] == true {
            data["space"] = match before
                .as_ref()
                .map(|b| &b.data["space"])
                .filter(|s| s["measured_on"] == today.to_string())
            {
                Some(space) => space.clone(),
                None => facts::space(&project.path, today),
            };
        }
        let fp = facts::fingerprint(&data);
        self.store
            .save_facts(&project.id, &self.now(), &fp, &data)?;
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
        self.store.facts(id)?.ok_or_else(|| {
            WatchtowerError::new("facts_missing", format!("Could not measure '{id}'."))
        })
    }

    pub fn facts(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        let stored = self.stored_facts(id)?;
        Ok(extend(
            json!({"project": id, "collected_at": stored.collected_at}),
            stored.data,
        ))
    }

    // ---------- the agent's daily run ----------
    pub fn due(&self, max_age_days: i64, refresh: bool) -> Result<Value> {
        if refresh {
            self.refresh_facts(None, true)?;
        }
        let now = (self.clock)();
        let (mut projects, mut problems, mut up_to_date) = (Vec::new(), Vec::new(), Vec::new());
        let brief = self.store.latest_portfolio()?;
        let mut cards_changed = match brief.as_ref().and_then(|p| p.basis.as_ref()) {
            Some(basis) => *basis != self.portfolio_basis()?,
            None => false,
        };
        let reactions = self.store.feedback()?;
        for p in self.store.projects()? {
            if p.status == "archived" {
                continue;
            }
            let stored = self.store.facts(&p.id)?;
            let card = self.store.latest_card(&p.id)?;
            if let (Some(pf), Some(c)) = (&brief, &card) {
                if pf.basis.is_none() {
                    cards_changed = cards_changed
                        || self
                            .brief_changes(&p.id, &c.data, pf)?
                            .is_none_or(|f| !f.is_empty());
                }
            }
            let warning_acks = self.warning_acknowledgements(&p.id)?;
            let tasks = self.project_task_basis(&p.id)?;
            let (answered, status_changed) = card
                .as_ref()
                .map(|c| owner_changes(c, &p, &reactions, &warning_acks, &tasks))
                .unwrap_or_default();
            if stored
                .as_ref()
                .is_some_and(|s| s.data["error"] == "path_missing")
            {
                problems.push(json!({"id": p.id, "problem": "path_missing", "path": p.path}));
                continue;
            }
            let reason = match &card {
                None => Some("no_card"),
                Some(c)
                    if stored
                        .as_ref()
                        .is_some_and(|s| s.fingerprint != c.fingerprint) =>
                {
                    Some("changed")
                }
                // a card written while CI was unsettled is due once CI turns red
                Some(c) if stored.as_ref().is_some_and(|s| ci_changed(c, &s.data)) => {
                    Some("ci_changed")
                }
                Some(_) if answered => Some("answered"),
                Some(_) if status_changed => Some("status_changed"),
                Some(c) if (now - parse(&c.created_at)).num_days() >= max_age_days => Some("stale"),
                Some(_) => None,
            };
            // cards written before Watchtower added a field lack it; the next card fills it
            let new_fields = card
                .as_ref()
                .map(|c| Kind::Card.missing_fields(&c.data))
                .unwrap_or_default();
            let reason = reason.or((!new_fields.is_empty()).then_some("new_fields"));
            let mut entry = match reason {
                Some("changed") => {
                    let changes = match (card.as_ref().and_then(|c| c.basis.as_ref()), &stored) {
                        (Some(before), Some(now)) => {
                            json!(facts::changes(before, &facts::basis(&now.data)))
                        }
                        _ => Value::Null,
                    };
                    json!({"id": p.id, "name": p.name, "reason": "changed", "changes": changes})
                }
                Some(reason) => json!({"id": p.id, "name": p.name, "reason": reason}),
                None => {
                    up_to_date.push(json!(p.id));
                    continue;
                }
            };
            if !new_fields.is_empty() {
                entry["new_fields"] = json!(new_fields);
            }
            projects.push(entry);
        }
        let portfolio_reason = match brief {
            None => Some("missing"),
            Some(pf)
                if parse(&pf.created_at).with_timezone(&Local).date_naive() != self.today() =>
            {
                Some("new_day")
            }
            Some(pf) if !self.unchecked_ideas(Some(&pf.data))?.is_empty() => Some("owner_idea"),
            Some(_) if cards_changed => Some("cards_changed"),
            Some(_) => None,
        };
        let any_due = !projects.is_empty();
        // cards that only gain new fields do not change what the brief builds on
        let brief_affected = projects.iter().any(|e| e["reason"] != "new_fields");
        let next = if any_due {
            "For each project listed: `watchtower context <id>`, study the repository, then `watchtower card put <id> --file <card.json> --context-token <context_token>`. \
             Afterwards: `watchtower portfolio context` and `watchtower portfolio put --file <portfolio.json> --context-token <context_token>`. Use the token from each corresponding context."
        } else if portfolio_reason.is_some() {
            "Every card is current; only the portfolio brief is due: `watchtower portfolio context`, then `watchtower portfolio put --file <portfolio.json> --context-token <context_token>` with that context's token."
        } else {
            "Nothing is due. Every card and the portfolio brief are current."
        };
        Ok(json!({
            "today": self.today().to_string(),
            "projects": projects,
            "up_to_date": up_to_date,
            "problems": problems,
            "portfolio": {
                "due": portfolio_reason.is_some() || brief_affected,
                "reason": portfolio_reason.or(brief_affected.then_some("cards_will_change")),
            },
            "next": next,
        }))
    }

    pub fn context(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        self.stored_facts(id)?;
        self.context_with_token(Some(id), || self.context_data(id))
    }

    fn context_with_token(
        &self,
        project: Option<&str>,
        read: impl Fn() -> Result<Value>,
    ) -> Result<Value> {
        // Recheck around the read so the token never blesses a mixture of versions.
        for _ in 0..3 {
            let token = self.store.context_token(project)?;
            let mut data = read()?;
            if token == self.store.context_token(project)? {
                data["context_token"] = json!(token);
                return Ok(data);
            }
        }
        Err(Self::version_conflict(project))
    }

    fn version_conflict(project: Option<&str>) -> WatchtowerError {
        let command = project.map_or_else(
            || "watchtower portfolio context".to_owned(),
            |id| format!("watchtower context {id}"),
        );
        WatchtowerError::new("version_conflict", "The context changed. Nothing was saved.")
            .hint(format!("Read `{command}` again, reconcile your document with the new facts, owner instructions and previous document, then retry with its context_token. Do not merely replace the token."))
    }

    fn check_context(
        &self,
        project: Option<&str>,
        token: Option<&str>,
        dry_run: bool,
    ) -> Result<()> {
        match token {
            Some(token) if token != self.store.context_token(project)? => {
                Err(Self::version_conflict(project))
            }
            None if !dry_run => {
                let command = project.map_or_else(
                    || "watchtower portfolio context".to_owned(),
                    |id| format!("watchtower context {id}"),
                );
                Err(WatchtowerError::new("context_required", "Saving requires the context_token from the context you used.")
                    .hint(format!("Read `{command}`, prepare your document from that context, and pass `--context-token <context_token>`. Schema-only --dry-run may omit it.")))
            }
            _ => Ok(()),
        }
    }

    fn guarded_version(
        write: crate::store::GuardedWrite,
        kind: Kind,
        project: Option<&str>,
    ) -> Result<i64> {
        match write {
            crate::store::GuardedWrite::Saved(version) => Ok(version),
            crate::store::GuardedWrite::Conflict => Err(Self::version_conflict(project)),
            crate::store::GuardedWrite::ItemConflict(conflicts) => {
                let (code, name) = match kind {
                    Kind::Card => ("invalid_card", "card"),
                    Kind::Portfolio => ("invalid_portfolio", "brief"),
                };
                Err(WatchtowerError::new(code, format!("The {name} was not saved: {}", conflicts.join("; ")))
                    .hint("Keep an existing id only for the same item in the same project card or brief. Omit id for a new item."))
            }
        }
    }

    fn context_data(&self, id: &str) -> Result<Value> {
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
        // Ratings from every project show the owner's taste; replies only matter to their own project.
        let reactions = self.store.feedback()?;
        let feedback: Vec<Value> = reactions
            .iter()
            .filter(|f| f.project_id.as_deref() == Some(id) || RATINGS.contains(&f.value.as_str()))
            .map(feedback_json)
            .collect();
        let next = format!(
            "Write the card with `watchtower card put {id} --file <card.json> --context-token <context_token>`{}",
            if card.is_some() {
                " (add --merge to send only the fields that changed)."
            } else {
                "."
            }
        );
        Ok(json!({
            "today": self.today().to_string(),
            "project": pick(&project_json(&project), &["id", "name", "path", "status", "status_note", "goal", "goal_due"]),
            "facts": extend(json!({"collected_at": stored.collected_at}), stored.data),
            "new_fields": card.as_ref().map(|c| Kind::Card.missing_fields(&c.data)),
            "previous_card": card.map(|c| json!({"version": c.version, "created_at": c.created_at, "card": c.data})),
            "since_previous_card": since,
            "feedback": feedback,
            "feedback_summary": feedback_summary(&reactions),
            "tasks": self.tasks(Some(id))?["tasks"],
            "warning_acknowledgements": self.warning_acknowledgements(id)?,
            "next": next,
        }))
    }

    fn merged(previous: Option<&Value>, payload: Value, merge: bool, what: &str) -> Result<Value> {
        if !merge {
            return Ok(payload);
        }
        let Some(previous) = previous else {
            return Err(WatchtowerError::new(
                "nothing_to_merge",
                format!("There is no previous {what} to merge into."),
            )
            .hint(format!("Send a complete {what} without --merge.")));
        };
        Ok(if payload.is_object() {
            extend(previous.clone(), payload)
        } else {
            payload
        })
    }

    /// Validates and saves a card; with `dry_run` only validates and reports what would be saved.
    pub fn put_card(&self, id: &str, payload: Value, merge: bool, dry_run: bool) -> Result<Value> {
        self.project(id)?;
        if !dry_run {
            self.stored_facts(id)?;
        }
        let token = self.store.context_token(Some(id))?;
        self.put_card_guarded(id, payload, merge, dry_run, Some(&token))
    }

    pub fn put_card_guarded(
        &self,
        id: &str,
        payload: Value,
        merge: bool,
        dry_run: bool,
        context_token: Option<&str>,
    ) -> Result<Value> {
        self.check_context(Some(id), context_token, dry_run)?;
        let project = self.project(id)?;
        let previous = self.store.latest_card(id)?;
        let sent = sent_fields(&payload, merge);
        let kept = kept_fields(previous.as_ref().map(|c| &c.data), sent.as_deref());
        let payload = Self::merged(previous.as_ref().map(|c| &c.data), payload, merge, "card")?;
        let (clean, warnings) = cards::validate(Kind::Card, &payload, &kept).map_err(|problems| {
            let merged = if merge { " With --merge, fields marked (from the previous card) were not in your document: send corrected versions of those too." } else { "" };
            WatchtowerError::new("invalid_card", format!("The card was not saved: {}", mark_inherited(&problems, sent.as_deref())))
                .hint(format!("Fix the listed fields and send the card again.{merged} `watchtower schema card` prints the full schema."))
        })?;
        self.validate_item_ids(Kind::Card, Some(id), &clean)?;
        let changed = changed_fields(Kind::Card, previous.as_ref().map(|c| &c.data), &clean);
        // fields the previous card lacked and this save did not send end up empty; say so
        let lacking = previous
            .as_ref()
            .map(|c| Kind::Card.missing_fields(&c.data))
            .unwrap_or_default();
        let unfilled: Vec<String> = Kind::Card
            .missing_fields(&payload)
            .into_iter()
            .filter(|f| lacking.contains(f))
            .collect();
        let mut warnings = warnings;
        // the owner copies `how` as it stands, so it must not be a sentence, a URL behind a command or a shortcut to click
        let prose: Vec<&str> = clean["handbook"]["run"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["how"].as_str().is_some_and(not_typed))
            .filter_map(|r| r["what"].as_str())
            .collect();
        if !prose.is_empty() {
            warnings.push(format!(
                "handbook.run: `how` of \"{}\" is not a plain command or URL: it has backticks, a URL after a command, or a shortcut to click. Watchtower copies `how` as it stands, so it holds exactly what the owner types, or one URL to open; a URL gets its own entry, and how to open the installed app belongs in `handbook.installed.open`. Send the whole handbook with that fixed.",
                prose.join("\", \"")
            ));
        }
        if !unfilled.is_empty() {
            warnings.push(format!(
                "The previous card lacked {} and this card does not fill them, so they are saved empty. Send them with --merge (a list or the handbook in full).",
                unfilled.join(", ")
            ));
        }
        if dry_run {
            self.check_context(Some(id), context_token, true)?;
            let items = dry_items(Kind::Card, &payload, &clean);
            return Ok(
                json!({"saved": false, "dry_run": true, "project": id, "changed_fields": changed, "unfilled_fields": unfilled, "warnings": warnings, "items": items}),
            );
        }
        let stored = self.stored_facts(id)?;
        let parts = facts::basis(&stored.data);
        let owner = owner_basis(
            &project,
            &self.store.feedback()?,
            &self.warning_acknowledgements(id)?,
            &self.project_task_basis(id)?,
        );
        let basis = CardBasis {
            fingerprint: &stored.fingerprint,
            head: stored.data["head"].as_str(),
            ci: settled_ci(&stored.data),
            parts: &parts,
            owner: Some(&owner),
        };
        let write = self.store.add_card_guarded(
            id,
            &self.now(),
            &basis,
            &clean,
            context_token.expect("checked before validation"),
        )?;
        let version = Self::guarded_version(write, Kind::Card, Some(id))?;
        Ok(
            json!({"saved": true, "project": id, "version": version, "changed_fields": changed, "unfilled_fields": unfilled, "warnings": warnings, "items": item_ids(Kind::Card, &clean)}),
        )
    }

    pub fn card(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        let card = self.store.latest_card(id)?.ok_or_else(|| {
            WatchtowerError::new("no_card", format!("Project '{id}' has no card yet."))
                .hint(format!("Run `watchtower context {id}` and write one."))
        })?;
        Ok(
            json!({"project": id, "version": card.version, "created_at": card.created_at, "card": card.data}),
        )
    }

    pub fn portfolio_context(&self) -> Result<Value> {
        self.context_with_token(None, || self.portfolio_context_data())
    }

    fn portfolio_context_data(&self) -> Result<Value> {
        let mut rows = Vec::new();
        let previous = self.store.latest_portfolio()?;
        let reactions = self.store.feedback()?;
        // changes and milestones from the start of the review week on, the material for `week_review`
        let week_from = self.review_week().1.to_string();
        for p in self.store.projects()? {
            if p.status == "archived" {
                continue;
            }
            let stored = self.store.facts(&p.id)?;
            let card = self.store.latest_card(&p.id)?;
            let changed = match (&previous, &card) {
                (Some(pf), Some(c)) => json!(self.brief_changes(&p.id, &c.data, pf)?),
                _ => Value::Null,
            };
            let card_outdated = card_outdated(
                card.as_ref(),
                stored.as_ref(),
                &p,
                &reactions,
                &self.warning_acknowledgements(&p.id)?,
                &self.project_task_basis(&p.id)?,
            );
            let f = stored.map(|s| s.data).unwrap_or_else(|| json!({}));
            let card_json = card.map(|card| {
                let c = &card.data;
                let picked = |key: &str, fields: &[&str]| -> Vec<Value> { c[key].as_array().into_iter().flatten().map(|x| pick(x, fields)).collect() };
                json!({
                    "created_at": card.created_at, "outdated": card_outdated, "changed_since_brief": changed,
                    "one_liner": c["one_liner"], "phase": c["phase"], "headline": c["headline"],
                    "reentry": c["reentry"],
                    "waiting_on_you": picked("waiting_on_you", &["id", "title"]),
                    "open_ends": picked("open_ends", &["kind", "title", "age_days"]),
                    "health_notes": c["health_notes"],
                    "decisions": c["decisions"],
                    "suggestions": picked("suggestions", &["id", "type", "title", "why"]),
                    "cross_project_hints": c["cross_project_hints"],
                    "recent_changes": dated_since(&c["recent_changes"], &week_from),
                    "milestones": dated_since(&c["milestones"], &week_from),
                })
            });
            rows.push(json!({
                "id": p.id, "name": p.name, "status": p.status, "status_note": p.status_note, "goal": p.goal, "goal_due": p.goal_due,
                "warning_acknowledgements": self.warning_acknowledgements(&p.id)?,
                "facts": {
                    "last_work": f["last_work"],
                    "commits": pick(&f["commits"], &["total", "last_7d", "last_30d", "last"]),
                    "commits_review_week": f["commits"]["weekly"].as_array().and_then(|w| w.len().checked_sub(2).map(|i| w[i].clone())),
                    "has_remote": f["has_remote"], "ahead": f["ahead"],
                    "dirty": pick(&f["dirty"], &["files", "lines_added", "lines_removed", "oldest_change"]),
                    "ci": pick(&f["ci"], &["state", "failing", "at"]),
                    "unmerged_branches": f["branches"]["unmerged"].as_array().map_or(0, Vec::len),
                    "extra_worktrees": f["worktrees"]["count"],
                },
                "card": card_json,
            }));
        }
        let unchecked = self.unchecked_ideas(previous.as_ref().map(|p| &p.data))?;
        let (week, from, to) = self.review_week();
        let reviewed = previous
            .as_ref()
            .is_some_and(|p| p.data["week_review"]["week"] == week.as_str());
        Ok(json!({
            "today": self.today().to_string(),
            "review_week": {"week": week, "from": from.to_string(), "to": to.to_string(), "due": !reviewed},
            "owner_ideas": self.store.ideas()?.iter().map(|i| extend(idea_json(i), json!({"checked": !unchecked.contains(&i.id)}))).collect::<Vec<_>>(),
            "projects": rows,
            "previous_portfolio": previous.map(|p| json!({"version": p.version, "created_at": p.created_at, "portfolio": p.data})),
            "owner_last_opened": self.store.pref("last_visit")?,
            "feedback": reactions.iter().map(feedback_json).collect::<Vec<_>>(),
            "feedback_summary": feedback_summary(&reactions),
            "tasks": self.tasks(None)?["tasks"],
            "next": "Write the brief with `watchtower portfolio put --file <portfolio.json> --context-token <context_token>`.",
        }))
    }

    pub fn put_portfolio(&self, payload: Value, merge: bool, dry_run: bool) -> Result<Value> {
        let token = self.store.context_token(None)?;
        self.put_portfolio_guarded(payload, merge, dry_run, Some(&token))
    }

    pub fn put_portfolio_guarded(
        &self,
        payload: Value,
        merge: bool,
        dry_run: bool,
        context_token: Option<&str>,
    ) -> Result<Value> {
        self.check_context(None, context_token, dry_run)?;
        let previous = self.store.latest_portfolio()?;
        let sent = sent_fields(&payload, merge);
        let kept = kept_fields(previous.as_ref().map(|p| &p.data), sent.as_deref());
        let payload = Self::merged(
            previous.as_ref().map(|p| &p.data),
            payload,
            merge,
            "portfolio brief",
        )?;
        let (clean, warnings) = cards::validate(Kind::Portfolio, &payload, &kept).map_err(|problems| {
            WatchtowerError::new("invalid_portfolio", format!("The brief was not saved: {}", mark_inherited(&problems, sent.as_deref())))
                .hint("Fix the listed fields and send it again. `watchtower schema portfolio` prints the full schema.")
        })?;
        self.validate_item_ids(Kind::Portfolio, None, &clean)?;
        let known: Vec<String> = self.store.projects()?.into_iter().map(|p| p.id).collect();
        let items = |key: &str| clean[key].as_array().cloned().unwrap_or_default();
        let mut refs: Vec<String> = Vec::new();
        for item in items("focus").iter().chain(items("park_or_revive").iter()) {
            refs.extend(item["project"].as_str().map(str::to_owned));
        }
        for (list, key) in [
            ("patterns", "projects"),
            ("synergies", "projects"),
            ("idea_checks", "fits"),
        ] {
            for item in items(list) {
                refs.extend(
                    item[key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned)),
                );
            }
        }
        let mut unknown: Vec<String> = refs.into_iter().filter(|r| !known.contains(r)).collect();
        unknown.sort();
        unknown.dedup();
        if !unknown.is_empty() {
            let mut known = known;
            known.sort();
            return Err(WatchtowerError::new(
                "unknown_project",
                format!(
                    "The brief names unknown project ids: {}.",
                    unknown.join(", ")
                ),
            )
            .hint(format!(
                "Use the ids from `watchtower portfolio context`: {}.",
                known.join(", ")
            )));
        }
        let ideas: Vec<i64> = self.store.ideas()?.iter().map(|i| i.id).collect();
        let strange: Vec<String> = items("idea_checks")
            .iter()
            .filter_map(|c| c["idea"].as_i64())
            .filter(|id| !ideas.contains(id))
            .map(|id| id.to_string())
            .collect();
        if !strange.is_empty() {
            return Err(WatchtowerError::new("unknown_idea", format!("The brief assesses ideas that are not parked: {}.", strange.join(", ")))
                .hint("Use the ids from `owner_ideas` in `watchtower portfolio context`; the owner may have removed an idea since."));
        }
        let mut clean = clean;
        // a brief written later in the week keeps the week's review
        let (week, _, _) = self.review_week();
        if clean["week_review"].is_null() {
            if let Some(kept) = previous
                .as_ref()
                .map(|p| &p.data["week_review"])
                .filter(|r| r["week"] == week.as_str())
            {
                clean["week_review"] = kept.clone();
            }
        }
        if dry_run {
            self.check_context(None, context_token, true)?;
            return Ok(
                json!({"saved": false, "dry_run": true, "warnings": warnings, "items": dry_items(Kind::Portfolio, &payload, &clean)}),
            );
        }
        let write = self.store.add_portfolio_guarded(
            &self.now(),
            &clean,
            &self.portfolio_basis()?,
            context_token.expect("checked before validation"),
        )?;
        let version = Self::guarded_version(write, Kind::Portfolio, None)?;
        Ok(
            json!({"saved": true, "version": version, "warnings": warnings, "items": item_ids(Kind::Portfolio, &clean)}),
        )
    }

    /// Parks an idea of the owner's; the next brief assesses it.
    pub fn add_idea(&self, text: &str) -> Result<Value> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > IDEA_MAX {
            return Err(WatchtowerError::new(
                "invalid_idea",
                format!("An idea needs 1 to {IDEA_MAX} characters."),
            ));
        }
        let id = self.store.add_idea(text, &self.now())?;
        Ok(json!({"id": id, "text": text}))
    }

    pub fn remove_idea(&self, id: i64) -> Result<Value> {
        if !self.store.remove_idea(id)? {
            return Err(WatchtowerError::new(
                "idea_not_found",
                format!("No parked idea with id {id}."),
            )
            .hint("`watchtower ideas list` shows the parked ideas."));
        }
        Ok(json!({"removed": id}))
    }

    pub fn ideas(&self) -> Result<Value> {
        Ok(json!(self
            .store
            .ideas()?
            .iter()
            .map(idea_json)
            .collect::<Vec<_>>()))
    }

    pub fn portfolio(&self) -> Result<Value> {
        let p = self.store.latest_portfolio()?.ok_or_else(|| {
            WatchtowerError::new("no_portfolio", "No portfolio brief has been written yet.")
                .hint("Run `watchtower portfolio context` and write one.")
        })?;
        Ok(json!({"version": p.version, "created_at": p.created_at, "portfolio": p.data}))
    }

    // ---------- feedback and preferences ----------
    /// Records the owner's reaction to an item of the current cards or brief: a rating
    /// (`up`, `later`, `down`) of a suggestion or idea, `answered` (the answer is the note)
    /// or `done` for a question, `done` for a focus item. Watchtower keeps the item as it read,
    /// so agents still see what was rated after it left the card. `value: None` withdraws
    /// the reaction; `note: None` keeps the note already stored.
    pub fn set_feedback(
        &self,
        item_id: &str,
        value: Option<&str>,
        note: Option<&str>,
    ) -> Result<Value> {
        self.set_feedback_until(item_id, value, note, None)
    }

    pub fn set_feedback_until(
        &self,
        item_id: &str,
        value: Option<&str>,
        note: Option<&str>,
        until: Option<&str>,
    ) -> Result<Value> {
        self.write_feedback(item_id, value, note, until, false)
    }

    pub fn restore_feedback(&self, item_id: &str, snapshot: Option<&Value>) -> Result<Value> {
        let Some(snapshot) = snapshot.filter(|s| !s.is_null()) else {
            return self.write_feedback(item_id, None, None, None, true);
        };
        let invalid = || {
            WatchtowerError::new("invalid_feedback", "The previous reaction must contain value, note and an optional snoozed_until date.")
            .hint("Restore the exact reaction snapshot captured before the change.")
        };
        let object = snapshot.as_object().ok_or_else(invalid)?;
        if object
            .keys()
            .any(|key| !["value", "note", "snoozed_until", "at"].contains(&key.as_str()))
        {
            return Err(invalid());
        }
        let value = snapshot["value"].as_str().ok_or_else(invalid)?;
        let note = snapshot["note"].as_str().ok_or_else(invalid)?;
        let until = match snapshot.get("snoozed_until") {
            None | Some(Value::Null) => None,
            Some(Value::String(date)) => Some(date.as_str()),
            _ => return Err(invalid()),
        };
        if value == "later" && until.is_none() {
            return Err(invalid());
        }
        self.write_feedback(item_id, Some(value), Some(note), until, true)
    }

    fn write_feedback(
        &self,
        item_id: &str,
        value: Option<&str>,
        note: Option<&str>,
        until: Option<&str>,
        restore: bool,
    ) -> Result<Value> {
        let invalid_snooze = || {
            WatchtowerError::new(
                "invalid_snooze",
                "A snooze date must be a future date as YYYY-MM-DD and only applies to later.",
            )
            .hint("Choose a date after today for later, or omit until for other reactions.")
        };
        let explicit_until = if let Some(until) = until {
            let date =
                NaiveDate::parse_from_str(until, "%Y-%m-%d").map_err(|_| invalid_snooze())?;
            if value != Some("later")
                || until.len() != 10
                || date.to_string() != until
                || (!restore && date <= self.today())
            {
                return Err(invalid_snooze());
            }
            Some(until.to_owned())
        } else {
            None
        };
        let Some(value) = value else {
            self.store.delete_feedback(item_id)?;
            return Ok(json!({"item_id": item_id, "value": null}));
        };
        let existing = self
            .store
            .feedback()?
            .into_iter()
            .find(|f| f.item_id == item_id);
        let (project_id, item) =
            match (self.find_item(item_id)?, &existing) {
                (Some((project, item)), _) => (project, Some(item)),
                (None, Some(f)) => (f.project_id.clone(), f.item.clone()),
                (None, None) => return Err(WatchtowerError::new(
                    "unknown_item",
                    format!("No current card or brief has an item with id '{item_id}'."),
                )
                .hint(
                    "Use an id from `watchtower card show <id>` or `watchtower portfolio show`.",
                )),
            };
        let kind = item
            .as_ref()
            .and_then(|i| i["kind"].as_str())
            .unwrap_or("suggestion");
        let allowed = reactions_for(kind);
        if !allowed.contains(&value) {
            return Err(WatchtowerError::new(
                "invalid_feedback",
                format!("'{value}' does not fit a {kind}."),
            )
            .hint(format!("Use one of: {}.", allowed.join(", "))));
        }
        let note = match note {
            Some(n) => n.trim().to_owned(),
            None => existing
                .as_ref()
                .map(|f| f.note.clone())
                .unwrap_or_default(),
        };
        if value == "answered" && note.is_empty() {
            return Err(
                WatchtowerError::new("answer_missing", "An answer needs its text.")
                    .hint("Send the answer as the note, or mark the question `done`."),
            );
        }
        let title = match (&item, &existing) {
            (Some(item), _) => title_of(item),
            (None, Some(f)) => f.title.clone(),
            (None, None) => String::new(),
        };
        let at = self.now();
        let snoozed_until = if value == "later" {
            explicit_until
                .or_else(|| existing.as_ref().and_then(snoozed_until))
                .or_else(|| Some((self.today() + chrono::Duration::days(SNOOZE_DAYS)).to_string()))
        } else {
            None
        };
        self.store.set_feedback(&Feedback {
            item_id: item_id.to_owned(),
            project_id,
            title,
            value: value.to_owned(),
            at,
            note: note.clone(),
            item,
            snoozed_until: snoozed_until.clone(),
        })?;
        Ok(
            json!({"item_id": item_id, "value": value, "note": note, "snoozed_until": snoozed_until}),
        )
    }

    /// The item with this id in the current cards and brief: (its project, the item with its `kind`).
    fn find_item(&self, item_id: &str) -> Result<Option<(Option<String>, Value)>> {
        let mut docs: Vec<(Option<String>, Kind, Value)> = Vec::new();
        for p in self.store.projects()? {
            if let Some(card) = self.store.latest_card(&p.id)? {
                docs.push((Some(p.id), Kind::Card, card.data));
            }
        }
        if let Some(pf) = self.store.latest_portfolio()? {
            docs.push((None, Kind::Portfolio, pf.data));
        }
        for (project, kind, doc) in docs {
            for (list, _, item_kind) in kind.items() {
                let Some(item) = doc[*list]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|i| i["id"] == item_id)
                else {
                    continue;
                };
                let mut snapshot = json!({"kind": item_kind});
                for (key, value) in item
                    .as_object()
                    .into_iter()
                    .flatten()
                    .filter(|(k, _)| *k != "id")
                {
                    snapshot[key] = value.clone();
                }
                let project = project.or_else(|| item["project"].as_str().map(str::to_owned));
                return Ok(Some((project, snapshot)));
            }
        }
        Ok(None)
    }

    pub fn feedback(&self, project_id: Option<&str>) -> Result<Value> {
        Ok(Value::Array(
            self.store
                .feedback()?
                .iter()
                .filter(|f| project_id.is_none() || f.project_id.as_deref() == project_id)
                .map(feedback_json)
                .collect(),
        ))
    }

    pub fn warning_acknowledgements(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        Ok(self.store.warning_acknowledgements(id)?)
    }

    pub fn project_warnings(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        let facts = self
            .store
            .facts(id)?
            .map(|facts| facts.data)
            .unwrap_or(Value::Null);
        let acknowledgements = self.store.warning_acknowledgements(id)?;
        let mut warnings = fact_warnings(&facts);
        for warning in &mut warnings {
            let previous = acknowledgements
                .as_array()
                .into_iter()
                .flatten()
                .find(|ack| ack["key"] == warning["key"]);
            warning["acknowledged"] = json!(previous.is_some_and(|ack| warning_ack_applies(
                warning["key"].as_str().unwrap_or(""),
                &ack["basis"],
                &warning["basis"]
            )));
            warning["note"] = previous
                .map(|ack| ack["note"].clone())
                .unwrap_or_else(|| json!(""));
        }
        Ok(json!(warnings))
    }

    pub fn set_warning_ack(&self, id: &str, key: &str, note: Option<&str>) -> Result<Value> {
        self.project(id)?;
        if !["no_remote", "ci_none", "worktrees"].contains(&key) {
            return Err(WatchtowerError::new(
                "invalid_warning",
                "This warning cannot be acknowledged.",
            )
            .hint("Use no_remote, ci_none or worktrees from the project's current warnings."));
        }
        let note = note.map(str::trim);
        if note.is_some_and(|note| note.is_empty() || note.chars().count() > 600) {
            return Err(WatchtowerError::new("invalid_warning", "An acknowledgement needs a reason of 1 to 600 characters.")
                .hint("Explain why the measured condition is intentional, or omit the note to withdraw the acknowledgement."));
        }
        let warnings = self.project_warnings(id)?;
        let current = warnings
            .as_array()
            .into_iter()
            .flatten()
            .find(|warning| warning["key"] == key);
        if note.is_some() && current.is_none() {
            return Err(WatchtowerError::new(
                "warning_not_present",
                "This measured warning is not present.",
            )
            .hint("Refresh the project and choose one of its current warnings."));
        }
        self.store.set_warning_ack(
            id,
            key,
            note,
            &current
                .map(|warning| warning["basis"].clone())
                .unwrap_or(Value::Null),
            &self.now(),
        )?;
        Ok(json!({"project": id, "key": key, "warnings": self.project_warnings(id)?}))
    }

    /// Feedback is keyed globally by item id, so an id must remain in its original
    /// document and item kind even after a rated item leaves the current card.
    fn validate_item_ids(&self, kind: Kind, project: Option<&str>, doc: &Value) -> Result<()> {
        let mut owners = Vec::new();
        for p in self.store.projects()? {
            if let Some(card) = self.store.latest_card(&p.id)? {
                for item in item_ids(Kind::Card, &card.data) {
                    if let Some(id) = item["id"].as_str() {
                        owners.push((
                            id.to_owned(),
                            Some(p.id.clone()),
                            item["kind"].as_str().map(str::to_owned),
                        ));
                    }
                }
            }
        }
        if let Some(brief) = self.store.latest_portfolio()? {
            for item in item_ids(Kind::Portfolio, &brief.data) {
                if let Some(id) = item["id"].as_str() {
                    owners.push((
                        id.to_owned(),
                        None,
                        item["kind"].as_str().map(str::to_owned),
                    ));
                }
            }
        }
        for feedback in self.store.feedback()? {
            let item_kind = feedback
                .item
                .as_ref()
                .and_then(|item| item["kind"].as_str());
            let scope = match item_kind {
                Some("suggestion" | "question") | None => feedback.project_id,
                Some(_) => None,
            };
            owners.push((feedback.item_id, scope, item_kind.map(str::to_owned)));
        }
        let mut conflicts = Vec::new();
        for (list, _, item_kind) in kind.items() {
            for (index, item) in doc[*list].as_array().into_iter().flatten().enumerate() {
                let Some(id) = item["id"].as_str() else {
                    continue;
                };
                if owners.iter().any(|(other_id, other_project, other_kind)| {
                    other_id == id
                        && (other_project.as_deref() != project
                            || other_kind
                                .as_deref()
                                .is_some_and(|other_kind| other_kind != *item_kind))
                }) {
                    conflicts.push(format!(
                        "{list}[{index}].id: '{id}' belongs to another card, brief or item kind"
                    ));
                }
            }
        }
        if !conflicts.is_empty() {
            let (code, name) = match kind {
                Kind::Card => ("invalid_card", "card"),
                Kind::Portfolio => ("invalid_portfolio", "brief"),
            };
            return Err(WatchtowerError::new(code, format!("The {name} was not saved: {}", conflicts.join("; ")))
                .hint("Keep an existing id only for the same item in the same project card or brief. Omit id for a new item."));
        }
        Ok(())
    }

    pub fn set_pref(&self, key: &str, value: &Value) -> Result<()> {
        if !PREFS.contains(&key) {
            return Err(WatchtowerError::new(
                "invalid_pref",
                format!("Unknown preference '{key}'."),
            ));
        }
        Ok(self.store.set_pref(key, value)?)
    }

    /// The ids of parked ideas the latest brief has not assessed yet.
    fn unchecked_ideas(&self, brief: Option<&Value>) -> Result<Vec<i64>> {
        let checked: Vec<i64> = brief
            .and_then(|b| b["idea_checks"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|c| c["idea"].as_i64())
            .collect();
        Ok(self
            .store
            .ideas()?
            .into_iter()
            .map(|i| i.id)
            .filter(|id| !checked.contains(id))
            .collect())
    }

    /// The calendar week before this one, which the week review looks back on.
    fn review_week(&self) -> (String, NaiveDate, NaiveDate) {
        let today = self.today();
        let from =
            today - chrono::Duration::days(i64::from(today.weekday().num_days_from_monday()) + 7);
        (
            from.format("%G-W%V").to_string(),
            from,
            from + chrono::Duration::days(6),
        )
    }

    /// The brief-relevant card fields that differ from the card as it was when the brief was written;
    /// None when the project had no card then.
    fn brief_changes(
        &self,
        id: &str,
        card: &Value,
        brief: &StoredPortfolio,
    ) -> Result<Option<Vec<&'static str>>> {
        let then = match &brief.basis {
            Some(basis) => basis
                .get(id)
                .and_then(|p| p.get("card"))
                .filter(|c| !c.is_null())
                .cloned(),
            None => self.store.card_at(id, &brief.created_at)?,
        };
        Ok(then.map(|then| {
            BRIEF_FIELDS
                .iter()
                .copied()
                .filter(|f| then.get(*f) != card.get(*f))
                .collect()
        }))
    }

    /// What the brief builds on, including removals and owner changes without a new card.
    fn portfolio_basis(&self) -> Result<Value> {
        let mut basis = Map::new();
        let tasks = self.store.tasks()?;
        if !tasks.is_empty() {
            // The brief also assesses standalone work and tasks whose project was removed.
            basis.insert(
                "@tasks".to_owned(),
                Value::Array(
                    tasks
                        .iter()
                        .map(|task| pick(task, &["id", "status", "revision"]))
                        .collect(),
                ),
            );
        }
        for p in self
            .store
            .projects()?
            .into_iter()
            .filter(|p| p.status != "archived")
        {
            let card = self
                .store
                .latest_card(&p.id)?
                .map(|c| pick(&c.data, BRIEF_FIELDS));
            let mut entry = json!({
                "project": pick(&project_json(&p), &["name", "status", "status_note", "goal", "goal_due"]),
                "card": card,
            });
            let warning_acks = self.warning_acknowledgements(&p.id)?;
            if warning_acks.as_array().is_some_and(|acks| !acks.is_empty()) {
                entry["warning_acks"] = warning_acks;
            }
            let tasks = self.project_task_basis(&p.id)?;
            if tasks.as_array().is_some_and(|tasks| !tasks.is_empty()) {
                entry["tasks"] = tasks;
            }
            basis.insert(p.id.clone(), entry);
        }
        Ok(Value::Object(basis))
    }

    fn project_task_basis(&self, id: &str) -> Result<Value> {
        Ok(Value::Array(
            self.store
                .tasks()?
                .iter()
                .filter(|task| {
                    task["projects"]
                        .as_array()
                        .is_some_and(|projects| projects.iter().any(|p| p == id))
                })
                .map(|task| pick(task, &["id", "status", "revision"]))
                .collect(),
        ))
    }

    // ---------- tracked daily runs ----------
    pub fn run_start(&self, refresh: bool) -> Result<Value> {
        let due = self.due(14, refresh)?;
        let mut projects = Vec::new();
        for project in self
            .store
            .projects()?
            .into_iter()
            .filter(|p| p.status != "archived")
        {
            let mut reason = due["projects"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| entry["id"] == project.id)
                .map(|entry| entry["reason"].clone())
                .or_else(|| {
                    due["problems"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|entry| entry["id"] == project.id)
                        .map(|entry| entry["problem"].clone())
                });
            let facts = self.store.facts(&project.id)?;
            if facts.as_ref().is_none_or(|facts| facts.data["ok"] != true) {
                reason = Some(
                    facts
                        .as_ref()
                        .and_then(|facts| facts.data.get("error"))
                        .cloned()
                        .unwrap_or_else(|| json!("facts_missing")),
                );
            }
            projects.push(json!({
                "id": project.id, "name": project.name,
                "initial_version": self.store.latest_card(&project.id)?.map(|card| card.version).unwrap_or(0),
                "was_due": reason.is_some(), "reason": reason,
            }));
        }
        let initial = json!({
            "projects": projects,
            "portfolio_version": self.store.latest_portfolio()?.map(|brief| brief.version).unwrap_or(0),
            "portfolio_due": due["portfolio"]["due"],
        });
        let id = self.store.start_agent_run(&self.now(), &initial)?;
        let mut status = self.agent_run_response(id)?;
        status["due"] = due;
        status["next"] = json!(format!("For every project in due.projects: read `watchtower context <id>`, study the repository, and save its card with that context_token. Then read `watchtower portfolio context` and save the due brief with its context_token. Record failures with `watchtower runs fail {id}`. Finish with `watchtower runs finish {id}`, even when nothing was due; only complete confirms a successful run."));
        Ok(status)
    }

    fn require_agent_run(&self, id: i64) -> Result<crate::store::StoredAgentRun> {
        self.store.agent_run(id)?.ok_or_else(|| WatchtowerError::new("unknown_run", format!("Agent run {id} does not exist.")).hint("Use `watchtower runs status` to see the latest run, or `watchtower runs start` to begin one."))
    }

    pub fn run_fail(&self, run_id: i64, project_id: Option<&str>, error: &str) -> Result<Value> {
        let run = self.require_agent_run(run_id)?;
        if run
            .result
            .as_ref()
            .is_some_and(|result| result["state"] == "complete")
        {
            return Err(WatchtowerError::new(
                "run_complete",
                "A successfully completed run is immutable.",
            )
            .hint("Start another run to record new work or failures."));
        }
        let error = error.trim();
        if error.is_empty() || error.chars().count() > 2_000 {
            return Err(WatchtowerError::new(
                "invalid_run_error",
                "Give a nonempty failure description of at most 2000 characters.",
            )
            .hint("Describe the failed step briefly, without credentials."));
        }
        if let Some(id) = project_id {
            self.project(id)?;
        }
        if !self.store.fail_agent_run(
            run_id,
            project_id.unwrap_or("@portfolio"),
            &json!({"error": error, "at": self.now()}),
        )? {
            return Err(WatchtowerError::new(
                "run_complete",
                "The run completed before this failure was recorded.",
            )
            .hint("Start another run to record new work or failures."));
        }
        self.agent_run_response(run_id)
    }

    pub fn run_finish(&self, run_id: i64) -> Result<Value> {
        self.require_agent_run(run_id)?;
        // A view spans several reads. Compare its starting evidence again inside
        // the final write transaction, including failures reported by other agents.
        for _ in 0..3 {
            let expected = self.store.agent_run_token(run_id)?;
            let run = self.require_agent_run(run_id)?;
            if run
                .result
                .as_ref()
                .is_some_and(|result| result["state"] == "complete")
            {
                return self.agent_run_response(run_id);
            }
            let result = self.agent_run_view(&run, true)?;
            if self
                .store
                .finish_agent_run(run_id, &self.now(), &result, &expected)?
            {
                return self.agent_run_response(run_id);
            }
        }
        Err(WatchtowerError::new("run_conflict", "The run or its project evidence kept changing. No completion was recorded.")
            .hint(format!("Inspect `watchtower runs status` and retry `watchtower runs finish {run_id}` after current writes finish.")))
    }

    fn agent_run_response(&self, run_id: i64) -> Result<Value> {
        let mut status = self.run_status()?;
        let run = self.agent_run_view(&self.require_agent_run(run_id)?, false)?;
        status["state"] = run["state"].clone();
        status["run_id"] = json!(run_id);
        status["repair_task"] = json!(Self::agent_run_repair_task(&run));
        status["run"] = run;
        Ok(status)
    }

    fn agent_run_repair_task(run: &Value) -> Option<String> {
        if run["state"] == "complete" {
            return None;
        }
        let projects = run["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|project| project["status"] != "current")
            .filter_map(|project| project["id"].as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!("Vervollständige den Watchtower-Lauf {}. Prüfe zuerst `watchtower runs status` und `watchtower due`. Bearbeite alle dort fälligen Projekte mit der Watchtower-Skill; offene Projekte dieses Laufs: {}. Lies Antworten und Statusnotizen des Besitzers. Schreibe Karten und danach den fälligen Portfolio-Brief jeweils mit dem context_token aus dem zuvor gelesenen Kontext. Melde nicht behebbare Fehler mit `watchtower runs fail {}`. Schließe mit `watchtower runs finish {}` ab und behaupte Vollständigkeit nur, wenn der zurückgegebene Status complete ist.", run["id"], if projects.is_empty() { "keine" } else { &projects }, run["id"], run["id"]))
    }

    fn pending_run_replies(
        &self,
        project: &Project,
        card: Option<&StoredCard>,
        reactions: &[Feedback],
    ) -> usize {
        reactions
            .iter()
            .filter(|reply| {
                reply.project_id.as_deref() == Some(project.id.as_str())
                    && kind_of(reply) == "question"
                    && matches!(reply.value.as_str(), "answered" | "done")
                    && card.is_none_or(|card| match &card.owner_basis {
                        Some(basis) => {
                            basis["questions"].get(&reply.item_id)
                                != Some(&json!({"value": reply.value, "note": reply.note}))
                        }
                        None => reply.at > card.created_at,
                    })
            })
            .count()
    }

    fn agent_run_view(&self, run: &crate::store::StoredAgentRun, finishing: bool) -> Result<Value> {
        if let Some(result) = run
            .result
            .as_ref()
            .filter(|result| result["state"] == "complete")
        {
            return Ok(result.clone());
        }
        let due = self.due(14, false)?;
        let reactions = self.store.feedback()?;
        let registered = self.store.projects()?;
        let mut initial = run.initial["projects"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for project in registered
            .iter()
            .filter(|project| project.status != "archived")
        {
            if !initial.iter().any(|entry| entry["id"] == project.id) {
                initial.push(json!({"id": project.id, "name": project.name, "initial_version": 0, "was_due": true}));
            }
        }
        let mut projects = Vec::new();
        for initial in initial {
            let id = initial["id"].as_str().unwrap_or("");
            let current = registered.iter().find(|project| project.id == id);
            let card = self.store.latest_card(id)?;
            let version = card.as_ref().map(|card| card.version).unwrap_or(0);
            let mut item = json!({
                "id": id, "name": current.map(|project| project.name.as_str()).unwrap_or(initial["name"].as_str().unwrap_or(id)),
                "initial_version": initial["initial_version"], "card_version": version,
                "was_due": initial["was_due"], "status": "current", "reason": null,
                "pending_owner_replies": 0, "error": null,
            });
            if current.is_none_or(|project| project.status == "archived") {
                item["reason"] = json!(if current.is_some() {
                    "project_archived"
                } else {
                    "project_removed"
                });
                projects.push(item);
                continue;
            }
            let project = current.expect("active project was checked");
            item["pending_owner_replies"] =
                json!(self.pending_run_replies(project, card.as_ref(), &reactions));
            let facts = self.store.facts(id)?;
            item["facts_collected_at"] = json!(facts.as_ref().map(|facts| &facts.collected_at));
            if facts.as_ref().is_none_or(|facts| facts.data["ok"] != true) {
                item["status"] = json!("failed");
                item["error"] = facts
                    .as_ref()
                    .and_then(|facts| facts.data.get("error"))
                    .cloned()
                    .unwrap_or_else(|| json!("facts_missing"));
                item["reason"] = item["error"].clone();
            } else if card.is_none() {
                item["status"] = json!("pending");
                item["reason"] = json!("no_card");
            } else if let Some(reason) = due["projects"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| entry["id"] == id)
            {
                item["status"] = json!("pending");
                item["reason"] = reason["reason"].clone();
            }
            if let Some(failure) = run
                .failures
                .get(id)
                .filter(|failure| version <= failure["version"].as_i64().unwrap_or(0))
            {
                item["status"] = json!("failed");
                item["reason"] = json!("agent_failed");
                item["error"] = failure["error"].clone();
            }
            projects.push(item);
        }
        let current = projects
            .iter()
            .filter(|project| project["status"] == "current")
            .count();
        let failed = projects
            .iter()
            .filter(|project| project["status"] == "failed")
            .count();
        let pending = projects.len() - current - failed;
        let version = self
            .store
            .latest_portfolio()?
            .map(|brief| brief.version)
            .unwrap_or(0);
        let mut portfolio = json!({
            "initial_version": run.initial["portfolio_version"], "version": version,
            "status": if due["portfolio"]["due"] == true { "pending" } else { "current" },
            "reason": due["portfolio"]["reason"], "error": null,
        });
        if let Some(failure) = run
            .failures
            .get("@portfolio")
            .filter(|failure| version <= failure["version"].as_i64().unwrap_or(0))
        {
            portfolio["status"] = json!("failed");
            portfolio["reason"] = json!("agent_failed");
            portfolio["error"] = failure["error"].clone();
        }
        let complete = pending == 0 && failed == 0 && portfolio["status"] == "current";
        let state = if finishing && complete {
            "complete"
        } else if failed > 0 || portfolio["status"] == "failed" {
            "failed"
        } else if finishing || run.finished_at.is_some() {
            "incomplete"
        } else if (self.clock)() - parse(&run.started_at) >= chrono::Duration::hours(2) {
            "stalled"
        } else {
            "running"
        };
        Ok(json!({
            "id": run.id, "started_at": run.started_at,
            "finished_at": if finishing { Some(self.now()) } else { run.finished_at.clone() },
            "state": state, "counts": {"total": projects.len(), "current": current, "pending": pending, "failed": failed},
            "projects": projects, "portfolio": portfolio,
        }))
    }

    /// Stored data only: opening the window never starts Git or the agent.
    pub fn run_status(&self) -> Result<Value> {
        let runs = self.store.agent_runs()?;
        let latest = runs
            .first()
            .map(|run| self.agent_run_view(run, false))
            .transpose()?;
        let state = latest
            .as_ref()
            .and_then(|run| run["state"].as_str())
            .unwrap_or("unknown");
        let reactions = self.store.feedback()?;
        let mut pending_owner_replies = 0;
        for project in self
            .store
            .projects()?
            .into_iter()
            .filter(|project| project.status != "archived")
        {
            pending_owner_replies += self.pending_run_replies(
                &project,
                self.store.latest_card(&project.id)?.as_ref(),
                &reactions,
            );
        }
        let repair_task = latest.as_ref().and_then(Self::agent_run_repair_task);
        let last_successful = self.store.last_successful_agent_run()?.map(|run| json!({"id": run.id, "started_at": run.started_at, "finished_at": run.finished_at}));
        let history: Vec<Value> = runs.iter().map(|run| {
            let state = latest.as_ref().filter(|latest| latest["id"] == run.id).and_then(|latest| latest["state"].as_str())
                .or_else(|| run.result.as_ref().and_then(|result| result["state"].as_str()))
                .unwrap_or_else(|| if (self.clock)() - parse(&run.started_at) >= chrono::Duration::hours(2) { "stalled" } else { "running" });
            json!({"id": run.id, "started_at": run.started_at, "finished_at": run.finished_at, "state": state})
        }).collect();
        Ok(
            json!({"state": state, "latest": latest, "last_successful": last_successful, "pending_owner_replies": pending_owner_replies, "repair_task": repair_task, "history": history}),
        )
    }

    // ---------- the window ----------
    /// Records that the owner opened a project. When the open before was at least
    /// `SINCE_DAYS` ago, returns what happened since: commits, card updates, a phase
    /// change and the card's changes and milestones dated after it.
    pub fn open_project(&self, id: &str) -> Result<Value> {
        let p = self.project(id)?;
        let now = (self.clock)();
        let long_ago = p
            .opened_at
            .as_deref()
            .is_none_or(|l| now - parse(l) > chrono::Duration::hours(1));
        let reference = if long_ago {
            p.opened_at.clone()
        } else {
            p.previous_opened_at.clone()
        };
        self.store
            .set_opened(id, &stamp(now), reference.as_deref())?;
        let Some(then) = reference.filter(|r| (now - parse(r)).num_days() >= SINCE_DAYS) else {
            return Ok(json!({"since": null}));
        };
        let card_then = self.store.card_at(id, &then)?;
        let card_now = self.store.latest_card(id)?.map(|c| c.data);
        let day = parse(&then).with_timezone(&Local).date_naive().to_string();
        let dated_after = |key: &str| -> Vec<Value> {
            card_now
                .as_ref()
                .and_then(|c| c[key].as_array())
                .into_iter()
                .flatten()
                .filter(|x| x["date"].as_str().is_some_and(|d| d >= day.as_str()))
                .cloned()
                .collect()
        };
        let phase = |c: &Option<Value>| {
            c.as_ref()
                .and_then(|c| c["phase"].as_str().map(str::to_owned))
        };
        let (phase_then, phase_now) = (phase(&card_then), phase(&card_now));
        Ok(json!({"since": {
            "at": then,
            "days": (now - parse(&then)).num_days(),
            "commits": facts::count_after(&p.path, &then),
            "card_updates": self.store.cards_after(id, &then)?,
            "phase": if phase_then.is_some() && phase_then != phase_now { json!({"from": phase_then, "to": phase_now}) } else { Value::Null },
            "recent_changes": dated_after("recent_changes"),
            "milestones": dated_after("milestones"),
        }}))
    }

    /// Changes remain unread until the owner explicitly acknowledges the exact
    /// snapshot they saw, including across window refreshes and restarts.
    fn changes_digest(&self, first_visit: bool) -> Result<Value> {
        let state = self.store.changes_snapshot(first_visit, &self.now())?;
        let current_snapshot = digest_measurements(&state.current, state.baseline.as_ref());
        let mut entries = Vec::new();
        if let Some(baseline) = &state.baseline {
            let before = &baseline["snapshot"];
            let old_feedback = before["feedback"].as_array().cloned().unwrap_or_default();
            let feedback = current_snapshot["feedback"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for project in current_snapshot["projects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["status"] != "archived")
            {
                let old = before["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| p["id"] == project["id"] && p["status"] != "archived");
                let previous = old
                    .map(|p| digest_items(p, &old_feedback))
                    .unwrap_or_default();
                let current = digest_items(project, &feedback);
                for (key, item) in &current {
                    let prior = previous.get(key);
                    if prior.is_some_and(|old| old.comparison == item.comparison) {
                        continue;
                    }
                    let (kind, prefix) = match item.category {
                        "applied" => ("decision_applied", "Antwort in Karte berücksichtigt"),
                        "risk" if prior.is_some() => ("new", "Risiko verändert"),
                        "risk" => ("new", "Neues Risiko"),
                        "open" if prior.is_some() => ("new", "Offenes Ende aktualisiert"),
                        "open" => ("new", "Neu offen"),
                        "question" if prior.is_some() => ("new", "Frage aktualisiert"),
                        "question" => ("new", "Neue Frage"),
                        "decision" => ("new", "Entscheidung dokumentiert"),
                        _ => ("new", "Neu"),
                    };
                    entries.push(json!({
                        "project": project["id"], "kind": kind,
                        "title": format!("{prefix}: {}", item.title),
                        "detail": item.detail, "next_step": item.next_step,
                    }));
                }
                for (key, item) in &previous {
                    if current.contains_key(key) {
                        continue;
                    }
                    let (prefix, detail) = match item.category {
                        "risk" => ("Nicht mehr gemessen", "Die aktuellen Git-Fakten zeigen dieses Risiko nicht mehr."),
                        "open" => ("Nicht mehr offen in der Karte", "Die aktuelle Karte führt dieses offene Ende nicht mehr auf."),
                        "question" => ("Frage nicht mehr offen", "Die Frage ist beantwortet, abgehakt oder wird in der aktuellen Karte nicht mehr gestellt."),
                        _ => continue,
                    };
                    // A failed measurement is not proof that measured risks
                    // disappeared. Keep the last known warning unread instead.
                    if item.category == "risk"
                        && (project["facts"]["ok"] != true
                            || (key == "risk:ci_red" && project["facts"]["ci"]["state"] != "green")
                            || (key == "risk:ci_none"
                                && !matches!(
                                    project["facts"]["ci"]["state"].as_str(),
                                    Some("green" | "red")
                                ))
                            || (key == "risk:no_remote" && project["facts"]["has_remote"] != true))
                    {
                        continue;
                    }
                    entries.push(json!({
                        "project": project["id"], "kind": "resolved",
                        "title": format!("{prefix}: {}", item.title),
                        "detail": detail, "next_step": "",
                    }));
                }
            }
        }
        Ok(json!({
            "since": state.baseline.as_ref().map(|b| b["at"].clone()),
            "token": state.token, "total": entries.len(), "entries": entries,
            "has_baseline": state.baseline.is_some(),
        }))
    }

    pub fn mark_changes_read(&self, token: &str) -> Result<Value> {
        let state = self.store.changes_snapshot(false, &self.now())?;
        let snapshot = digest_measurements(&state.current, state.baseline.as_ref());
        if state.token != token
            || !self
                .store
                .acknowledge_changes(token, &snapshot, &self.now())?
        {
            return Err(WatchtowerError::new(
                "stale_changes",
                "New changes arrived while you were reading.",
            )
            .hint("Refresh the overview, read the new changes, then mark them as read."));
        }
        Ok(json!({"ok": true, "changes": self.changes_digest(false)?}))
    }

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
            let long_ago = last
                .as_ref()
                .and_then(Value::as_str)
                .is_none_or(|l| now - parse(l) > chrono::Duration::hours(1));
            if long_ago {
                previous = last.clone().unwrap_or(Value::Null);
                self.store.set_pref("previous_visit", &previous)?;
            }
            self.store.set_pref("last_visit", &json!(stamp(now)))?;
        }
        let mut projects = Vec::new();
        let reactions = self.store.feedback()?;
        for p in self.store.projects()? {
            let stored = self.store.facts(&p.id)?;
            let card = self.store.latest_card(&p.id)?;
            let outdated = card_outdated(
                card.as_ref(),
                stored.as_ref(),
                &p,
                &reactions,
                &self.warning_acknowledgements(&p.id)?,
                &self.project_task_basis(&p.id)?,
            );
            // the headline of every card version, newest first: the project's story in one line per update
            let history: Vec<Value> = self
                .store
                .card_history(&p.id)?
                .into_iter()
                .enumerate()
                .map(|(i, (at, data))| json!({"version": i + 1, "at": at, "headline": data["headline"], "phase": data["phase"]}))
                .rev()
                .collect();
            projects.push(extend(
                pick(
                    &project_json(&p),
                    &[
                        "id",
                        "name",
                        "path",
                        "status",
                        "status_note",
                        "goal",
                        "goal_due",
                    ],
                ),
                json!({
                    "facts": stored.as_ref().map(|s| s.data.clone()),
                    "facts_collected_at": stored.map(|s| s.collected_at),
                    "card": card.as_ref().map(|c| c.data.clone()),
                    "card_updated_at": card.map(|c| c.created_at),
                    "card_outdated": outdated,
                    "warnings": self.project_warnings(&p.id)?,
                    "history": history,
                }),
            ));
        }
        let changes = self.changes_digest(visit)?;
        let portfolio = self.store.latest_portfolio()?;
        let feedback: Map<String, Value> = reactions
            .into_iter()
            .map(|f| {
                let until = snoozed_until(&f);
                (
                    f.item_id,
                    json!({"value": f.value, "at": f.at, "note": f.note, "snoozed_until": until}),
                )
            })
            .collect();
        Ok(json!({
            "today": self.today().to_string(),
            "now": stamp(now),
            "previous_visit": previous,
            "changes": changes,
            "projects": projects,
            "portfolio": portfolio.as_ref().map(|p| p.data.clone()),
            "portfolio_updated_at": portfolio.map(|p| p.created_at),
            "ideas": self.store.ideas()?.iter().map(idea_json).collect::<Vec<_>>(),
            "tasks": self.store.tasks()?,
            "review_week": self.review_week().0,
            "run_status": self.run_status()?,
            "feedback": feedback,
            "snooze_days": SNOOZE_DAYS,
            "prefs": {
                "style": self.store.pref("style")?.unwrap_or_else(|| json!("handover")),
                "hidden_modules": self.store.pref("hidden_modules")?.unwrap_or_else(|| json!([])),
            },
        }))
    }
}

// An assigned task keeps its own snapshot; rewriting cards never loses the handoff.
impl Watchtower {
    pub fn create_task(&self, payload: Value) -> Result<Value> {
        let object = payload
            .as_object()
            .ok_or_else(|| task_error("The task must be a JSON object."))?;
        let allowed = [
            "projects",
            "source_id",
            "title",
            "why",
            "evidence",
            "done_when",
        ];
        if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(task_error(format!("Unknown task field '{key}'.")));
        }
        let mut projects = object
            .get("projects")
            .and_then(Value::as_array)
            .ok_or_else(|| task_error("projects must be an array of project ids."))?
            .iter()
            .map(|id| {
                id.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| task_error("Every project id must be a string."))
            })
            .collect::<Result<Vec<_>>>()?;
        projects.sort();
        projects.dedup();
        let project_snapshots = projects
            .iter()
            .map(|id| {
                self.project(id)
                    .map(|project| pick(&project_json(&project), &["id", "name", "path"]))
            })
            .collect::<Result<Vec<_>>>()?;
        let title = task_text(&payload, "title", true, 600)?;
        let why = task_text(&payload, "why", false, 2000)?;
        let evidence = task_text(&payload, "evidence", false, 4000)?;
        let done_when = task_text(&payload, "done_when", false, 2000)?;
        let source = task_text(&payload, "source_id", false, 1000)?;
        let source_id = (!source.is_empty()).then_some(source);
        let source_key = source_id
            .as_ref()
            .map(|source| json!([projects, source]).to_string());
        let at = self.now();
        let data = json!({
            "projects": projects, "project_snapshots": project_snapshots, "source_id": source_id, "title": title, "why": why,
            "evidence": evidence, "done_when": done_when, "created_at": at, "updated_at": at,
            "reported_at": null, "completed_at": null, "result": "", "result_evidence": "",
            "history": [{"actor": "owner", "status": "assigned", "at": at}],
        });
        let (task, created) = self
            .store
            .create_tracked_task(source_key.as_deref(), &data)?;
        Ok(json!({"created": created, "task": task}))
    }

    pub fn tasks(&self, project_id: Option<&str>) -> Result<Value> {
        if let Some(id) = project_id {
            self.project(id)?;
        }
        let tasks: Vec<Value> = self
            .store
            .tasks()?
            .into_iter()
            .filter(|task| {
                project_id.is_none_or(|id| {
                    task["projects"]
                        .as_array()
                        .is_some_and(|projects| projects.iter().any(|project| project == id))
                })
            })
            .collect();
        Ok(json!({"tasks": tasks}))
    }

    pub fn report_task(
        &self,
        id: i64,
        status: &str,
        result: &str,
        evidence: &str,
    ) -> Result<Value> {
        let target = match status {
            "done" => "review",
            "blocked" => "blocked",
            _ => return Err(WatchtowerError::new("invalid_task_status", "Agents report a task as done or blocked.")
                .hint("Use done with a result and evidence; completion still needs the owner's confirmation.")),
        };
        let result = task_text(&json!({"result": result}), "result", true, 4000)?;
        let evidence = task_text(
            &json!({"evidence": evidence}),
            "evidence",
            target == "review",
            4000,
        )?;
        let mut task = self.tracked_task(id)?;
        if !active_task(&task) {
            return Err(WatchtowerError::new(
                "invalid_task_transition",
                "This task is already closed.",
            )
            .hint("Read the task's current state. Only the owner can reopen it."));
        }
        if task["status"] == target
            && task["result"] == result
            && task["result_evidence"] == evidence
        {
            return Ok(json!({"task": task}));
        }
        let at = self.now();
        task["result"] = json!(result);
        task["result_evidence"] = json!(evidence);
        task["reported_at"] = json!(at);
        task["completed_at"] = Value::Null;
        self.save_task_change(task, target, json!({"actor": "agent", "status": target, "at": at, "result": result, "evidence": evidence}))
    }

    pub fn set_task_status(&self, id: i64, status: &str) -> Result<Value> {
        self.change_task_status(id, status, None)
    }

    /// Confirm only the report or state that was actually shown to the owner.
    pub fn set_task_status_guarded(&self, id: i64, status: &str, revision: i64) -> Result<Value> {
        self.change_task_status(id, status, Some(revision))
    }

    fn change_task_status(&self, id: i64, status: &str, revision: Option<i64>) -> Result<Value> {
        if !matches!(status, "assigned" | "done" | "cancelled") {
            return Err(WatchtowerError::new("invalid_task_status", "Owner task status must be assigned, done or cancelled.")
                .hint("assigned reopens a task; done confirms its reported result; cancelled closes it without completion."));
        }
        let mut task = self.tracked_task(id)?;
        if revision.is_some_and(|revision| task["revision"] != revision) {
            return Err(WatchtowerError::new("task_changed", "The task changed since it was displayed.")
                .hint("Reload the task and read its latest result before confirming, reopening or cancelling it."));
        }
        if task["status"] == status {
            return Ok(json!({"task": task}));
        }
        if (status == "done" && task["status"] != "review")
            || (status == "cancelled" && task["status"] == "done")
        {
            return Err(WatchtowerError::new("invalid_task_transition", "This task cannot make that transition.")
                .hint("Confirm done only after an agent report is ready for review. Reopen a completed task before cancelling it."));
        }
        if status == "assigned"
            && !task["source_id"].is_null()
            && self.store.tasks()?.iter().any(|other| {
                other["id"] != task["id"]
                    && active_task(other)
                    && other["source_id"] == task["source_id"]
                    && other["projects"] == task["projects"]
            })
        {
            return Err(WatchtowerError::new(
                "invalid_task_transition",
                "Another active task already tracks this source.",
            )
            .hint("Continue or close that task before reopening this one."));
        }
        let at = self.now();
        task["completed_at"] = if status == "done" {
            json!(at)
        } else {
            Value::Null
        };
        self.save_task_change(
            task,
            status,
            json!({"actor": "owner", "status": status, "at": at}),
        )
    }

    fn tracked_task(&self, id: i64) -> Result<Value> {
        self.store.task(id)?.ok_or_else(|| {
            WatchtowerError::new("unknown_task", format!("Task {id} does not exist."))
                .hint("Use `watchtower tasks list` to find the current task ids.")
        })
    }

    fn save_task_change(&self, mut task: Value, status: &str, event: Value) -> Result<Value> {
        let id = task["id"].as_i64().expect("stored task id");
        let revision = task["revision"].as_i64().expect("stored task revision");
        task["updated_at"] = event["at"].clone();
        task["history"]
            .as_array_mut()
            .expect("stored task history")
            .push(event);
        task["status"] = json!(status);
        task["revision"] = json!(revision + 1);
        let saved = self.store.replace_task(id, revision, status, &task).map_err(|error| {
            if matches!(&error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::ConstraintViolation) {
                WatchtowerError::new("invalid_task_transition", "Another active task already tracks this source.")
                    .hint("Reload the tasks before continuing.")
            } else { error.into() }
        })?;
        if !saved {
            return Err(WatchtowerError::new(
                "task_changed",
                "The task changed while this update was being saved.",
            )
            .hint("Reload the task and check the latest owner decision before retrying."));
        }
        Ok(json!({"task": task}))
    }
}

fn active_task(task: &Value) -> bool {
    matches!(
        task["status"].as_str(),
        Some("assigned" | "blocked" | "review")
    )
}

fn task_error(message: impl Into<String>) -> WatchtowerError {
    WatchtowerError::new("invalid_task", message)
        .hint("Use projects (an array of ids), title, and optional source_id, why, evidence and done_when text.")
}

fn task_text(payload: &Value, field: &str, required: bool, max: usize) -> Result<String> {
    let invalid = |message: String| {
        WatchtowerError::new("invalid_task", message).hint(format!(
            "Send {field} as {}text with at most {max} characters.",
            if required { "nonempty " } else { "" }
        ))
    };
    let text = match payload.get(field) {
        None | Some(Value::Null) if !required => "",
        Some(Value::String(text)) => text.trim(),
        _ => return Err(invalid(format!("{field} must be text."))),
    };
    if (required && text.is_empty()) || text.chars().count() > max {
        return Err(invalid(format!(
            "{field} must {}contain at most {max} characters.",
            if required { "be nonempty and " } else { "" }
        )));
    }
    Ok(text.to_owned())
}
