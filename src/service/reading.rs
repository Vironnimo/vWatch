//! Report indexes, semantic reading state and owner window projections.
use super::*;

/// Opening a project at least this many days after the last time shows what happened since.
const SINCE_DAYS: i64 = 7;

/// Semantic identities keep regenerated item ids and advancing age counters out
/// of the owner's unread list. The visible text remains exactly as authored.
pub(super) fn digest_text(value: &Value) -> String {
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

/// Compare what the owner reads, not generated ids, elapsed ages or retired estimates.
/// Lists are compared as sets of semantic values so merely reordering a report is
/// not another unread report. Event-level markers are calculated separately.
fn reading_content(value: &Value) -> Value {
    match value {
        Value::String(_) => json!(digest_text(value)),
        Value::Array(items) => {
            let mut items: Vec<Value> = items.iter().map(reading_content).collect();
            items.sort_by_key(Value::to_string);
            Value::Array(items)
        }
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !["id", "effort", "age_days"].contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), reading_content(value)))
                .collect(),
        ),
        value => value.clone(),
    }
}

pub(super) fn document_item(kind: Kind, doc: &Value, item_id: &str) -> Option<Value> {
    for (list, _, item_kind) in kind.items() {
        if let Some(item) = doc[*list]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["id"] == item_id)
        {
            let mut snapshot = item.clone();
            snapshot.as_object_mut()?.remove("id");
            snapshot["kind"] = json!(item_kind);
            return Some(snapshot);
        }
    }
    None
}

/// What the window needs from one saved card or brief version: its headline and
/// phase, and the ids of its rated items as (index in `Kind::items`, id).
#[derive(Default, Debug, PartialEq)]
struct Digest {
    headline: Value,
    phase: Value,
    items: Vec<(usize, String)>,
}

/// The fields a digest reads from a saved document; everything else is skipped
/// unparsed. A document of another shape falls back to a full parse.
#[derive(serde::Deserialize)]
struct DigestFields {
    #[serde(default)]
    headline: Value,
    #[serde(default)]
    phase: Value,
    #[serde(default)]
    suggestions: Option<Vec<Listed>>,
    #[serde(default)]
    waiting_on_you: Option<Vec<Listed>>,
    #[serde(default)]
    focus: Option<Vec<Listed>>,
    #[serde(default)]
    patterns: Option<Vec<Listed>>,
    #[serde(default)]
    synergies: Option<Vec<Listed>>,
    #[serde(default)]
    new_apps: Option<Vec<Listed>>,
    #[serde(default)]
    park_or_revive: Option<Vec<Listed>>,
}

#[derive(serde::Deserialize)]
struct Listed {
    #[serde(default)]
    id: Value,
}

fn digest(kind: Kind, text: &str) -> Digest {
    let fields = text
        .trim_start()
        .starts_with('{')
        .then(|| serde_json::from_str::<DigestFields>(text).ok())
        .flatten();
    let Some(mut fields) = fields else {
        return full_digest(kind, text);
    };
    let mut items = Vec::new();
    for (list, (name, _, _)) in kind.items().iter().enumerate() {
        let listed = match *name {
            "suggestions" => fields.suggestions.take(),
            "waiting_on_you" => fields.waiting_on_you.take(),
            "focus" => fields.focus.take(),
            "patterns" => fields.patterns.take(),
            "synergies" => fields.synergies.take(),
            "new_apps" => fields.new_apps.take(),
            "park_or_revive" => fields.park_or_revive.take(),
            _ => return full_digest(kind, text),
        };
        items.extend(
            listed
                .into_iter()
                .flatten()
                .filter_map(|item| match item.id {
                    Value::String(id) => Some((list, id)),
                    _ => None,
                }),
        );
    }
    Digest {
        headline: fields.headline,
        phase: fields.phase,
        items,
    }
}

fn full_digest(kind: Kind, text: &str) -> Digest {
    let doc: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let items = kind
        .items()
        .iter()
        .enumerate()
        .flat_map(|(list, (name, _, _))| {
            doc[*name]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["id"].as_str())
                .map(move |id| (list, id.to_owned()))
        })
        .collect();
    Digest {
        headline: doc["headline"].clone(),
        phase: doc["phase"].clone(),
        items,
    }
}

