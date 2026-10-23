//! Readable observations output; shared escaping and receipt framing stay in the parent.
use super::{escaped, field, fields, line, list, percent, refresh_item, table, text, tree};
use serde_json::{json, Value};

pub(super) fn render(command: &str, data: &Value, out: &mut String) {
    match command {
        "facts.history" => {
            line(
                out,
                0,
                &format!(
                    "{} saved measurements, oldest first.",
                    list(&data["samples"]).len()
                ),
            );
            line(out, 0, &format!("Note: {}", text(&data["note"])));
            field(out, "Measurements", &data["samples"], 0);
        }
        "statistics" => {
            line(
                out,
                0,
                &format!(
                    "Stored statistics from {} through {}.",
                    text(&data["from"]),
                    text(&data["until"])
                ),
            );
            line(out, 0, &format!("Note: {}", text(&data["note"])));
            let summary = &data["summary"];
            line(
                out,
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
                line(out, 0, "Source code languages, largest first:");
            }
            for l in languages {
                let share = l["percent"]
                    .as_f64()
                    .map(|p| format!(" ({})", percent(p, false)));
                line(
                    out,
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
                    out,
                    0,
                    &format!("Project {}: {}", text(&p["id"]), text(&p["name"])),
                );
                let c = &p["current"];
                field(
                    out,
                    "Last observation",
                    &json!({"at":c["at"],"branch":c["branch"],"ok":c["ok"],
                    "source_code":if c["ok"]==true && c["code"]["ok"]==true {c["code"]["source_code"].clone()}else{Value::Null},
                    "dirty_files":c["dirty"]["files"],"ahead":c["ahead"],"behind":c["behind"],"ci":c["ci"]["state"]}),
                    1,
                );
                field(out, "Baseline at", &p["baseline"]["at"], 1);
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
                line(out, 1, &format!("Change: {change}"));
                fields(out, p, &["comparison_note", "confirmed_completions"], 1);
                let activity = &p["activity"];
                line(out, 1, "Reconstructed Git activity:");
                field(out, "Complete", &activity["complete"], 2);
                let coverage: Vec<String> = list(&activity["coverage"])
                    .iter()
                    .map(|c| format!("{} through {}", text(&c["from"]), text(&c["through"])))
                    .collect();
                line(
                    out,
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
                field(out, "Totals", &activity["totals"], 2);
                line(
                    out,
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
                out,
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
            line(out, 0, &format!("Note: {}", text(&data["note"])));
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
                line(out, 0, &format!("Next: {next}"));
            }
        }
        "history.show" => {
            line(out, 0, "Saved content event with full before/after values.");
            tree(out, &data["event"], 0);
        }
        "questions" => {
            let questions = list(&data["questions"]);
            line(
                out,
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
                    out,
                    0,
                    &format!("Note: {n} snoozed questions are left out until their return date."),
                );
            }
            for q in questions {
                line(
                    out,
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
                        out,
                        1,
                        &format!("{}. {}{mark}{effect}", i + 1, text(&option["answer"])),
                    );
                }
            }
        }
        "proposals" => {
            line(
                out,
                0,
                &format!(
                    "{} retained agent proposals, newest first appearance first.",
                    list(&data["proposals"]).len()
                ),
            );
            if let Some(category) = data["category"].as_str() {
                line(out, 0, &format!("Note: category {category} only; omit --category for both ideas and recommendations."));
            }
            field(out, "Proposals", &data["proposals"], 0);
        }
        "facts.refresh" => {
            let rows = list(&data["refreshed"]);
            let failed = rows.iter().filter(|row| row["ok"] != true).count();
            if failed == 0 {
                line(
                    out,
                    0,
                    &format!("Facts refreshed for {} projects.", rows.len()),
                );
            } else {
                line(out, 0, &format!("Error: facts refreshed for {} of {} projects; {failed} failed. Successful measurements remain saved.", rows.len()-failed, rows.len()));
            }
            for item in rows {
                refresh_item(out, item, 0);
            }
        }
        "facts.show" => {
            line(
                out,
                0,
                if data["facts"]["ok"] == false {
                    "Stored facts: the latest measurement failed."
                } else {
                    "Stored facts from the latest measurement."
                },
            );
            tree(out, &data["facts"], 0);
        }
        "due" => due(out, data),
        "overview" => overview(out, data),
        _ => unreachable!("command dispatch selects this renderer"),
    }
}

pub(super) fn due(out: &mut String, data: &Value) {
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
