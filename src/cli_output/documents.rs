//! Readable documents output; shared escaping and receipt framing stay in the parent.
use super::{escaped, field, fields, label, line, list, text, tree, tree_except};
use serde_json::{json, Value};

pub(super) fn render(command: &str, data: &Value, data_dir: &std::path::Path, out: &mut String) {
    match command {
        "card.compare" | "portfolio.compare" => {
            line(
                out,
                0,
                &format!(
                    "{} changed fields between versions {} and {}.",
                    list(&data["changes"]).len(),
                    text(&data["from"]),
                    text(&data["to"])
                ),
            );
            fields(
                out,
                data,
                &["project", "before_at", "after_at", "changes"],
                0,
            );
        }
        "context" | "portfolio.context" => {
            line(
                out,
                0,
                if command == "context" {
                    "Project context for writing its card."
                } else if data.get("owner_ideas").is_none() {
                    "Full project details from the portfolio context."
                } else {
                    "Portfolio context for writing the brief."
                },
            );
            field(out, "Context token", &data["context_token"], 0);
            if command == "portfolio.context" {
                line(
                    out,
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
                        &format!("vwatch portfolio context --project {first}"),
                    );
                    line(out, 0, &format!("Note: {} projects unchanged since the previous brief are summaries (detail: summary). For the full card details of one, run `{details}`; repeat --project for several.", summaries.len()));
                }
                tree_except(out, &portfolio_context_view(data), &["context_token"], 0);
            } else {
                tree_except(out, data, &["context_token"], 0);
            }
        }
        "card.show" | "portfolio.show" => {
            line(
                out,
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
            tree(out, &view, 0);
        }
        "card.put" | "portfolio.put" | "card.restore" | "portfolio.restore" => {
            let subject = if command.starts_with("card.") {
                "Card"
            } else {
                "Portfolio brief"
            };
            if data["dry_run"] == true {
                line(
                    out,
                    0,
                    &format!("{subject} is valid. Dry run: nothing was saved."),
                );
            } else if data["already_saved"] == true {
                line(
                    out,
                    0,
                    &format!(
                        "{subject} already saved as version {}; no version was added.",
                        text(&data["version"])
                    ),
                );
            } else if data["saved"] == false {
                line(
                    out,
                    0,
                    &format!("{subject} unchanged; no version was added."),
                );
            } else {
                line(
                    out,
                    0,
                    &format!("{subject} saved as version {}.", text(&data["version"])),
                );
            }
            if let Some(token) = data.get("context_token") {
                field(out, "Context token", token, 0);
            }
            if matches!(command, "card.put" | "portfolio.put") {
                fields(out, data, &["project"], 0);
                for key in ["changed_fields", "unfilled_fields"] {
                    if let Some(value) = data.get(key) {
                        line(
                            out,
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
                fields(out, data, &["warnings"], 0);
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
                    out,
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
                        out,
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
                    out,
                    data,
                    &["saved", "dry_run", "already_saved", "context_token"],
                    0,
                );
            }
        }
        "card.history" | "portfolio.history" => {
            line(out, 0, "Saved document versions, oldest first.");
            field(out, "Context token", &data["context_token"], 0);
            tree_except(out, data, &["context_token"], 0);
        }
        "schema.card" | "schema.portfolio" => {
            line(
                out,
                0,
                if command == "schema.card" {
                    "Project card fields and limits."
                } else {
                    "Portfolio brief fields and limits."
                },
            );
            line(
                out,
                0,
                "Use the exact field names below in the document. Required fields are marked.",
            );
            schema_fields(out, &data["schema"], "", 0);
        }
        _ => unreachable!("command dispatch selects this renderer"),
    }
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