struct IndexedVersion {
    row: i64,
    created_at: String,
    digest: Digest,
    /// The newest version of its project or of the briefs, which can still be
    /// rewritten in place; it is read again until a newer version exists.
    newest: bool,
}

/// Every saved card and brief version, digested once. Older versions never change,
/// so the window's repeated reads only load new and still-newest versions.
#[derive(Default)]
pub(super) struct DocumentIndex {
    /// Card versions per project id, oldest first.
    cards: std::collections::HashMap<String, Vec<IndexedVersion>>,
    briefs: Vec<IndexedVersion>,
    cards_after: i64,
    briefs_after: i64,
}

impl DocumentIndex {
    fn refresh(&mut self, store: &Store) -> Result<()> {
        let newest = |v: &&IndexedVersion| v.newest;
        let cards_again: Vec<i64> = self
            .cards
            .values()
            .flatten()
            .filter(newest)
            .map(|v| v.row)
            .collect();
        let briefs_again: Vec<i64> = self.briefs.iter().filter(newest).map(|v| v.row).collect();
        let rows = store.document_rows(
            self.cards_after,
            &cards_again,
            self.briefs_after,
            &briefs_again,
        )?;
        fn place(versions: &mut Vec<IndexedVersion>, version: IndexedVersion) {
            match versions.binary_search_by_key(&version.row, |v| v.row) {
                Ok(i) => versions[i] = version,
                Err(i) => versions.insert(i, version),
            }
        }
        for row in rows.cards {
            self.cards_after = self.cards_after.max(row.id);
            let version = IndexedVersion {
                row: row.id,
                digest: digest(Kind::Card, &row.data),
                created_at: row.created_at,
                newest: rows.latest_cards.contains(&row.id),
            };
            place(
                self.cards
                    .entry(row.project_id.unwrap_or_default())
                    .or_default(),
                version,
            );
        }
        for row in rows.briefs {
            self.briefs_after = self.briefs_after.max(row.id);
            let version = IndexedVersion {
                row: row.id,
                digest: digest(Kind::Portfolio, &row.data),
                created_at: row.created_at,
                newest: rows.latest_brief == Some(row.id),
            };
            place(&mut self.briefs, version);
        }
        Ok(())
    }

    fn cards(&self, project: &str) -> &[IndexedVersion] {
        self.cards.get(project).map_or(&[], Vec::as_slice)
    }
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
    // Only authored developments belong in this reading list. Measured Git
    // state has its own project view, including when old baselines contain it.
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

impl VWatch {
    /// Record an owner visit; after SINCE_DAYS show commits and authored changes since it.
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
    fn changes_digest(&self) -> Result<Value> {
        self.reading_state().map(|(changes, _)| changes)
    }

    fn reading_state(&self) -> Result<(Value, Map<String, Value>)> {
        let state = self.store.changes_snapshot()?;
        let current_snapshot = &state.current;
        let mut entries = Vec::new();
        let mut projects = Map::new();
        if let Some(baseline) = &state.baseline {
            let before = &baseline["snapshot"];
            let old_feedback = before["feedback"].as_array().map_or(&[][..], Vec::as_slice);
            let feedback = current_snapshot["feedback"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
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
                    .map(|p| digest_items(p, old_feedback))
                    .unwrap_or_default();
                let current = digest_items(project, feedback);
                let unread_recent: Vec<usize> = project["card"]["recent_changes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(index, item)| {
                        let key = digest_identity("change", item);
                        previous
                            .get(&key)
                            .is_none_or(|prior| {
                                prior.comparison != json!(digest_text(&item["detail"]))
                            })
                            .then_some(index)
                    })
                    .collect();
                let first_entry = entries.len();
                for (key, item) in &current {
                    let prior = previous.get(key);
                    if prior.is_some_and(|old| old.comparison == item.comparison) {
                        continue;
                    }
                    let (kind, prefix) = match item.category {
                        "applied" => ("decision_applied", "Antwort in Karte berücksichtigt"),
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
                        "open" => ("Nicht mehr offen in der Karte", "Die aktuelle Karte führt dieses offene Ende nicht mehr auf."),
                        "question" => ("Frage nicht mehr offen", "Die Frage ist beantwortet, abgehakt oder wird in der aktuellen Karte nicht mehr gestellt."),
                        _ => continue,
                    };
                    entries.push(json!({
                        "project": project["id"], "kind": "resolved",
                        "title": format!("{prefix}: {}", item.title),
                        "detail": detail, "next_step": "",
                    }));
                }
                if let Some(id) = project["id"].as_str() {
                    let old_card = old.map_or(&Value::Null, |p| &p["card"]);
                    projects.insert(id.to_owned(), json!({
                        "card_unread": entries.len() != first_entry
                            || (project["card"] != *old_card
                                && reading_content(&project["card"]) != reading_content(old_card)),
                        "unread_recent_changes": unread_recent,
                    }));
                }
            }
        }
        Ok((
            json!({
                "since": state.baseline.as_ref().map(|b| b["at"].clone()),
                "token": state.token, "total": entries.len(), "entries": entries,
                "has_baseline": state.baseline.is_some(),
            }),
            projects,
        ))
    }

