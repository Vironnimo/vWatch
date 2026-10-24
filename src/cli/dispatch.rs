//! Document decoding and thin application-service dispatch.
use std::io::Read;
use std::path::Path;

use clap::ValueEnum;
use serde_json::{json, Value};

use super::parser::{
    CardCmd, ChangesCmd, Cmd, DocArgs, FactsCmd, FeedbackCmd, HistoryCmd, IdeasCmd, JobsCmd,
    PortfolioCmd, ProjectsCmd, RunsCmd, SchemaKind, SettingsCmd, TasksCmd, WarningsCmd,
};
use crate::cards::{self, Kind};
use crate::service::{Result, VWatch, VWatchError};
use crate::system;

/// Decode a document: UTF-8 with or without BOM, or UTF-16 with BOM (Windows PowerShell's default).
fn decode(bytes: &[u8], origin: &str) -> Result<String> {
    let invalid_encoding = || {
        VWatchError::new("invalid_json", format!("{origin} has invalid text encoding. Nothing was saved."))
            .hint("Save the complete JSON document as UTF-8, or UTF-16 with a byte-order mark, and retry.")
    };
    let utf16 = |be: bool| {
        if !(bytes.len() - 2).is_multiple_of(2) {
            return Err(invalid_encoding());
        }
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| {
                if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| invalid_encoding())
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => std::str::from_utf8(rest)
            .map(str::to_owned)
            .map_err(|_| invalid_encoding()),
        _ => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| invalid_encoding()),
    }
}

fn read_document(doc: &DocArgs, stdin: &mut dyn Read) -> Result<Value> {
    read_input(doc.file.as_deref(), doc.input_json.as_deref(), stdin)
}

fn read_input(file: Option<&str>, input_json: Option<&str>, stdin: &mut dyn Read) -> Result<Value> {
    let (text, origin) = match (input_json, file) {
        (Some(json), _) => (json.to_owned(), "--input-json".to_owned()),
        (None, Some("-")) => {
            let mut buf = Vec::new();
            stdin.read_to_end(&mut buf).map_err(|e| {
                input_error("stdin", &e)
            })?;
            (decode(&buf, "stdin")?, "stdin".to_owned())
        }
        (None, Some(file)) => {
            if Path::new(file).is_dir() {
                return Err(VWatchError::new("file_unreadable", format!("{file} is a folder, not a document file. Nothing was saved."))
                    .hint("Pass the JSON document file inside that folder with --file."));
            }
            let bytes = std::fs::read(file).map_err(|e| {
                input_error(file, &e)
            })?;
            (decode(&bytes, file)?, file.to_owned())
        }
        (None, None) => {
            return Err(
                VWatchError::new("missing_document", "No document was supplied. Nothing was saved.")
                    .hint("Write the document to a UTF-8 file, then repeat this command with --file followed by that file's path. Use --file - only when JSON is piped into stdin."),
            )
        }
    };
    if text.trim().is_empty() {
        return Err(VWatchError::new("invalid_json", format!("{origin} is empty. Nothing was saved."))
            .hint("Write the complete JSON document to a UTF-8 file and repeat this command with --file followed by its path. Closed stdin supplies no document."));
    }
    serde_json::from_str(&text).map_err(|e| {
        VWatchError::new("invalid_json", format!("{origin} is not valid JSON at line {}, column {}: {e}. Nothing was saved.", e.line(), e.column()))
            .hint("Fix the JSON at that location and repeat this command. Use a file-writing tool to preserve quotes and backslashes, and pass the file with --file.")
    })
}

fn input_error(origin: &str, error: &std::io::Error) -> VWatchError {
    use std::io::ErrorKind;
    let (cause, recovery) = match error.kind() {
        ErrorKind::NotFound => ("does not exist", "Write the document at that path or correct --file, then repeat this command."),
        ErrorKind::PermissionDenied => ("cannot be read with this account", "Use a readable document file and pass its path with --file."),
        ErrorKind::IsADirectory => ("is a folder, not a document file", "Pass the JSON file inside that folder with --file."),
        _ => ("could not be read", "Ensure the document is a readable file, close any exclusive writer, then repeat this command."),
    };
    VWatchError::new(
        "file_unreadable",
        format!("{origin} {cause}. Nothing was saved."),
    )
    .hint(recovery)
}

fn reject_feedback_filter(project: Option<&str>) -> Result<()> {
    if project.is_some() {
        return Err(VWatchError::new("invalid_arguments", "--project filters feedback reads; it cannot select a reaction to change. Nothing was changed.")
            .hint("Remove --project from this command. The explicit item ID selects the reaction to change."));
    }
    Ok(())
}

