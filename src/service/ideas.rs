//! Own ideas and the owner-controlled idea collection.
use super::*;

/// Longest parked idea, in characters.
const IDEA_MAX: usize = 600;

/// A retained proposal as agents read it, with the same category as the window.
pub(super) fn without_effort(mut proposal: Value) -> Value {
    let category = cards::proposal_category(
        &proposal["item"],
        proposal["kind"].as_str().unwrap_or("suggestion"),
    );
    proposal["category"] = json!(category);
    if let Some(item) = proposal["item"].as_object_mut() {
        item.remove("effort");
        item.insert("category".into(), json!(category));
    }
    proposal
}

pub(super) fn idea_json(i: &Idea) -> Value {
    json!({"id": i.id, "item_id":format!("idea:{}",i.id), "text": i.text, "added_at": i.added_at,"revision":i.revision})
}

impl VWatch {
    /// Parks an idea of the owner's; the next brief assesses it.
    pub fn add_idea(&self, text: &str) -> Result<Value> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > IDEA_MAX {
            return Err(VWatchError::new(
                "invalid_idea",
                format!("An idea needs 1 to {IDEA_MAX} characters."),
            ));
        }
        let (id, created) = self.store.add_idea(text, &self.now())?;
        Ok(
            json!({"id": id, "item_id":format!("idea:{id}"), "text": text, "created": created, "already_exists": !created,
            "undo": created.then(||format!("vwatch ideas remove {id}"))}),
        )
    }

    pub fn update_idea(&self, id: i64, text: &str, revision: i64) -> Result<Value> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > IDEA_MAX {
            return Err(VWatchError::new(
                "invalid_idea",
                format!("An idea needs 1 to {IDEA_MAX} characters. Nothing was changed."),
            ));
        }
        let receipt = self
            .store
            .update_idea_text(id, text, revision, &self.now())?
            .ok_or_else(|| {
                VWatchError::new(
                    "idea_changed",
                    "The idea changed or is unavailable. Nothing was changed.",
                )
                .hint("Read vwatch ideas list, reconcile the text and use its current revision.")
            })?;
        Ok(with_receipt(json!({"idea":self.store.idea(id)?}), receipt))
    }

    pub fn remove_idea(&self, id: i64) -> Result<Value> {
        let idea = self.store.idea(id)?.ok_or_else(|| {
            VWatchError::new(
                "idea_not_found",
                format!("No idea with id {id}. Nothing was changed."),
            )
            .hint("Read `vwatch ideas list` or `vwatch ideas trash` for existing idea ids.")
        })?;
        let changed = self.store.set_idea_removed(id, Some(&self.now()))?;
        let idea = self.store.idea(id)?.unwrap_or(idea);
        Ok(
            json!({"removed": id, "idea": idea, "changed": changed, "already_removed": !changed,
            "undo": changed.then(||format!("vwatch ideas restore {id}"))}),
        )
    }

    pub fn ideas_trash(&self) -> Result<Value> {
        Ok(json!(self.store.removed_ideas()?))
    }

    pub fn restore_idea(&self, id: i64) -> Result<Value> {
        let idea = self.store.idea(id)?.ok_or_else(|| {
            VWatchError::new(
                "idea_not_found",
                format!("No idea with id {id}. Nothing was changed."),
            )
            .hint("Read `vwatch ideas trash` for removed idea ids.")
        })?;
        let restored = self.store.set_idea_removed(id, None)?;
        let idea = self.store.idea(id)?.unwrap_or(idea);
        Ok(
            json!({"idea": idea, "restored": restored, "already_active": !restored,
            "undo": restored.then(||format!("vwatch ideas remove {id}"))}),
        )
    }

    pub fn ideas(&self) -> Result<Value> {
        Ok(json!(self
            .store
            .ideas()?
            .iter()
            .map(idea_json)
            .collect::<Vec<_>>()))
    }

    pub(super) fn idea_source(&self, id: &str) -> Result<(Option<String>, Value)> {
        let source = self.find_item(id)?;
        source.filter(|(_,item)| matches!(item["kind"].as_str(),Some("suggestion"|"new_app"|"synergy"|"owner_idea")))
            .ok_or_else(|| VWatchError::new("unknown_idea",format!("No idea or recommendation has item ID '{id}'. Nothing was changed."))
                .hint("Use item IDs from vwatch proposals; for your own ideas, use the item_id from vwatch ideas list."))
    }

    pub fn idea_status(&self, id: &str) -> Result<Value> {
        let (_, item) = self.idea_source(id)?;
        let mut state = pick(
            &idea_projection(id, self.store.idea_snapshot(id)?),
            &["item_id", "title", "status", "archived", "revision", "task"],
        );
        state["title"] = item["title"].clone();
        Ok(json!({"idea_status":state}))
    }

    /// Owner decisions about an idea are separate from taste feedback and agent reports.
    pub fn set_idea_status(&self, id: &str, status: &str, revision: &str) -> Result<Value> {
        if !["open", "in_progress", "implemented", "archived", "restore"].contains(&status) {
            return Err(VWatchError::new(
                "invalid_idea_status",
                "Unknown idea status. Nothing was changed.",
            )
            .hint("Use open, in_progress, implemented, archived or restore."));
        }
        let (project, item) = self.idea_source(id)?;
        let current = idea_projection(id, self.store.idea_snapshot(id)?);
        if !matches!(status, "archived" | "restore")
            && current["active_tasks"].as_u64().unwrap_or(0) > 1
        {
            return Err(VWatchError::new(
                "ambiguous_idea_tasks",
                "Several active tasks track this idea. Nothing was changed.",
            )
            .hint("Use vwatch tasks list and close or finish the individual tasks first."));
        }
        let mut projects = project.iter().map(|p| json!(p)).collect::<Vec<_>>();
        if projects.is_empty() {
            projects = item["projects"]
                .as_array()
                .or_else(|| item["assessment"]["fits"].as_array())
                .cloned()
                .unwrap_or_default();
        }
        projects.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        projects.dedup();
        let mut snapshots = Vec::new();
        for p in &projects {
            if let Some(p) = self.store.project(p.as_str().unwrap_or(""))? {
                snapshots.push(pick(&project_json(&p), &["id", "name", "path"]));
            }
        }
        let at = self.now();
        let fields = json!({"title":item["title"],"why":item["why"].as_str().or(item["pitch"].as_str()).unwrap_or(""),"evidence":item["evidence"].as_str().unwrap_or(""),"done_when":item["done_when"].as_str().unwrap_or("")});
        let seed = new_task(
            json!(projects),
            json!(snapshots),
            json!(id),
            item.clone(),
            &fields,
            &at,
        );
        let archive_action = matches!(status, "archived" | "restore");
        let changed = if archive_action {
            current["archived"] != (status == "archived")
        } else {
            current["status"] != status
        };
        let mut change = crate::store::IdeaChange {
            project,
            title: item["title"].as_str().unwrap_or("").into(),
            archive: None,
            task: None,
        };
        if changed && archive_action {
            change.archive = Some(status == "archived");
        } else if changed {
            let status = match status {
                "in_progress" => "assigned",
                "implemented" => "done",
                _ => "cancelled",
            };
            let mut task = if current["task"].is_null() {
                seed
            } else {
                current["task"].clone()
            };
            task["completed_at"] = if status == "done" {
                json!(at)
            } else {
                Value::Null
            };
            record_task_transition(
                &mut task,
                status,
                json!({"actor":"owner","status":status,"at":at,"action":"idea_status"}),
            );
            change.task = Some(crate::store::IdeaTaskChange {
                id: task["id"].as_i64(),
                status: status.into(),
                source_key: task_source_key(&task),
                data: task,
            });
        }
        let (changed,snapshot) = self.store.set_idea_state(id,revision,&change,&at)?.ok_or_else(||
            VWatchError::new("idea_changed", "The idea status or its task changed since it was displayed. Nothing was changed.")
                .hint(format!("Read vwatch ideas status {id}, review the current state and use its revision.")))?;
        let result = idea_projection(id, snapshot);
        let mut state = pick(
            &result,
            &["item_id", "title", "status", "archived", "revision", "task"],
        );
        state["title"] = item["title"].clone();
        let mut response = json!({"changed":changed,"idea_status":state});
        if changed && matches!(status, "archived" | "restore") {
            response["undo"] = json!(format!(
                "vwatch ideas status {id} {} --revision {}",
                if status == "archived" {
                    "restore"
                } else {
                    "archived"
                },
                state["revision"].as_str().unwrap_or("")
            ));
        }
        Ok(response)
    }

    pub(super) fn idea_states(&self, project: Option<&str>, include_open: bool) -> Result<Value> {
        let mut items = self
            .store
            .proposals()?
            .into_iter()
            .filter_map(|p| {
                p["id"].as_str().map(|id| {
                    (
                        id.to_owned(),
                        p["item"]["title"].clone(),
                        p["scope"].clone(),
                        p["item"]["projects"].clone(),
                    )
                })
            })
            .collect::<Vec<_>>();
        items.extend(self.store.ideas()?.iter().map(|i| {
            (
                format!("idea:{}", i.id),
                json!(i.text),
                Value::Null,
                Value::Null,
            )
        }));
        // Older sources can survive only through feedback or assignment snapshots.
        for feedback in self.store.feedback()? {
            if feedback.item.as_ref().is_some_and(is_idea_item) {
                items.push((
                    feedback.item_id,
                    json!(feedback.title),
                    json!(feedback.project_id),
                    Value::Null,
                ));
            }
        }
        for task in self.store.tasks()? {
            if is_idea_item(&task["source_snapshot"]) {
                if let Some(id) = task["source_id"].as_str() {
                    items.push((
                        id.into(),
                        task["title"].clone(),
                        Value::Null,
                        task["projects"].clone(),
                    ));
                }
            }
        }
        let mut states = Map::new();
        for (id, title, scope, projects) in items {
            if states.contains_key(&id) {
                continue;
            }
            let state = idea_projection(&id, self.store.idea_snapshot(&id)?);
            if !include_open && state["status"] == "open" && state["archived"] != true {
                continue;
            }
            if project.is_some_and(|p| {
                scope != p
                    && !projects
                        .as_array()
                        .is_some_and(|ps| ps.iter().any(|v| v == p))
                    && !state["task"]["projects"]
                        .as_array()
                        .is_some_and(|ps| ps.iter().any(|v| v == p))
            }) {
                continue;
            }
            let mut compact = pick(&state, &["status", "archived", "revision", "task"]);
            compact["title"] = title;
            states.insert(id, compact);
        }
        Ok(Value::Object(states))
    }

    /// Historical source enrichment can discover ideas missing from old proposal tables.
    pub(super) fn idea_state_projection(&self, id: &str) -> Result<Value> {
        Ok(idea_projection(id, self.store.idea_snapshot(id)?))
    }
}

fn is_idea_item(item: &Value) -> bool {
    matches!(
        item["kind"].as_str(),
        Some("suggestion" | "new_app" | "synergy" | "owner_idea")
    )
}

/// Select once in the application owner; revisions still cover every source task.
fn idea_projection(id: &str, snapshot: crate::store::IdeaSnapshot) -> Value {
    let task = snapshot
        .tasks
        .iter()
        .filter(|t| active_task(t))
        .max_by_key(|t| t["id"].as_i64())
        .or_else(|| snapshot.tasks.iter().max_by_key(|t| t["id"].as_i64()))
        .cloned()
        .unwrap_or(Value::Null);
    let status = match task["status"].as_str() {
        Some("assigned" | "blocked" | "review") => "in_progress",
        Some("done") => "implemented",
        _ => "open",
    };
    json!({"item_id":id,"status":status,"archived":snapshot.archive.as_ref().is_some_and(|a|a["archived"]==true),"revision":snapshot.revision,"task":task,"active_tasks":snapshot.tasks.iter().filter(|t|active_task(t)).count()})
}
