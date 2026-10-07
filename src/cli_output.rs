//! Human-facing CLI views. Machine output is selected explicitly with --json.

use serde_json::Value;

pub(super) fn human(command: &str, data: &Value) -> String {
    let mut out = String::new();
    match command {
        "status" => {
            line(
                &mut out,
                0,
                &format!("Watchtower {}", text(&data["version"])),
            );
            line(
                &mut out,
                0,
                &format!(
                    "{} Projekte, davon {} mit Karte.",
                    text(&data["projects"]),
                    text(&data["with_card"])
                ),
            );
            fields(
                &mut out,
                data,
                &["data_dir", "database", "exe", "skill_dir"],
                0,
            );
            if data["projects"] == 0 {
                line(
                    &mut out,
                    0,
                    "Füge ein Projekt im Fenster oder mit watchtower projects add <Ordner> hinzu.",
                );
            }
        }
        "projects.list" => {
            let projects = list(&data["projects"]);
            line(
                &mut out,
                0,
                &format!("{} Projekte registriert.", projects.len()),
            );
            for project in projects {
                project_details(&mut out, project, 0);
                line(
                    &mut out,
                    1,
                    if project["has_card"] == true {
                        "Karte vorhanden"
                    } else {
                        "Noch keine Karte"
                    },
                );
                fields(
                    &mut out,
                    project,
                    &["card_updated_at", "facts_collected_at"],
                    1,
                );
            }
        }
        "projects.add" | "projects.rename" | "projects.update" | "projects.status"
        | "projects.goal" => {
            let p = &data["project"];
            let action = match command {
                "projects.add" => "Projekt registriert.",
                "projects.rename" => "Projekt umbenannt. Karten und Historie bleiben erhalten.",
                "projects.update" if data["changed"] == false => "Projekt unverändert.",
                "projects.update" => "Projekt aktualisiert. Karten und Historie bleiben erhalten.",
                "projects.status" => "Projektstatus gespeichert.",
                "projects.goal" if !present(&p["goal"]) => "Projektziel entfernt.",
                _ => "Projektziel gespeichert.",
            };
            line(&mut out, 0, action);
            project_details(&mut out, p, 0);
            if present(&p["warning"]) {
                field(&mut out, "Warnung", &p["warning"], 0);
            }
            if !p["facts"].is_null() {
                refresh_item(&mut out, &p["facts"], 0);
            }
            if !data["facts_refresh"].is_null() {
                refresh_item(&mut out, &data["facts_refresh"], 0);
            }
        }
        "projects.remove" => line(
            &mut out,
            0,
            &format!(
                "Projekt {} und seine gespeicherten Karten entfernt.",
                text(&data["removed"])
            ),
        ),
        "projects.discover" => {
            field(&mut out, "Durchsuchter Ordner", &data["folder"], 0);
            let found = list(&data["candidates"]);
            line(
                &mut out,
                0,
                &format!("{} neue Projektordner gefunden.", found.len()),
            );
            for p in found {
                line(
                    &mut out,
                    0,
                    &format!(
                        "- {}{}",
                        text(&p["name"]),
                        if p["recommended"] == true {
                            " (empfohlen)"
                        } else {
                            ""
                        }
                    ),
                );
                fields(&mut out, p, &["path", "suggested_id", "last_commit"], 1);
            }
            field(&mut out, "Bereits registriert", &data["already_watched"], 0);
        }
        "facts.refresh" => {
            let refreshed = list(&data["refreshed"]);
            line(
                &mut out,
                0,
                &format!(
                    "Faktenmessung für {} Projekte abgeschlossen.",
                    refreshed.len()
                ),
            );
            for item in refreshed {
                refresh_item(&mut out, item, 0);
            }
        }
        "facts.show" => facts(&mut out, &data["facts"], 0),
        "due" => due(&mut out, data, 0),
        "context" => {
            line(&mut out, 0, "Projektkontext");
            project_details(&mut out, &data["project"], 0);
            facts(&mut out, &data["facts"], 0);
            if data["previous_card"].is_null() {
                line(&mut out, 0, "Noch keine Karte vorhanden.");
            } else {
                document(
                    &mut out,
                    &data["previous_card"],
                    "card",
                    "Bisherige Karte",
                    0,
                );
            }
            fields(
                &mut out,
                data,
                &[
                    "since_previous_card",
                    "new_fields",
                    "feedback",
                    "feedback_summary",
                    "tasks",
                    "warning_acknowledgements",
                    "context_token",
                ],
                0,
            );
            line(&mut out, 0, &format!("Karte speichern: watchtower card put {} --file <Karte.json> --context-token <Kontext-Token>", text(&data["project"]["id"])));
        }
        "card.show" => document(&mut out, data, "card", "Projektkarte", 0),
        "portfolio.show" => document(&mut out, data, "portfolio", "Portfolio-Brief", 0),
        "card.put" | "portfolio.put" => {
            let name = if command == "card.put" {
                "Karte"
            } else {
                "Portfolio-Brief"
            };
            if data["dry_run"] == true {
                line(
                    &mut out,
                    0,
                    &format!("{name} geprüft. Probelauf: nichts gespeichert."),
                );
            } else {
                line(
                    &mut out,
                    0,
                    &format!("{name} gespeichert, Version {}.", text(&data["version"])),
                );
            }
            fields(
                &mut out,
                data,
                &[
                    "project",
                    "changed_fields",
                    "unfilled_fields",
                    "warnings",
                    "items",
                ],
                0,
            );
        }
        "portfolio.context" => {
            line(&mut out, 0, "Kontext für den Portfolio-Brief");
            fields(
                &mut out,
                data,
                &["today", "review_week", "owner_last_opened"],
                0,
            );
            for p in list(&data["projects"]) {
                project_details(&mut out, p, 0);
                field(&mut out, "Fakten", &p["facts"], 1);
                if p["card"].is_null() {
                    line(&mut out, 1, "Noch keine Karte vorhanden.");
                } else {
                    field(&mut out, "Karte", &p["card"], 1);
                }
                field(
                    &mut out,
                    "Bewusst bestätigte Hinweise",
                    &p["warning_acknowledgements"],
                    1,
                );
            }
            if data["previous_portfolio"].is_null() {
                line(&mut out, 0, "Noch kein Portfolio-Brief vorhanden.");
            } else {
                document(
                    &mut out,
                    &data["previous_portfolio"],
                    "portfolio",
                    "Bisheriger Portfolio-Brief",
                    0,
                );
            }
            fields(
                &mut out,
                data,
                &[
                    "owner_ideas",
                    "feedback",
                    "feedback_summary",
                    "tasks",
                    "context_token",
                ],
                0,
            );
            line(&mut out, 0, "Brief speichern: watchtower portfolio put --file <Brief.json> --context-token <Kontext-Token>");
        }
        "feedback" => collection(&mut out, "Rückmeldungen", &data["feedback"]),
        "ideas.list" => collection(&mut out, "Geparkte Ideen", &data["ideas"]),
        "ideas.add" => {
            line(&mut out, 0, "Idee geparkt.");
            tree(&mut out, &data["idea"], 0);
        }
        "ideas.remove" => line(
            &mut out,
            0,
            &format!("Idee {} entfernt.", text(&data["removed"])),
        ),
        "runs.start" | "runs.status" | "runs.fail" | "runs.finish" => run(&mut out, command, data),
        "tasks.list" => collection(&mut out, "Aufträge", &data["tasks"]),
        "tasks.report" => {
            line(
                &mut out,
                0,
                if data["task"]["status"] == "review" {
                    "Ergebnis gemeldet. Deine Bestätigung steht noch aus."
                } else {
                    "Blockierung gemeldet."
                },
            );
            tree(&mut out, &data["task"], 0);
        }
        "schema.card" | "schema.portfolio" => {
            line(
                &mut out,
                0,
                if command == "schema.card" {
                    "Felder einer Projektkarte"
                } else {
                    "Felder eines Portfolio-Briefs"
                },
            );
            line(
                &mut out,
                0,
                "Pflichtfelder sind markiert. Feldnamen werden beim Schreiben exakt so verwendet.",
            );
            schema_fields(&mut out, &data["schema"], "", 0);
        }
        _ => tree(&mut out, data, 0),
    }
    out.trim_end().to_owned()
}

