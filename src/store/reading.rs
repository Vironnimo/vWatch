//! Persistence for reading.
use super::*;
/// A consistent copy of the inputs used by the owner's unread-change digest.
pub(crate) struct ChangesSnapshot {
    pub current: Value,
    pub baseline: Option<Value>,
    pub token: String,
}

fn changes_source(db: &Connection) -> Result<Value> {
    let mut statement = db.prepare(
        "SELECT p.id, p.name, p.status, c.id, c.data, c.owner_basis
         FROM projects p
         LEFT JOIN cards c ON c.id = (SELECT MAX(id) FROM cards WHERE project_id = p.id)
         WHERE p.removed_at IS NULL ORDER BY p.id",
    )?;
    let projects: Vec<Value> = statement
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
                "card_version": row.get::<_, Option<i64>>(3)?,
                "card": row.get::<_, Option<String>>(4)?.map(json),
                "owner_basis": row.get::<_, Option<String>>(5)?.map(json),
            }))
        })?
        .collect::<Result<_>>()?;
    let mut statement = db.prepare(
        "SELECT item_id, project_id, title, value, note, item FROM feedback ORDER BY item_id",
    )?;
    let feedback: Vec<Value> = statement
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?, "project": row.get::<_, Option<String>>(1)?,
                "title": row.get::<_, String>(2)?, "value": row.get::<_, String>(3)?,
                "note": row.get::<_, String>(4)?,
                "item": row.get::<_, Option<String>>(5)?.map(json),
            }))
        })?
        .collect::<Result<_>>()?;
    Ok(serde_json::json!({"projects": projects, "feedback": feedback}))
}

fn changes_token(snapshot: &Value) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(snapshot.to_string().as_bytes()))
}

impl Store {
    /// Reading or visiting never creates or moves the explicit read baseline.
    pub(crate) fn changes_snapshot(&self) -> Result<ChangesSnapshot> {
        self.with(|db| {
            let transaction =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Deferred)?;
            let current = changes_source(&transaction)?;
            let token = changes_token(&current);
            let baseline = transaction
                .query_row(
                    "SELECT value FROM prefs WHERE key = 'changes_read'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(json);
            transaction.commit()?;
            Ok(ChangesSnapshot {
                current,
                baseline,
                token,
            })
        })
    }

    /// The comparison and acknowledgement share a write transaction, so another
    /// process cannot sneak an unread card into the acknowledged baseline.
    pub(crate) fn acknowledge_changes(
        &self,
        token: &str,
        snapshot: &Value,
        at: &str,
    ) -> Result<bool> {
        self.with(|db| {
            let transaction =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let current = changes_source(&transaction)?;
            if changes_token(&current) != token {
                return Ok(false);
            }
            let baseline = serde_json::json!({"at": at, "snapshot": snapshot});
            transaction.execute(
                "INSERT INTO prefs(key, value) VALUES('changes_read', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [baseline.to_string()],
            )?;
            transaction.commit()?;
            Ok(true)
        })
    }
}
