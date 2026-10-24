//! Assignment snapshots, reports and owner transitions.
use super::*;
// An assigned task keeps its own snapshot; rewriting cards never loses the handoff.

impl VWatch {
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
        let source_snapshot = if let Some(source_id) = &source_id {
            match self.find_item(source_id)? {
                Some((_, item)) => Some(item),
                None => self
                    .store
                    .feedback()?
                    .into_iter()
                    .find(|feedback| &feedback.item_id == source_id)
                    .and_then(|feedback| feedback.item),
            }
        } else {
            None
        };
        let at = self.now();
        let mut data = new_task(
            json!(projects),
            json!(project_snapshots),
            json!(source_id),
            json!(source_snapshot),
            &json!({"title":title,"why":why,"evidence":evidence,"done_when":done_when}),
            &at,
        );
        data["history"] = json!([{"actor":"owner","status":"assigned","at":at}]);
        let (task, created) = self
            .store
            .create_tracked_task(Some(&task_source_key(&data)), &data)?;
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

    pub fn task(&self, id: i64) -> Result<Value> {
        Ok(json!({"task": self.tracked_task(id)?}))
    }

    /// Accept the same report from inline CLI fields or a document file.
    pub fn report_task_document(&self, id: i64, payload: Value) -> Result<Value> {
        self.tracked_task(id)?;
        let invalid = |message: String| {
            VWatchError::new("invalid_task", format!("{message} No task was changed."))
                .hint("Send a report object with status (done or blocked), result text, and evidence text. Evidence is required for done and can be omitted for blocked.")
        };
        let object = payload
            .as_object()
            .ok_or_else(|| invalid("The task report must be a JSON object.".into()))?;
        if let Some(key) = object
            .keys()
            .find(|key| !["status", "result", "evidence"].contains(&key.as_str()))
        {
            return Err(invalid(format!("Unknown task report field '{key}'.")));
        }
        let text = |field: &str, required: bool| -> Result<&str> {
            match object.get(field) {
                Some(Value::String(value)) => Ok(value),
                None if !required => Ok(""),
                _ => Err(invalid(format!("{field} must be text."))),
            }
        };
        let status = text("status", true)?.trim().to_ascii_lowercase();
        self.report_task(id, &status, text("result", true)?, text("evidence", false)?)
    }

