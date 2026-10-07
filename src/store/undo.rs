//! Atomic, field-scoped undo receipts for owner edits.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map};

#[derive(Clone, Serialize, Deserialize)]
pub(super) enum OwnerScope {
    Project { id: String, fields: Vec<String> },
    Feedback { id: String },
    Warning { id: String, key: String },
    Prefs { keys: Vec<String> },
}

impl OwnerScope {
    fn keys(&self) -> Vec<String> {
        match self {
            Self::Project { id, fields } => fields
                .iter()
                .map(|field| format!("project:{id}:{field}"))
                .collect(),
            Self::Feedback { id } => vec![format!("feedback:{id}")],
            Self::Warning { id, key } => vec![format!("warning:{id}:{key}")],
            Self::Prefs { keys } => keys.iter().map(|key| format!("pref:{key}")).collect(),
        }
    }

    fn metadata_key(&self) -> Option<String> {
        match self {
            Self::Project { id, fields }
                if fields
                    .iter()
                    .any(|key| matches!(key.as_str(), "goal" | "status")) =>
            {
                Some(format!("project:{id}:status_changed_at"))
            }
            _ => None,
        }
    }

    fn target(&self) -> Value {
        match self {
            Self::Project { id, fields } => json!({"project": id, "fields": fields}),
            Self::Feedback { id } => json!({"item_id": id}),
            Self::Warning { id, key } => json!({"project": id, "warning": key}),
            Self::Prefs { keys } => json!({"preferences": keys}),
        }
    }

    fn snapshot(&self, db: &Connection) -> Result<Value> {
        match self {
            Self::Project { id, fields } => {
                let project = db.query_row("SELECT * FROM projects WHERE id=?1 AND removed_at IS NULL", [id], project).optional()?;
                let Some(project) = project else { return Ok(Value::Null); };
                let value = serde_json::to_value(project).expect("project serialization");
                let data: Map<String, Value> = fields.iter().map(|field| (field.clone(), value[field].clone())).collect();
                Ok(json!({"value": data, "at": value["status_changed_at"]}))
            }
            Self::Feedback { id } => db.query_row("SELECT * FROM feedback WHERE item_id=?1", [id], |row| {
                Ok(json!({"value": {
                    "item_id": id, "project_id": row.get::<_, Option<String>>("project_id")?,
                    "title": row.get::<_, String>("title")?, "value": row.get::<_, String>("value")?,
                    "note": row.get::<_, String>("note")?, "item": row.get::<_, Option<String>>("item")?.map(super::json),
                    "snoozed_until": row.get::<_, Option<String>>("snoozed_until")?
                }, "at": row.get::<_, String>("at")?}))
            }).optional().map(|value| value.unwrap_or(Value::Null)),
            Self::Warning { id, key } => db.query_row("SELECT * FROM warning_acks WHERE project_id=?1 AND kind=?2", params![id,key], |row| {
                Ok(json!({"value": {"project": id, "key": key, "note": row.get::<_, String>("note")?, "basis": super::json(row.get("basis")?)}, "at": row.get::<_, String>("at")?}))
            }).optional().map(|value| value.unwrap_or(Value::Null)),
            Self::Prefs { keys } => {
                let mut values = Map::new();
                for key in keys {
                    let value = db.query_row("SELECT value FROM prefs WHERE key=?1", [key], |row| row.get::<_, String>(0)).optional()?;
                    // Distinguish an absent preference from a stored JSON null.
                    values.insert(key.clone(), value.map(|value| json!({"value": super::json(value)})).unwrap_or(Value::Null));
                }
                Ok(json!({"value": values}))
            }
        }
    }
}

pub enum OwnerUndo {
    Restored(Value),
    Missing,
    Conflict { target: Value, current: Value },
    PathTaken(String),
}

