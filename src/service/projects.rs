//! Project identity, discovery and repository refresh coordination.
use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const REFRESH_WORKERS: usize = 4;

const DISCOVER_DEPTH: usize = 2;

/// Repositories with a commit in this many days are recommended when discovering.
const RECENT_DAYS: i64 = 120;

pub const STATUSES: &[&str] = &["active", "parked", "done", "archived"];

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
impl VWatch {
    pub(super) fn project(&self, id: &str) -> Result<Project> {
        if let Some(p) = self.store.project(id)? {
            return Ok(p);
        }
        if let Some(p) = self.store.project_including_removed(id)? {
            return Err(VWatchError::new(
                "project_removed",
                format!(
                    "Project '{}' ({}) is in the trash. Nothing was changed.",
                    p.id, p.name
                ),
            )
            .hint(format!(
                "To restore its cards and history: vwatch projects restore {}",
                p.id
            )));
        }
        let known: Vec<String> = self
            .store
            .projects()?
            .into_iter()
            .map(|p| format!("{} ({})", p.id, p.name))
            .collect();
        let hint = if known.is_empty() {
            "No projects are registered. To watch the project the user requested, use projects add with its existing folder path; quote paths containing spaces.".to_owned()
        } else {
            let shown = known
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if known.len() > 10 {
                format!("Known project ids (first 10 of {}): {shown}. Read vwatch projects list for the complete list; choose the intended project explicitly.",known.len())
            } else {
                format!("Known project ids: {shown}. Use the exact id of the intended project.")
            }
        };
        Err(VWatchError::new("project_not_found", format!("No project with id '{id}'.")).hint(hint))
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

    fn register(
        &self,
        path: &str,
        id: Option<&str>,
        name: Option<&str>,
    ) -> Result<(Project, bool)> {
        let name = name.map(str::trim);
        if name.is_some_and(str::is_empty) {
            return Err(VWatchError::new(
                "invalid_name",
                "The name must not be empty. No project was registered.",
            )
            .hint(
                "Pass a nonempty display name with --name, or omit --name to use the folder name.",
            ));
        }
        if path.trim().is_empty() {
            return Err(VWatchError::new(
                "invalid_path",
                "The folder must not be empty. No project was registered.",
            )
            .hint("Pass the existing project folder; quote paths containing spaces."));
        }
        let resolved = resolve(path);
        let resolved_str = resolved.to_string_lossy().into_owned();
        if !resolved.is_dir() {
            return Err(VWatchError::new(
                "path_missing",
                format!("Folder does not exist: {resolved_str}. No project was registered."),
            ).hint("Pass an existing project folder and quote paths containing spaces. If a watched project moved, run `vwatch projects list`, then use projects update with its existing id and --path to preserve its cards and history."));
        }
        if let Some(p) = self
            .store
            .all_projects()?
            .into_iter()
            .find(|p| same_path(&p.path, &resolved_str))
        {
            if p.removed_at.is_some() {
                return Err(VWatchError::new(
                    "project_removed",
                    format!(
                        "This folder belongs to removed project '{}' ({}). Nothing was changed.",
                        p.id, p.name
                    ),
                )
                .hint(format!(
                    "Restore its cards and history with: vwatch projects restore {}",
                    p.id
                )));
            }
            if id.is_none_or(|id| id == p.id) && name.is_none_or(|name| name == p.name) {
                return Ok((p, false));
            }
            return Err(VWatchError::new("already_registered", format!("This folder is already registered as '{}' ({}) with different identity fields. Nothing was changed.",p.id,p.name))
                .hint(format!("Use projects update {} with --name for an intentional rename; the existing id remains {}.",p.id,p.id)));
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
                VWatchError::new("invalid_id", format!("Invalid project id '{id}'."))
                    .hint("Use 1 to 40 lowercase letters, digits or hyphens; start with a letter or digit."),
            );
        }
        if self.store.project_including_removed(&id)?.is_some() {
            return Err(VWatchError::new(
                "id_taken",
                format!("The id '{id}' is already used."),
            )
            .hint(format!("If this is the same project after a rename or move, update project {id} with --name for the display name or --path for the existing folder. Its cards and history stay attached. For a different project, pass another id with --id.")));
        }
        match self.store.add_project(&id,name.unwrap_or(&folder),&resolved_str,&self.now())? {
            ProjectRegistration::Added => self.project(&id).map(|p|(p,true)),
            ProjectRegistration::Existing => self.project(&id).map(|p|(p,false)),
            ProjectRegistration::IdTaken => Err(VWatchError::new("id_taken",format!("Project id '{id}' was registered by another caller. Nothing was changed by this call.")).hint("Choose a different id for a different project, or inspect the existing project first.")),
            ProjectRegistration::PathTaken(other) => Err(VWatchError::new("already_registered",format!("This folder was registered as '{other}' by another caller. Nothing was changed by this call.")).hint(format!("Read vwatch projects show {other} to inspect its identity before any edit."))),
        }
    }

