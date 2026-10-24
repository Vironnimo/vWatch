//! Owner feedback, warning decisions and preference validation.
use super::*;

/// How the owner rates suggestions, patterns, synergies, app ideas and verdicts.
pub const RATINGS: &[&str] = &["up", "later", "down"];

pub const PREFS: &[&str] = &["style", "hidden_modules", "language"];

pub const SNOOZE_DAYS: i64 = 14;

fn reactions_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "question" => &["answered", "done", "later"],
        "focus" => &["done"],
        _ => RATINGS,
    }
}

pub(super) fn kind_of(f: &Feedback) -> &str {
    f.item
        .as_ref()
        .and_then(|i| i["kind"].as_str())
        .unwrap_or("")
}

/// A short label for an item: its title, or for a verdict the project and the verdict.
pub(super) fn title_of(item: &Value) -> String {
    match item["title"].as_str() {
        Some(t) => t.to_owned(),
        None => format!(
            "{}: {}",
            item["project"].as_str().unwrap_or(""),
            item["verdict"].as_str().unwrap_or("")
        ),
    }
}

fn feedback_json(f: &Feedback) -> Value {
    json!({
        "id": f.item_id, "project": f.project_id, "value": f.value, "note": f.note, "at": f.at,
        "snoozed_until": snoozed_until(f),
        "item": f.item.clone().unwrap_or_else(|| json!({"title": f.title})),
    })
}

pub(super) fn feedback_context(f: &Feedback) -> Value {
    let mut feedback = feedback_json(f);
    if matches!(
        feedback["item"]["kind"].as_str(),
        Some("suggestion" | "focus" | "synergy" | "new_app")
    ) {
        if let Some(item) = feedback["item"].as_object_mut() {
            item.remove("effort");
        }
    }
    feedback
}

pub(super) fn snoozed_until(f: &Feedback) -> Option<String> {
    (f.value == "later").then(|| {
        f.snoozed_until.clone().unwrap_or_else(|| {
            (parse(&f.at).with_timezone(&Local).date_naive() + chrono::Duration::days(SNOOZE_DAYS))
                .to_string()
        })
    })
}

