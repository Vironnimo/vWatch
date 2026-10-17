//! Signals derived from saved history: how long a measured risk has lasted and
//! whether it grew, and how long each owner-facing item has been shown.
use super::*;
use crate::store::RiskObservation;
use std::collections::HashMap;

const CARD_LISTS: &[(&str, &str)] = &[
    ("waiting_on_you", "question"),
    ("suggestions", "suggestion"),
    ("open_ends", "open_end"),
];
const BRIEF_LISTS: &[(&str, &str)] = &[
    ("focus", "focus"),
    ("patterns", "pattern"),
    ("synergies", "synergy"),
    ("new_apps", "new_app"),
    ("park_or_revive", "verdict"),
];
/// Focus items taken out of the brief unfinished stay visible this long.
const DROPPED_FOCUS_DAYS: i64 = 14;

#[derive(Clone)]
pub(crate) struct Seen {
    kind: &'static str,
    first: String,
    last: String,
    versions: u32,
    item: Value,
}

/// Item appearances per document scope, extended incrementally by saved row id.
#[derive(Default)]
pub(crate) struct AgeIndex(HashMap<String, (i64, HashMap<String, Seen>)>);

/// Items without an id (open ends) are recognised by their unchanged title.
fn item_key(kind: &str, item: &Value) -> Option<String> {
    match item["id"].as_str() {
        Some(id) => Some(id.to_owned()),
        None => item["title"]
            .as_str()
            .map(|t| format!("{kind}:{}", digest_text(&json!(t)))),
    }
}

fn level(kind: &str, o: &RiskObservation) -> Option<bool> {
    if !o.ok {
        return None;
    }
    match kind {
        "uncommitted" => o.dirty_files.map(|n| n > 0),
        "unpushed" => o.ahead.map(|n| n > 0),
        _ => match o.ci.as_deref() {
            Some("red") => Some(true),
            Some("green") => Some(false),
            _ => None,
        },
    }
}

fn amount(kind: &str, o: &RiskObservation) -> Value {
    match kind {
        "uncommitted" => json!({"files": o.dirty_files, "lines_added": o.lines_added}),
        "unpushed" => json!({"commits": o.ahead}),
        _ => Value::Null,
    }
}

fn trend(kind: &str, then: &RiskObservation, now: &RiskObservation) -> Value {
    let (a, b) = match kind {
        "uncommitted" => (
            then.lines_added.or(then.dirty_files),
            now.lines_added.or(now.dirty_files),
        ),
        "unpushed" => (then.ahead, now.ahead),
        _ => return Value::Null,
    };
    match (a, b) {
        (Some(a), Some(b)) if b * 10 > a * 11 => json!("growing"),
        (Some(a), Some(b)) if b * 10 < a * 9 => json!("shrinking"),
        (Some(_), Some(_)) => json!("steady"),
        _ => Value::Null,
    }
}

/// Calendar days in local time, as the owner counts them.
fn local_days(at: &str, now: DateTime<Utc>) -> i64 {
    let day = |t: DateTime<Utc>| t.with_timezone(&Local).date_naive();
    (day(now) - day(parse(at))).num_days().max(0)
}

/// Unbroken runs of uncommitted work, unpushed commits and failed CI that last
/// until the current measurement. `at_least` means the run started before the
/// first saved observation, so `since` is a lower bound.
pub(crate) fn risk_streaks(
    observations: &[RiskObservation],
    facts: &Value,
    collected_at: &str,
    now: DateTime<Utc>,
) -> Vec<Value> {
    let current = RiskObservation {
        at: collected_at.to_owned(),
        ok: facts["ok"] == true,
        dirty_files: facts["dirty"]["files"].as_i64(),
        lines_added: facts["dirty"]["lines_added"].as_i64(),
        ahead: facts["ahead"].as_i64(),
        ci: facts["ci"]["state"].as_str().map(str::to_owned),
    };
    let mut out = Vec::new();
    for kind in ["uncommitted", "unpushed", "ci_red"] {
        if level(kind, &current) != Some(true) {
            continue;
        }
        let mut start = &current;
        let mut broken = false;
        for o in observations.iter().filter(|o| o.at <= current.at) {
            match level(kind, o) {
                Some(true) => start = o,
                Some(false) => {
                    broken = true;
                    break;
                }
                None => {}
            }
        }
        let mut streak = json!({
            "kind": kind, "since": start.at, "days": local_days(&start.at, now),
            "at_least": !broken, "then": amount(kind, start), "now": amount(kind, &current),
            "trend": trend(kind, start, &current),
        });
        if let Some(fields) = streak.as_object_mut().filter(|_| kind == "ci_red") {
            for empty in ["then", "now", "trend"] {
                fields.remove(empty);
            }
            fields.insert("last_run_at".into(), facts["ci"]["at"].clone());
        }
        out.push(streak);
    }
    out
}