    pub fn add_project(&self, path: &str, id: Option<&str>, name: Option<&str>) -> Result<Value> {
        let (project, created) = self.register(path, id, name)?;
        let refreshed = self.refresh_facts(Some(std::slice::from_ref(&project.id)), true)?;
        let mut result = extend(
            project_json(&project),
            json!({"facts": refreshed[0], "created":created, "already_exists":!created, "undo":created.then(||format!("vwatch projects remove {}",project.id))}),
        );
        if !facts::is_git_repo(&project.path) {
            result["warning"] = json!("The folder is not a git repository; only a card written by an agent can describe it.");
        }
        Ok(result)
    }

    /// Register several folders at once and measure them in parallel.
    pub fn add_projects(&self, paths: &[String]) -> Result<Value> {
        let mut added = Vec::new();
        let mut already_registered = Vec::new();
        let mut failed = Vec::new();
        for path in paths {
            match self.register(path, None, None) {
                Ok((p, created)) => {
                    if !created {
                        already_registered.push(p.id.clone());
                    }
                    added.push(p);
                }
                Err(e) => failed.push(json!({"path": path, "error": e.to_json()})),
            }
        }
        let ids: Vec<String> = added.iter().map(|p| p.id.clone()).collect();
        let refreshed = if ids.is_empty() {
            Vec::new()
        } else {
            self.refresh_facts(Some(&ids), true)?
        };
        Ok(
            json!({"added": added.iter().map(project_json).collect::<Vec<_>>(), "failed": failed,"refreshed":refreshed,"already_registered":already_registered}),
        )
    }

    pub fn remove_project(&self, id: &str) -> Result<Value> {
        let mut project = self.store.project_including_removed(id)?.ok_or_else(|| {
            VWatchError::new(
                "project_not_found",
                format!("No project with id '{id}'. Nothing was changed."),
            )
            .hint("Read vwatch projects list or vwatch projects trash for existing ids.")
        })?;
        let removed_at = self.now();
        let changed = self.store.set_project_removed(id, Some(&removed_at))?;
        if changed {
            project.removed_at = Some(removed_at);
        }
        Ok(
            json!({"removed": id, "project": project_json(&project), "changed": changed,
            "already_removed": !changed, "undo": changed.then(||format!("vwatch projects restore {id}"))}),
        )
    }

    pub fn trash_projects(&self) -> Result<Value> {
        Ok(
            json!({"projects": self.store.all_projects()?.iter().filter(|p| p.removed_at.is_some()).map(|p|
            extend(project_json(p), json!({"removed_at":p.removed_at}))).collect::<Vec<_>>() }),
        )
    }