    pub fn report_task(
        &self,
        id: i64,
        status: &str,
        result: &str,
        evidence: &str,
    ) -> Result<Value> {
        let mut task = self.tracked_task(id)?;
        let target = match status {
            "done" => "review",
            "blocked" => "blocked",
            _ => return Err(VWatchError::new("invalid_task_status", "Agents report a task as done or blocked.")
                .hint("Use done with a result and evidence; completion still needs the owner's confirmation.")),
        };
        let result = task_text(&json!({"result": result}), "result", true, 4000)?;
        let evidence = task_text(
            &json!({"evidence": evidence}),
            "evidence",
            target == "review",
            4000,
        )?;
        if task["result"] == result
            && task["result_evidence"] == evidence
            && (task["status"] == target || (target == "review" && task["status"] == "done"))
        {
            return Ok(json!({"task":task,"already_reported":true}));
        }
        if !active_task(&task) {
            return Err(VWatchError::new(
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
            return Err(VWatchError::new("invalid_task_status", "Owner task status must be assigned, done or cancelled.")
                .hint("assigned reopens a task; done confirms its reported result; cancelled closes it without completion."));
        }
        let mut task = self.tracked_task(id)?;
        if revision.is_some_and(|revision| task["revision"] != revision) {
            return Err(VWatchError::new("task_changed", "The task changed since it was displayed.")
                .hint("Reload the task and read its latest result before confirming, reopening or cancelling it."));
        }
        if task["status"] == status {
            return Ok(json!({"task": task}));
        }
        if (status == "done" && task["status"] != "review")
            || (status == "cancelled" && task["status"] == "done")
        {
            return Err(VWatchError::new("invalid_task_transition", "This task cannot make that transition.")
                .hint("Confirm done only after an agent report is ready for review. Reopen a completed task before cancelling it."));
        }
        if status == "assigned"
            && self.store.tasks()?.iter().any(|other| {
                other["id"] != task["id"]
                    && active_task(other)
                    && task_source_key(other) == task_source_key(&task)
            })
        {
            return Err(VWatchError::new(
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
        if let Some(task) = self.store.task(id)? {
            return Ok(task);
        }
        let tasks = self.store.tasks()?;
        let choices: Vec<String> = tasks
            .iter()
            .take(8)
            .map(|task| {
                format!(
                    "{}: {} ({})",
                    task["id"],
                    task["title"],
                    task["status"].as_str().unwrap_or("unknown")
                )
            })
            .collect();
        Err(VWatchError::new(
            "unknown_task",
            format!("Task {id} does not exist. No task was changed."),
        )
        .hint(if choices.is_empty() {
            "There are no tasks.".into()
        } else {
            format!(
                "Available task IDs{}: {}.{}",
                if tasks.len() > choices.len() {
                    " (first 8)"
                } else {
                    ""
                },
                choices.join(", "),
                if tasks.len() > choices.len() {
                    " Use `vwatch tasks list` for all tasks."
                } else {
                    ""
                }
            )
        }))
    }

    fn save_task_change(&self, mut task: Value, status: &str, event: Value) -> Result<Value> {
        let id = task["id"].as_i64().expect("stored task id");
        let revision = task["revision"].as_i64().expect("stored task revision");
        record_task_transition(&mut task, status, event);
        let saved = self.store.replace_task(id, revision, status, &task).map_err(|error| {
            if matches!(&error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::ConstraintViolation) {
                VWatchError::new("invalid_task_transition", "Another active task already tracks this source.")
                    .hint("Reload the tasks before continuing.")
            } else { error.into() }
        })?;
        if !saved {
            return Err(VWatchError::new(
                "task_changed",
                "The task changed while this update was being saved.",
            )
            .hint("Reload the task and check the latest owner decision before retrying."));
        }
        Ok(json!({"task": task}))
    }
}

pub(super) fn task_source_key(task: &Value) -> String {
    if task["source_id"].is_string() {
        json!([task["projects"], task["source_id"]]).to_string()
    } else {
        format!(
            "manual:{}",
            document_request(&pick(
                task,
                &["projects", "title", "why", "evidence", "done_when"]
            ))
        )
    }
}

pub(super) fn active_task(task: &Value) -> bool {
    matches!(
        task["status"].as_str(),
        Some("assigned" | "blocked" | "review")
    )
}

fn task_error(message: impl Into<String>) -> VWatchError {
    VWatchError::new("invalid_task", message)
        .hint("Use projects (an array of ids), title, and optional source_id, why, evidence and done_when text.")
}

fn task_text(payload: &Value, field: &str, required: bool, max: usize) -> Result<String> {
    let invalid = |message: String| {
        VWatchError::new("invalid_task", format!("{message} No task was changed.")).hint(format!(
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

pub(super) fn new_task(
    projects: Value,
    project_snapshots: Value,
    source_id: Value,
    source_snapshot: Value,
    fields: &Value,
    at: &str,
) -> Value {
    json!({"projects":projects,"project_snapshots":project_snapshots,"source_id":source_id,"source_snapshot":source_snapshot,
        "title":fields["title"],"why":fields["why"],"evidence":fields["evidence"],"done_when":fields["done_when"],
        "created_at":at,"updated_at":at,"reported_at":null,"completed_at":null,"result":"","result_evidence":"","history":[]})
}

pub(super) fn record_task_transition(task: &mut Value, status: &str, event: Value) {
    task["updated_at"] = event["at"].clone();
    task["history"]
        .as_array_mut()
        .expect("stored task history")
        .push(event);
    task["status"] = json!(status);
    if let Some(revision) = task["revision"].as_i64() {
        task["revision"] = json!(revision + 1);
    }
}