fn versions(db: &Connection, keys: &[String]) -> Result<Value> {
    let mut values = Map::new();
    for key in keys {
        let version = db
            .query_row(
                "SELECT version FROM owner_field_versions WHERE field=?1",
                [key],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        values.insert(key.clone(), json!(version));
    }
    Ok(Value::Object(values))
}

fn bump(db: &Connection, keys: &[String]) -> Result<Value> {
    for key in keys {
        db.execute("INSERT INTO owner_field_versions(field,version) VALUES(?1,1) ON CONFLICT(field) DO UPDATE SET version=version+1", [key])?;
    }
    versions(db, keys)
}

fn visible(snapshot: &Value) -> Value {
    if snapshot.is_null() {
        Value::Null
    } else {
        snapshot["value"].clone()
    }
}

fn receipt(
    scope: &OwnerScope,
    token: Option<&str>,
    before: &Value,
    after: &Value,
    changed: bool,
) -> Value {
    let mut value = json!({"changed": changed, "target": scope.target(), "previous": visible(before), "current": visible(after)});
    if let Some(token) = token {
        value["undo"] = json!(format!("watchtower undo {token}"));
    }
    value
}

impl Store {
    /// The edit, before/after snapshots and version counters commit together.
    pub(super) fn owner_change(
        &self,
        scope: OwnerScope,
        at: &str,
        change: impl FnOnce(&Connection) -> Result<()>,
    ) -> Result<Value> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let before = scope.snapshot(&tx)?;
            change(&tx)?;
            let after = scope.snapshot(&tx)?;
            if visible(&before) == visible(&after) {
                // Rolling back also preserves timestamps on a repeated call.
                let existing = tx.query_row("SELECT token,before_data,after_data,versions FROM owner_undo WHERE scope=?1 AND undone_at IS NULL ORDER BY rowid DESC LIMIT 1", [serde_json::to_string(&scope).unwrap()], |row| Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, String>(2)?,row.get::<_, String>(3)?))).optional()?;
                if let Some((token, old_before, old_after, old_versions)) = existing {
                    let old_after = super::json(old_after);
                    let old_versions = super::json(old_versions);
                    let current = versions(&tx, &scope.keys())?;
                    if visible(&old_after) == visible(&before) && current.as_object().unwrap().iter().all(|(key,value)| old_versions[key] == *value) {
                        return Ok(receipt(&scope, Some(&token), &super::json(old_before), &before, false));
                    }
                }
                return Ok(receipt(&scope, None, &before, &before, false));
            }
            let mut keys = scope.keys();
            keys.extend(scope.metadata_key());
            let after_versions = bump(&tx, &keys)?;
            let token: String = tx.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
            tx.execute("INSERT INTO owner_undo(token,scope,before_data,after_data,versions,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![token,serde_json::to_string(&scope).unwrap(),before.to_string(),after.to_string(),after_versions.to_string(),at])?;
            tx.commit()?;
            Ok(receipt(&scope, Some(&token), &before, &after, true))
        })
    }

    pub fn undo_owner_change(&self, token: &str, at: &str) -> Result<OwnerUndo> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let stored = tx.query_row("SELECT scope,before_data,after_data,versions,undone_at,undone_versions FROM owner_undo WHERE token=?1", [token], |row| Ok((row.get::<_, String>(0)?,row.get::<_, String>(1)?,row.get::<_, String>(2)?,row.get::<_, String>(3)?,row.get::<_, Option<String>>(4)?,row.get::<_, Option<String>>(5)?))).optional()?;
            let Some((scope, before, after, after_versions, undone_at, undone_versions)) = stored else { return Ok(OwnerUndo::Missing); };
            let scope: OwnerScope = serde_json::from_str(&scope).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(error)))?;
            let before = super::json(before);
            let after = super::json(after);
            let expected_versions = super::json(undone_versions.unwrap_or(after_versions));
            let current = scope.snapshot(&tx)?;
            let keys = scope.keys();
            let current_versions = versions(&tx, &keys)?;
            let expected = if undone_at.is_some() { &before } else { &after };
            if visible(&current) != visible(expected) || keys.iter().any(|key| current_versions[key] != expected_versions[key]) {
                return Ok(OwnerUndo::Conflict { target: scope.target(), current: visible(&current) });
            }
            if undone_at.is_some() {
                return Ok(OwnerUndo::Restored(json!({"restored": true,"changed":false,"target":scope.target(),"current":visible(&current)})));
            }
            if let OwnerScope::Project { id, fields } = &scope {
                if fields.iter().any(|field| field == "path") {
                    let path = before["value"]["path"].as_str().unwrap_or("");
                    let all = tx.prepare("SELECT * FROM projects WHERE id != ?1")?.query_map([id],project)?.collect::<Result<Vec<_>>>()?;
                    if let Some(other) = all.iter().find(|project| same_path(&project.path,path)) {
                        return Ok(OwnerUndo::PathTaken(other.id.clone()));
                    }
                }
            }
            scope.restore(&tx, &before, &after, &expected_versions)?;
            let restored = scope.snapshot(&tx)?;
            let mut all_keys = keys;
            all_keys.extend(scope.metadata_key());
            let restored_versions = bump(&tx, &all_keys)?;
            tx.execute("UPDATE owner_undo SET undone_at=?2,undone_versions=?3 WHERE token=?1", params![token,at,restored_versions.to_string()])?;
            tx.commit()?;
            Ok(OwnerUndo::Restored(json!({"restored":true,"changed":true,"target":scope.target(),"previous":visible(&after),"current":visible(&restored)})))
        })
    }

    pub fn set_owner_prefs(&self, updates: &[(&str, Value)], at: &str) -> Result<Value> {
        let mut keys = updates
            .iter()
            .map(|(key, _)| key.to_string())
            .collect::<Vec<_>>();
        keys.sort();
        keys.dedup();
        self.owner_change(OwnerScope::Prefs { keys },at, |db| {
            for (key,value) in updates {
                db.execute("INSERT INTO prefs(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value.to_string()])?;
            }
            Ok(())
        })
    }
}

