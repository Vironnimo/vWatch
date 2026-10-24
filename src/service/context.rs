//! Agent context assembly and context-token checks.
use super::*;

/// Items shown at least this many days are listed for the brief as long-standing.
const LONG_STANDING_DAYS: i64 = 7;

const COMMITS_SINCE_LIMIT: usize = 150;

/// A retained proposal as a one-line reminder; `vwatch proposals` has its full text,
/// and sending only its id in the document it belongs to restores it.
fn proposal_summary(proposal: Value) -> Value {
    let p = without_effort(proposal);
    json!({"id": p["id"], "scope": p["scope"], "kind": p["kind"], "category": p["category"], "title": p["item"]["title"], "first_seen": p["first_seen"]})
}

/// The items of a dated list (`date`, `title`) from `from` on.
fn dated_since(list: &Value, from: &str) -> Vec<Value> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|i| i["date"].as_str().is_some_and(|d| d >= from))
        .map(|i| pick(i, &["date", "title"]))
        .collect()
}

/// Keep historical review evidence separate from changes that happened this week.
fn dated_between(list: &Value, from: &str, to: &str) -> Vec<Value> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            item["date"]
                .as_str()
                .is_some_and(|date| date >= from && date <= to)
        })
        .cloned()
        .collect()
}

/// Weekly buckets are anchored to the collection date, not the context's date.
/// A measurement made during the review week cannot establish its final total.
fn commits_in_review_week(stored: &StoredFacts, from: NaiveDate) -> Option<u64> {
    if stored.data["ok"] != true {
        return None;
    }
    let collected = DateTime::parse_from_rfc3339(&stored.collected_at)
        .ok()?
        .with_timezone(&Local)
        .date_naive();
    let monday =
        collected - chrono::Duration::days(collected.weekday().num_days_from_monday().into());
    let weeks_ago = (monday - from).num_days() / 7;
    if weeks_ago < 1 {
        return None;
    }
    let weekly = stored.data["commits"]["weekly"].as_array()?;
    weekly
        .get(
            weekly
                .len()
                .checked_sub(usize::try_from(weeks_ago).ok()? + 1)?,
        )?
        .as_u64()
}

impl VWatch {
    pub fn context(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        self.stored_facts(id)?;
        self.context_with_token(Some(id), || self.context_data(id))
    }