fn list(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => o.values().any(present),
        _ => true,
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(true) => "ja".into(),
        Value::Bool(false) => "nein".into(),
        Value::Null => "unbekannt".into(),
        _ => String::new(),
    }
}

fn line(out: &mut String, depth: usize, content: &str) {
    for part in content.lines() {
        out.push_str(&"  ".repeat(depth));
        out.push_str(part);
        out.push('\n');
    }
}

fn fields(out: &mut String, data: &Value, keys: &[&str], depth: usize) {
    for key in keys {
        field_key(out, key, &data[*key], depth);
    }
}

fn field_key(out: &mut String, key: &str, value: &Value, depth: usize) {
    if !present(value)
        || matches!(
            key,
            "ok" | "fingerprint" | "dirty_fingerprint" | "identity_revision" | "revision" | "next"
        )
    {
        return;
    }
    if matches!(
        key,
        "changes" | "new_fields" | "changed_fields" | "unfilled_fields" | "changed_since_brief"
    ) && value.is_array()
        && list(value).iter().all(Value::is_string)
    {
        line(out, depth, &format!("{}:", label(key)));
        for name in list(value).iter().filter_map(Value::as_str) {
            let parts = name
                .split('.')
                .map(|part| label(part.trim_end_matches("[]")))
                .collect::<Vec<_>>();
            line(out, depth + 1, &format!("- {}", parts.join(" / ")));
        }
        return;
    }
    if let Some(raw) = value.as_str() {
        if matches!(
            key,
            "status"
                | "state"
                | "phase"
                | "value"
                | "reason"
                | "problem"
                | "verdict"
                | "severity"
                | "effort"
                | "kind"
                | "type"
        ) {
            line(out, depth, &format!("{}: {}", label(key), meaning(raw)));
            return;
        }
    }
    field(out, &label(key), value, depth);
}

