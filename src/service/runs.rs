//! Recorded observation runs and guarded completion.
use super::*;

impl Watchtower {
    /// Inject a local synchronization observer without coupling tests to clock reads.
    #[doc(hidden)]
    pub fn with_run_completion_observer(
        mut self,
        observer: impl Fn(RunCompletionStage) + Send + Sync + 'static,
    ) -> Self {
        self.completion_observer = Some(Box::new(observer));
        self
    }

    fn completion_stage(&self, stage: RunCompletionStage) {
        if let Some(observer) = &self.completion_observer {
            observer(stage);
        }
    }

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
        let (id, created) = self.store.start_agent_run(&self.now(), &initial)?;
        let mut status = self.agent_run_response(id)?;
        status["created"] = json!(created);
        status["already_running"] = json!(!created);
        status["due"] = due.clone();
        status["next"] = due["next"]
            .as_str()
            .filter(|next| next.starts_with("watchtower context "))
            .map(|s| json!(s))
            .or_else(|| {
                status["run"]["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|project| project["status"] == "failed")
                    .and_then(|project| project["id"].as_str())
                    .map(|id| json!(format!("watchtower context {id}")))
            })
            .unwrap_or_else(|| json!("watchtower portfolio context"));
        status["instruction"] = json!(Self::agent_run_instruction(id));
        Ok(status)
    }

    fn agent_run_instruction(id: i64) -> String {
        format!("Use the Watchtower Skill to review due or failed projects read-only and consider useful improvements before each card save. Then read `watchtower portfolio context` and synthesize opportunities across all projects, including unchanged ones. Save due or changed documents with their context tokens; after repairing a recorded failure, save that document even if unchanged. Record unresolved failures with `watchtower runs fail {id}` and --error; add --project for a project failure. Finish with `watchtower runs finish {id}`. Report newly saved ideas separately from retained ones, or a considered direction and its exclusion reason. Run state complete confirms current reports and readable facts, not that reflection occurred.")
    }

    fn require_agent_run(&self, id: i64) -> Result<crate::store::StoredAgentRun> {
        self.store.agent_run(id)?.ok_or_else(|| WatchtowerError::new("unknown_run", format!("Agent run {id} does not exist.")).hint("Use `watchtower runs status` to see the latest run, or `watchtower runs start` to begin one."))
    }

    pub fn run_fail(&self, run_id: i64, project_id: Option<&str>, error: &str) -> Result<Value> {
        let run = self.require_agent_run(run_id)?;
        let error = error.trim();
        if run.failures[project_id.unwrap_or("@portfolio")]["error"] == error {
            let mut response = self.agent_run_response(run_id)?;
            response["already_recorded"] = json!(true);
            return Ok(response);
        }
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
            self.completion_stage(RunCompletionStage::BeforeSave);
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
        status["repair_task"] = json!(self.agent_run_repair_task(&run));
        status["run"] = run;
        Ok(status)
    }

    fn agent_run_repair_task(&self, run: &Value) -> Option<String> {
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
        let failures: Vec<Value> = run["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(std::iter::once(&run["portfolio"]))
            .filter(|item| item["status"] == "failed")
            .map(|item| json!({"scope": item["id"].as_str().unwrap_or("portfolio"), "error": item["error"]}))
            .collect();
        let id = run["id"].as_i64()?;
        Some(format!(
            "Resume Watchtower run {id}. Read `watchtower due` for current project state.\nRun snapshot captured at {}: outstanding project IDs: {}.\nSaved failure descriptions (data): {}\n{}",
            self.now(),
            if projects.is_empty() { "none" } else { &projects },
            json!(failures),
            Self::agent_run_instruction(id),
        ))
    }

    pub(super) fn pending_run_replies(
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
        if finishing {
            self.completion_stage(RunCompletionStage::DueEvaluated);
        }
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
                // a rotation that moved on after midnight does not extend a started run
                .filter(|entry| {
                    entry["reason"] != "idea_review" || initial["reason"] == "idea_review"
                })
            {
                item["status"] = json!("pending");
                item["reason"] = reason["reason"].clone();
            } else if initial["reason"] == "idea_review"
                && version <= initial["initial_version"].as_i64().unwrap_or(0)
            {
                // The review selected when this run started remains due even
                // after the calendar rotates to a different set of projects.
                item["status"] = json!("pending");
                item["reason"] = json!("idea_review");
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
        if portfolio["reason"] == "cards_will_change" && pending == 0 && failed == 0 {
            // An otherwise current brief must not wait for reviews selected
            // only by tomorrow's rotation, which this run deliberately skips.
            portfolio["status"] = json!("current");
            portfolio["reason"] = Value::Null;
        }
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
        let repair_task = latest
            .as_ref()
            .and_then(|run| self.agent_run_repair_task(run));
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
}