/// The owner's ratings counted per kind and type of item: which directions they tend to take up or reject.
pub(super) fn feedback_summary(reactions: &[Feedback]) -> Value {
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

impl VWatch {
    // ---------- feedback and preferences ----------
    /// Records the owner's reaction to an item of the current cards or brief: a rating
    /// (`up`, `later`, `down`) of a suggestion or idea, `answered` (the answer is the note)
    /// or `done` for a question, `done` for a focus item. vWatch keeps the item as it read,
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
            VWatchError::new("invalid_feedback", "The previous reaction must contain value, note and an optional snoozed_until date.")
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
            VWatchError::new(
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
            let receipt = self.store.delete_feedback(item_id, &self.now())?;
            return Ok(with_receipt(
                json!({"item_id": item_id, "value": null}),
                receipt,
            ));
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
                (None, None) => return Err(VWatchError::new(
                    "unknown_item",
                    format!("No current card, brief or owner idea has an item with id '{item_id}'."),
                )
                .hint(
                    "Use an existing item ID from overview, or idea:<id> for an active owner idea from ideas list.",
                )),
            };
        let kind = item
            .as_ref()
            .and_then(|i| i["kind"].as_str())
            .unwrap_or("suggestion");
        let allowed = reactions_for(kind);
        if !allowed.contains(&value) {
            return Err(VWatchError::new(
                "invalid_feedback",
                format!("'{value}' does not fit a {kind}."),
            )
            .hint(format!("Use one of: {}.", allowed.join(", "))));
        }
        let title = match (&item, &existing) {
            (Some(item), _) => title_of(item),
            (None, Some(f)) => f.title.clone(),
            (None, None) => String::new(),
        };
        let at = self.now();
        let default_until = (self.today() + chrono::Duration::days(SNOOZE_DAYS)).to_string();
        let receipt = self
            .store
            .set_feedback(
                &Feedback {
                    item_id: item_id.to_owned(),
                    project_id,
                    title,
                    value: value.to_owned(),
                    at,
                    note: String::new(),
                    item,
                    snoozed_until: None,
                },
                |current| {
                    let note = note.map(|note| note.trim().to_owned()).unwrap_or_else(|| {
                        current
                            .as_ref()
                            .map(|feedback| feedback.note.clone())
                            .unwrap_or_default()
                    });
                    if value == "answered" && note.is_empty() {
                        return None;
                    }
                    let until = if value == "later" {
                        explicit_until
                            .or_else(|| current.as_ref().and_then(snoozed_until))
                            .or(Some(default_until))
                    } else {
                        None
                    };
                    Some((note, until))
                },
            )?
            .ok_or_else(|| {
                VWatchError::new(
                    "answer_missing",
                    "An answer needs its text. Nothing was changed.",
                )
                .hint("Send the answer as the note, or mark the question done.")
            })?;
        // Report the values saved under the transaction, including inherited concurrent edits.
        let current = &receipt.current;
        Ok(with_receipt(
            json!({"item_id": item_id, "value": current["value"], "note": current["note"], "snoozed_until": current["snoozed_until"]}),
            receipt,
        ))
    }

    /// The item with this id in the current cards and brief: (its project, the item with its `kind`).
    pub(super) fn find_item(&self, item_id: &str) -> Result<Option<(Option<String>, Value)>> {
        if let Some(id) = item_id
            .strip_prefix("idea:")
            .and_then(|id| id.parse::<i64>().ok())
        {
            if format!("idea:{id}") == item_id {
                if let Some(idea) = self
                    .store
                    .idea(id)?
                    .filter(|idea| idea["removed_at"].is_null())
                {
                    let assessment = self.store.latest_portfolio()?.and_then(|brief| {
                        brief.data["idea_checks"].as_array().and_then(|checks| {
                            checks.iter().find(|check| check["idea"] == id).cloned()
                        })
                    });
                    return Ok(Some((
                        None,
                        json!({
                            "kind": "owner_idea", "title": idea["text"], "text": idea["text"],
                            "added_at": idea["added_at"], "assessment": assessment,
                        }),
                    )));
                }
            }
            return Ok(None);
        }
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
            if let Some(snapshot) = document_item(kind, &doc, item_id) {
                let item = &snapshot;
                let project = project.or_else(|| item["project"].as_str().map(str::to_owned));
                return Ok(Some((project, snapshot)));
            }
        }
        if let Some(proposal) = self
            .store
            .proposals()?
            .into_iter()
            .find(|p| p["id"] == item_id)
        {
            let project = proposal["scope"]
                .as_str()
                .filter(|s| *s != "@portfolio")
                .map(str::to_owned);
            if let Some(id) = &project {
                if self.store.project(id)?.is_none() {
                    return Ok(None);
                }
            }
            return Ok(Some((project, proposal["item"].clone())));
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
        Ok(acknowledged_warnings(
            &facts,
            &self.store.warning_acknowledgements(id)?,
        ))
    }

    pub fn set_warning_ack(&self, id: &str, key: &str, note: Option<&str>) -> Result<Value> {
        self.project(id)?;
        if !["no_remote", "ci_none", "worktrees"].contains(&key) {
            return Err(VWatchError::new(
                "invalid_warning",
                "This warning cannot be acknowledged.",
            )
            .hint("Use no_remote, ci_none or worktrees from the project's current warnings."));
        }
        let note = note.map(str::trim);
        if note.is_some_and(|note| note.is_empty() || note.chars().count() > 600) {
            return Err(VWatchError::new("invalid_warning", "An acknowledgement needs a reason of 1 to 600 characters.")
                .hint("Explain why the measured condition is intentional, or omit the note to withdraw the acknowledgement."));
        }
        let warnings = self.project_warnings(id)?;
        let current = warnings
            .as_array()
            .into_iter()
            .flatten()
            .find(|warning| warning["key"] == key);
        if note.is_some() && current.is_none() {
            return Err(VWatchError::new(
                "warning_not_present",
                "This measured warning is not present.",
            )
            .hint("Refresh the project and choose one of its current warnings."));
        }
        let receipt = self.store.set_warning_ack(
            id,
            key,
            note,
            &current
                .map(|warning| warning["basis"].clone())
                .unwrap_or(Value::Null),
            &self.now(),
        )?;
        Ok(with_receipt(
            json!({"project": id, "key": key, "warnings": self.project_warnings(id)?}),
            receipt,
        ))
    }

    pub fn validate_pref(&self, key: &str, value: &Value) -> Result<()> {
        const MODULES: &[&str] = &[
            "focus", "projects", "waiting", "safety", "recent", "across", "ideas", "pulse", "kpis",
            "cards", "tasks", "changes", "night", "space", "parking", "week",
        ];
        match key {
            "language" if !value.as_str().is_some_and(|v| ["en", "de"].contains(&v)) => Err(
                VWatchError::new("invalid_pref", "Invalid language. Nothing was changed.")
                    .hint("Use en (English) or de (German)."),
            ),
            "style"
                if !value.as_str().is_some_and(|style| {
                    ["handover", "board", "handbook", "ideas", "statistics"].contains(&style)
                }) =>
            {
                Err(
                    VWatchError::new("invalid_pref", "Invalid style. Nothing was changed.")
                        .hint("Use handover, board, ideas or statistics."),
                )
            }
            "hidden_modules"
                if !value.as_array().is_some_and(|modules| {
                    modules
                        .iter()
                        .all(|m| m.as_str().is_some_and(|m| MODULES.contains(&m)))
                }) =>
            {
                Err(VWatchError::new(
                    "invalid_pref",
                    "hidden_modules must contain existing module ids. Nothing was changed.",
                )
                .hint(format!(
                    "Use a list of these ids, or an empty list to show everything: {}.",
                    MODULES.join(", ")
                )))
            }
            key if !PREFS.contains(&key) => Err(VWatchError::new(
                "invalid_pref",
                format!("Unknown preference '{key}'. Nothing was changed."),
            )
            .hint("Editable preferences: style, hidden_modules, language.")),
            _ => Ok(()),
        }
    }

    pub fn set_pref(&self, key: &str, value: &Value) -> Result<()> {
        self.update_prefs(&[(key, value.clone())]).map(drop)
    }

    /// One owner choice governs the window and newly authored report content.
    pub(crate) fn report_language(&self) -> Result<String> {
        Ok(
            match self
                .store
                .pref("language")?
                .and_then(|v| v.as_str().map(str::to_owned))
            {
                Some(value) if value == "de" => value,
                _ => "en".to_owned(),
            },
        )
    }
}