fn field(out: &mut String, title: &str, value: &Value, depth: usize) {
    if !present(value) {
        return;
    }
    match value {
        Value::Object(_) | Value::Array(_) => {
            line(out, depth, &format!("{title}:"));
            tree(out, value, depth + 1);
        }
        _ => line(out, depth, &format!("{title}: {}", text(value))),
    }
}

/// Nested documents retain all content, using labels and list items rather than serialization.
fn tree(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                field_key(out, key, value, depth);
            }
        }
        Value::Array(items) => {
            for item in items {
                if item.is_object() {
                    let heading_key = ["title", "name", "text", "term", "what", "path", "id"]
                        .into_iter()
                        .find(|k| present(&item[*k]));
                    let heading = heading_key
                        .map(|k| text(&item[k]))
                        .unwrap_or_else(|| "Eintrag".into());
                    line(out, depth, &format!("- {heading}"));
                    for (key, value) in item.as_object().expect("object checked") {
                        if Some(key.as_str()) != heading_key {
                            field_key(out, key, value, depth + 1);
                        }
                    }
                } else if item.is_array() {
                    tree(out, item, depth);
                } else if present(item) {
                    line(out, depth, &format!("- {}", text(item)));
                }
            }
        }
        _ => line(out, depth, &text(value)),
    }
}

fn collection(out: &mut String, title: &str, value: &Value) {
    line(out, 0, &format!("{title}: {}", list(value).len()));
    tree(out, value, 0);
}

fn project_title(project: &Value) -> String {
    if project["name"] == project["id"] || !present(&project["name"]) {
        text(&project["id"])
    } else {
        format!("{} (ID: {})", text(&project["name"]), text(&project["id"]))
    }
}

fn project_details(out: &mut String, project: &Value, depth: usize) {
    line(out, depth, &project_title(project));
    fields(
        out,
        project,
        &["path", "status", "status_note", "goal", "goal_due"],
        depth + 1,
    );
}

