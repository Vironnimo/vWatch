//! Persistence for owner state.
use super::*;
impl Store {
    /// Resolve omitted fields from the current reaction while holding the write transaction.
    /// A rejected resolution leaves both the reaction and its undo history unchanged.
    pub fn set_feedback(
        &self,
        f: &Feedback,
        resolve: impl FnOnce(Option<Feedback>) -> Option<(String, Option<String>)>,
    ) -> Result<Option<OwnerReceipt>> {
        let mut accepted = true;
        let receipt = self.owner_change(OwnerScope::Feedback { id: f.item_id.clone() }, &f.at, |db| {
            let current = db.query_row("SELECT * FROM feedback WHERE item_id=?1",[&f.item_id], |row| Ok(Feedback {
                item_id: row.get("item_id")?, project_id: row.get("project_id")?, title: row.get("title")?,
                value: row.get("value")?, at: row.get("at")?, note: row.get("note")?,
                item: row.get::<_,Option<String>>("item")?.map(json), snoozed_until: row.get("snoozed_until")?,
            })).optional()?;
            let Some((note, snoozed_until)) = resolve(current) else {
                accepted = false;
                return Ok(());
            };
            db.execute(
                "INSERT INTO feedback(item_id, project_id, title, value, at, note, item, snoozed_until) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(item_id) DO UPDATE SET project_id = excluded.project_id, title = excluded.title, value = excluded.value,
                 at = excluded.at, note = excluded.note, item = excluded.item, snoozed_until = excluded.snoozed_until",
                params![f.item_id, f.project_id, f.title, f.value, f.at, note, f.item.as_ref().map(Value::to_string), snoozed_until],
            ).map(drop)
        })?;
        Ok(accepted.then_some(receipt))
    }

    pub fn delete_feedback(&self, item_id: &str, at: &str) -> Result<OwnerReceipt> {
        self.owner_change(OwnerScope::Feedback { id: item_id.into() }, at, |db| {
            db.execute("DELETE FROM feedback WHERE item_id = ?1", [item_id])
                .map(drop)
        })
    }

    /// All reactions, newest first.
    pub fn feedback(&self) -> Result<Vec<Feedback>> {
        self.with(|db| {
            db.prepare("SELECT item_id, project_id, title, value, at, note, item, snoozed_until FROM feedback ORDER BY at DESC")?
                .query_map([], |r| {
                    Ok(Feedback {
                        item_id: r.get(0)?,
                        project_id: r.get(1)?,
                        title: r.get(2)?,
                        value: r.get(3)?,
                        at: r.get(4)?,
                        note: r.get(5)?,
                        item: r.get::<_, Option<String>>(6)?.map(json),
                        snoozed_until: r.get(7)?,
                    })
                })?
                .collect()
        })
    }

    pub fn warning_acknowledgements(&self, project: &str) -> Result<Value> {
        self.with(|db| {
            let entries: Vec<Value> = db.prepare("SELECT kind, note, basis FROM warning_acks WHERE project_id = ?1 ORDER BY kind")?
                .query_map([project], |r| Ok(serde_json::json!({
                    "key": r.get::<_, String>(0)?, "note": r.get::<_, String>(1)?,
                    "basis": json(r.get(2)?),
                })))?.collect::<Result<_>>()?;
            Ok(Value::Array(entries))
        })
    }

    pub fn set_warning_ack(
        &self,
        project: &str,
        key: &str,
        note: Option<&str>,
        basis: &Value,
        at: &str,
    ) -> Result<OwnerReceipt> {
        self.owner_change(OwnerScope::Warning { id:project.into(), key:key.into() },at, |db| {
            match note {
                Some(note) => db.execute("INSERT INTO warning_acks(project_id,kind,note,basis,at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(project_id,kind) DO UPDATE SET note=excluded.note,basis=excluded.basis,at=excluded.at",
                    params![project,key,note,basis.to_string(),at]),
                None => db.execute("DELETE FROM warning_acks WHERE project_id=?1 AND kind=?2", params![project,key]),
            }.map(drop)
        })
    }
    // ideas
    pub fn add_idea(&self, text: &str, at: &str) -> Result<(i64, bool)> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let existing = tx.query_row("SELECT id FROM ideas WHERE text = ?1 AND removed_at IS NULL ORDER BY id LIMIT 1", [text], |r| r.get(0)).optional()?;
            if let Some(id) = existing { return Ok((id, false)); }
            tx.execute("INSERT INTO ideas(text, added_at) VALUES(?1, ?2)", params![text, at])?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            Ok((id, true))
        })
    }

    pub fn idea(&self, id: i64) -> Result<Option<Value>> {
        self.with(|db| db.query_row("SELECT id,text,added_at,removed_at,revision FROM ideas WHERE id=?1", [id], |r| Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?, "text": r.get::<_, String>(1)?,
            "added_at": r.get::<_, String>(2)?, "removed_at": r.get::<_, Option<String>>(3)?, "revision":r.get::<_,i64>(4)?
        }))).optional())
    }

    pub fn set_idea_removed(&self, id: i64, at: Option<&str>) -> Result<bool> {
        self.with(|db| db.execute(
            "UPDATE ideas SET removed_at=?2 WHERE id=?1 AND ((?2 IS NULL AND removed_at IS NOT NULL) OR (?2 IS NOT NULL AND removed_at IS NULL))", params![id,at]
        ).map(|n| n > 0))
    }

    pub fn removed_ideas(&self) -> Result<Vec<Value>> {
        self.with(|db| db.prepare("SELECT id,text,added_at,removed_at FROM ideas WHERE removed_at IS NOT NULL ORDER BY id")?
            .query_map([], |r| Ok(serde_json::json!({"id":r.get::<_,i64>(0)?,"text":r.get::<_,String>(1)?,"added_at":r.get::<_,String>(2)?,"removed_at":r.get::<_,String>(3)?})))?.collect())
    }

    /// All parked ideas, oldest first.
    pub fn ideas(&self) -> Result<Vec<Idea>> {
        self.with(|db| {
            db.prepare("SELECT id, text, added_at,revision FROM ideas WHERE removed_at IS NULL ORDER BY id")?
                .query_map([], |r| {
                    Ok(Idea {
                        id: r.get(0)?,
                        text: r.get(1)?,
                        added_at: r.get(2)?,
                        revision: r.get(3)?,
                    })
                })?
                .collect()
        })
    }
    // prefs
    pub fn pref(&self, key: &str) -> Result<Option<Value>> {
        self.with(|db| {
            db.query_row("SELECT value FROM prefs WHERE key = ?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
        })
        .map(|v| v.map(json))
    }

    pub fn set_pref(&self, key: &str, value: &Value) -> Result<()> {
        self.with(|db| {
            db.execute("INSERT INTO prefs(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value.to_string()])
                .map(drop)
        })
    }
}