fn text_matches(value: &Value, query: &str) -> bool {
    match value {
        Value::String(text) => text.to_lowercase().contains(query),
        Value::Array(values) => values.iter().any(|value| text_matches(value, query)),
        Value::Object(values) => values.values().any(|value| text_matches(value, query)),
        _ => false,
    }
}

pub(super) fn execute(cmd: Cmd, data_dir: &Path, stdin: &mut dyn Read) -> Result<Value> {
    if let Cmd::Output { token, start } = cmd {
        return Ok(json!({"page": crate::cli_paging::resume(data_dir, &token, start)?}));
    }
    if let Cmd::Jobs { action } = cmd {
        return match action {
            JobsCmd::List => crate::cli_jobs::command("list", None, data_dir),
            JobsCmd::Status { token } => crate::cli_jobs::command("status", Some(&token), data_dir),
            JobsCmd::Wait { token } => crate::cli_jobs::command("wait", Some(&token), data_dir),
        };
    }
    if let Cmd::Usage { since } = cmd {
        return crate::cli_usage::report(data_dir, since.as_deref());
    }
    if let Cmd::Activity = cmd {
        return crate::cli_usage::activity(data_dir);
    }
    if let Cmd::Schema { kind } = cmd {
        let kind = match kind {
            SchemaKind::Card => Kind::Card,
            SchemaKind::Portfolio => Kind::Portfolio,
        };
        return Ok(json!({"schema": cards::json_schema(kind)}));
    }
    std::fs::create_dir_all(data_dir).map_err(|e| {
        VWatchError::new(
            "storage_error",
            format!("Cannot create data folder {}: {e}.", data_dir.display()),
        )
        .hint("Check that vWatch can write to its data folder.")
    })?;
    let db = system::db_path(data_dir);
    let svc = VWatch::open(&db)?;
    Ok(match cmd {
        Cmd::Schema { .. }
        | Cmd::Output { .. }
        | Cmd::Jobs { .. }
        | Cmd::Usage { .. }
        | Cmd::Activity => unreachable!("handled above"),
        Cmd::Overview { visit, refresh } => {
            let refreshed = if refresh {
                Some(svc.refresh_facts(None, true)?)
            } else {
                None
            };
            let mut state = svc.state(visit)?;
            if let Some(refreshed) = refreshed {
                state["refreshed"] = json!(refreshed);
            }
            state
        }
        Cmd::Undo { token } => svc.undo(&token)?,
        Cmd::Statistics {
            project,
            from,
            until,
        } => svc.statistics(project.as_deref(), from.as_deref(), until.as_deref())?,
        Cmd::Questions { project } => svc.questions(project.as_deref())?,
        Cmd::Proposals { project, category } => {
            svc.proposals(project.as_deref(), category.as_deref())?
        }
        Cmd::History { action } => match action {
            HistoryCmd::List {
                project,
                entity,
                id,
                before,
            } => svc.history(project.as_deref(), entity.as_deref(), id.as_deref(), before)?,
            HistoryCmd::Show { id } => svc.history_event(id)?,
        },
        Cmd::Status => {
            // A freshly installed exe hands out its own Skill before the window ever starts.
            let skill = system::export_skill().ok();
            let projects = svc.list_projects()?;
            let list = projects.as_array().cloned().unwrap_or_default();
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "now": chrono::Local::now().to_rfc3339(),
                "exe": system::exe_path().to_string_lossy(),
                "data_dir": data_dir.to_string_lossy(),
                "database": db.to_string_lossy(),
                "logs": {"usage": crate::cli_usage::path(data_dir).to_string_lossy()},
                "skill_dir": skill.map(|d| d.to_string_lossy().into_owned()),
                "projects": list.len(),
                "with_card": list.iter().filter(|p| p["has_card"] == json!(true)).count(),
                "hint": if list.is_empty() { json!("No projects yet. Register an existing repository folder with projects add when the owner asks to watch it. Use the folder's actual path.") } else { Value::Null },
            })
        }
        Cmd::Projects { action } => match action {
            ProjectsCmd::List { query, text } => {
                let query = query.or(text);
                let mut projects = svc.list_projects()?;
                if let Some(query) = query.as_ref() {
                    let query = query.to_lowercase();
                    let state = svc.state(false)?;
                    let matches: Vec<&Value> = state["projects"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|p| {
                            ["id", "name", "path", "card"]
                                .iter()
                                .any(|field| text_matches(&p[field], &query))
                        })
                        .collect();
                    if let Some(list) = projects.as_array_mut() {
                        list.retain(|p| matches.iter().any(|other| other["id"] == p["id"]));
                    }
                }
                json!({"projects": projects, "query": query})
            }
            ProjectsCmd::Show { id } => {
                let state = svc.state(false)?;
                let project = state["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| p["id"] == id)
                    .cloned();
                match project {
                    Some(project) => json!({"project": project}),
                    None => {
                        svc.context(&id)?;
                        unreachable!("context rejects an unknown project")
                    }
                }
            }
            ProjectsCmd::Visit { id } => {
                let mut result = svc.open_project(&id)?;
                result["project"] = json!(id);
                result
            }
            ProjectsCmd::Add { paths, id, name } => {
                if paths.len() == 1 {
                    json!({"project": svc.add_project(&paths[0], id.as_deref(), name.as_deref())?})
                } else if id.is_some() || name.is_some() {
                    return Err(VWatchError::new("invalid_arguments", "--id and --name apply to one folder. No projects were added.")
                        .hint("Add these folders together without --id and --name, or run a separate projects add command for each custom ID and name."));
                } else {
                    svc.add_projects(&paths)?
                }
            }
            ProjectsCmd::Remove { id } => svc.remove_project(&id)?,
            ProjectsCmd::Trash => svc.trash_projects()?,
            ProjectsCmd::Restore { id } => svc.restore_project(&id)?,
            ProjectsCmd::Rename { id, name } => json!({"project": svc.rename_project(&id, &name)?}),
            ProjectsCmd::Update { id, name, path } => {
                svc.update_project(&id, name.as_deref(), path.as_deref())?
            }
            ProjectsCmd::Status { id, status, note } => {
                let status = status
                    .to_possible_value()
                    .expect("every status has a name")
                    .get_name()
                    .to_owned();
                json!({"project": svc.set_status(&id, &status, note.as_deref())?})
            }
            ProjectsCmd::Goal { id, goal, due } => {
                json!({"project": svc.set_goal(&id, &goal, due.as_deref())?})
            }
            ProjectsCmd::Discover { root } => svc.discover(&root)?,
        },
        Cmd::Facts { action } => match action {
            FactsCmd::History { id } => svc.facts_history(&id)?,
            FactsCmd::Refresh { ids, no_ci } => {
                json!({"refreshed": svc.refresh_facts(Some(&ids), !no_ci)?})
            }
            FactsCmd::Show { id } => json!({"facts": svc.facts(&id)?}),
        },
        Cmd::Due {
            max_age_days,
            no_refresh,
        } => svc.due(max_age_days, !no_refresh)?,
        Cmd::Context { id } => svc.context(&id)?,
        Cmd::Card { action } => match action {
            CardCmd::Compare { id, from, to } => svc.compare_documents(Some(&id), from, to)?,
            CardCmd::Put { id, doc } => svc.put_card_guarded(
                &id,
                read_document(&doc, stdin)?,
                doc.merge,
                doc.dry_run,
                doc.context_token.as_deref(),
            )?,
            CardCmd::Show { id, version } => match version {
                Some(version) => svc.card_version(&id, version)?,
                None => svc.card(&id)?,
            },
            CardCmd::History { id } => svc.card_history(&id)?,
            CardCmd::Restore {
                id,
                version,
                context_token,
            } => svc.restore_card(&id, version, Some(&context_token))?,
        },
        Cmd::Portfolio { action } => match action {
            PortfolioCmd::Compare { from, to } => svc.compare_documents(None, from, to)?,
            PortfolioCmd::Context { project } => svc.portfolio_context(&project)?,
            PortfolioCmd::Put { doc } => svc.put_portfolio_guarded(
                read_document(&doc, stdin)?,
                doc.merge,
                doc.dry_run,
                doc.context_token.as_deref(),
            )?,
            PortfolioCmd::Show { version } => match version {
                Some(version) => svc.portfolio_version(version)?,
                None => svc.portfolio()?,
            },
            PortfolioCmd::History => svc.portfolio_history()?,
            PortfolioCmd::Restore {
                version,
                context_token,
            } => svc.restore_portfolio(version, Some(&context_token))?,
        },
        Cmd::Feedback { project, action } => match action {
            None => json!({"feedback": svc.feedback(project.as_deref())?}),
            Some(FeedbackCmd::List { project: selected }) => {
                if project.is_some() && selected.is_some() && project != selected {
                    return Err(VWatchError::new(
                        "invalid_arguments",
                        "Two different --project filters were supplied. Nothing was changed.",
                    )
                    .hint("Pass one --project value to select the reactions for that project."));
                }
                json!({"feedback": svc.feedback(selected.as_deref().or(project.as_deref()))?})
            }
            Some(FeedbackCmd::Set {
                id,
                value,
                note,
                until,
            }) => {
                reject_feedback_filter(project.as_deref())?;
                svc.set_feedback_until(
                    &id,
                    Some(&value.to_lowercase()),
                    note.as_deref(),
                    until.as_deref(),
                )?
            }
            Some(FeedbackCmd::Clear { id }) => {
                reject_feedback_filter(project.as_deref())?;
                svc.set_feedback(&id, None, None)?
            }
            Some(FeedbackCmd::Restore { id, input }) => {
                reject_feedback_filter(project.as_deref())?;
                let snapshot =
                    read_input(input.file.as_deref(), input.input_json.as_deref(), stdin)?;
                svc.restore_feedback(&id, Some(&snapshot))?
            }
        },
        Cmd::Ideas { action } => match action {
            IdeasCmd::Status { id, status, revision } => match status {
                Some(status) => svc.set_idea_status(&id, &status.to_ascii_lowercase(), revision.as_deref().ok_or_else(|| VWatchError::new("invalid_arguments", "A status change requires --revision from ideas status. Nothing was changed."))?)?,
                None => svc.idea_status(&id)?,
            },
            IdeasCmd::Update { id, text, revision } => svc.update_idea(id, &text, revision)?,
            IdeasCmd::List => json!({"ideas": svc.ideas()?}),
            IdeasCmd::Add { text } => json!({"idea": svc.add_idea(&text)?}),
            IdeasCmd::Remove { id } => svc.remove_idea(id)?,
            IdeasCmd::Trash => json!({"ideas": svc.ideas_trash()?}),
            IdeasCmd::Restore { id } => svc.restore_idea(id)?,
        },
        Cmd::Runs { action } => match action {
            RunsCmd::Start { no_refresh } => svc.run_start(!no_refresh)?,
            RunsCmd::Status => svc.run_status()?,
            RunsCmd::Fail { id, project, error } => svc.run_fail(id, project.as_deref(), &error)?,
            RunsCmd::Finish { id } => svc.run_finish(id)?,
        },
        Cmd::Tasks { action } => match action {
            TasksCmd::List { project } => svc.tasks(project.as_deref())?,
            TasksCmd::Show { id } => svc.task(id)?,
            TasksCmd::Create {
                project,
                title,
                why,
                evidence,
                done_when,
                source_id,
                file,
            } => {
                let payload = if let Some(file) = file {
                    read_input(Some(&file), None, stdin)?
                } else {
                    let mut payload = json!({"projects": project, "title": title, "why": why, "evidence": evidence, "done_when": done_when});
                    if let Some(source_id) = source_id {
                        payload["source_id"] = json!(source_id);
                    }
                    payload
                };
                svc.create_task(payload)?
            }
            TasksCmd::Confirm { id, revision } => {
                svc.set_task_status_guarded(id, "done", revision)?
            }
            TasksCmd::Reopen { id, revision } => {
                svc.set_task_status_guarded(id, "assigned", revision)?
            }
            TasksCmd::Cancel { id, revision } => {
                svc.set_task_status_guarded(id, "cancelled", revision)?
            }
            TasksCmd::Report {
                id,
                status,
                result,
                evidence,
                file,
            } => {
                let payload = if let Some(file) = file {
                    read_input(Some(&file), None, stdin)?
                } else {
                    json!({"status": status, "result": result, "evidence": evidence})
                };
                svc.report_task_document(id, payload)?
            }
        },
        Cmd::Warnings { action } => match action {
            WarningsCmd::List { id } => {
                json!({"project": id, "warnings": svc.project_warnings(&id)?, "acknowledgements": svc.warning_acknowledgements(&id)?})
            }
            WarningsCmd::Acknowledge { id, key, note } => {
                svc.set_warning_ack(&id, &key.to_ascii_lowercase(), Some(&note))?
            }
            WarningsCmd::Clear { id, key } => {
                svc.set_warning_ack(&id, &key.to_ascii_lowercase(), None)?
            }
        },
        Cmd::Changes { action } => match action {
            ChangesCmd::Show => json!({"changes": svc.state(false)?["changes"]}),
            ChangesCmd::Read { token } => svc.mark_changes_read(&token)?,
        },
        Cmd::Settings { action } => {
            let mut result = if let SettingsCmd::Update {
                style,
                hidden_modules,
            } = action
            {
                if style.is_none() && hidden_modules.is_none() {
                    return Err(VWatchError::new("invalid_arguments", "No preference was supplied. Nothing was changed.")
                        .hint("Pass --style handover (Today), --style board (Projects), --style ideas or --style statistics, or pass --hidden-modules with comma-separated module IDs."));
                }
                let mut updates = Vec::new();
                if let Some(style) = style {
                    updates.push(("style", json!(style.to_lowercase())));
                }
                if let Some(modules) = hidden_modules {
                    let modules: Vec<&str> = modules
                        .split(',')
                        .map(str::trim)
                        .filter(|m| !m.is_empty())
                        .collect();
                    updates.push(("hidden_modules", json!(modules)));
                }
                svc.update_prefs(&updates)?
            } else {
                json!({"prefs": svc.state(false)?["prefs"]})
            };
            let mut settings = system::settings();
            settings["data_dir"] = json!(data_dir.to_string_lossy());
            result["settings"] = settings;
            result
        }
    })
}