impl OwnerScope {
    fn restore(
        &self,
        db: &Connection,
        before: &Value,
        after: &Value,
        expected_versions: &Value,
    ) -> Result<()> {
        let value = &before["value"];
        match self {
            Self::Project { id, fields } => {
                for field in fields {
                    // Scopes are generated internally, but keep SQL identifiers bounded.
                    if !["name", "path", "status", "status_note", "goal", "goal_due"]
                        .contains(&field.as_str())
                    {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    db.execute(
                        &format!("UPDATE projects SET {field}=?2 WHERE id=?1"),
                        params![id, value[field].as_str()],
                    )?;
                }
                if fields
                    .iter()
                    .any(|field| matches!(field.as_str(), "name" | "path"))
                {
                    db.execute(
                        "UPDATE projects SET identity_revision=identity_revision+1 WHERE id=?1",
                        [id],
                    )?;
                }
                if fields.iter().any(|field| field == "path") {
                    db.execute("DELETE FROM facts WHERE project_id=?1", [id])?;
                }
                if let Some(key) = self.metadata_key() {
                    if versions(db, std::slice::from_ref(&key))?[&key] == expected_versions[&key] {
                        db.execute("UPDATE projects SET status_changed_at=?2 WHERE id=?1 AND status_changed_at IS ?3",params![id,before["at"].as_str(),after["at"].as_str()])?;
                    }
                }
            }
            Self::Feedback { id } => {
                if before.is_null() {
                    db.execute("DELETE FROM feedback WHERE item_id=?1", [id])?;
                } else {
                    db.execute("INSERT INTO feedback(item_id,project_id,title,value,at,note,item,snoozed_until) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(item_id) DO UPDATE SET project_id=excluded.project_id,title=excluded.title,value=excluded.value,at=excluded.at,note=excluded.note,item=excluded.item,snoozed_until=excluded.snoozed_until", params![id,value["project_id"].as_str(),value["title"].as_str(),value["value"].as_str(),before["at"].as_str(),value["note"].as_str(),(!value["item"].is_null()).then(||value["item"].to_string()),value["snoozed_until"].as_str()])?;
                }
            }
            Self::Warning { id, key } => {
                if before.is_null() {
                    db.execute(
                        "DELETE FROM warning_acks WHERE project_id=?1 AND kind=?2",
                        params![id, key],
                    )?;
                } else {
                    db.execute("INSERT INTO warning_acks(project_id,kind,note,basis,at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project_id,kind) DO UPDATE SET note=excluded.note,basis=excluded.basis,at=excluded.at",params![id,key,value["note"].as_str(),value["basis"].to_string(),before["at"].as_str()])?;
                }
            }
            Self::Prefs { keys } => {
                for key in keys {
                    if value[key].is_null() {
                        db.execute("DELETE FROM prefs WHERE key=?1", [key])?;
                    } else {
                        db.execute("INSERT INTO prefs(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value[key]["value"].to_string()])?;
                    }
                }
            }
        }
        Ok(())
    }
}
