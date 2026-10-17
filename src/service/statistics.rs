//! Read models for durable history and comparable observations.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn differences(before: &Value, after: &Value) -> Vec<Value> {
    let keys: BTreeSet<&String> = before
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .chain(after.as_object().into_iter().flat_map(|o| o.keys()))
        .collect();
    keys.into_iter()
        .filter(|key| before[*key] != after[*key])
        .map(|key| json!({"field":key,"before":before[key],"after":after[key]}))
        .collect()
}

fn sample_view(sample: &Value) -> Value {
    let f = &sample["facts"];
    json!({"id":sample["id"],"at":sample["at"],"origin":sample["origin"],"path":sample["path"],
        "ok":f["ok"],"error":f["error"],"branch":f["branch"],"head":f["head"],
        "code":f["code"],"commits":f["commits"]["total"],"dirty":f["dirty"],
        "ahead":f["ahead"],"behind":f["behind"],"unpushed_since":f["unpushed_since"],
        "worktrees":f["worktrees"],"branches":f["branches"],"release":f["release"],"ci":f["ci"],"space":f["space"]})
}

impl Watchtower {
    fn historical_project(&self, id: &str) -> Result<Project> {
        self.store.project_including_removed(id)?.ok_or_else(|| {
            WatchtowerError::new(
                "unknown_project",
                format!("Project '{id}' is not registered. Nothing was changed."),
            )
            .hint("Read watchtower projects list or watchtower projects trash for project IDs.")
        })
    }

