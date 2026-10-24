//! Readable operations output; shared escaping and receipt framing stay in the parent.
use super::observations::due;
use super::{
    collection, escaped, field, fields, line, list, refresh_item, safe_command, table, text, tree,
    tree_except,
};
use serde_json::Value;

pub(super) fn render(command: &str, data: &Value, out: &mut String) {
    match command {
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
                    out,
                    0,
                    &format!(
                        "{} projects registered; {already} were already present.",
                        added.len()
                    ),
                );
            } else {
                line(out, 0, &format!("Error: {} projects registered; {} registrations and {refresh_failed} facts measurements failed. Registered projects remain saved.", added.len(), failed.len()));
            }
            field(out, "Registered projects", &data["added"], 0);
            if !failed.is_empty() {
                field(out, "Failures", errors, 0);
            }
            for item in list(&data["refreshed"]) {
                refresh_item(out, item, 0);
            }
        }
        "status" => {
            line(
                out,
                0,
                &format!(
                    "vWatch {}: {} projects, {} with a card.",
                    text(&data["version"]),
                    text(&data["projects"]),
                    text(&data["with_card"])
                ),
            );
            fields(
                out,
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
                line(out, 0, "No projects are registered. Add the existing project folder with projects add.");
            }
        }
        "projects.list" | "projects.removed" | "projects.trash" => table(
            out,
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
            line(out, 0, "Project record and saved history.");
            tree(out, &data["project"], 0);
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
                line(out, 0, &format!("Error: {action}, but facts could not be refreshed. The project change remains saved."));
            } else {
                line(out, 0, &format!("{action}."));
            }
            tree_except(out, &data["project"], &["facts"], 0);
            if refresh.is_object() {
                refresh_item(out, refresh, 0);
            }
        }
        "projects.remove" => {
            line(
                out,
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
            tree(out, &data["project"], 0);
        }
        "projects.visit" => {
            line(out, 0, "Project visit recorded.");
            tree(out, data, 0);
        }
        "projects.discover" => {
            table(
                out,
                list(&data["candidates"]),
                "unregistered repositories in the searched folders",
                "selected repository first, then newest commit first",
                &["suggested_id", "name", "last_commit", "recommended", "path"],
            );
            field(out, "Searched folder", &data["folder"], 0);
            line(
                out,
                0,
                &format!(
                    "Note: {} already registered repositories were excluded.",
                    text(&data["already_watched"])
                ),
            );
        }
        "feedback" | "feedback.list" => collection(
            out,
            "feedback entries",
            "newest feedback first",
            &data["feedback"],
        ),
        "feedback.set" | "feedback.clear" | "feedback.restore" => {
            line(
                out,
                0,
                match command {
                    "feedback.clear" => "Feedback cleared.",
                    "feedback.restore" => "Feedback restored.",
                    _ => "Feedback saved.",
                },
            );
            tree(out, data, 0);
        }
        "ideas.list" | "ideas.removed" | "ideas.trash" => collection(
            out,
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
                out,
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
            tree(out, &data["idea"], 0);
        }
        "ideas.status" => {
            line(
                out,
                0,
                if data["changed"] == true {
                    "Idea status saved; rating unchanged."
                } else {
                    "Idea status; rating unchanged."
                },
            );
            tree(out, &data["idea_status"], 0);
        }
        "ideas.remove" => {
            line(
                out,
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
            tree(out, &data["idea"], 0);
        }
        "runs.start" | "runs.status" | "runs.fail" | "runs.finish" => run(out, command, data),
        "tasks.list" => table(
            out,
            list(&data["tasks"]),
            "tasks matching the selection, including completed and cancelled tasks",
            "newest ID first",
            &["id", "status", "revision", "title", "projects"],
        ),
        "tasks.create" | "tasks.show" | "tasks.report" | "tasks.confirm" | "tasks.reopen"
        | "tasks.cancel" => {
            line(
                out,
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
            fields(out, &data["task"], &["id", "status", "revision"], 0);
            tree_except(out, &data["task"], &["id", "status", "revision"], 0);
        }
        "warnings.list" | "warnings.acknowledge" | "warnings.clear" => {
            line(
                out,
                0,
                match command {
                    "warnings.acknowledge" => {
                        "Warning acknowledgement saved; measured facts remain visible."
                    }
                    "warnings.clear" => "Warning acknowledgement cleared.",
                    _ => "Measured warnings and deliberate acknowledgements.",
                },
            );
            tree(out, data, 0);
        }
        "changes.show" | "changes.read" => {
            line(
                out,
                0,
                if command == "changes.read" {
                    "Changes marked as read."
                } else {
                    "Changes since the explicitly acknowledged snapshot."
                },
            );
            tree(out, &data["changes"], 0);
        }
        "settings.show" | "settings.update" => {
            line(
                out,
                0,
                if command == "settings.update" {
                    "Settings saved."
                } else {
                    "vWatch settings."
                },
            );
            tree(out, data, 0);
        }
        "undo" => {
            line(
                out,
                0,
                if data["changed"] == false {
                    "Change already reverted."
                } else {
                    "Change reverted."
                },
            );
            if let Some(note) = data["note"].as_str() {
                line(out, 0, &format!("Note: {}", escaped(note)));
            }
            fields(out, data, &["target", "current", "notes"], 0);
        }
        "usage" => {
            line(
                out,
                0,
                &format!(
                    "{} recorded calls in the selected usage period.",
                    text(&data["calls"])
                ),
            );
            line(out, 0, "Note: command shapes contain no argument values. Repeated shapes do not prove repeated effects.");
            if data["older_events_omitted"] == true {
                line(
                    out,
                    0,
                    "Note: older log events are omitted; these counts cover the retained events.",
                );
            }
            fields(out, data, &["since", "log", "invalid_log_lines"], 0);
            table(
                out,
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
                out,
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
            out,
            list(&data["jobs"]),
            "saved jobs in this instance",
            "newest first",
            &["token", "state", "command", "created_at"],
        ),
        "jobs.status" | "jobs.wait" => {
            let job = &data["job"];
            line(out, 0, match job["state"].as_str() {
                Some("running") => "Not finished: the command is still running; it has not returned a result.",
                Some("completed") if data["result"]["exit_code"] == 0 => "Command returned successfully; its recorded result is available.",
                Some("completed") => "Error: the command returned a failure; its recorded result is available.",
                _ => "Error: no completed result or active worker could be confirmed; the outcome is unknown. Changes can have happened.",
            });
            tree(out, job, 0);
            fields(out, &data["result"], &["exit_code"], 0);
            if let Some(next) = data["next"].as_str().filter(|value| safe_command(value)) {
                line(out, 0, &format!("Next: {next}"));
            }
        }
        "activity" => {
            line(
                out,
                0,
                &format!(
                    "{} latest recorded calls of {} retained calls, newest first.",
                    list(&data["activity"]).len(),
                    text(&data["total_recorded_calls"])
                ),
            );
            line(out, 0, "Note: a command result does not confirm a complete daily run. Unknown means the result was not recorded; changes can still have happened.");
            if data["older_events_omitted"] == true {
                line(out, 0, "Note: older log events are omitted.");
            }
            table(
                out,
                list(&data["activity"]),
                "recent calls (at most 20)",
                "newest first",
                &["at", "command", "targets", "outcome", "duration_ms"],
            );
            fields(out, data, &["invalid_log_lines"], 0);
            line(out, 0, "For a stopped command, inspect its job or current project, document or run before repeating a write.");
        }
        _ => unreachable!("command dispatch selects this renderer"),
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