    pub fn restore_project(&self, id: &str) -> Result<Value> {
        let mut project = self.store.project_including_removed(id)?.ok_or_else(|| {
            VWatchError::new(
                "project_not_found",
                format!("No project with id '{id}'. Nothing was changed."),
            )
            .hint("Read vwatch projects list or vwatch projects trash for existing ids.")
        })?;
        let restored = self.store.set_project_removed(id, None)?;
        project.removed_at = None;
        Ok(
            json!({"project": project_json(&project), "restored": restored, "already_active": !restored,
            "undo": restored.then(||format!("vwatch projects remove {id}"))}),
        )
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<Value> {
        let mut result = self.update_project(id, Some(name), None)?;
        let project = result.as_object_mut().unwrap().remove("project").unwrap();
        Ok(extend(project, result))
    }

    /// Change a registered project's identity details, preserving its stable id and history.
    pub fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        path: Option<&str>,
    ) -> Result<Value> {
        let before = self.project(id)?;
        let fields = [("name", name.is_some()), ("path", path.is_some())]
            .into_iter()
            .filter(|(_, present)| *present)
            .map(|(field, _)| field.to_owned())
            .collect();
        if name.is_none() && path.is_none() {
            return Err(VWatchError::new(
                "invalid_update",
                "No project changes were supplied.",
            )
            .hint(
                "Supply --name with a display name, --path with an existing folder, or both. The project id stays unchanged.",
            ));
        }
        let name = name.map(str::trim).unwrap_or(&before.name);
        if name.is_empty() {
            return Err(
                VWatchError::new("invalid_name", "The name must not be empty.")
                    .hint("Pass a nonempty display name with --name."),
            );
        }
        let path = match path {
            Some(path) if path.trim().is_empty() => return Err(VWatchError::new(
                "invalid_path",
                "The folder must not be empty.",
            )
            .hint("Pass the existing project folder with --path. vWatch does not move folders.")),
            Some(path) => {
                let resolved = resolve(path.trim());
                if !resolved.is_dir() {
                    return Err(VWatchError::new("path_missing", format!("Folder does not exist: {}", resolved.display()))
                        .hint("Move or rename the folder first, then pass its new location with --path. Nothing was changed."));
                }
                let resolved = resolved.to_string_lossy().into_owned();
                if same_path(&before.path, &resolved) {
                    before.path.clone()
                } else {
                    resolved
                }
            }
            None => before.path.clone(),
        };
        let receipt = match self.store.update_project(&before, name, &path, fields, &self.now())? {
            ProjectUpdate::Updated(receipt) => *receipt,
            ProjectUpdate::Conflict => return Err(VWatchError::new("version_conflict", "The project changed while you were editing it.")
                .hint("Read `vwatch projects list` and retry the intended changes.")),
            ProjectUpdate::PathTaken(other) => return Err(VWatchError::new("already_registered", format!("This folder is already registered as '{other}'."))
                .hint(format!("Use the existing project '{other}'. Rename or relink the existing project to preserve its identity. Nothing was changed."))),
        };
        let project = self.project(id)?;
        let mut result = with_receipt(json!({"project": project_json(&project)}), receipt);
        if path != before.path {
            // The path and name are already saved. Report refresh trouble separately
            // so a caller never mistakes a measurement failure for a rejected edit.
            result["facts_refresh"] = match self.refresh_one(&project, self.today(), true) {
                Ok(refreshed) => refreshed,
                Err(error) => json!({"id": id, "ok": false, "error": error.to_json()}),
            };
        }
        Ok(result)
    }

