//! Interpretation freshness and portfolio evidence bases.
use super::*;

/// A quiet active project's ideas are reviewed when its card is at least this old.
const IDEA_REVIEW_DAYS: i64 = 7;

/// Active projects whose ideas come up for review each day, in rotation.
const IDEA_REVIEWS_PER_DAY: usize = 2;

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
    "usage",
];

pub const DEFAULT_MAX_AGE_DAYS: i64 = 14;

/// The active projects whose ideas are reviewed today: a daily rotation over
/// registration order, stable within the day so saving one card never makes another due.
fn idea_review_rotation(projects: &[crate::store::Project], today: NaiveDate) -> Vec<String> {
    let active: Vec<&str> = projects
        .iter()
        .filter(|p| p.status == "active")
        .map(|p| p.id.as_str())
        .collect();
    let start = today.num_days_from_ce().unsigned_abs() as usize * IDEA_REVIEWS_PER_DAY;
    (0..IDEA_REVIEWS_PER_DAY.min(active.len()))
        .map(|i| active[(start + i) % active.len()].to_owned())
        .collect()
}

/// The state of the project's assigned tasks that its card answers to.
pub(super) fn task_basis(tasks: &[Value], id: &str) -> Value {
    Value::Array(
        tasks
            .iter()
            .filter(|task| {
                task["projects"]
                    .as_array()
                    .is_some_and(|projects| projects.iter().any(|p| p == id))
            })
            .map(|task| pick(task, &["id", "status", "revision"]))
            .collect(),
    )
}

/// A CI state firm enough to compare: `green` or `red`. Running, unknown or skipped
/// lookups say nothing about whether a card's CI statement still holds.
pub(super) fn settled_ci(facts: &Value) -> Option<&str> {
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
                || project.identity_revision > 0
                || warning_acks.as_array().is_some_and(|acks| !acks.is_empty())
                || tasks.as_array().is_some_and(|tasks| !tasks.is_empty()),
        ),
    }
}

