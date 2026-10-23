//! Plain output shared by people and agents. Stored content is always data.
use serde_json::{json, Value};

pub(super) fn human(command: &str, data: &Value, data_dir: &std::path::Path) -> String {
    let mut out = String::new();
    match command {
        "facts.history" => {
            line(
                &mut out,
                0,
                &format!(
                    "{} saved measurements, oldest first.",
                    list(&data["samples"]).len()
                ),
            );
            line(&mut out, 0, &format!("Note: {}", text(&data["note"])));
            field(&mut out, "Measurements", &data["samples"], 0);
        }
        "statistics" => {
            line(
                &mut out,
                0,
                &format!(
                    "Stored statistics from {} through {}.",
                    text(&data["from"]),
                    text(&data["until"])
                ),
            );
            line(&mut out, 0, &format!("Note: {}", text(&data["note"])));
            let summary = &data["summary"];
            line(
                &mut out,
                0,
                &format!(
                    "Summary: {} of {} projects measured completely; {} source code lines.",
                    text(&summary["measured_projects"]),
                    text(&summary["projects"]),
                    text(&summary["source_code"])
                ),
            );
            let mut languages: Vec<&Value> = list(&summary["languages"]).iter().collect();
            languages.sort_by(|a, b| {
                b["code"]
                    .as_u64()
                    .cmp(&a["code"].as_u64())
                    .then_with(|| text(&a["language"]).cmp(&text(&b["language"])))
            });
            if !languages.is_empty() {
                line(&mut out, 0, "Source code languages, largest first:");
            }
            for l in languages {
                let share = l["percent"]
                    .as_f64()
                    .map(|p| format!(" ({})", percent(p, false)));
                line(
                    &mut out,
                    1,
                    &format!(
                        "{}: {} lines{}",
                        text(&l["language"]),
                        text(&l["code"]),
                        share.unwrap_or_default()
                    ),
                );
            }
            for p in list(&data["projects"]) {
                line(
                    &mut out,
                    0,
                    &format!("Project {}: {}", text(&p["id"]), text(&p["name"])),
                );
                let c = &p["current"];
                field(
                    &mut out,
                    "Last observation",
                    &json!({"at":c["at"],"branch":c["branch"],"ok":c["ok"],
                    "source_code":if c["ok"]==true && c["code"]["ok"]==true {c["code"]["source_code"].clone()}else{Value::Null},
                    "dirty_files":c["dirty"]["files"],"ahead":c["ahead"],"behind":c["behind"],"ci":c["ci"]["state"]}),
                    1,
                );
                field(&mut out, "Baseline at", &p["baseline"]["at"], 1);
                let change = &p["change"];
                let change = match change["code"].as_i64() {
                    Some(lines) => format!(
                        "{lines:+} source code lines{}",
                        change["percent"]
                            .as_f64()
                            .map(|p| format!(" ({})", percent(p, true)))
                            .unwrap_or_default()
                    ),
                    None => "-".into(),
                };
                line(&mut out, 1, &format!("Change: {change}"));
                fields(
                    &mut out,
                    p,
                    &["comparison_note", "confirmed_completions"],
                    1,
                );
                let activity = &p["activity"];
                line(&mut out, 1, "Reconstructed Git activity:");
                field(&mut out, "Complete", &activity["complete"], 2);
                let coverage: Vec<String> = list(&activity["coverage"])
                    .iter()
                    .map(|c| format!("{} through {}", text(&c["from"]), text(&c["through"])))
                    .collect();
                line(
                    &mut out,
                    2,
                    &format!(
                        "Coverage: {}",
                        if coverage.is_empty() {
                            "none".into()
                        } else {
                            coverage.join("; ")
                        }
                    ),
                );
                field(&mut out, "Totals", &activity["totals"], 2);
                line(
                    &mut out,
                    1,
                    &format!(
                        "{} observation points in the period: the first of each UTC day and every change. facts history with this project ID reads full measurements; statistics --json also includes daily activity and language details.",
                        list(&p["samples"]).len()
                    ),
                );
            }
        }
        "history.list" => {
            table(
                &mut out,
                list(&data["events"]),
                "saved content events",
                "newest first",
                &[
                    "id",
                    "at",
                    "entity",
                    "entity_id",
                    "operation",
                    "title",
                    "fields",
                ],
            );
            line(&mut out, 0, &format!("Note: {}", text(&data["note"])));
            if !data["next_before"].is_null() {
                let mut next = format!(
                    "watchtower history list --before {}",
                    text(&data["next_before"])
                );
                for (field, flag) in [
                    ("project", "project"),
                    ("entity", "entity"),
                    ("entity_id", "id"),
                ] {
                    if let Some(value) = data[field].as_str() {
                        next.push_str(&format!(" --{flag} '{}'", value.replace('\'', "''")));
                    }
                }
                line(&mut out, 0, &format!("Next: {next}"));
            }
        }
        "history.show" => {
            line(
                &mut out,
                0,
                "Saved content event with full before/after values.",
            );
            tree(&mut out, &data["event"], 0);
        }
        "card.compare" | "portfolio.compare" => {
            line(
                &mut out,
                0,
                &format!(
                    "{} changed fields between versions {} and {}.",
                    list(&data["changes"]).len(),
                    text(&data["from"]),
                    text(&data["to"])
                ),
            );
            fields(
                &mut out,
                data,
                &["project", "before_at", "after_at", "changes"],
                0,
            );
        }
        "questions" => {
            let questions = list(&data["questions"]);
            line(
                &mut out,
                0,
                &format!(
                    "{} open question{} for the owner{}, longest waiting first.",
                    questions.len(),
                    if questions.len() == 1 { "" } else { "s" },
                    data["project"]
                        .as_str()
                        .map(|p| format!(" in {p}"))
                        .unwrap_or_default()
                ),
            );
            if let Some(n) = data["snoozed"].as_u64().filter(|n| *n > 0) {
                line(
                    &mut out,
                    0,
                    &format!("Note: {n} snoozed questions are left out until their return date."),
                );
            }
            for q in questions {
                line(
                    &mut out,
                    0,
                    &format!(
                        "- {} | {} | shown {} days | {}",
                        text(&q["id"]),
                        text(&q["project"]),
                        q["days"].as_i64().map_or("?".to_owned(), |d| d.to_string()),
                        text(&q["title"])
                    ),
                );
                let recommended = q["recommended"].as_u64();
                for (i, option) in list(&q["options"]).iter().enumerate() {
                    let mark = if recommended == Some(i as u64 + 1) {
                        " (recommended)"
                    } else {
                        ""
                    };
                    let effect = option["effect"]
                        .as_str()
                        .filter(|e| !e.is_empty())
                        .map(|e| format!(": {}", escaped(e)))
                        .unwrap_or_default();
                    line(
                        &mut out,
                        1,
                        &format!("{}. {}{mark}{effect}", i + 1, text(&option["answer"])),
                    );
                }
            }
        }
        "proposals" => {
            line(
                &mut out,
                0,
                &format!(
                    "{} retained agent proposals, newest first appearance first.",
                    list(&data["proposals"]).len()
                ),
            );
            if let Some(category) = data["category"].as_str() {
                line(&mut out, 0, &format!("Note: category {category} only; omit --category for both ideas and recommendations."));
            }
            field(&mut out, "Proposals", &data["proposals"], 0);
        }
        "projects.add" if data["added"].is_array() => {
            let errors = if data["errors"].is_array() {
                &data["errors"]
            } else {
                &data["failed"]
            };
            let added = list(&data["added"]);
            let failed = list(errors);
            let already = list(&data["already_registered"]).len();
            let refresh_failed = list(&data["refreshed"])
                .iter()
                .filter(|item| item["ok"] != true)
                .count();
            if failed.is_empty() && refresh_failed == 0 {
                line(
                    &mut out,
                    0,
                    &format!(
                        "{} projects registered; {already} were already present.",
                        added.len()
                    ),
                );
            } else {
                line(&mut out, 0, &format!("Error: {} projects registered; {} registrations and {refresh_failed} facts measurements failed. Registered projects remain saved.", added.len(), failed.len()));
            }
            field(&mut out, "Registered projects", &data["added"], 0);
            if !failed.is_empty() {
                field(&mut out, "Failures", errors, 0);
            }
            for item in list(&data["refreshed"]) {
                refresh_item(&mut out, item, 0);
            }
        }
        "status" => {
            line(
                &mut out,
                0,
                &format!(
                    "Watchtower {}: {} projects, {} with a card.",
                    text(&data["version"]),
                    text(&data["projects"]),
                    text(&data["with_card"])
                ),
            );
            fields(
                &mut out,
                data,
                &[
                    "now",
                    "data_dir",
                    "instance_source",
                    "database",
                    "exe",
                    "skill_dir",
                    "logs",
                ],
                0,
            );
            if data["projects"] == 0 {
                line(&mut out, 0, "No projects are registered. Add the existing project folder with projects add.");
            }
        }
        "projects.list" | "projects.removed" | "projects.trash" => table(
            &mut out,
            list(&data["projects"]),
            if command == "projects.list" {
                "registered projects, including archived projects"
            } else {
                "removed projects"
            },
            "oldest registration first",
            &["id", "name", "status", "has_card"],
        ),
        "projects.show" => {
            line(&mut out, 0, "Project record and saved history.");
            tree(&mut out, &data["project"], 0);
        }
        "projects.add" | "projects.rename" | "projects.update" | "projects.status"
        | "projects.goal" | "projects.restore" => {
            let refresh = if data["facts_refresh"].is_object() {
                &data["facts_refresh"]
            } else {
                &data["project"]["facts"]
            };
            let action = match command {
                "projects.add"
                    if data["project"]["already_exists"] == true
                        || data["already_exists"] == true =>
                {
                    "Project already registered"
                }
                "projects.add" => "Project registered",
                "projects.rename" => "Project renamed; its ID and saved history are unchanged",
                "projects.update" if data["changed"] == false => "Project unchanged",
                "projects.update" => "Project updated; its ID and saved history are unchanged",
                "projects.status" => "Project status saved",
                "projects.restore" if data["already_active"] == true => "Project already active",
                "projects.restore" => "Project restored with its saved history",
                _ if data["project"]["goal"].is_null() || data["project"]["goal"] == "" => {
                    "Project goal cleared"
                }
                _ => "Project goal saved",
            };
            if refresh["ok"] == false {
                line(&mut out, 0, &format!("Error: {action}, but facts could not be refreshed. The project change remains saved."));
            } else {
                line(&mut out, 0, &format!("{action}."));
            }
            tree_except(&mut out, &data["project"], &["facts"], 0);
            if refresh.is_object() {
                refresh_item(&mut out, refresh, 0);
            }
        }
        "projects.remove" => {
            line(
                &mut out,
                0,
                &format!(
                    "Project {} {} from the watch list. Its saved data can be restored.",
                    text(&data["removed"]),
                    if data["already_removed"] == true {
                        "already removed"
                    } else {
                        "removed"
                    }
                ),
            );
            tree(&mut out, &data["project"], 0);
        }
        "projects.visit" => {
            line(&mut out, 0, "Project visit recorded.");
            tree(&mut out, data, 0);
        }
        "projects.discover" => {
            table(
                &mut out,
                list(&data["candidates"]),
                "unregistered repositories in the searched folders",
                "selected repository first, then newest commit first",
                &["suggested_id", "name", "last_commit", "recommended", "path"],
            );
            field(&mut out, "Searched folder", &data["folder"], 0);
            line(
                &mut out,
                0,
                &format!(
                    "Note: {} already registered repositories were excluded.",
                    text(&data["already_watched"])
                ),
            );
        }
        "facts.refresh" => {
            let rows = list(&data["refreshed"]);
            let failed = rows.iter().filter(|row| row["ok"] != true).count();
            if failed == 0 {
                line(
                    &mut out,
                    0,
                    &format!("Facts refreshed for {} projects.", rows.len()),
                );
            } else {
                line(&mut out, 0, &format!("Error: facts refreshed for {} of {} projects; {failed} failed. Successful measurements remain saved.", rows.len()-failed, rows.len()));
            }
            for item in rows {
                refresh_item(&mut out, item, 0);
            }
        }
        "facts.show" => {
            line(
                &mut out,
                0,
                if data["facts"]["ok"] == false {
                    "Stored facts: the latest measurement failed."
                } else {
                    "Stored facts from the latest measurement."
                },
            );
            tree(&mut out, &data["facts"], 0);
        }
        "due" => due(&mut out, data),
        "context" | "portfolio.context" => {
            line(
                &mut out,
                0,
                if command == "context" {
                    "Project context for writing its card."
                } else if data.get("owner_ideas").is_none() {
                    "Full project details from the portfolio context."
                } else {
                    "Portfolio context for writing the brief."
                },
            );
            field(&mut out, "Context token", &data["context_token"], 0);
            if command == "portfolio.context" {
                line(
                    &mut out,
                    0,
                    "Note: archived and removed projects are excluded from the portfolio.",
                );
                let summaries: Vec<&str> = list(&data["projects"])
                    .iter()
                    .filter(|p| p["card"]["detail"] == "summary")
                    .filter_map(|p| p["id"].as_str())
                    .collect();
                if let Some(first) = summaries.first() {
                    let details = crate::cli::scoped_command(
                        data_dir,
                        &format!("watchtower portfolio context --project {first}"),
                    );
                    line(&mut out, 0, &format!("Note: {} projects unchanged since the previous brief are summaries (detail: summary). For the full card details of one, run `{details}`; repeat --project for several.", summaries.len()));
                }
                tree_except(
                    &mut out,
                    &portfolio_context_view(data),
                    &["context_token"],
                    0,
                );
            } else {
                tree_except(&mut out, data, &["context_token"], 0);
            }
        }
        "card.show" | "portfolio.show" => {
            line(
                &mut out,
                0,
                &format!(
                    "{} version {}.",
                    if command == "card.show" {
                        "Project card"
                    } else {
                        "Portfolio brief"
                    },
                    text(&data["version"])
                ),
            );
            let mut view = data.clone();
            let (field, kind) = if command == "card.show" {
                ("card", crate::cards::Kind::Card)
            } else {
                ("portfolio", crate::cards::Kind::Portfolio)
            };
            if let Some(doc) = view.get_mut(field) {
                *doc = kind.current_view(doc);
            }
            tree(&mut out, &view, 0);
        }
        "card.put" | "portfolio.put" | "card.restore" | "portfolio.restore" => {
            let subject = if command.starts_with("card.") {
                "Card"
            } else {
                "Portfolio brief"
            };
            if data["dry_run"] == true {
                line(
                    &mut out,
                    0,
                    &format!("{subject} is valid. Dry run: nothing was saved."),
                );
            } else if data["already_saved"] == true {
                line(
                    &mut out,
                    0,
                    &format!(
                        "{subject} already saved as version {}; no version was added.",
                        text(&data["version"])
                    ),
                );
            } else if data["saved"] == false {
                line(
                    &mut out,
                    0,
                    &format!("{subject} unchanged; no version was added."),
                );
            } else {
                line(
                    &mut out,
                    0,
                    &format!("{subject} saved as version {}.", text(&data["version"])),
                );
            }
            if let Some(token) = data.get("context_token") {
                field(&mut out, "Context token", token, 0);
            }
            if matches!(command, "card.put" | "portfolio.put") {
                fields(&mut out, data, &["project"], 0);
                for key in ["changed_fields", "unfilled_fields"] {
                    if let Some(value) = data.get(key) {
                        line(
                            &mut out,
                            0,
                            &format!(
                                "{}: {}",
                                label(key),
                                if list(value).is_empty() {
                                    "none".to_owned()
                                } else {
                                    text(value)
                                }
                            ),
                        );
                    }
                }
                fields(&mut out, data, &["warnings"], 0);
                let items = list(&data["items"]);
                let counts: Vec<String> = ["new", "kept", "edited", "restored"]
                    .iter()
                    .map(|change| {
                        (
                            change,
                            items.iter().filter(|i| i["change"] == *change).count(),
                        )
                    })
                    .filter(|(_, n)| *n > 0)
                    .map(|(change, n)| format!("{n} {change}"))
                    .collect();
                line(
                    &mut out,
                    0,
                    &format!(
                        "Items: {}{} (ID | kind | title | change)",
                        items.len(),
                        if counts.is_empty() {
                            String::new()
                        } else {
                            format!(", {}", counts.join(", "))
                        }
                    ),
                );
                for item in items {
                    line(
                        &mut out,
                        1,
                        &format!(
                            "- {} | {} | {} | {}",
                            if item["id"].is_null() && data["dry_run"] == true {
                                "assigned on save".to_owned()
                            } else {
                                text(&item["id"])
                            },
                            text(&item["kind"]),
                            text(&item["title"]),
                            text(&item["change"])
                        ),
                    );
                }
            } else {
                tree_except(
                    &mut out,
                    data,
                    &["saved", "dry_run", "already_saved", "context_token"],
                    0,
                );
            }
        }
        "card.history" | "portfolio.history" => {
            line(&mut out, 0, "Saved document versions, oldest first.");
            field(&mut out, "Context token", &data["context_token"], 0);
            tree_except(&mut out, data, &["context_token"], 0);
        }
        "feedback" | "feedback.list" => collection(
            &mut out,
            "feedback entries",
            "newest feedback first",
            &data["feedback"],
        ),
        "feedback.set" | "feedback.clear" | "feedback.restore" => {
            line(
                &mut out,
                0,
                match command {
                    "feedback.clear" => "Feedback cleared.",
                    "feedback.restore" => "Feedback restored.",
                    _ => "Feedback saved.",
                },
            );
            tree(&mut out, data, 0);
        }
        "ideas.list" | "ideas.removed" | "ideas.trash" => collection(
            &mut out,
            if command == "ideas.list" {
                "parked ideas"
            } else {
                "removed ideas"
            },
            "oldest ID first",
            &data["ideas"],
        ),
        "ideas.add" | "ideas.restore" | "ideas.update" => {
            line(
                &mut out,
                0,
                if data["idea"]["already_exists"] == true || data["already_active"] == true {
                    "Idea already present."
                } else if command == "ideas.update" {
                    if data["changed"] == false {
                        "Idea unchanged."
                    } else {
                        "Idea updated; its ID and history are preserved."
                    }
                } else if command == "ideas.add" {
                    "Idea saved."
                } else {
                    "Idea restored."
                },
            );
            tree(&mut out, &data["idea"], 0);
        }
        "ideas.remove" => {
            line(
                &mut out,
                0,
                &format!(
                    "Idea {} {}; it can be restored.",
                    text(&data["removed"]),
                    if data["already_removed"] == true {
                        "already removed"
                    } else {
                        "removed"
                    }
                ),
            );
            tree(&mut out, &data["idea"], 0);
        }
        "runs.start" | "runs.status" | "runs.fail" | "runs.finish" => run(&mut out, command, data),
        "tasks.list" => table(
            &mut out,
            list(&data["tasks"]),
            "tasks matching the selection, including completed and cancelled tasks",
            "newest ID first",
            &["id", "status", "revision", "title", "projects"],
        ),
        "tasks.create" | "tasks.show" | "tasks.report" | "tasks.confirm" | "tasks.reopen"
        | "tasks.cancel" => {
            line(
                &mut out,
                0,
                match command {
                    "tasks.create" if data["created"] == false => "Task already exists.",
                    "tasks.create" => "Task created.",
                    "tasks.report" if data["already_reported"] == true && data["task"]["status"] == "done" => "Task result was already reported and confirmed by the user. Current state: done.",
                    "tasks.report" if data["already_reported"] == true && data["task"]["status"] == "review" => "Task result was already reported. Current state: review; the user's confirmation is still required.",
                    "tasks.report" if data["already_reported"] == true && data["task"]["status"] == "blocked" => "Task blockage was already reported. Current state: blocked.",
                    "tasks.report" if data["task"]["status"] == "review" => {
                        "Task result reported; the user's confirmation is still required."
                    }
                    "tasks.report" => "Task blockage reported.",
                    "tasks.confirm" => "Task result confirmed.",
                    "tasks.reopen" => "Task reopened.",
                    "tasks.cancel" => "Task cancelled.",
                    _ => "Task record, result and history.",
                },
            );
            fields(&mut out, &data["task"], &["id", "status", "revision"], 0);
            tree_except(&mut out, &data["task"], &["id", "status", "revision"], 0);
        }
        "warnings.list" | "warnings.acknowledge" | "warnings.clear" => {
            line(
                &mut out,
                0,
                match command {
                    "warnings.acknowledge" => {
                        "Warning acknowledgement saved; measured facts remain visible."
                    }
                    "warnings.clear" => "Warning acknowledgement cleared.",
                    _ => "Measured warnings and deliberate acknowledgements.",
                },
            );
            tree(&mut out, data, 0);
        }
        "changes.show" | "changes.read" => {
            line(
                &mut out,
                0,
                if command == "changes.read" {
                    "Changes marked as read."
                } else {
                    "Changes since the explicitly acknowledged snapshot."
                },
            );
            tree(&mut out, &data["changes"], 0);
        }
        "settings.show" | "settings.update" => {
            line(
                &mut out,
                0,
                if command == "settings.update" {
                    "Settings saved."
                } else {
                    "Watchtower settings."
                },
            );
            tree(&mut out, data, 0);
        }
        "overview" => overview(&mut out, data),
        "undo" => {
            line(
                &mut out,
                0,
                if data["changed"] == false {
                    "Change already reverted."
                } else {
                    "Change reverted."
                },
            );
            if let Some(note) = data["note"].as_str() {
                line(&mut out, 0, &format!("Note: {}", escaped(note)));
            }
            fields(&mut out, data, &["target", "current", "notes"], 0);
        }
        "usage" => {
            line(
                &mut out,
                0,
                &format!(
                    "{} recorded calls in the selected usage period.",
                    text(&data["calls"])
                ),
            );
            line(&mut out, 0, "Note: command shapes contain no argument values. Repeated shapes do not prove repeated effects.");
            if data["older_events_omitted"] == true {
                line(
                    &mut out,
                    0,
                    "Note: older log events are omitted; these counts cover the retained events.",
                );
            }
            fields(&mut out, data, &["since", "log", "invalid_log_lines"], 0);
            table(
                &mut out,
                list(&data["commands"]),
                "command shapes",
                "command name order",
                &[
                    "command",
                    "calls",
                    "failed",
                    "invalid",
                    "unknown",
                    "not_finished",
                    "large_outputs",
                ],
            );
            fields(
                &mut out,
                data,
                &[
                    "repeated_command_shapes",
                    "list_then_change",
                    "show_after_change",
                    "invalid_call_flags",
                    "parent_processes",
                    "notes",
                ],
                0,
            );
        }
        "jobs.list" => table(
            &mut out,
            list(&data["jobs"]),
            "saved jobs in this instance",
            "newest first",
            &["token", "state", "command", "created_at"],
        ),
        "jobs.status" | "jobs.wait" => {
            let job = &data["job"];
            line(&mut out, 0, match job["state"].as_str() {
                Some("running") => "Not finished: the command is still running; it has not returned a result.",
                Some("completed") if data["result"]["exit_code"] == 0 => "Command returned successfully; its recorded result is available.",
                Some("completed") => "Error: the command returned a failure; its recorded result is available.",
                _ => "Error: no completed result or active worker could be confirmed; the outcome is unknown. Changes can have happened.",
            });
            tree(&mut out, job, 0);
            fields(&mut out, &data["result"], &["exit_code"], 0);
            if let Some(next) = data["next"].as_str().filter(|value| safe_command(value)) {
                line(&mut out, 0, &format!("Next: {next}"));
            }
        }
        "activity" => {
            line(
                &mut out,
                0,
                &format!(
                    "{} latest recorded calls of {} retained calls, newest first.",
                    list(&data["activity"]).len(),
                    text(&data["total_recorded_calls"])
                ),
            );
            line(&mut out, 0, "Note: a command result does not confirm a complete daily run. Unknown means the result was not recorded; changes can still have happened.");
            if data["older_events_omitted"] == true {
                line(&mut out, 0, "Note: older log events are omitted.");
            }
            table(
                &mut out,
                list(&data["activity"]),
                "recent calls (at most 20)",
                "newest first",
                &["at", "command", "targets", "outcome", "duration_ms"],
            );
            fields(&mut out, data, &["invalid_log_lines"], 0);
            line(&mut out, 0, "For a stopped command, inspect its job or current project, document or run before repeating a write.");
        }
        "schema.card" | "schema.portfolio" => {
            line(
                &mut out,
                0,
                if command == "schema.card" {
                    "Project card fields and limits."
                } else {
                    "Portfolio brief fields and limits."
                },
            );
            line(
                &mut out,
                0,
                "Use the exact field names below in the document. Required fields are marked.",
            );
            schema_fields(&mut out, &data["schema"], "", 0);
        }
        _ => {
            line(&mut out, 0, "Command result.");
            tree(&mut out, data, 0);
        }
    }
    if command == "projects.list" && data["query"].is_string() {
        line(
            &mut out,
            0,
            "Note: only projects matching the supplied query are shown.",
        );
        field(&mut out, "Query", &data["query"], 0);
    }
    if matches!(command, "context" | "portfolio.context") {
        if let Some(instruction) = data["next"]
            .as_str()
            .filter(|value| !value.starts_with("watchtower "))
        {
            line(&mut out, 0, &escaped(instruction));
        }
    }
    if let Some(undo) = data
        .get("undo")
        .or_else(|| data.get("project").and_then(|v| v.get("undo")))
        .or_else(|| data.get("task").and_then(|v| v.get("undo")))
        .or_else(|| data.get("idea").and_then(|v| v.get("undo")))
        .and_then(Value::as_str)
        .filter(|value| safe_command(value))
    {
        line(&mut out, 0, &format!("Undo: {undo}"));
    }
    if matches!(command, "due" | "runs.start" | "undo") {
        if let Some(next) = data["next"].as_str().filter(|value| safe_command(value)) {
            line(&mut out, 0, &format!("Next: {next}"));
        }
    }
    out.trim_end().to_owned()
}