    pub fn statistics(
        &self,
        project: Option<&str>,
        from: Option<&str>,
        until: Option<&str>,
    ) -> Result<Value> {
        let parse_date = |input: &str| {
            NaiveDate::parse_from_str(input, "%Y-%m-%d")
                .ok()
                .filter(|d| d.to_string() == input)
                .ok_or_else(|| {
                    WatchtowerError::new(
                        "invalid_date",
                        format!("'{input}' is not a YYYY-MM-DD date. Nothing was changed."),
                    )
                })
        };
        let end = until.map(parse_date).transpose()?.unwrap_or(self.today());
        let start = from
            .map(parse_date)
            .transpose()?
            .unwrap_or(end - chrono::Duration::days(29));
        if start > end {
            return Err(WatchtowerError::new(
                "invalid_range",
                "The start date is after the end date. Nothing was changed.",
            )
            .hint("Pass --from on or before --until."));
        }
        let from = start.to_string();
        let until = end.to_string();
        let projects = match project {
            Some(id) => vec![self.historical_project(id)?],
            None => self.store.projects()?,
        };
        let tasks = self.store.tasks()?;
        let mut output = Vec::new();
        let mut totals = BTreeMap::<String, u64>::new();
        let mut known = 0;
        for p in projects {
            let period = self.store.period_samples(&p.id, &from, &until)?;
            let in_period: Vec<&Value> = period.samples.iter().collect();
            let previous = period.previous.as_ref();
            let last = in_period.last().copied().or(previous);
            let complete = |s: &&Value| {
                s["facts"]["ok"] == true
                    && s["facts"]["code"]["ok"] == true
                    && s["facts"]["code"]["source_code"].is_number()
            };
            let baseline = previous
                .filter(complete)
                .or_else(|| in_period.iter().copied().find(complete))
                .or(previous)
                .or_else(|| in_period.first().copied());
            // Only these two views carry every language and the largest files.
            let full = |sample: Option<&Value>| -> Result<Option<Value>> {
                match sample.and_then(|s| s["id"].as_i64()) {
                    Some(id) => Ok(self.store.fact_sample(id)?.as_ref().map(sample_view)),
                    None => Ok(None),
                }
            };
            let current = full(last)?;
            let base = match (baseline, last) {
                (Some(a), Some(b)) if a["id"] == b["id"] => current.clone(),
                _ => full(baseline)?,
            };
            let comparable = match (baseline, last) {
                (Some(a), Some(b)) if a["id"] != b["id"] => {
                    a["facts"]["code"]["ok"] == true
                        && b["facts"]["code"]["ok"] == true
                        && in_period
                            .iter()
                            .filter(|s| {
                                (s["at"].as_str(), s["id"].as_i64())
                                    >= (a["at"].as_str(), a["id"].as_i64())
                            })
                            .all(|s| {
                                s["facts"]["ok"] == true
                                    && s["facts"]["code"]["ok"] == true
                                    && s["facts"]["branch"] == a["facts"]["branch"]
                                    && s["path"] == a["path"]
                                    && s["facts"]["code"]["method"] == a["facts"]["code"]["method"]
                            })
                }
                _ => false,
            };
            let change = if comparable {
                let a = baseline.unwrap()["facts"]["code"]["source_code"].as_i64();
                let b = last.unwrap()["facts"]["code"]["source_code"].as_i64();
                a.zip(b).map(|(a,b)|json!({"code":b-a,"percent":(a!=0).then(||(b-a) as f64*100.0/a as f64)}))
            } else {
                None
            };
            if let Some(c) = &current {
                if c["ok"] == true && c["code"]["ok"] == true {
                    known += 1;
                    for lang in c["code"]["languages"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|l| l["category"] == "source")
                    {
                        *totals
                            .entry(lang["language"].as_str().unwrap_or("Unknown").to_owned())
                            .or_default() += lang["code"].as_u64().unwrap_or(0);
                    }
                }
            }
            // Git dates can reconstruct activity before the first Watchtower observation.
            let activity = period.hotspots.as_ref();
            let mut activity_days = BTreeMap::<String, Value>::new();
            let mut coverage = Vec::new();
            for observation in period.reconstructions.iter().filter(|r| {
                last.is_none_or(|s| r["branch"] == s["facts"]["branch"] && r["path"] == s["path"])
            }) {
                let a = &observation["activity"];
                let (Some(a_from), Some(a_until)) = (a["from"].as_str(), a["through"].as_str())
                else {
                    continue;
                };
                let (Ok(a_from), Ok(a_until)) = (
                    NaiveDate::parse_from_str(a_from, "%Y-%m-%d"),
                    NaiveDate::parse_from_str(a_until, "%Y-%m-%d"),
                ) else {
                    continue;
                };
                coverage.push((a_from.max(start), a_until.min(end)));
                // A newer reconstruction replaces the covered dates, including days now absent after a rebase.
                activity_days.retain(|date, _| {
                    date.as_str() < a["from"].as_str().unwrap()
                        || date.as_str() > a["through"].as_str().unwrap()
                });
                for day in a["days"].as_array().into_iter().flatten() {
                    if let Some(date) = day["date"]
                        .as_str()
                        .filter(|date| *date >= from.as_str() && *date <= until.as_str())
                    {
                        activity_days.insert(date.to_owned(), day.clone());
                    }
                }
            }
            coverage.sort();
            let mut merged: Vec<(NaiveDate, NaiveDate)> = Vec::new();
            for (start, end) in coverage {
                if start > end {
                    continue;
                }
                if let Some(last) = merged
                    .last_mut()
                    .filter(|last| last.1.succ_opt().is_some_and(|day| day >= start))
                {
                    last.1 = last.1.max(end);
                } else {
                    merged.push((start, end));
                }
            }
            let complete_activity = merged.len() == 1 && merged[0].0 == start && merged[0].1 == end;
            let days: Vec<Value> = activity_days.into_values().collect();
            let activity_counts=complete_activity.then(||json!({"commits":days.iter().filter_map(|d|d["commits"].as_u64()).sum::<u64>(),
                "active_days":days.len(),"added":days.iter().filter_map(|d|d["added"].as_u64()).sum::<u64>(),"removed":days.iter().filter_map(|d|d["removed"].as_u64()).sum::<u64>()}));
            let completed = tasks
                .iter()
                .filter(|t| {
                    t["projects"]
                        .as_array()
                        .is_some_and(|ids| ids.iter().any(|id| id == &p.id))
                })
                .flat_map(|t| t["history"].as_array().into_iter().flatten())
                .filter(|e| {
                    e["status"] == "done"
                        && e["actor"] == "owner"
                        && e["at"]
                            .as_str()
                            .and_then(|s| s.get(..10))
                            .is_some_and(|d| d >= from.as_str() && d <= until.as_str())
                })
                .count();
            output.push(json!({"id":p.id,"name":p.name,"status":p.status,"removed":p.removed_at.is_some(),
                "current":current,"baseline":base,"change":change,"comparable":comparable,
                "comparison_note":if comparable {"Comparable observed working-tree measurements."} else {"Two complete measurements with the same branch, folder and counting method are required; gaps prevent comparison."},
                "samples":in_period.into_iter().map(sample_view).collect::<Vec<_>>(),
                "activity":{"origin":"reconstructed","complete":complete_activity,"coverage_from":merged.first().map(|r|r.0.to_string()),"coverage_through":merged.last().map(|r|r.1.to_string()),"coverage":merged.iter().map(|r|json!({"from":r.0.to_string(),"through":r.1.to_string()})).collect::<Vec<_>>(),"branch":last.map(|s|&s["facts"]["branch"]),"days":days,"totals":activity_counts,
                    "hotspots":activity.map(|a|&a["hotspots"]),"hotspots_from":activity.map(|a|if a["hotspots_from"].is_null(){&a["from"]}else{&a["hotspots_from"]}),"hotspots_through":activity.map(|a|&a["through"])},
                "confirmed_completions":completed}));
        }
        let code: u64 = totals.values().sum();
        let languages:Vec<Value>=totals.into_iter().map(|(language,lines)|json!({"language":language,"code":lines,"percent":(code>0).then(||lines as f64*100.0/code as f64)})).collect();
        Ok(
            json!({"from":from,"until":until,"projects":output,"summary":{"projects":output.len(),"measured_projects":known,"source_code":(known>0).then_some(code),"languages":languages},
            "note":"Stored observations only; no refresh or acknowledgement. Samples are the first observation of each UTC day and every change; an observation holds until the next one. Code is the working tree, including uncommitted files. Activity is reconstructed from available Git history. Missing observations are unknown."}),
        )
    }