pub(super) fn card_outdated(
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

/// Content, rather than timestamps, keeps owner edits within one clock tick visible.
pub(super) fn owner_basis(
    p: &Project,
    reactions: &[Feedback],
    warning_acks: &Value,
    tasks: &Value,
) -> Value {
    let questions: Map<String, Value> = reactions
        .iter()
        .filter(|f| f.project_id.as_deref() == Some(&p.id) && kind_of(f) == "question")
        // postponing a question is not an answer the card must react to
        .filter(|f| f.value != "later")
        .map(|f| (f.item_id.clone(), json!({"value": f.value, "note": f.note})))
        .collect();
    let mut project = pick(
        &project_json(p),
        &["status", "status_note", "goal", "goal_due"],
    );
    // Existing cards have no revision. Zero keeps their owner basis unchanged
    // until their project is actually edited, even within the same clock tick.
    if p.identity_revision > 0 {
        project["identity_revision"] = json!(p.identity_revision);
    }
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

impl Watchtower {
    pub fn due(&self, max_age_days: i64, refresh: bool) -> Result<Value> {
        let refreshed = if refresh {
            self.refresh_facts(None, true)?
        } else {
            Vec::new()
        };
        let now = (self.clock)();
        let (mut projects, mut problems, mut up_to_date) = (Vec::new(), Vec::new(), Vec::new());
        let brief = self.store.latest_portfolio()?;
        let mut cards_changed = match brief.as_ref().and_then(|p| p.basis.as_ref()) {
            Some(basis) => *basis != self.portfolio_basis()?,
            None => false,
        };
        let reactions = self.store.feedback()?;
        let registered = self.store.projects()?;
        let tasks = self.store.tasks()?;
        let reviewed_today = idea_review_rotation(&registered, self.today());
        for p in registered {
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
            let project_tasks = task_basis(&tasks, &p.id);
            let (answered, status_changed) = card
                .as_ref()
                .map(|c| owner_changes(c, &p, &reactions, &warning_acks, &project_tasks))
                .unwrap_or_default();
            if stored.as_ref().is_none_or(|s| s.data["ok"] != true) {
                let problem = stored.as_ref().map_or("facts_missing", |s| {
                    s.data["error"].as_str().unwrap_or("facts_unavailable")
                });
                let error = stored.as_ref().map(|s| s.data["error"].clone());
                problems.push(
                    json!({"id":p.id,"name":p.name,"problem":problem,"error":error,"path":p.path}),
                );
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
                Some(c)
                    if reviewed_today.contains(&p.id)
                        && (now - parse(&c.created_at)).num_days() >= IDEA_REVIEW_DAYS =>
                {
                    Some("idea_review")
                }
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
            Some(pf) if !Kind::Portfolio.missing_fields(&pf.data).is_empty() => Some("new_fields"),
            Some(_) if cards_changed => Some("cards_changed"),
            Some(_) => None,
        };
        let any_due = !projects.is_empty();
        // cards that only gain new fields do not change what the brief builds on
        let brief_affected = projects.iter().any(|e| e["reason"] != "new_fields");
        let next = if any_due {
            projects
                .first()
                .and_then(|p| p["id"].as_str())
                .map(|id| format!("watchtower context {id}"))
        } else if portfolio_reason.is_some() {
            Some("watchtower portfolio context".to_owned())
        } else {
            None
        };
        Ok(json!({
            "today": self.today().to_string(),
            "projects": projects,
            "up_to_date": up_to_date,
            "problems": problems,
            "refreshed": refreshed,
            "portfolio": {
                "due": portfolio_reason.is_some() || brief_affected,
                "reason": portfolio_reason.or(brief_affected.then_some("cards_will_change")),
            },
            "next": next,
        }))
    }

    pub(super) fn unchecked_ideas(&self, brief: Option<&Value>) -> Result<Vec<i64>> {
        let checked: Vec<i64> = brief
            .and_then(|b| b["idea_checks"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|c| c["idea"].as_i64())
            .collect();
        // The brief's basis is large; only an edited, already checked idea needs it.
        let mut basis = None;
        let mut unchecked = Vec::new();
        for idea in self.store.ideas()? {
            let stale = checked.contains(&idea.id) && idea.revision > 1 && {
                if basis.is_none() {
                    basis = Some(
                        self.store
                            .latest_portfolio()?
                            .and_then(|p| p.basis)
                            .unwrap_or(Value::Null),
                    );
                }
                basis
                    .as_ref()
                    .map_or(&Value::Null, |b| &b["@ideas"][idea.id.to_string()])
                    .as_i64()
                    != Some(idea.revision)
            };
            if !checked.contains(&idea.id) || stale {
                unchecked.push(idea.id);
            }
        }
        Ok(unchecked)
    }

    /// The calendar week before this one, which the week review looks back on.
    pub(super) fn review_week(&self) -> (String, NaiveDate, NaiveDate) {
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
    pub(super) fn brief_changes(
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
    pub(super) fn portfolio_basis(&self) -> Result<Value> {
        let mut basis = Map::new();
        let archives = self.store.idea_archives()?;
        if !archives.is_empty() {
            basis.insert("@idea_archives".into(), json!(archives));
        }
        basis.insert(
            "@ideas".into(),
            json!(self
                .store
                .ideas()?
                .into_iter()
                .map(|i| (i.id.to_string(), json!(i.revision)))
                .collect::<Map<String, Value>>()),
        );
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
            if p.identity_revision > 0 {
                entry["project"]["identity_revision"] = json!(p.identity_revision);
            }
            let warning_acks = self.warning_acknowledgements(&p.id)?;
            if warning_acks.as_array().is_some_and(|acks| !acks.is_empty()) {
                entry["warning_acks"] = warning_acks;
            }
            let project_tasks = task_basis(&tasks, &p.id);
            if project_tasks
                .as_array()
                .is_some_and(|tasks| !tasks.is_empty())
            {
                entry["tasks"] = project_tasks;
            }
            basis.insert(p.id.clone(), entry);
        }
        Ok(Value::Object(basis))
    }

    pub(super) fn project_task_basis(&self, id: &str) -> Result<Value> {
        Ok(task_basis(&self.store.tasks()?, id))
    }
}