fn refresh_item(out: &mut String, item: &Value, depth: usize) {
    if item["ok"] == true {
        line(
            out,
            depth,
            &format!(
                "{}: Fakten {}.",
                text(&item["id"]),
                if item["changed"] == true {
                    "aktualisiert"
                } else {
                    "unverändert"
                }
            ),
        );
    } else {
        line(
            out,
            depth,
            &format!(
                "Warnung für {}: Fakten konnten nicht gemessen werden.",
                text(&item["id"])
            ),
        );
        if let Some(error) = item["error"].as_str() {
            line(out, depth + 1, meaning(error));
        } else {
            field(out, "Fehler", &item["error"], depth + 1);
        }
    }
}

fn facts(out: &mut String, facts: &Value, depth: usize) {
    line(
        out,
        depth,
        &if facts["project"].is_null() {
            "Gemessene Fakten".into()
        } else {
            format!("Fakten für {}", text(&facts["project"]))
        },
    );
    field_key(out, "collected_at", &facts["collected_at"], depth + 1);
    if facts["ok"] == false {
        line(out, depth + 1, "Faktenmessung fehlgeschlagen.");
        if let Some(error) = facts["error"].as_str() {
            line(out, depth + 1, meaning(error));
        } else {
            field_key(out, "error", &facts["error"], depth + 1);
        }
        return;
    }
    fields(
        out,
        facts,
        &[
            "branch",
            "head",
            "last_work",
            "has_remote",
            "remotes",
            "upstream",
            "ahead",
            "behind",
            "unpushed_since",
            "dirty",
            "commits",
            "worktrees",
            "branches",
            "ci",
            "release",
            "night",
            "collisions",
            "space",
        ],
        depth + 1,
    );
}

fn due(out: &mut String, data: &Value, depth: usize) {
    let projects = list(&data["projects"]);
    line(
        out,
        depth,
        &format!(
            "{} Projekte brauchen eine neue Karte; {} sind aktuell.",
            projects.len(),
            list(&data["up_to_date"]).len()
        ),
    );
    for p in projects {
        line(
            out,
            depth,
            &format!(
                "- {}: {}",
                project_title(p),
                meaning(p["reason"].as_str().unwrap_or("unknown"))
            ),
        );
        fields(out, p, &["changes", "new_fields"], depth + 1);
    }
    for p in list(&data["problems"]) {
        line(
            out,
            depth,
            &format!(
                "Warnung für {}: {}",
                text(&p["id"]),
                meaning(p["problem"].as_str().unwrap_or("unknown"))
            ),
        );
        fields(out, p, &["path", "error"], depth + 1);
    }
    if data["portfolio"]["due"] == true {
        line(
            out,
            depth,
            &format!(
                "Portfolio-Brief fällig: {}.",
                meaning(data["portfolio"]["reason"].as_str().unwrap_or("unknown"))
            ),
        );
    } else {
        line(out, depth, "Portfolio-Brief ist aktuell.");
    }
}

fn document(out: &mut String, data: &Value, key: &str, title: &str, depth: usize) {
    line(
        out,
        depth,
        &format!("{title}, Version {}", text(&data["version"])),
    );
    fields(out, data, &["project", "created_at"], depth + 1);
    let keys = if key == "card" {
        &[
            "headline",
            "one_liner",
            "phase",
            "phase_evidence",
            "stack",
            "personality",
            "reentry",
            "waiting_on_you",
            "open_ends",
            "suggestions",
            "recent_changes",
            "milestones",
            "features",
            "vision",
            "health_notes",
            "decisions",
            "cross_project_hints",
            "handbook",
        ][..]
    } else {
        &[
            "greeting",
            "focus",
            "attention_note",
            "patterns",
            "synergies",
            "new_apps",
            "park_or_revive",
            "idea_checks",
            "week_review",
        ][..]
    };
    fields(out, &data[key], keys, depth + 1);
}