fn list(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// A heading or table cell never introduces a line or terminal command.
fn escaped(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if c.is_control()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
            {
                out.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            _ => out.push(c),
        }
    }
    out
}
fn text(value: &Value) -> String {
    match value {
        Value::String(s) => {
            if s.is_empty() {
                "-".into()
            } else {
                escaped(s)
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.into(),
        Value::Null => "-".into(),
        Value::Array(items) if items.iter().all(|v| !v.is_object() && !v.is_array()) => {
            items.iter().map(text).collect::<Vec<_>>().join(", ")
        }
        _ => "-".into(),
    }
}
/// One decimal is enough to read a share; a tiny nonzero share never reads as zero.
fn percent(value: f64, signed: bool) -> String {
    let rounded = format!("{:.1}", value.abs());
    let sign = if value < 0.0 {
        "-"
    } else if signed && value > 0.0 {
        "+"
    } else {
        ""
    };
    if value != 0.0 && rounded == "0.0" {
        format!("{sign}<0.1%")
    } else {
        format!("{sign}{rounded}%")
    }
}
fn line(out: &mut String, depth: usize, content: &str) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(content);
    out.push('\n');
}
/// Render weekly findings once, adding their full evidence to the matching card
/// entry before referencing its position. JSON retains the complete arrays;
/// unmatched or ambiguous evidence remains fully rendered in its original place.
fn portfolio_context_view(data: &Value) -> Value {
    let mut view = data.clone();
    for project in view["projects"].as_array_mut().into_iter().flatten() {
        for key in ["recent_changes", "milestones"] {
            let (Some(evidence), Some(entries)) = (
                project["review_week_evidence"][key].as_array(),
                project["card"][key].as_array(),
            ) else {
                continue;
            };
            if evidence.is_empty() {
                continue;
            }
            let matches = evidence
                .iter()
                .map(|item| {
                    let candidates = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| {
                            entry.as_object().is_some_and(|fields| {
                                !fields.is_empty()
                                    && fields
                                        .iter()
                                        .all(|(key, value)| item.get(key) == Some(value))
                            })
                        })
                        .collect::<Vec<_>>();
                    (candidates.len() == 1).then(|| (candidates[0].0, item.clone()))
                })
                .collect::<Option<Vec<_>>>();
            if let Some(matches) = matches {
                // Do not fold distinct evidence into the same abbreviated row.
                let unique = matches
                    .iter()
                    .map(|(index, _)| *index)
                    .collect::<std::collections::HashSet<_>>();
                if unique.len() != matches.len() {
                    continue;
                }
                for (index, evidence) in &matches {
                    project["card"][key][*index] = evidence.clone();
                }
                project["review_week_evidence"][key] = json!(format!(
                    "Same entries as this project's card.{key}: {} (1-based positions; full evidence above).",
                    matches.iter().map(|(index, _)| (index + 1).to_string()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
    }
    view
}

fn fields(out: &mut String, data: &Value, keys: &[&str], depth: usize) {
    for key in keys {
        if let Some(value) = data.get(*key) {
            field(out, &label(key), value, depth);
        }
    }
}
fn field(out: &mut String, title: &str, value: &Value, depth: usize) {
    let title = escaped(title);
    match value {
        Value::Object(map) if !map.is_empty() => {
            line(out, depth, &format!("{title}:"));
            tree(out, value, depth + 1);
        }
        Value::Array(items) if !items.is_empty() => {
            line(out, depth, &format!("{title}: {} entries", items.len()));
            tree(out, value, depth + 1);
        }
        Value::Object(_) | Value::Array(_) => line(out, depth, &format!("{title}: none")),
        Value::String(s) if s.contains('\n') => {
            line(out, depth, &format!("{title}:"));
            for part in s.split('\n') {
                line(out, depth + 1, &escaped(part));
            }
        }
        _ => line(out, depth, &format!("{title}: {}", text(value))),
    }
}
fn tree(out: &mut String, value: &Value, depth: usize) {
    tree_except(out, value, &[], depth);
}
fn tree_except(out: &mut String, value: &Value, excluded: &[&str], depth: usize) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                // Service recipes can contain placeholders. The operating Skill owns
                // recipes; they are not executable continuations of this output.
                if excluded.contains(&key.as_str())
                    || matches!(key.as_str(), "next" | "repair_task" | "undo")
                {
                    continue;
                }
                field(out, &label(key), value, depth);
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Value::Object(map) = item {
                    let keys: Vec<_> = [
                        "id", "item_id", "title", "name", "text", "term", "what", "path",
                    ]
                    .into_iter()
                    .filter(|key| {
                        map.get(*key)
                            .is_some_and(|v| v.is_string() || v.is_number())
                    })
                    .take(2)
                    .collect();
                    let heading = keys
                        .iter()
                        .map(|key| text(&item[*key]))
                        .collect::<Vec<_>>()
                        .join(" | ");
                    line(
                        out,
                        depth,
                        &format!(
                            "- {}",
                            if heading.is_empty() {
                                "Entry"
                            } else {
                                &heading
                            }
                        ),
                    );
                    tree_except(out, item, &keys, depth + 1);
                } else if item.is_array() {
                    tree(out, item, depth + 1);
                } else {
                    line(out, depth, &format!("- {}", text(item)));
                }
            }
        }
        _ => line(out, depth.max(1), &text(value)),
    }
}
fn collection(out: &mut String, scope: &str, order: &str, value: &Value) {
    line(
        out,
        0,
        &format!(
            "{} {scope}, {order}; all matching entries.",
            list(value).len()
        ),
    );
    tree(out, value, 0);
}
fn table(out: &mut String, rows: &[Value], scope: &str, order: &str, columns: &[&str]) {
    line(
        out,
        0,
        &format!("{} {scope}, {order}; all matching rows.", rows.len()),
    );
    if rows.is_empty() {
        return;
    }
    let mut cells = vec![columns
        .iter()
        .map(|key| key.to_uppercase())
        .collect::<Vec<_>>()];
    cells.extend(rows.iter().map(|row| {
        columns
            .iter()
            .map(|key| text(&row[*key]))
            .collect::<Vec<_>>()
    }));
    let widths: Vec<_> = (0..columns.len())
        .map(|i| {
            cells
                .iter()
                .map(|row| row[i].chars().count())
                .max()
                .unwrap_or(0)
                // Keep every value, but do not pad all other rows to the length
                // of one unusually long title, ID or path. Paging bounds output.
                .min(72)
        })
        .collect();
    for (row_index, values) in cells.iter().enumerate() {
        let row = values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                if i + 1 == values.len() {
                    value.clone()
                } else {
                    format!(
                        "{value}{}",
                        " ".repeat(widths[i].saturating_sub(value.chars().count()))
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        line(out, usize::from(row_index != 0), &row);
    }
}
fn refresh_item(out: &mut String, item: &Value, depth: usize) {
    let state = if item["ok"] == true {
        if item["changed"] == true {
            "updated"
        } else {
            "unchanged"
        }
    } else if item["ok"] == false {
        "failed"
    } else {
        "unknown"
    };
    line(
        out,
        depth + 1,
        &format!("{}: facts {state}.", text(&item["id"])),
    );
    for key in ["error", "warning", "warnings", "hint"] {
        if let Some(value) = item.get(key).filter(|value| match value {
            Value::Null => false,
            Value::String(text) => !text.is_empty(),
            Value::Array(items) => !items.is_empty(),
            Value::Object(fields) => !fields.is_empty(),
            _ => true,
        }) {
            field(out, &label(key), value, depth + 2);
        }
    }
}
fn due(out: &mut String, data: &Value) {
    let projects = list(&data["projects"]);
    let problems = list(&data["problems"]);
    line(
        out,
        0,
        &format!(
            "{}{} projects need a card; {} are current; {} have a facts problem.",
            if problems.is_empty() { "" } else { "Error: " },
            projects.len(),
            list(&data["up_to_date"]).len(),
            problems.len()
        ),
    );
    line(out, 0, "Note: archived and removed projects are excluded; registered projects are in registration order.");
    if !problems.is_empty() {
        line(out, 0, "Note: results for the other projects remain available. Continue those projects and resolve or report the facts problems before completing the daily run.");
    }
    fields(out, data, &["today"], 0);
    if !projects.is_empty() {
        table(
            out,
            projects,
            "due projects",
            "oldest registration first",
            &["id", "name", "reason"],
        );
        for project in projects {
            if !project["changes"].is_null() || !project["new_fields"].is_null() {
                field(out, "Project", &project["id"], 0);
                fields(out, project, &["changes", "new_fields"], 1);
            }
        }
    }
    if !problems.is_empty() {
        field(out, "Facts problems", &data["problems"], 0);
    }
    if data["portfolio"]["due"] == true {
        line(
            out,
            0,
            &format!(
                "Portfolio brief is due: {}.",
                text(&data["portfolio"]["reason"])
            ),
        );
    } else if data["portfolio"]["due"] == false {
        line(out, 0, "Portfolio brief is current.");
    } else {
        line(
            out,
            0,
            "Note: whether the portfolio brief is due is unknown.",
        );
    }
}
fn run(out: &mut String, command: &str, data: &Value) {
    let run = if data["run"].is_object() {
        &data["run"]
    } else {
        &data["latest"]
    };
    if run.is_null() {
        line(out, 0, "No agent run has been recorded.");
    } else {
        let state = run["state"].as_str().unwrap_or("unknown");
        let action = match command {
            "runs.start" => "started or resumed",
            "runs.fail" if data["already_recorded"] == true => {
                "failure was already recorded; current state"
            }
            "runs.fail" => "failure recorded",
            "runs.finish" => "finished",
            _ => "status",
        };
        line(
            out,
            0,
            &format!(
                "{}Agent run {} {action}: {state}.",
                if (command == "runs.finish" && state != "complete")
                    || (command == "runs.start" && !list(&data["due"]["problems"]).is_empty())
                {
                    "Error: "
                } else {
                    ""
                },
                text(&run["id"])
            ),
        );
        if state != "complete" {
            line(
                out,
                0,
                "Note: this run does not confirm a complete daily update.",
            );
        }
        if let Some(instruction) = data["instruction"].as_str() {
            line(out, 0, &escaped(instruction));
        }
        tree(out, run, 0);
    }
    if data["due"].is_object() {
        due(out, &data["due"]);
    }
    field(out, "Last successful run", &data["last_successful"], 0);
    fields(out, data, &["pending_owner_replies"], 0);
    if command == "runs.status" {
        line(
            out,
            0,
            "Note: history contains at most the 20 latest runs, newest first.",
        );
        field(out, "Run history", &data["history"], 0);
    }
}
fn overview(out: &mut String, data: &Value) {
    let projects = list(&data["projects"]);
    let failed = list(&data["refreshed"])
        .iter()
        .filter(|row| row["ok"] != true)
        .count();
    if failed > 0 {
        line(out, 0, &format!("Error: overview contains {failed} failed facts measurements; successful measurements remain saved."));
    } else {
        line(
            out,
            0,
            &format!(
                "Watchtower overview: {} registered projects, including archived projects.",
                projects.len()
            ),
        );
    }
    line(out, 0, "Note: this is a summary. projects show reads the complete project record; portfolio show reads the complete brief.");
    fields(
        out,
        data,
        &["now", "previous_visit", "portfolio_updated_at"],
        0,
    );
    field(out, "Portfolio brief", &data["portfolio"], 0);
    for project in projects {
        line(
            out,
            0,
            &format!(
                "- {} | {} [{}]",
                text(&project["id"]),
                text(&project["name"]),
                text(&project["status"])
            ),
        );
        fields(
            out,
            project,
            &[
                "status_note",
                "goal",
                "goal_due",
                "facts_collected_at",
                "card_updated_at",
            ],
            1,
        );
        line(
            out,
            1,
            &format!(
                "Card state: {}",
                if project["card"].is_null() {
                    "missing"
                } else if project["card_outdated"] == true {
                    "outdated"
                } else {
                    "current"
                }
            ),
        );
        fields(
            out,
            &project["card"],
            &[
                "headline",
                "phase",
                "reentry",
                "waiting_on_you",
                "open_ends",
            ],
            1,
        );
        let facts = &project["facts"];
        if facts.is_null() {
            line(out, 1, "Facts: not measured.");
        } else if facts["ok"] == false {
            line(out, 1, "Facts: last measurement failed.");
            fields(out, facts, &["error"], 2);
        } else {
            line(
                out,
                1,
                &format!(
                    "Facts: {} uncommitted files; {} unpushed commits; remote {}; CI {}.",
                    text(&facts["dirty"]["files"]),
                    text(&facts["ahead"]),
                    text(&facts["has_remote"]),
                    text(&facts["ci"]["state"])
                ),
            );
        }
        let warnings: Vec<_> = list(&project["warnings"]).iter().map(|warning| json!({"key":warning["key"],"severity":warning["severity"],"title":warning["title"],"acknowledged":warning["acknowledged"],"note":warning["note"]})).collect();
        if !warnings.is_empty() {
            field(out, "Warnings", &Value::Array(warnings), 1);
        }
    }
    let tasks: Vec<_> = list(&data["tasks"])
        .iter()
        .filter(|task| {
            matches!(
                task["status"].as_str(),
                Some("assigned" | "blocked" | "review")
            )
        })
        .cloned()
        .collect();
    line(out, 0, "Note: the task summary includes assigned, blocked and review tasks; tasks list also includes completed and cancelled tasks.");
    table(
        out,
        &tasks,
        "open tasks",
        "newest ID first",
        &["id", "status", "title", "projects"],
    );
    field(out, "Unacknowledged changes", &data["changes"]["total"], 0);
    line(
        out,
        0,
        &format!(
            "Parked ideas: {}; ideas list reads them in full.",
            list(&data["ideas"]).len()
        ),
    );
    field(
        out,
        "Latest daily run state",
        &data["run_status"]["state"],
        0,
    );
    field(
        out,
        "Last successful daily run",
        &data["run_status"]["last_successful"],
        0,
    );
}
fn schema_fields(out: &mut String, schema: &Value, prefix: &str, depth: usize) {
    if let Some(properties) = schema["properties"].as_object() {
        for (key, value) in properties {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            let required = list(&schema["required"]).iter().any(|v| v == key);
            let inner = list(&value["anyOf"]).first().unwrap_or(value);
            line(
                out,
                depth,
                &format!(
                    "{}: {}; {}",
                    escaped(&path),
                    schema_description(value),
                    if required { "required" } else { "optional" }
                ),
            );
            if inner["type"] == "object" {
                schema_fields(out, inner, &path, depth + 1);
            }
            if inner["type"] == "array" {
                if inner["items"]["type"] == "object" {
                    schema_fields(out, &inner["items"], &format!("{path}[]"), depth + 1);
                } else {
                    line(
                        out,
                        depth + 1,
                        &format!("Per entry: {}", schema_description(&inner["items"])),
                    );
                }
            }
        }
    }
}
fn schema_description(schema: &Value) -> String {
    let inner = list(&schema["anyOf"]).first().unwrap_or(schema);
    let mut limits = vec![text(&inner["type"])];
    if !schema["anyOf"].is_null() {
        limits.push("nullable".into());
    }
    for (field, name) in [
        ("minLength", "minimum characters"),
        ("maxLength", "maximum characters"),
        ("maxItems", "maximum entries"),
        ("minimum", "minimum"),
        ("maximum", "maximum"),
    ] {
        if !inner[field].is_null() {
            limits.push(format!("{name}: {}", text(&inner[field])));
        }
    }
    if !inner["enum"].is_null() {
        limits.push(format!("values: {}", text(&inner["enum"])));
    }
    if inner["format"] == "date" {
        limits.push("ISO date YYYY-MM-DD".into());
    } else if let Some(pattern) = inner["pattern"].as_str() {
        limits.push(format!("pattern: {}", escaped(pattern)));
    }
    if let Some(description) = inner["description"].as_str() {
        limits.push(escaped(description));
    }
    limits.join("; ")
}
fn safe_command(command: &str) -> bool {
    command.starts_with("watchtower ")
        && !command.chars().any(char::is_control)
        && !command.contains(['<', '>'])
}
fn label(key: &str) -> String {
    let value = match key {
        "id" => "ID",
        "item_id" => "Item ID",
        "note" => "Saved note",
        "ci" => "CI",
        "url" => "URL",
        "context_token" => "Context token",
        "hidden_modules" => "Collapsed modules",
        "has_card" => "Card available",
        "one_liner" => "One-line description",
        "waiting_on_you" => "Questions for the user",
        "owner_ideas" => "The user's parked ideas",
        "status_note" => "The user's status instruction",
        "goal" => "The user's goal",
        "goal_due" => "Goal due date",
        "owner_last_opened" => "Last opened by the user",
        "pending_owner_replies" => "User replies awaiting incorporation",
        _ => {
            return {
                let words = key.replace('_', " ");
                let mut chars = words.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
        }
    };
    value.into()
}