    pub(super) fn context_with_token(
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

    pub(super) fn version_conflict(project: Option<&str>) -> VWatchError {
        let command = project.map_or_else(
            || "vwatch portfolio context".to_owned(),
            |id| format!("vwatch context {id}"),
        );
        VWatchError::new("version_conflict", "The context changed. Nothing was saved.")
            .hint(format!("Read `{command}` again, reconcile your document with the new facts, owner instructions and previous document, then retry with its context_token. Do not merely replace the token."))
    }

    pub(super) fn check_context(
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
                    || "vwatch portfolio context".to_owned(),
                    |id| format!("vwatch context {id}"),
                );
                Err(VWatchError::new("context_required", "Saving requires the context_token from the context you used.")
                    .hint(format!("Read `{command}`, prepare your document from that context, and pass its context_token value with --context-token. Schema-only --dry-run may omit it.")))
            }
            _ => Ok(()),
        }
    }

    pub(super) fn guarded_version(
        write: crate::store::GuardedWrite,
        kind: Kind,
        project: Option<&str>,
    ) -> Result<(i64, String, bool)> {
        match write {
            crate::store::GuardedWrite::Saved(version, token) => Ok((version, token, false)),
            crate::store::GuardedWrite::Replayed(version, token) => Ok((version, token, true)),
            crate::store::GuardedWrite::Conflict => Err(Self::version_conflict(project)),
            crate::store::GuardedWrite::ItemConflict(conflicts) => {
                let (code, name) = match kind {
                    Kind::Card => ("invalid_card", "card"),
                    Kind::Portfolio => ("invalid_portfolio", "brief"),
                };
                Err(VWatchError::new(code, format!("The {name} was not saved: {}", conflicts.join("; ")))
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
            .map(feedback_context)
            .collect();
        let mut facts_context = stored.data.clone();
        if let Some(object) = facts_context.as_object_mut() {
            object.remove("activity");
        }
        if let Some(code) = facts_context["code"].as_object_mut() {
            code.remove("largest_files");
        }
        let item_persistence = match &card {
            Some(c) => self.card_persistence(id, &c.data, &reactions)?,
            None => Vec::new(),
        };
        let risk_streaks = self.project_streaks(id, Some(&stored))?;
        let next = format!("Study the repository read-only and write the card to a UTF-8 file. Save project {id} with card put, supplying --file with that file and --context-token with this context's token.{}", if card.is_some() {  " Use --merge only when sending changed top-level fields; send each changed list in full, where an unchanged or retained item can be just {\"id\": ...}." } else { " Send a complete card." });
        Ok(json!({
            "today": self.today().to_string(),
            "project": pick(&project_json(&project), &["id", "name", "path", "status", "status_note", "goal", "goal_due"]),
            "facts": extend(json!({"collected_at": stored.collected_at}), facts_context),
            "new_fields": card.as_ref().map(|c| Kind::Card.missing_fields(&c.data)),
            "previous_card": card.map(|c| json!({"version": c.version, "created_at": c.created_at, "card": Kind::Card.current_view(&c.data)})),
            "since_previous_card": since,
            "retained_proposals":self.store.proposals()?.into_iter().filter(|p|p["scope"]==id && p["in_report"]==false).map(proposal_summary).collect::<Vec<_>>(),
            "risk_streaks": risk_streaks,
            "item_persistence": item_persistence,
            "feedback": feedback,
            "feedback_summary": feedback_summary(&reactions),
            "idea_states": self.idea_states(Some(id), false)?,
            "tasks": self.tasks(Some(id))?["tasks"],
            "warning_acknowledgements": self.warning_acknowledgements(id)?,
            "next": next,
        }))
    }

    /// The context for the brief. Projects unchanged since the previous brief come as
    /// summaries; `detail` returns only the named projects, in full.
    pub fn portfolio_context(&self, detail: &[String]) -> Result<Value> {
        for id in detail {
            self.project(id)?;
        }
        self.context_with_token(None, || self.portfolio_context_data(detail))
    }

    fn portfolio_context_data(&self, detail: &[String]) -> Result<Value> {
        let mut rows = Vec::new();
        let mut open_questions = Vec::new();
        let previous = self.store.latest_portfolio()?;
        let reactions = self.store.feedback()?;
        let tasks = self.store.tasks()?;
        let (week, from, to) = self.review_week();
        let week_from = from.to_string();
        let week_to = to.to_string();
        let reviewed = previous
            .as_ref()
            .is_some_and(|p| p.data["week_review"]["week"] == week.as_str());
        for p in self.store.projects()? {
            if p.status == "archived" || (!detail.is_empty() && !detail.contains(&p.id)) {
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
                &task_basis(&tasks, &p.id),
            );
            let collected_at = stored.as_ref().map(|s| &s.collected_at);
            let review_commits = stored
                .as_ref()
                .and_then(|s| commits_in_review_week(s, from));
            let f = stored.as_ref().map(|s| &s.data).unwrap_or(&Value::Null);
            let review_evidence = card.as_ref().map(|card| json!({
                "recent_changes": dated_between(&card.data["recent_changes"], &week_from, &week_to),
                "milestones": dated_between(&card.data["milestones"], &week_from, &week_to),
            }));
            let card_data = card.as_ref().map(|card| card.data.clone());
            // a card the previous brief already built on needs only a reminder of its content
            let summary = detail.is_empty()
                && !card_outdated
                && changed.as_array().is_some_and(Vec::is_empty);
            let card_json = card.map(|card| {
                let c = &Kind::Card.current_view(&card.data);
                let picked = |key: &str, fields: &[&str]| -> Vec<Value> { c[key].as_array().into_iter().flatten().map(|x| pick(x, fields)).collect() };
                if summary {
                    return json!({
                        "detail": "summary",
                        "version": card.version, "created_at": card.created_at, "outdated": card_outdated, "changed_since_brief": changed,
                        "one_liner": c["one_liner"], "phase": c["phase"], "headline": c["headline"],
                        "next_step": c["reentry"]["next_step"],
                        "usage": c["usage"]["summary"],
                        "waiting_on_you": picked("waiting_on_you", &["id", "title"]),
                        "open_ends": picked("open_ends", &["kind", "title", "age_days"]),
                        "health_notes": c["health_notes"],
                        "decisions": c["decisions"],
                        "suggestions": picked("suggestions", &["id", "category", "title"]),
                        "cross_project_hints": c["cross_project_hints"],
                        "recent_changes": dated_since(&c["recent_changes"], &week_from),
                        "milestones": dated_since(&c["milestones"], &week_from),
                    });
                }
                json!({
                    "detail": "full",
                    "version": card.version, "created_at": card.created_at, "outdated": card_outdated, "changed_since_brief": changed,
                    "one_liner": c["one_liner"], "phase": c["phase"], "headline": c["headline"],
                    "reentry": c["reentry"],
                    "waiting_on_you": picked("waiting_on_you", &["id", "title", "source", "options", "recommended"]),
                    "open_ends": picked("open_ends", &["kind", "title", "detail", "source", "age_days"]),
                    "health_notes": c["health_notes"],
                    "decisions": c["decisions"],
                    "suggestions": picked("suggestions", &["id", "category", "type", "title", "why", "evidence", "done_when"]),
                    "cross_project_hints": c["cross_project_hints"],
                    "usage": c["usage"],
                    "recent_changes": dated_since(&c["recent_changes"], &week_from),
                    "milestones": dated_since(&c["milestones"], &week_from),
                })
            });
            let risk_streaks = self.project_streaks(&p.id, stored.as_ref())?;
            let persistence = match &card_data {
                Some(data) => self.card_persistence(&p.id, data, &reactions)?,
                None => Vec::new(),
            };
            let replied = |row: &&Value| {
                row["owner_reaction"]["value"]
                    .as_str()
                    .is_some_and(|v| v == "answered" || v == "done")
            };
            open_questions.extend(
                persistence
                    .iter()
                    .filter(|row| row["kind"] == "question")
                    .filter(|row| !replied(row))
                    .cloned(),
            );
            // The card above already shows each item's text; an ID identifies it.
            let long_standing: Vec<Value> = persistence
                .into_iter()
                .filter(|row| row["days"].as_i64().unwrap_or(0) >= LONG_STANDING_DAYS)
                .map(|mut row| {
                    if let Some(fields) = row.as_object_mut() {
                        fields.remove("first_seen");
                        if fields.get("id").is_some_and(Value::is_string) {
                            fields.remove("title");
                        } else {
                            fields.remove("id");
                        }
                    }
                    row
                })
                .collect();
            rows.push(json!({
                "id": p.id, "name": p.name, "status": p.status, "status_note": p.status_note, "goal": p.goal, "goal_due": p.goal_due,
                "risk_streaks": risk_streaks,
                "long_standing_items": long_standing,
                "warning_acknowledgements": self.warning_acknowledgements(&p.id)?,
                "facts": {
                    "ok": f["ok"], "error": f["error"], "collected_at": collected_at,
                    "last_work": f["last_work"],
                    "commits": pick(&f["commits"], &["total", "last_7d", "last_30d", "last"]),
                    "commits_review_week": review_commits,
                    "has_remote": f["has_remote"], "ahead": f["ahead"],
                    "dirty": pick(&f["dirty"], &["files", "lines_added", "lines_removed", "oldest_change"]),
                    "ci": pick(&f["ci"], &["state", "failing", "at"]),
                    "unmerged_branches": f["branches"]["unmerged"].as_array().map(Vec::len),
                    "extra_worktrees": f["worktrees"]["count"],
                },
                "card": card_json,
            }));
            if !reviewed {
                if let Some(row) = rows.last_mut() {
                    row["review_week_evidence"] = json!(review_evidence);
                }
            }
        }
        if !detail.is_empty() {
            return Ok(json!({
                "today": self.today().to_string(),
                "projects": rows,
                "next": "Use these details while preparing the brief from the full portfolio context, then save it with portfolio put, supplying --file and --context-token.",
            }));
        }
        let unchecked = self.unchecked_ideas(previous.as_ref().map(|p| &p.data))?;
        Ok(json!({
            "today": self.today().to_string(),
            "review_week": {"week": week, "from": from.to_string(), "to": to.to_string(), "due": !reviewed},
            "owner_ideas": self.store.ideas()?.iter().map(|i| extend(idea_json(i), json!({"checked": !unchecked.contains(&i.id)}))).collect::<Vec<_>>(),
            "retained_proposals":self.store.proposals()?.into_iter().filter(|p|p["in_report"]==false && (p["scope"]=="@portfolio" || rows.iter().any(|r|r["id"]==p["scope"]))).map(proposal_summary).collect::<Vec<_>>(),
            "brief_persistence": self.brief_persistence(previous.as_ref(), &reactions)?,
            "owner_response": self.owner_response(&reactions, &open_questions),
            "projects": rows,
            "new_fields": previous.as_ref().map(|p| Kind::Portfolio.missing_fields(&p.data)),
            "previous_portfolio": previous.map(|p| json!({"version": p.version, "created_at": p.created_at, "portfolio": Kind::Portfolio.current_view(&p.data)})),
            "owner_last_opened": self.store.pref("last_visit")?,
            "feedback": reactions.iter().map(feedback_context).collect::<Vec<_>>(),
            "feedback_summary": feedback_summary(&reactions),
            "idea_states": self.idea_states(None, false)?,
            "tasks": tasks,
            "next": "Prepare the brief from this context and save the document to a UTF-8 file. Use portfolio put with --file for that file and --context-token for this context's token.",
        }))
    }
}