fn run(out: &mut String, command: &str, data: &Value) {
    let run = if data["run"].is_object() {
        &data["run"]
    } else {
        &data["latest"]
    };
    if run.is_null() {
        line(out, 0, "Noch kein Agentenlauf erfasst.");
    } else {
        let action = match command {
            "runs.start" => "gestartet",
            "runs.fail" => "Fehler erfasst",
            "runs.finish" => "beendet",
            _ => "Status",
        };
        line(
            out,
            0,
            &format!(
                "Agentenlauf {} {action}: {}.",
                text(&run["id"]),
                meaning(run["state"].as_str().unwrap_or("unknown"))
            ),
        );
        fields(out, run, &["started_at", "finished_at"], 0);
        let counts = &run["counts"];
        line(
            out,
            0,
            &format!(
                "Projekte: {} aktuell, {} offen, {} fehlgeschlagen.",
                text(&counts["current"]),
                text(&counts["pending"]),
                text(&counts["failed"])
            ),
        );
        for project in list(&run["projects"]) {
            line(
                out,
                0,
                &format!(
                    "- {}: {}",
                    project_title(project),
                    meaning(project["status"].as_str().unwrap_or("unknown"))
                ),
            );
            fields(
                out,
                project,
                &["reason", "error", "pending_owner_replies"],
                1,
            );
        }
        field(out, "Portfolio-Brief", &run["portfolio"], 0);
    }
    if !data["due"].is_null() {
        due(out, &data["due"], 0);
    }
    if data["last_successful"].is_null() {
        line(out, 0, "Noch kein vollständig erfolgreicher Lauf.");
    } else {
        field(
            out,
            "Letzter erfolgreicher Lauf",
            &data["last_successful"],
            0,
        );
    }
    fields(out, data, &["pending_owner_replies", "repair_task"], 0);
    if command == "runs.status" {
        field(out, "Bisherige Läufe", &data["history"], 0);
    }
    if command == "runs.start" {
        line(
            out,
            0,
            &format!(
                "Nach der Bearbeitung abschließen: watchtower runs finish {}",
                text(&run["id"])
            ),
        );
    }
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
                    "{path}: {}; {}",
                    schema_description(value),
                    if required { "Pflichtfeld" } else { "optional" }
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
                        &format!("Pro Eintrag: {}", schema_description(&inner["items"])),
                    );
                }
            }
        }
    }
}