    pub fn history(
        &self,
        project: Option<&str>,
        entity: Option<&str>,
        id: Option<&str>,
        before: Option<i64>,
    ) -> Result<Value> {
        if let Some(id) = project {
            self.historical_project(id)?;
        }
        if let Some(entity) = entity {
            if ![
                "card",
                "portfolio",
                "project",
                "idea",
                "proposal",
                "task",
                "feedback",
                "warning",
            ]
            .contains(&entity)
            {
                return Err(WatchtowerError::new(
                    "invalid_entity",
                    "Unknown history entity. Nothing was changed.",
                )
                .hint("Use card, portfolio, project, idea, proposal, task, feedback or warning."));
            }
        }
        let rows = self
            .store
            .content_history(project, entity, id, before, 31)?;
        let mut rows = rows
            .into_iter()
            .filter(|e| before.is_none_or(|id| e["id"].as_i64().is_some_and(|n| n < id)));
        let events: Vec<Value> = rows
            .by_ref()
            .take(30)
            .map(|mut e| {
                e["fields"] = json!(differences(&e["before"], &e["after"])
                    .iter()
                    .map(|d| d["field"].clone())
                    .collect::<Vec<_>>());
                e["title"] = e["after"]["headline"].clone();
                for key in ["title", "text", "name"] {
                    if e["title"].is_null() {
                        e["title"] = e["after"][key].clone();
                    }
                }
                e.as_object_mut().unwrap().remove("before");
                e.as_object_mut().unwrap().remove("after");
                e
            })
            .collect();
        let next = rows
            .next()
            .and_then(|_| events.last().map(|e| e["id"].clone()));
        Ok(
            json!({"events":events,"next_before":next,"project":project,"entity":entity,"entity_id":id,
            "note":"Newest saved events first. Retained baselines do not establish earlier edits or authorship."}),
        )
    }

    pub fn history_event(&self, id: i64) -> Result<Value> {
        let mut event = self
            .store
            .content_history(None, None, None, Some(id.saturating_add(1)), 1)?
            .into_iter()
            .find(|e| e["id"] == id)
            .ok_or_else(|| {
                WatchtowerError::new(
                    "history_not_found",
                    format!("History event {id} does not exist. Nothing was changed."),
                )
                .hint("Read watchtower history list for saved event IDs.")
            })?;
        event["changes"] = json!(differences(&event["before"], &event["after"]));
        Ok(json!({"event":event}))
    }

    pub fn compare_documents(&self, project: Option<&str>, from: i64, to: i64) -> Result<Value> {
        let (before, after) = if let Some(id) = project {
            self.historical_project(id)?;
            let get = |v| {
                self.store
                    .card_version(id, v)
                    .map(|c| c.map(|c| json!({"at":c.created_at,"document":c.data})))
            };
            (get(from)?, get(to)?)
        } else {
            let get = |v| {
                self.store
                    .portfolio_version(v)
                    .map(|c| c.map(|c| json!({"at":c.created_at,"document":c.data})))
            };
            (get(from)?, get(to)?)
        };
        let (Some(before), Some(after)) = (before, after) else {
            return Err(WatchtowerError::new("version_not_found","One of the requested document versions does not exist. Nothing was changed.").hint("Read card history with the project ID, or portfolio history, for saved version numbers."));
        };
        Ok(
            json!({"project":project,"from":from,"to":to,"before_at":before["at"],"after_at":after["at"],"changes":differences(&before["document"],&after["document"])}),
        )
    }

    pub fn proposals(&self, project: Option<&str>, category: Option<&str>) -> Result<Value> {
        if category.is_some_and(|c| !["idea", "recommendation"].contains(&c)) {
            return Err(WatchtowerError::new(
                "invalid_category",
                "Unknown proposal category. Nothing was changed.",
            )
            .hint("Use idea or recommendation, or omit --category for both."));
        }
        if let Some(id) = project {
            self.historical_project(id)?;
        }
        Ok(
            json!({"category":category,"proposals":self.store.proposals()?.into_iter().map(super::without_effort).filter(|p|project.is_none_or(|id|p["scope"]==id) && category.is_none_or(|c|p["category"]==c)).collect::<Vec<_>>()}),
        )
    }

    pub fn facts_history(&self, id: &str) -> Result<Value> {
        self.historical_project(id)?;
        Ok(
            json!({"project":id,"samples":self.store.fact_samples(Some(id),None,None)?,"note":"Every saved observation, oldest first, including failed measurements. A refresh is saved when it differs from the previous observation or is the first of its UTC day. Retained marks the last facts available when history recording began."}),
        )
    }
}
