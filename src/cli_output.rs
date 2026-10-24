//! Plain output shared by people and agents. Stored content is always data.
use serde_json::Value;

mod documents;
mod observations;
mod operations;

pub(super) fn human(command: &str, data: &Value, data_dir: &std::path::Path) -> String {
    let mut out = String::new();
    match command {
        "card.compare" | "portfolio.compare" | "context" | "portfolio.context" | "card.show"
        | "portfolio.show" | "card.put" | "portfolio.put" | "card.restore"
        | "portfolio.restore" | "card.history" | "portfolio.history" | "schema.card"
        | "schema.portfolio" => documents::render(command, data, data_dir, &mut out),
        "facts.history" | "statistics" | "history.list" | "history.show" | "questions"
        | "proposals" | "facts.refresh" | "facts.show" | "due" | "overview" => {
            observations::render(command, data, &mut out)
        }
        "projects.add"
        | "status"
        | "projects.list"
        | "projects.removed"
        | "projects.trash"
        | "projects.show"
        | "projects.rename"
        | "projects.update"
        | "projects.status"
        | "projects.goal"
        | "projects.restore"
        | "projects.remove"
        | "projects.visit"
        | "projects.discover"
        | "feedback"
        | "feedback.list"
        | "feedback.set"
        | "feedback.clear"
        | "feedback.restore"
        | "ideas.list"
        | "ideas.removed"
        | "ideas.trash"
        | "ideas.add"
        | "ideas.restore"
        | "ideas.update"
        | "ideas.status"
        | "ideas.remove"
        | "runs.start"
        | "runs.status"
        | "runs.fail"
        | "runs.finish"
        | "tasks.list"
        | "tasks.create"
        | "tasks.show"
        | "tasks.report"
        | "tasks.confirm"
        | "tasks.reopen"
        | "tasks.cancel"
        | "warnings.list"
        | "warnings.acknowledge"
        | "warnings.clear"
        | "changes.show"
        | "changes.read"
        | "settings.show"
        | "settings.update"
        | "undo"
        | "usage"
        | "jobs.list"
        | "jobs.status"
        | "jobs.wait"
        | "activity" => operations::render(command, data, &mut out),
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
            .filter(|value| !value.starts_with("vwatch "))
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
                    || matches!(key.as_str(), "next" | "repair_task" | "undo" | "undo_token")
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
fn safe_command(command: &str) -> bool {
    command.starts_with("vwatch ")
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
    for key in [
        "error",
        "warning",
        "warnings",
        "hint",
        "incomplete_measurements",
    ] {
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
