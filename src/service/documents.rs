//! Document validation, guarded writes, replay and restoration.
use super::*;

/// Whether a handbook command reads like something other than what the owner types:
/// commands quoted in a sentence, a URL behind a command, or a shortcut to click.
fn not_typed(how: &str) -> bool {
    let lower = how.to_lowercase();
    how.contains(['`', '„', '“'])
        || how.contains("; http")
        || [
            "startmen",
            "start-men",
            "start menu",
            "doppelklick",
            "double-click",
        ]
        .iter()
        .any(|w| lower.contains(w))
}

pub(super) fn document_request(value: &Value) -> String {
    use sha1::{Digest, Sha1};
    format!("request1-{:x}", Sha1::digest(value.to_string().as_bytes()))
}

/// The ids vWatch keeps for a saved document's items, for the agent's reply.
pub(super) fn item_ids(kind: Kind, doc: &Value) -> Vec<Value> {
    kind.items()
        .iter()
        .flat_map(|(list, _, item_kind)| {
            doc[*list]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |i| json!({"id": i["id"], "kind": item_kind, "title": title_of(i)}))
        })
        .collect()
}

/// The top-level fields whose content differs from the previous version (all of them for a first version).
fn changed_fields(kind: Kind, previous: Option<&Value>, clean: &Value) -> Vec<String> {
    // compare against the previous version with today's defaults filled in, so fields the schema
    // gained since do not count as changes
    let previous = previous
        .map(|p| cards::validate(kind, p, &top_fields(p)).map_or_else(|_| p.clone(), |(c, _)| c));
    let previous = previous.as_ref();
    let map = clean.as_object().into_iter().flatten();
    map.filter(|(k, v)| previous.is_none_or(|p| p.get(k.as_str()) != Some(v)))
        .map(|(k, _)| k.clone())
        .collect()
}