impl Watchtower {
    /// Item appearances of one card scope (`Some(project)`) or of the brief (`None`).
    fn item_index(&self, project: Option<&str>) -> Result<HashMap<String, Seen>> {
        let scope = project.unwrap_or("@portfolio").to_owned();
        let lists = if project.is_some() {
            CARD_LISTS
        } else {
            BRIEF_LISTS
        };
        let mut index = self.ages.lock().unwrap_or_else(|e| e.into_inner());
        let (after, seen) = index.0.entry(scope).or_default();
        let names: Vec<&str> = lists.iter().map(|(name, _)| *name).collect();
        for (row, at, parsed) in self.store.document_lists_after(project, *after, &names)? {
            for (list, (_, kind)) in parsed.iter().zip(lists) {
                for item in list.as_array().into_iter().flatten() {
                    let Some(key) = item_key(kind, item) else {
                        continue;
                    };
                    let entry = seen.entry(key).or_insert_with(|| Seen {
                        kind,
                        first: at.clone(),
                        last: at.clone(),
                        versions: 0,
                        item: Value::Null,
                    });
                    entry.last = at.clone();
                    entry.versions += 1;
                    entry.item = item.clone();
                }
            }
            *after = row;
        }
        Ok(seen.clone())
    }

    fn days_since(&self, at: &str) -> i64 {
        (self.today() - parse(at).with_timezone(&Local).date_naive()).num_days()
    }

    fn persistence_rows(
        &self,
        seen: &HashMap<String, Seen>,
        doc: &Value,
        lists: &[(&str, &str)],
        reactions: &[Feedback],
    ) -> Vec<Value> {
        let mut rows = Vec::new();
        for (list, kind) in lists {
            for item in doc[*list].as_array().into_iter().flatten() {
                let Some(s) = item_key(kind, item).and_then(|key| seen.get(&key)) else {
                    continue;
                };
                let reaction = item["id"]
                    .as_str()
                    .and_then(|id| reactions.iter().find(|f| f.item_id == id))
                    .map(|f| json!({"value": f.value, "at": f.at}));
                let mut row = json!({
                    "kind": kind, "id": item["id"], "title": item["title"],
                    "first_seen": s.first, "days": self.days_since(&s.first), "versions": s.versions,
                });
                if item["project"].is_string() {
                    row["project"] = item["project"].clone();
                }
                if let Some(reaction) = reaction {
                    row["owner_reaction"] = reaction;
                }
                rows.push(row);
            }
        }
        rows
    }

    /// How long each question, suggestion and open end of the current card has been shown.
    pub(crate) fn card_persistence(
        &self,
        project: &str,
        card: &Value,
        reactions: &[Feedback],
    ) -> Result<Vec<Value>> {
        let seen = self.item_index(Some(project))?;
        Ok(self.persistence_rows(&seen, card, CARD_LISTS, reactions))
    }

    /// Brief items with their age, and recent focus items that left the brief
    /// without the owner marking them done.
    pub(crate) fn brief_persistence(
        &self,
        brief: Option<&StoredPortfolio>,
        reactions: &[Feedback],
    ) -> Result<Value> {
        let Some(brief) = brief else {
            return Ok(json!({"items": [], "dropped_focus": []}));
        };
        let seen = self.item_index(None)?;
        let items = self.persistence_rows(&seen, &brief.data, BRIEF_LISTS, reactions);
        let mut dropped: Vec<(&String, Value)> = seen
            .values()
            .filter(|s| s.kind == "focus" && s.last < brief.created_at)
            .filter(|s| self.days_since(&s.last) <= DROPPED_FOCUS_DAYS)
            .filter(|s| {
                !reactions
                    .iter()
                    .any(|f| Some(f.item_id.as_str()) == s.item["id"].as_str() && f.value == "done")
            })
            .map(|s| {
                (&s.last, json!({
                    "id": s.item["id"], "title": s.item["title"], "project": s.item["project"],
                    "first_seen": s.first, "last_seen": s.last, "briefs": s.versions,
                }))
            })
            .collect();
        dropped.sort_by(|a, b| b.0.cmp(a.0));
        Ok(json!({"items": items, "dropped_focus": dropped.into_iter().map(|(_, v)| v).collect::<Vec<_>>()}))
    }

    /// Measured risks of a project with how long they have lasted.
    pub(crate) fn project_streaks(&self, project: &str, stored: Option<&StoredFacts>) -> Result<Vec<Value>> {
        let Some(stored) = stored else {
            return Ok(Vec::new());
        };
        let observations = self.store.risk_observations(project)?;
        Ok(risk_streaks(
            &observations,
            &stored.data,
            &stored.collected_at,
            (self.clock)(),
        ))
    }

    /// Whether the owner reacts at all: the context for how insistently to ask.
    pub(crate) fn owner_response(&self, reactions: &[Feedback], open_questions: &[Value]) -> Value {
        let last = reactions.iter().map(|f| f.at.as_str()).max();
        let recent = reactions
            .iter()
            .filter(|f| self.days_since(&f.at) <= 14)
            .count();
        let oldest = open_questions
            .iter()
            .filter_map(|q| q["days"].as_i64())
            .max();
        json!({
            "last_reaction_at": last, "reactions_last_14_days": recent,
            "open_questions": open_questions.len(), "oldest_open_question_days": oldest,
        })
    }
}