fn schema_description(schema: &Value) -> String {
    let inner = list(&schema["anyOf"]).first().unwrap_or(schema);
    let ty = match inner["type"].as_str() {
        Some("string") => "Text",
        Some("integer") => "Ganzzahl",
        Some("object") => "Feldgruppe",
        Some("array") => "Liste",
        _ => "Wert",
    };
    let mut limits = vec![ty.to_owned()];
    if !schema["anyOf"].is_null() {
        limits.push("darf leer sein (null)".into());
    }
    for (field, name) in [
        ("minLength", "Mindestlänge"),
        ("maxLength", "Höchstlänge"),
        ("maxItems", "Höchstzahl der Einträge"),
        ("minimum", "Minimum"),
        ("maximum", "Maximum"),
    ] {
        if !inner[field].is_null() {
            limits.push(format!(
                "{name}: {}{}",
                text(&inner[field]),
                if field.ends_with("Length") {
                    " Zeichen"
                } else {
                    ""
                }
            ));
        }
    }
    if !inner["enum"].is_null() {
        limits.push(format!(
            "Werte: {}",
            list(&inner["enum"])
                .iter()
                .map(text)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if inner["format"] == "date" {
        limits.push("Datum JJJJ-MM-TT".into());
    } else if let Some(pattern) = inner["pattern"].as_str() {
        limits.push(format!("Muster: {pattern}"));
    }
    if let Some(description) = inner["description"].as_str() {
        let words = description
            .strip_prefix("At most ")
            .and_then(|s| s.strip_suffix(" words."));
        limits.push(
            words
                .map(|n| format!("höchstens {n} Wörter"))
                .unwrap_or_else(|| description.into()),
        );
    }
    limits.join("; ")
}

fn meaning(value: &str) -> &str {
    match value {
        "active" => "aktiv",
        "parked" => "geparkt",
        "done" => "erledigt",
        "archived" => "archiviert",
        "cooling" => "wenig Aktivität",
        "dormant" => "ruhend",
        "maintenance" => "Wartung",
        "running" => "läuft",
        "stalled" => "festgefahren",
        "failed" => "fehlgeschlagen",
        "complete" => "vollständig abgeschlossen",
        "incomplete" => "unvollständig",
        "current" => "aktuell",
        "pending" => "offen",
        "unknown" => "unbekannt",
        "assigned" => "vergeben",
        "review" => "wartet auf deine Bestätigung",
        "blocked" => "blockiert",
        "cancelled" => "abgebrochen",
        "green" => "erfolgreich",
        "red" => "fehlgeschlagen",
        "none" => "nicht vorhanden",
        "skipped" => "nicht abgefragt",
        "up" => "gut",
        "down" => "abgelehnt",
        "later" => "später",
        "answered" => "Antwort erhalten",
        "dismissed" => "verworfen",
        "no_card" => "noch keine Karte",
        "changed" => "Repository geändert",
        "ci_changed" => "CI-Status geändert",
        "status_changed" => "Projektvorgaben geändert",
        "new_fields" => "neue Kartenfelder fehlen",
        "stale" => "Karte oder Brief veraltet",
        "no_portfolio" | "missing" => "noch kein Brief",
        "new_day" => "neuer Tagesbrief",
        "owner_idea" => "neue Idee von dir",
        "cards_changed" => "Karten geändert",
        "cards_will_change" => "Karten werden aktualisiert",
        "new_ideas" => "neue Ideen",
        "week_review" => "Wochenrückblick fällig",
        "owner_ideas" => "Ideen zu bewerten",
        "path_missing" => "Projektordner nicht gefunden",
        "not_a_git_repository" => "Ordner ist kein Git-Repository",
        "git_status_failed" => "Git-Status konnte nicht gelesen werden",
        "facts_missing" => "Fakten fehlen",
        "agent_failed" => "Agent hat einen Fehler gemeldet",
        "project_archived" => "Projekt archiviert",
        "project_removed" => "Projekt entfernt",
        "info" => "Hinweis",
        "warn" => "Warnung",
        "risk" => "Risiko",
        "park" => "parken",
        "revive" => "wieder aufnehmen",
        "finish" => "fertigstellen",
        "archive" => "archivieren",
        "build" => "umsetzen",
        "fold_in" => "in bestehendes Projekt aufnehmen",
        "drop" => "verwerfen",
        "15min" => "15 Minuten",
        "1h" => "1 Stunde",
        "half-day" => "ein halber Tag",
        "day+" => "mindestens ein Tag",
        "S" => "klein",
        "M" => "mittel",
        "L" => "groß",
        "next" => "nächster Schritt",
        "feature" => "Funktion",
        "cleanup" => "Aufräumen",
        "other" => "Sonstiges",
        "suggestion" => "Vorschlag",
        "question" => "Frage",
        "focus" => "Fokus",
        "pattern" => "Muster",
        "synergy" => "Synergie",
        "new_app" => "App-Idee",
        "verdict" => "Empfehlung",
        _ => value,
    }
}

fn label(key: &str) -> String {
    let label = match key {
        "id" => "ID",
        "name" => "Name",
        "path" => "Ordner",
        "project" => "Projekt",
        "projects" => "Projekte",
        "status" => "Status",
        "status_note" => "Deine Vorgabe",
        "goal" => "Dein Ziel",
        "goal_due" => "Zieldatum",
        "version" => "Version",
        "initial_version" => "Version zu Beginn",
        "card_version" => "Kartenversion",
        "data_dir" => "Datenordner",
        "database" => "Datenbank",
        "exe" => "Programm",
        "skill_dir" => "Agenten-Skill",
        "card_updated_at" => "Karte aktualisiert",
        "facts_collected_at" | "collected_at" => "Fakten gemessen",
        "created_at" | "added_at" => "Erstellt",
        "updated_at" => "Aktualisiert",
        "started_at" => "Begonnen",
        "finished_at" => "Beendet",
        "reported_at" => "Gemeldet",
        "completed_at" => "Abgeschlossen",
        "at" => "Zeitpunkt",
        "today" => "Heute",
        "suggested_id" => "Vorgeschlagene ID",
        "last_commit" => "Letzter Commit",
        "branch" => "Branch",
        "head" => "Commit",
        "last_work" => "Zuletzt bearbeitet",
        "has_remote" => "Remote vorhanden",
        "remotes" => "Remotes",
        "upstream" => "Vergleichsbranch",
        "ahead" => "Commits voraus",
        "behind" => "Commits zurück",
        "unpushed_since" => "Ungepusht seit",
        "dirty" | "uncommitted" => "Nicht committete Änderungen",
        "unpushed" => "Nicht gepushte Commits",
        "docs" => "Projektdokumentation",
        "side_branches" => "Weitere Branches und Worktrees",
        "readable" => "Lesbarkeit des Projektordners",
        "files" => "Dateien",
        "modified" => "Geändert",
        "untracked" => "Nicht erfasst",
        "deleted" => "Gelöscht",
        "lines_added" => "Zeilen hinzugefügt",
        "lines_removed" => "Zeilen entfernt",
        "oldest_change" => "Älteste Änderung",
        "newest_change" => "Neueste Änderung",
        "commits" => "Commits",
        "total" => "Insgesamt",
        "first" => "Erster",
        "last" => "Letzter",
        "last_7d" => "Letzte 7 Tage",
        "last_30d" => "Letzte 30 Tage",
        "last_90d" => "Letzte 90 Tage",
        "weekly" => "Commits je Woche, älteste zuerst",
        "worktrees" => "Weitere Worktrees",
        "count" => "Anzahl",
        "prunable" => "Verwaist",
        "items" => "Einträge",
        "dirty_files" => "Nicht committete Dateien",
        "branches" => "Branches",
        "merged_count" => "Zusammengeführte Branches",
        "unmerged" => "Nicht zusammengeführte Branches",
        "unmerged_branches" => "Nicht zusammengeführte Branches",
        "extra_worktrees" => "Weitere Worktrees",
        "ci" => "CI",
        "state" => "Zustand",
        "workflow" => "Workflow",
        "workflows" => "Workflows",
        "failing" => "Fehlgeschlagen",
        "conclusion" => "Ergebnis",
        "url" => "Adresse",
        "repo" => "Repository",
        "release" => "Release",
        "tag" => "Tag",
        "date" => "Datum",
        "commits_since" => "Commits seitdem",
        "night" => "Seit gestern Abend",
        "since" => "Seit",
        "reverts" => "Rücknahmen",
        "uncommitted_touched" => "Nicht committete Dateien bearbeitet",
        "collisions" => "Überschneidungen",
        "a" => "Erste Arbeitslinie",
        "b" => "Zweite Arbeitslinie",
        "space" => "Wiederherstellbare Build-Dateien",
        "total_bytes" => "Bytes insgesamt",
        "bytes" => "Bytes",
        "dirs" => "Ordner",
        "measured_on" => "Gemessen am",
        "one_liner" => "Kurzbeschreibung",
        "headline" => "Aktueller Stand",
        "stack" => "Technik",
        "phase" => "Phase",
        "phase_evidence" => "Begründung der Phase",
        "personality" => "Charakter",
        "recent_changes" => "Letzte Änderungen",
        "milestones" => "Meilensteine",
        "features" => "Funktionen",
        "open_ends" => "Offene Enden",
        "waiting_on_you" => "Fragen an dich",
        "vision" => "Vision",
        "statement" => "Zielbild",
        "progress_note" => "Fortschritt",
        "progress_pct" => "Fortschritt in Prozent",
        "reentry" => "Wiedereinstieg",
        "next_step" => "Nächster Schritt",
        "effort" => "Aufwand",
        "why" => "Warum",
        "done_when" => "Fertig, wenn",
        "health_notes" => "Zustand und Risiken",
        "decisions" => "Entscheidungen",
        "suggestions" => "Vorschläge",
        "cross_project_hints" => "Hinweise zu anderen Projekten",
        "handbook" => "Handbuch",
        "run" => "Starten und nutzen",
        "what" => "Wofür",
        "how" => "Befehl oder Adresse",
        "folder" => "Ordner",
        "installed" => "Installierte Anwendung",
        "open" => "Öffnen",
        "architecture" => "Aufbau",
        "places" => "Wichtige Stellen",
        "read_first" => "Zuerst lesen",
        "needs" => "Voraussetzungen",
        "terms" => "Begriffe",
        "term" => "Begriff",
        "meaning" => "Bedeutung",
        "title" => "Titel",
        "detail" => "Details",
        "source" => "Quelle",
        "source_id" => "Ursprünglicher Eintrag",
        "age_days" => "Alter in Tagen",
        "options" => "Antwortmöglichkeiten",
        "answer" => "Antwort",
        "effect" => "Auswirkung",
        "type" => "Art",
        "kind" => "Art",
        "evidence" => "Beleg",
        "greeting" => "Überblick",
        "focus" => "Heute wichtig",
        "patterns" => "Muster",
        "synergies" => "Verbindungen",
        "new_apps" => "Neue App-Ideen",
        "pitch" => "Idee",
        "because" => "Begründung",
        "park_or_revive" => "Parken oder wieder aufnehmen",
        "verdict" => "Empfehlung",
        "attention_note" => "Deine Aufmerksamkeit",
        "idea_checks" => "Bewertung deiner Ideen",
        "idea" => "Idee",
        "fits" => "Passende Projekte",
        "week_review" => "Wochenrückblick",
        "week" => "Woche",
        "done" => "Erledigt",
        "stalled" => "Stecken geblieben",
        "time_note" => "Zeitaufwand",
        "severity" => "Dringlichkeit",
        "feedback" => "Deine Rückmeldungen",
        "feedback_summary" => "Bisherige Bewertungen",
        "value" => "Bewertung oder Antwort",
        "note" => "Notiz",
        "snoozed_until" => "Zurückgestellt bis",
        "item" => "Bewerteter Eintrag",
        "up" => "Gut",
        "down" => "Abgelehnt",
        "later" => "Später",
        "warning_acknowledgements" => "Bewusst bestätigte Hinweise",
        "warning_acks" => "Bewusst bestätigte Hinweise",
        "key" => "Hinweis",
        "basis" => "Grundlage",
        "comparison" => "Vergleich",
        "tasks" => "Aufträge",
        "project_snapshots" => "Projekte bei Auftragserteilung",
        "result" => "Ergebnis",
        "result_evidence" => "Ergebnisbeleg",
        "history" => "Verlauf",
        "actor" => "Gemeldet durch",
        "owner_ideas" => "Deine geparkten Ideen",
        "checked" => "Bewertet",
        "review_week" => "Rückblick auf die Woche",
        "from" => "Von",
        "to" => "Bis",
        "due" => "Fällig",
        "owner_last_opened" => "Zuletzt von dir geöffnet",
        "outdated" => "Veraltet",
        "changed_since_brief" => "Seit dem Brief geändert",
        "commits_review_week" => "Commits in der Rückblickwoche",
        "context_token" => "Kontext-Token zum Speichern",
        "since_previous_card" => "Seit der bisherigen Karte",
        "days" => "Tage",
        "commit_count" => "Commits",
        "new_fields" => "Neue auszufüllende Felder",
        "changed_fields" => "Geänderte Felder",
        "unfilled_fields" => "Noch nicht ausgefüllte Felder",
        "warnings" => "Warnungen",
        "warning" => "Warnung",
        "reason" | "problem" => "Grund",
        "error" => "Fehler",
        "code" => "Fehlercode",
        "message" => "Meldung",
        "hint" => "Hinweis",
        "changes" => "Änderungen",
        "pending_owner_replies" => "Noch zu verarbeitende Antworten von dir",
        "repair_task" => "Auftrag zum Fortsetzen",
        _ => return key.replace('_', " "),
    };
    label.into()
}