    /// The owner's goal for a project; an empty goal clears it. Like a status change, it makes the card due.
    pub fn set_goal(&self, id: &str, goal: &str, due: Option<&str>) -> Result<Value> {
        self.project(id)?;
        let due = due.filter(|d| !d.is_empty());
        if let Some(d) = due {
            if NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
                return Err(
                    VWatchError::new("invalid_date", format!("'{d}' is not a date."))
                        .hint("Write the due date as YYYY-MM-DD."),
                );
            }
        }
        let goal = goal.trim();
        let receipt = self.store.set_goal(
            id,
            goal,
            if goal.is_empty() { None } else { due },
            &self.now(),
        )?;
        Ok(with_receipt(project_json(&self.project(id)?), receipt))
    }

    pub fn set_status(&self, id: &str, status: &str, note: Option<&str>) -> Result<Value> {
        self.project(id)?;
        if !STATUSES.contains(&status) {
            return Err(
                VWatchError::new("invalid_status", format!("Unknown status '{status}'."))
                    .hint(format!("Use one of: {}.", STATUSES.join(", "))),
            );
        }
        let receipt = self.store.set_status(id, status, note, &self.now())?;
        Ok(with_receipt(project_json(&self.project(id)?), receipt))
    }

    /// Git repositories directly inside `root` that are not registered yet, most recently committed first.
    /// Git repositories an owner may want to watch. A folder that is itself a repository
    /// comes first, followed by its neighbours; any other folder is searched two levels deep.
    /// Recently active repositories (and a chosen repository itself) are `recommended`.
    pub fn discover(&self, root: &str) -> Result<Value> {
        let base = resolve(root);
        if !base.is_dir() {
            return Err(VWatchError::new(
                "path_missing",
                format!("Folder does not exist: {}", base.to_string_lossy()),
            ).hint("Retry projects discover with an existing projects folder; quote paths containing spaces."));
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
                VWatchError::new(
                    "path_unreadable",
                    format!("Cannot read {}: {}", base.to_string_lossy(), crate::system::io_message(&e)),
                )
                .hint("Check that the folder is accessible with your current account, then retry discovery. No projects were registered.")
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
        for _ in 0..3 {
            let before = self.store.facts(&project.id)?;
            // Stored facts always describe the current folder (a relink deletes them), so slow scans
            // whose inputs are unchanged carry over from them.
            let mut data = (self.collect)(
                &project.path,
                today,
                with_ci,
                before.as_ref().map(|b| &b.data),
            );
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
                    .filter(|s| s["ok"] == true && s["measured_on"] == today.to_string())
                {
                    Some(space) => space.clone(),
                    None => facts::space(&project.path, today),
                };
            }
            let fp = facts::fingerprint(&data);
            match self.store.save_project_facts(project, before.as_ref().map(|facts| facts.revision), &self.now(), &fp, &data)? {
            crate::store::FactsSave::ProjectChanged => return Err(VWatchError::new("project_changed", format!("Project '{}' changed while its facts were being measured.", project.id))
                .hint("Read `vwatch projects list` and refresh the project's facts again. The outdated measurement was discarded.")),
            crate::store::FactsSave::FactsChanged => continue,
            crate::store::FactsSave::Saved => {}
        }
            return Ok(json!({
                "id": project.id,
                "ok": data["ok"].as_bool().unwrap_or(false),
                "error": data.get("error").cloned().unwrap_or(Value::Null),
                "incomplete_measurements": if data["ok"] == true {
                    [("dirty_lines", data["dirty"]["line_counts"]["ok"] == true), ("rebuildable_space", data["space"]["ok"] == true)]
                        .into_iter().filter_map(|(name, ok)| (!ok).then_some(name)).collect::<Vec<_>>()
                } else { Vec::new() },
                "changed": before.is_none_or(|b| b.fingerprint != fp),
            }));
        }
        Err(VWatchError::new("facts_conflict", format!("Facts for '{}' kept changing during refresh. The outdated measurements were discarded.", project.id))
            .hint("Retry the facts refresh after concurrent refreshes finish."))
    }

    pub(super) fn stored_facts(&self, id: &str) -> Result<crate::store::StoredFacts> {
        if let Some(f) = self.store.facts(id)? {
            return Ok(f);
        }
        self.refresh_facts(Some(&[id.to_owned()]), true)?;
        self.store.facts(id)?.ok_or_else(|| {
            VWatchError::new("facts_missing", format!("Could not measure '{id}'."))
                .hint(format!("Read `vwatch projects list` to check the folder, then run `vwatch facts refresh {id}` and inspect its per-project result before continuing."))
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
}