fn top_fields(doc: &Value) -> Vec<String> {
    doc.as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// With --merge, the previous version's fields the agent did not send: they are kept unchanged.
fn kept_fields(previous: Option<&Value>, sent: Option<&[String]>) -> Vec<String> {
    match (previous, sent) {
        (Some(previous), Some(sent)) => top_fields(previous)
            .into_iter()
            .filter(|f| !sent.contains(f))
            .collect(),
        _ => Vec::new(),
    }
}

/// With --merge, the top-level fields the agent actually sent.
fn sent_fields(payload: &Value, merge: bool) -> Option<Vec<String>> {
    merge.then(|| {
        payload
            .as_object()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    })
}

/// Validation problems, each marked when its field came from the previous version rather than the agent.
fn mark_inherited(problems: &[String], sent: Option<&[String]>, document: &str) -> String {
    problems
        .iter()
        .map(|p| {
            let field = p.split(['.', '[', ':']).next().unwrap_or("");
            match sent {
                Some(sent) if !sent.iter().any(|s| s == field) => {
                    format!("{p} (from the previous {document})")
                }
                _ => p.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A dry run's items: ids vWatch generated show `id: null`, because they are only assigned when saved.
fn dry_items(kind: Kind, sent: &Value, receipts: &[Value]) -> Vec<Value> {
    let given: Vec<&Value> = kind
        .items()
        .iter()
        .flat_map(|(list, _, _)| {
            sent[*list]
                .as_array()
                .into_iter()
                .flatten()
                .map(|i| &i["id"])
        })
        .collect();
    receipts
        .iter()
        .cloned()
        .map(|mut item| {
            if !given.contains(&&item["id"]) {
                item["id"] = Value::Null;
            }
            item
        })
        .collect()
}

/// Items sent as `{"id": ...}` alone keep their latest saved wording.
fn keep_items(
    kind: Kind,
    payload: &mut Value,
    saved: &HashMap<String, (&'static str, Value)>,
) -> Result<()> {
    let missing = cards::expand_kept_items(kind, payload, |list, id| {
        let (_, _, item_kind) = kind.items().iter().find(|(l, _, _)| *l == list)?;
        saved
            .get(id)
            .filter(|(kind, _)| kind == item_kind)
            .map(|(_, item)| item.clone())
    });
    if missing.is_empty() {
        return Ok(());
    }
    let (code, name) = match kind {
        Kind::Card => ("invalid_card", "card"),
        Kind::Portfolio => ("invalid_portfolio", "brief"),
    };
    Err(VWatchError::new(code, format!("The {name} was not saved: {} keeps an item by its id alone, but no saved version of this {name} has that item in that list.", missing.join(", ")))
        .hint(format!("Send that item's full content, or leave it out. An item sent as {{\"id\": ...}} alone must use an id from the context's previous {name} or its item history.")))
}

/// Ids the agent supplied that no saved version of this document used: they start a new item.
fn unknown_item_ids(
    kind: Kind,
    payload: &Value,
    saved: &HashMap<String, (&'static str, Value)>,
) -> Vec<String> {
    if saved.is_empty() {
        return Vec::new();
    }
    let name = match kind {
        Kind::Card => "card",
        Kind::Portfolio => "brief",
    };
    kind.items()
        .iter()
        .flat_map(|(list, _, _)| {
            payload[*list]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(move |(index, item)| {
                    let id = item["id"].as_str()?;
                    (!saved.contains_key(id)).then(|| format!(
                        "{list}[{index}].id '{id}' appears in no saved version of this {name}, so it is saved as a new item. To keep an earlier item, use its id from the context; for a new item, omit id."
                    ))
                })
        })
        .collect()
}

/// The saved items with whether each is new, kept unchanged, edited or restored from an earlier version.
fn item_receipts(
    kind: Kind,
    previous: Option<&Value>,
    saved: &HashMap<String, (&'static str, Value)>,
    clean: &Value,
) -> Vec<Value> {
    let previous = previous
        .map(|p| cards::validate(kind, p, &top_fields(p)).map_or_else(|_| p.clone(), |(c, _)| c));
    kind.items()
        .iter()
        .flat_map(|(list, _, item_kind)| {
            let before = previous.as_ref().map(|p| p[*list].clone()).unwrap_or(Value::Null);
            clean[*list]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |item| {
                    let earlier = before
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|old| old["id"] == item["id"]);
                    let change = match earlier {
                        Some(old) if *old == item => "kept",
                        Some(_) => "edited",
                        None if item["id"].as_str().is_some_and(|id| saved.contains_key(id)) => {
                            "restored"
                        }
                        None => "new",
                    };
                    json!({"id": item["id"], "kind": item_kind, "title": title_of(&item), "change": change})
                })
        })
        .collect()
}

impl VWatch {
    pub fn portfolio(&self) -> Result<Value> {
        let p = self.store.latest_portfolio()?.ok_or_else(|| {
            VWatchError::new("no_portfolio", "No portfolio brief has been written yet.")
                .hint("Run `vwatch portfolio context` and write one.")
        })?;
        Ok(json!({"version": p.version, "created_at": p.created_at, "portfolio": p.data}))
    }

    /// Cards and briefs written before questions and focus items had ids get them,
    /// so the owner can answer those items too.
    pub(super) fn give_items_ids(&self) -> Result<()> {
        let missing = |kind: Kind, doc: &Value| {
            kind.items().iter().any(|(list, _, _)| {
                doc[*list]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|i| !i["id"].is_string()))
            })
        };
        for p in self.store.projects()? {
            if let Some(card) = self
                .store
                .latest_card(&p.id)?
                .filter(|c| missing(Kind::Card, &c.data))
            {
                if let Ok((clean, _)) =
                    cards::validate(Kind::Card, &card.data, &top_fields(&card.data))
                {
                    self.store
                        .rewrite_card_if_current(&p.id, card.version, &card.data, &clean)?;
                }
            }
        }
        if let Some(pf) = self
            .store
            .latest_portfolio()?
            .filter(|p| missing(Kind::Portfolio, &p.data))
        {
            if let Ok((clean, _)) =
                cards::validate(Kind::Portfolio, &pf.data, &top_fields(&pf.data))
            {
                self.store
                    .rewrite_portfolio_if_current(pf.version, &pf.data, &clean)?;
            }
        }
        Ok(())
    }

    fn merged(previous: Option<&Value>, payload: Value, merge: bool, what: &str) -> Result<Value> {
        if !merge {
            return Ok(payload);
        }
        let Some(previous) = previous else {
            return Err(VWatchError::new(
                "nothing_to_merge",
                format!("There is no previous {what} to merge into."),
            )
            .hint(format!("Send a complete {what} without --merge.")));
        };
        Ok(if payload.is_object() {
            extend(previous.clone(), payload)
        } else {
            payload
        })
    }

    /// Validates and saves a card; with `dry_run` only validates and reports what would be saved.
    pub fn put_card(&self, id: &str, payload: Value, merge: bool, dry_run: bool) -> Result<Value> {
        self.project(id)?;
        if !dry_run {
            self.stored_facts(id)?;
        }
        let token = self.store.context_token(Some(id))?;
        self.put_card_guarded(id, payload, merge, dry_run, Some(&token))
    }

    pub fn put_card_guarded(
        &self,
        id: &str,
        payload: Value,
        merge: bool,
        dry_run: bool,
        context_token: Option<&str>,
    ) -> Result<Value> {
        let project = self.project(id)?;
        let request = document_request(&json!(["put", merge, payload]));
        if !dry_run {
            if let Some(result) = self.document_replay(Some(id), context_token, &request)? {
                return Ok(result);
            }
        }
        if let Err(error) = self.check_context(Some(id), context_token, dry_run) {
            if !dry_run {
                if let Some(result) = self.document_replay(Some(id), context_token, &request)? {
                    return Ok(result);
                }
            }
            return Err(error);
        }
        let previous = self.store.latest_card(id)?;
        let saved = self.saved_items(Some(id))?;
        let mut payload = payload;
        keep_items(Kind::Card, &mut payload, &saved)?;
        let sent = sent_fields(&payload, merge);
        let kept = kept_fields(previous.as_ref().map(|c| &c.data), sent.as_deref());
        let payload = Self::merged(previous.as_ref().map(|c| &c.data), payload, merge, "card")?;
        let (clean, warnings) = cards::validate(Kind::Card, &payload, &kept).map_err(|problems| {
            let merged = if merge { " With --merge, fields marked (from the previous card) were not in your document: send corrected versions of those too." } else { "" };
            VWatchError::new("invalid_card", format!("The card was not saved: {}", mark_inherited(&problems, sent.as_deref(), "card")))
                .hint(format!("Fix the listed fields and send the card again.{merged} `vwatch schema card` prints the full schema."))
        })?;
        self.validate_item_ids(Kind::Card, Some(id), &clean)?;
        let unmatched: Vec<String> = clean["waiting_on_you"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, q)| {
                q["recommended"]
                    .as_u64()
                    .is_some_and(|n| n as usize > q["options"].as_array().map_or(0, Vec::len))
            })
            .map(|(i, _)| format!("waiting_on_you[{i}].recommended"))
            .collect();
        if !unmatched.is_empty() {
            return Err(VWatchError::new("invalid_card", format!("The card was not saved: {} names an option the question does not have.", unmatched.join(", ")))
                .hint("Set recommended to the 1-based number of one of the question's options, or null when no option is recommended."));
        }
        let changed = changed_fields(Kind::Card, previous.as_ref().map(|c| &c.data), &clean);
        // fields the previous card lacked and this save did not send end up empty; say so
        let lacking = previous
            .as_ref()
            .map(|c| Kind::Card.missing_fields(&c.data))
            .unwrap_or_default();
        let unfilled: Vec<String> = Kind::Card
            .missing_fields(&payload)
            .into_iter()
            .filter(|f| lacking.contains(f))
            .collect();
        let mut warnings = warnings;
        warnings.extend(unknown_item_ids(Kind::Card, &payload, &saved));
        // the owner copies `how` as it stands, so it must not be a sentence, a URL behind a command or a shortcut to click
        let prose: Vec<&str> = clean["handbook"]["run"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["how"].as_str().is_some_and(not_typed))
            .filter_map(|r| r["what"].as_str())
            .collect();
        if !prose.is_empty() {
            warnings.push(format!(
                "handbook.run: `how` of \"{}\" is not a plain command or URL: it has backticks, a URL after a command, or a shortcut to click. vWatch copies `how` as it stands, so it holds exactly what the owner types, or one URL to open; a URL gets its own entry, and how to open the installed app belongs in `handbook.installed.open`. Send the whole handbook with that fixed.",
                prose.join("\", \"")
            ));
        }
        if !unfilled.is_empty() {
            warnings.push(format!(
                "The previous card lacked {} and this card does not fill them, so they are saved empty. Send them with --merge (a list or the handbook in full).",
                unfilled.join(", ")
            ));
        }
        let receipts = item_receipts(
            Kind::Card,
            previous.as_ref().map(|c| &c.data),
            &saved,
            &clean,
        );
        if dry_run {
            self.check_context(Some(id), context_token, true)?;
            let items = dry_items(Kind::Card, &payload, &receipts);
            return Ok(
                json!({"saved": false, "dry_run": true, "project": id, "changed_fields": changed, "unfilled_fields": unfilled, "warnings": warnings, "items": items}),
            );
        }
        let stored = self.stored_facts(id)?;
        let parts = facts::basis(&stored.data);
        let owner = owner_basis(
            &project,
            &self.store.feedback()?,
            &self.warning_acknowledgements(id)?,
            &self.project_task_basis(id)?,
        );
        let basis = CardBasis {
            fingerprint: &stored.fingerprint,
            head: stored.data["head"].as_str(),
            ci: settled_ci(&stored.data),
            parts: &parts,
            owner: Some(&owner),
        };
        let write = self.store.add_card_guarded(
            id,
            &self.now(),
            &basis,
            &clean,
            context_token.expect("checked before validation"),
            &request,
        )?;
        let (version, token, replayed) = Self::guarded_version(write, Kind::Card, Some(id))?;
        Ok(extend(
            self.document_saved(Some(id), version, &token, replayed),
            json!({"card":clean,"changed_fields": changed, "unfilled_fields": unfilled, "warnings": warnings, "items": receipts}),
        ))
    }

    fn document_saved(
        &self,
        project: Option<&str>,
        version: i64,
        token: &str,
        replayed: bool,
    ) -> Value {
        let undo = (version > 1).then(|| match project {
            Some(id) => format!(
                "vwatch card restore {id} --version {} --context-token {token}",
                version - 1
            ),
            None => format!(
                "vwatch portfolio restore --version {} --context-token {token}",
                version - 1
            ),
        });
        let mut result = json!({"saved":true,"version":version,"already_saved":replayed,"context_token":token,"undo":undo});
        if let Some(id) = project {
            result["project"] = json!(id);
        }
        result
    }

    fn document_replay(
        &self,
        project: Option<&str>,
        expected: Option<&str>,
        request: &str,
    ) -> Result<Option<Value>> {
        let Some(expected) = expected else {
            return Ok(None);
        };
        let Some((version, token)) = self.store.document_replay(project, expected, request)? else {
            return Ok(None);
        };
        let mut result = self.document_saved(project, version, &token, true);
        result["changed_fields"] = json!([]);
        result["unfilled_fields"] = json!([]);
        result["warnings"] = json!([]);
        let (kind, key, data) = match project {
            Some(id) => (
                Kind::Card,
                "card",
                self.store.card_version(id, version)?.map(|c| c.data),
            ),
            None => (
                Kind::Portfolio,
                "portfolio",
                self.store.portfolio_version(version)?.map(|p| p.data),
            ),
        };
        if let Some(data) = data {
            result["items"] = Value::Array(item_ids(kind, &data));
            result[key] = data;
        }
        Ok(Some(result))
    }

    pub fn card_history(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        self.context_with_token(Some(id), || {
            let versions = self.store.card_history(id)?.into_iter().enumerate().map(|(i,(at,data))|
                json!({"version":i+1,"created_at":at,"headline":data["headline"]})).collect::<Vec<_>>();
            Ok(json!({"project":id,"current_version":versions.len(),"versions":versions}))
        })
    }

    pub fn card_version(&self, id: &str, version: i64) -> Result<Value> {
        self.project(id)?;
        let card = self.store.card_version(id, version)?.ok_or_else(|| {
            VWatchError::new(
                "version_not_found",
                format!("Project '{id}' has no card version {version}. Nothing was changed."),
            )
            .hint(format!("Read vwatch card history {id} for saved versions."))
        })?;
        Ok(
            json!({"project":id,"version":card.version,"created_at":card.created_at,"card":card.data}),
        )
    }

    /// Restore appends the historical evidence too: restoring old wording does not
    /// assert that an agent has reconciled it with today's facts or owner decisions.
    pub fn restore_card(&self, id: &str, version: i64, token: Option<&str>) -> Result<Value> {
        self.project(id)?;
        let request = document_request(&json!(["restore", version]));
        if let Some(result) = self.document_replay(Some(id), token, &request)? {
            return Ok(extend(result, json!({"restored_from":version})));
        }
        if let Err(error) = self.check_context(Some(id), token, false) {
            if let Some(result) = self.document_replay(Some(id), token, &request)? {
                return Ok(extend(result, json!({"restored_from":version})));
            }
            return Err(error);
        }
        let current = self.store.latest_card(id)?;
        let card = self.store.card_version(id, version)?.ok_or_else(|| {
            VWatchError::new(
                "version_not_found",
                format!("Project '{id}' has no card version {version}. Nothing was changed."),
            )
            .hint(format!("Read vwatch card history {id} for saved versions."))
        })?;
        if current.as_ref().is_some_and(|c| c.version == version) {
            return Ok(extend(
                self.document_saved(Some(id), version, token.unwrap(), true),
                json!({"restored_from":version,"already_current":true,"undo":null}),
            ));
        }
        let parts = card.basis.unwrap_or(Value::Null);
        let basis = CardBasis {
            fingerprint: &card.fingerprint,
            head: card.head.as_deref(),
            ci: card.ci.as_deref(),
            parts: &parts,
            owner: card.owner_basis.as_ref(),
        };
        let write = self.store.add_card_guarded(
            id,
            &self.now(),
            &basis,
            &card.data,
            token.unwrap(),
            &request,
        )?;
        let (new_version, after, replayed) = Self::guarded_version(write, Kind::Card, Some(id))?;
        Ok(extend(
            self.document_saved(Some(id), new_version, &after, replayed),
            json!({"restored_from":version,"card":card.data}),
        ))
    }

    pub fn portfolio_history(&self) -> Result<Value> {
        self.context_with_token(None, || {
            let versions = self.store.portfolio_history()?.into_iter().map(|p|
                json!({"version":p.version,"created_at":p.created_at,"greeting":p.data["greeting"]})).collect::<Vec<_>>();
            Ok(json!({"current_version":versions.last().map(|p| p["version"].clone()).unwrap_or(json!(0)),"versions":versions}))
        })
    }

    pub fn portfolio_version(&self, version: i64) -> Result<Value> {
        let p = self.store.portfolio_version(version)?.ok_or_else(|| {
            VWatchError::new(
                "version_not_found",
                format!("There is no portfolio version {version}. Nothing was changed."),
            )
            .hint("Read vwatch portfolio history for saved versions.")
        })?;
        Ok(json!({"version":p.version,"created_at":p.created_at,"portfolio":p.data}))
    }

    pub fn restore_portfolio(&self, version: i64, token: Option<&str>) -> Result<Value> {
        let request = document_request(&json!(["restore", version]));
        if let Some(result) = self.document_replay(None, token, &request)? {
            return Ok(extend(result, json!({"restored_from":version})));
        }
        if let Err(error) = self.check_context(None, token, false) {
            if let Some(result) = self.document_replay(None, token, &request)? {
                return Ok(extend(result, json!({"restored_from":version})));
            }
            return Err(error);
        }
        let current = self.store.latest_portfolio()?;
        let p = self.store.portfolio_version(version)?.ok_or_else(|| {
            VWatchError::new(
                "version_not_found",
                format!("There is no portfolio version {version}. Nothing was changed."),
            )
            .hint("Read vwatch portfolio history for saved versions.")
        })?;
        if current.as_ref().is_some_and(|c| c.version == version) {
            return Ok(extend(
                self.document_saved(None, version, token.unwrap(), true),
                json!({"restored_from":version,"already_current":true,"undo":null}),
            ));
        }
        let basis = p.basis.unwrap_or(Value::Null);
        let write = self.store.add_portfolio_guarded(
            &self.now(),
            &p.data,
            &basis,
            token.unwrap(),
            &request,
        )?;
        let (new_version, after, replayed) = Self::guarded_version(write, Kind::Portfolio, None)?;
        Ok(extend(
            self.document_saved(None, new_version, &after, replayed),
            json!({"restored_from":version,"portfolio":p.data}),
        ))
    }

    pub fn card(&self, id: &str) -> Result<Value> {
        self.project(id)?;
        let card = self.store.latest_card(id)?.ok_or_else(|| {
            VWatchError::new("no_card", format!("Project '{id}' has no card yet."))
                .hint(format!("Run `vwatch context {id}` and write one."))
        })?;
        Ok(
            json!({"project": id, "version": card.version, "created_at": card.created_at, "card": card.data}),
        )
    }

    pub fn put_portfolio(&self, payload: Value, merge: bool, dry_run: bool) -> Result<Value> {
        let token = self.store.context_token(None)?;
        self.put_portfolio_guarded(payload, merge, dry_run, Some(&token))
    }

    pub fn put_portfolio_guarded(
        &self,
        payload: Value,
        merge: bool,
        dry_run: bool,
        context_token: Option<&str>,
    ) -> Result<Value> {
        let request = document_request(&json!(["put", merge, payload]));
        if !dry_run {
            if let Some(result) = self.document_replay(None, context_token, &request)? {
                return Ok(result);
            }
        }
        if let Err(error) = self.check_context(None, context_token, dry_run) {
            if !dry_run {
                if let Some(result) = self.document_replay(None, context_token, &request)? {
                    return Ok(result);
                }
            }
            return Err(error);
        }
        let previous = self.store.latest_portfolio()?;
        let saved = self.saved_items(None)?;
        let mut payload = payload;
        keep_items(Kind::Portfolio, &mut payload, &saved)?;
        let sent = sent_fields(&payload, merge);
        let kept = kept_fields(previous.as_ref().map(|p| &p.data), sent.as_deref());
        let payload = Self::merged(
            previous.as_ref().map(|p| &p.data),
            payload,
            merge,
            "portfolio brief",
        )?;
        let (clean, warnings) = cards::validate(Kind::Portfolio, &payload, &kept).map_err(|problems| {
            VWatchError::new("invalid_portfolio", format!("The brief was not saved: {}", mark_inherited(&problems, sent.as_deref(), "brief")))
                .hint(format!("Fix the listed fields and send it again. {} `vwatch schema portfolio` describes the fields.", if merge { "With --merge, fields marked (from the previous brief) were inherited: include corrected versions of those fields too." } else { "" }))
        })?;
        self.validate_item_ids(Kind::Portfolio, None, &clean)?;
        let known: Vec<String> = self.store.projects()?.into_iter().map(|p| p.id).collect();
        let items = |key: &str| clean[key].as_array().cloned().unwrap_or_default();
        let mut refs: Vec<String> = Vec::new();
        for item in items("focus").iter().chain(items("park_or_revive").iter()) {
            refs.extend(item["project"].as_str().map(str::to_owned));
        }
        for (list, key) in [
            ("patterns", "projects"),
            ("synergies", "projects"),
            ("idea_checks", "fits"),
        ] {
            for item in items(list) {
                refs.extend(
                    item[key]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned)),
                );
            }
        }
        let mut unknown: Vec<String> = refs.into_iter().filter(|r| !known.contains(r)).collect();
        unknown.sort();
        unknown.dedup();
        if !unknown.is_empty() {
            let mut known = known;
            known.sort();
            return Err(VWatchError::new(
                "unknown_project",
                format!(
                    "The brief names unknown project ids: {}.",
                    unknown.join(", ")
                ),
            )
            .hint(format!(
                "Use the ids from `vwatch portfolio context`: {}.",
                known.join(", ")
            )));
        }
        let ideas: Vec<i64> = self.store.ideas()?.iter().map(|i| i.id).collect();
        let strange: Vec<String> = items("idea_checks")
            .iter()
            .filter_map(|c| c["idea"].as_i64())
            .filter(|id| !ideas.contains(id))
            .map(|id| id.to_string())
            .collect();
        if !strange.is_empty() {
            return Err(VWatchError::new("unknown_idea", format!("The brief assesses ideas that are not parked: {}.", strange.join(", ")))
                .hint("Use the ids from `owner_ideas` in `vwatch portfolio context`; the owner may have removed an idea since."));
        }
        let mut warnings = warnings;
        warnings.extend(unknown_item_ids(Kind::Portfolio, &payload, &saved));
        let mut clean = clean;
        // a brief written later in the week keeps the week's review
        let (week, _, _) = self.review_week();
        if clean["week_review"].is_null() {
            if let Some(kept) = previous
                .as_ref()
                .map(|p| &p.data["week_review"])
                .filter(|r| r["week"] == week.as_str())
            {
                clean["week_review"] = kept.clone();
            }
        }
        let receipts = item_receipts(
            Kind::Portfolio,
            previous.as_ref().map(|p| &p.data),
            &saved,
            &clean,
        );
        if dry_run {
            self.check_context(None, context_token, true)?;
            return Ok(
                json!({"saved": false, "dry_run": true, "warnings": warnings, "items": dry_items(Kind::Portfolio, &payload, &receipts)}),
            );
        }
        let write = self.store.add_portfolio_guarded(
            &self.now(),
            &clean,
            &self.portfolio_basis()?,
            context_token.expect("checked before validation"),
            &request,
        )?;
        let (version, token, replayed) = Self::guarded_version(write, Kind::Portfolio, None)?;
        Ok(extend(
            self.document_saved(None, version, &token, replayed),
            json!({"portfolio":clean,"warnings": warnings, "items": receipts}),
        ))
    }

    /// Feedback is keyed globally by item id, so an id must remain in its original
    /// document and item kind even after a rated item leaves the current card.
    fn validate_item_ids(&self, kind: Kind, project: Option<&str>, doc: &Value) -> Result<()> {
        let conflicts = self.store.item_conflicts(kind, project, doc)?;
        if !conflicts.is_empty() {
            let (code, name) = match kind {
                Kind::Card => ("invalid_card", "card"),
                Kind::Portfolio => ("invalid_portfolio", "brief"),
            };
            return Err(VWatchError::new(code, format!("The {name} was not saved: {}", conflicts.join("; ")))
                .hint("Keep an existing id only for the same item in the same project card or brief. Omit id for a new item."));
        }
        Ok(())
    }
}