    pub fn mark_changes_read(&self, token: &str) -> Result<Value> {
        let state = self.store.changes_snapshot()?;
        if state.token != token
            || !self
                .store
                .acknowledge_changes(token, &state.current, &self.now())?
        {
            return Err(VWatchError::new(
                "stale_changes",
                "New changes arrived while you were reading.",
            )
            .hint("Refresh the overview, read the new changes, then mark them as read."));
        }
        Ok(json!({"ok": true, "changes": self.changes_digest()?}))
    }

    /// Changes whenever another connection, such as an agent's CLI call, commits a
    /// write. This service's own writes leave it unchanged.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self.store.data_version()?)
    }

    /// The window state with the `data_version` it reflects. The version is read
    /// first, so any write the state might miss changes it.
    pub fn versioned_state(&self, visit: bool) -> Result<(Value, i64)> {
        let version = self.data_version()?;
        Ok((self.state(visit)?, version))
    }

    /// Everything the window renders. Visits record re-entry context; only an
    /// explicit read acknowledgement changes unread report and event markers.
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
        // Capture the acknowledgement token before loading displayed cards. A
        // concurrent later write must invalidate this token, never be silently
        // acknowledged as though it had already been shown in the window.
        let (changes, reading) = self.reading_state()?;
        let mut projects = Vec::new();
        let reactions = self.store.feedback()?;
        let mut tasks = self.store.tasks()?;
        let mut documents = self.documents.lock().unwrap_or_else(|e| e.into_inner());
        documents.refresh(&self.store)?;
        let documents = &*documents;
        let suggestions = Kind::Card
            .items()
            .iter()
            .position(|(list, _, _)| *list == "suggestions");
        let registered = self.store.projects()?;
        for p in &registered {
            let stored = self.store.facts(&p.id)?;
            let card = self.store.latest_card(&p.id)?;
            let acknowledgements = self.store.warning_acknowledgements(&p.id)?;
            let outdated = card_outdated(
                card.as_ref(),
                stored.as_ref(),
                p,
                &reactions,
                &acknowledgements,
                &task_basis(&tasks, &p.id),
            );
            let warnings = acknowledged_warnings(
                stored.as_ref().map_or(&Value::Null, |s| &s.data),
                &acknowledgements,
            );
            let pending_owner_replies = self.pending_run_replies(p, card.as_ref(), &reactions);
            let risk_streaks = self.project_streaks(&p.id, stored.as_ref())?;
            let item_persistence = match &card {
                Some(c) => self.card_persistence(&p.id, &c.data, &reactions)?,
                None => Vec::new(),
            };
            let (facts, facts_collected_at) = stored.map(|s| (s.data, s.collected_at)).unzip();
            let versions = documents.cards(&p.id);
            let suggestion_ids: Vec<&str> = card
                .as_ref()
                .and_then(|c| c.data["suggestions"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|item| item["id"].as_str())
                .collect();
            // An edited or returning idea keeps the date of its first saved ID;
            // never use the current card's date.
            let mut suggestion_first_seen = Map::new();
            for version in versions {
                for (list, id) in &version.digest.items {
                    if Some(*list) == suggestions && suggestion_ids.contains(&id.as_str()) {
                        suggestion_first_seen
                            .entry(id.clone())
                            .or_insert_with(|| json!(version.created_at));
                    }
                }
            }
            let (card_data, card_updated_at) = card.map(|c| (c.data, c.created_at)).unzip();
            // the headline of every card version, newest first: the project's story in one line per update
            let history: Vec<Value> = versions
                .iter()
                .enumerate()
                .map(|(i, v)| json!({"version": i + 1, "at": v.created_at, "headline": v.digest.headline, "phase": v.digest.phase}))
                .rev()
                .collect();
            projects.push(extend(
                pick(
                    &project_json(p),
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
                    "facts": facts,
                    "facts_collected_at": facts_collected_at,
                    "card": card_data,
                    "pending_owner_replies": pending_owner_replies,
                    "card_updated_at": card_updated_at,
                    "suggestion_first_seen": suggestion_first_seen,
                    "card_outdated": outdated,
                    "warnings": warnings,
                    "risk_streaks": risk_streaks,
                    "item_persistence": item_persistence,
                    "history": history,
                }),
            ));
        }
        for project in &mut projects {
            let read = project["id"].as_str().and_then(|id| reading.get(id));
            project["card_unread"] = read
                .map(|read| read["card_unread"].clone())
                .unwrap_or(Value::Null);
            project["unread_recent_changes"] = read
                .map(|read| read["unread_recent_changes"].clone())
                .unwrap_or(Value::Null);
        }
        let portfolio = self.store.latest_portfolio_content()?;
        let brief_ids = |list: &str| -> Vec<String> {
            portfolio
                .as_ref()
                .and_then(|p| p.data[list].as_array())
                .into_iter()
                .flatten()
                .filter_map(|item| item["id"].as_str().map(str::to_owned))
                .collect()
        };
        let (new_app_ids, synergy_ids) = (brief_ids("new_apps"), brief_ids("synergies"));
        // Ratings and task snapshots outlive a removed project. Its archived
        // card history is still the authority for first-seen dates and kinds.
        let missing_projects: std::collections::BTreeSet<String> = reactions
            .iter()
            .filter_map(|feedback| feedback.project_id.as_deref())
            .chain(tasks.iter().flat_map(|task| {
                task["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
            }))
            .filter(|id| !registered.iter().any(|project| project.id == *id))
            .map(str::to_owned)
            .collect();
        // Historical identities keep their first appearance and last authored
        // version, for the items the window shows a date or older task source for.
        // Versions count in this order: watched projects, briefs, removed projects.
        let wanted: std::collections::HashSet<String> = reactions
            .iter()
            .map(|feedback| feedback.item_id.clone())
            .chain(
                tasks
                    .iter()
                    .map(|task| task["source_id"].as_str().unwrap_or("").to_owned()),
            )
            .chain(new_app_ids.iter().chain(&synergy_ids).cloned())
            .collect();
        let mut first_seen_at = std::collections::HashMap::<&str, &str>::new();
        let mut last_seen = std::collections::HashMap::<&str, (Kind, i64)>::new();
        let segments = registered
            .iter()
            .map(|p| (Kind::Card, documents.cards(&p.id)))
            .chain([(Kind::Portfolio, documents.briefs.as_slice())])
            .chain(
                missing_projects
                    .iter()
                    .map(|id| (Kind::Card, documents.cards(id))),
            );
        for (kind, versions) in segments {
            for version in versions {
                for (_, id) in &version.digest.items {
                    if wanted.contains(id) {
                        first_seen_at.entry(id).or_insert(&version.created_at);
                        last_seen.insert(id, (kind, version.row));
                    }
                }
            }
        }
        let first_seen = |id: &str| first_seen_at.get(id).map_or(Value::Null, |at| json!(at));
        let new_app_first_seen: Map<String, Value> = new_app_ids
            .iter()
            .map(|id| (id.clone(), first_seen(id)))
            .collect();
        let synergy_first_seen: Map<String, Value> = synergy_ids
            .iter()
            .map(|id| (id.clone(), first_seen(id)))
            .collect();
        let mut idea_states = self.idea_states(None, true)?;
        let enrich_task = |task: &mut Value| -> Result<()> {
            let source = task["source_id"].as_str().unwrap_or("").to_owned();
            let source = source.as_str();
            let snapshot = task
                .get("source_snapshot")
                .filter(|snapshot| !snapshot.is_null())
                .cloned()
                .or_else(|| {
                    reactions
                        .iter()
                        .find(|feedback| feedback.item_id == source)
                        .and_then(|feedback| feedback.item.clone())
                });
            let snapshot = match (snapshot, last_seen.get(source)) {
                (Some(snapshot), _) => Some(snapshot),
                (None, Some((kind, row))) => Some(
                    self.store
                        .document_data(*kind == Kind::Portfolio, *row)?
                        .and_then(|doc| document_item(*kind, &doc, source))
                        .unwrap_or(Value::Null),
                ),
                (None, None) => self.find_item(source)?.map(|(_, item)| item),
            };
            let date = snapshot
                .as_ref()
                .filter(|snapshot| snapshot["kind"] == "owner_idea")
                .map(|snapshot| snapshot["added_at"].clone())
                .unwrap_or_else(|| first_seen(source));
            task["source_kind"] = snapshot
                .as_ref()
                .map(|snapshot| snapshot["kind"].clone())
                .unwrap_or(Value::Null);
            task["source_snapshot"] = snapshot.unwrap_or(Value::Null);
            task["source_first_seen"] = date;
            Ok(())
        };
        for task in &mut tasks {
            enrich_task(task)?;
        }
        // Older Tasks can predate both source snapshots and retained proposals.
        // Their recovered source identifies the collection entry; selected content
        // and its guard still come together from a fresh transactional snapshot.
        for task in &tasks {
            let Some(id) = task["source_id"].as_str() else {
                continue;
            };
            if !idea_states[id].is_null()
                || !matches!(
                    task["source_kind"].as_str(),
                    Some("suggestion" | "new_app" | "synergy" | "owner_idea")
                )
            {
                continue;
            }
            let snapshot = self.idea_state_projection(id)?;
            let mut state = pick(&snapshot, &["status", "archived", "revision", "task"]);
            state["title"] = task["source_snapshot"]["title"].clone();
            idea_states[id] = state;
        }
        for state in idea_states
            .as_object_mut()
            .expect("idea states")
            .values_mut()
        {
            if !state["task"].is_null() {
                enrich_task(&mut state["task"])?;
            }
        }
        let brief_persistence = self.brief_persistence(portfolio.as_ref(), &reactions)?;
        let unchecked = self.unchecked_ideas(portfolio.as_ref().map(|p| &p.data))?;
        let feedback: Map<String, Value> = reactions
            .into_iter()
            .map(|f| {
                let until = snoozed_until(&f);
                let date = f.item.as_ref().filter(|item| item["kind"] == "owner_idea")
                    .map(|item| item["added_at"].clone()).unwrap_or_else(|| first_seen(&f.item_id));
                (
                    f.item_id,
                    json!({"value": f.value, "at": f.at, "note": f.note, "snoozed_until": until,
                        "item": f.item, "project_id": f.project_id, "title": f.title, "first_seen": date}),
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
            "portfolio_updated_at": portfolio.as_ref().map(|p| &p.created_at),
            "new_app_first_seen": new_app_first_seen,
            "synergy_first_seen": synergy_first_seen,
            "brief_persistence": brief_persistence,
            "ideas": self.store.ideas()?.iter().map(|i|extend(idea_json(i),json!({"checked":!unchecked.contains(&i.id)}))).collect::<Vec<_>>(),
            "proposals": self.store.proposals()?.into_iter().map(without_effort).collect::<Vec<_>>(),
            "tasks": tasks,
            "idea_states": idea_states,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skipping_digest_matches_a_full_parse() {
        let documents = [
            r#"{"headline":"H","phase":"build","suggestions":[{"id":"sg-1","title":"x","nested":{"a":[1,2.5,null]}},{"title":"no id"},{"id":7}],"waiting_on_you":[{"id":"q-1"}],"other":[{"id":"o-1"}]}"#,
            r#"  {"waiting_on_you":null,"suggestions":[]}"#,
            r#"{"suggestions":"not a list","headline":{"odd":true}}"#,
            r#"{"suggestions":[1,"two",{"id":"sg-2"}]}"#,
            r#"{"headline":"first","headline":"second","suggestions":[{"id":"a","id":"b"}]}"#,
            r#"{"focus":[{"id":"fc-1"}],"new_apps":[{"id":"ap-1"},{"id":"ap-2"}],"synergies":[{"id":"sy-1"}],"park_or_revive":[{"id":"vd-1"}],"patterns":[{"id":"pt-1"}]}"#,
            r#"["headline","phase"]"#,
            r#"{"headline":"cut"#,
            "",
            "null",
        ];
        for kind in [Kind::Card, Kind::Portfolio] {
            for text in documents {
                assert_eq!(digest(kind, text), full_digest(kind, text), "{text}");
            }
        }
    }
}
