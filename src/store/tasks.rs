//! Task storage and atomic idea snapshots; transition policy stays in the service.
use super::*;
/// All source tasks and the archive row captured under one read transaction.
pub(crate) struct IdeaSnapshot {
    pub tasks: Vec<Value>,
    pub archive: Option<Value>,
    pub revision: String,
}
/// A service-prepared owner change, guarded against the entire source snapshot.
pub(crate) struct IdeaChange {
    pub project: Option<String>,
    pub title: String,
    pub archive: Option<bool>,
    pub task: Option<IdeaTaskChange>,
}

pub(crate) struct IdeaTaskChange {
    pub id: Option<i64>,
    pub status: String,
    pub source_key: String,
    pub data: Value,
}

fn tracked_task(row: &Row) -> Result<Value> {
    let mut task = json(row.get("data")?);
    task["id"] = serde_json::json!(row.get::<_, i64>("id")?);
    task["status"] = serde_json::json!(row.get::<_, String>("status")?);
    task["revision"] = serde_json::json!(row.get::<_, i64>("revision")?);
    Ok(task)
}

fn idea_snapshot(db: &Connection, id: &str) -> Result<IdeaSnapshot> {
    let tasks = db.prepare("SELECT id,status,revision,data FROM tasks WHERE json_extract(data,'$.source_id')=?1 ORDER BY id DESC")?
        .query_map([id], tracked_task)?.collect::<Result<Vec<_>>>()?;
    let archive = db.query_row("SELECT item_id,project_id,title,archived,revision,at FROM idea_archives WHERE item_id=?1", [id], |r| Ok(serde_json::json!({
        "item_id":r.get::<_,String>(0)?,"project_id":r.get::<_,Option<String>>(1)?,"title":r.get::<_,String>(2)?,
        "archived":r.get::<_,bool>(3)?,"revision":r.get::<_,i64>(4)?,"at":r.get::<_,String>(5)?
    }))).optional()?;
    use sha1::{Digest, Sha1};
    let revision = format!(
        "is1-{:x}",
        Sha1::digest(
            serde_json::json!([id, tasks, archive])
                .to_string()
                .as_bytes()
        )
    );
    Ok(IdeaSnapshot {
        tasks,
        archive,
        revision,
    })
}

impl Store {
    pub(crate) fn idea_snapshot(&self, id: &str) -> Result<IdeaSnapshot> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Deferred)?;
            let snapshot = idea_snapshot(&tx, id)?;
            tx.commit()?;
            Ok(snapshot)
        })
    }

    pub fn idea_archives(&self) -> Result<Vec<Value>> {
        self.with(|db| db.prepare("SELECT item_id,project_id,title,archived,revision FROM idea_archives ORDER BY item_id")?
            .query_map([], |r| Ok(serde_json::json!({"item_id":r.get::<_,String>(0)?,"project_id":r.get::<_,Option<String>>(1)?,"title":r.get::<_,String>(2)?,"archived":r.get::<_,bool>(3)?,"revision":r.get::<_,i64>(4)?})))?.collect())
    }

    /// Recheck all source tasks/archive and apply the prepared change atomically.
    pub(crate) fn set_idea_state(
        &self,
        id: &str,
        revision: &str,
        change: &IdeaChange,
        at: &str,
    ) -> Result<Option<(bool, IdeaSnapshot)>> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let before=idea_snapshot(&tx,id)?;
            if before.revision != revision { return Ok(None); }
            if let Some(archived) = change.archive {
                tx.execute("INSERT INTO idea_archives(item_id,project_id,title,archived,revision,at) VALUES(?1,?2,?3,?4,1,?5) ON CONFLICT(item_id) DO UPDATE SET archived=excluded.archived,revision=revision+1,at=excluded.at", params![id,change.project,change.title,archived,at])?;
                let own=id.strip_prefix("idea:");
                let was_archived=before.archive.as_ref().is_some_and(|a|a["archived"]==true);
                tx.execute("INSERT INTO content_history(entity,entity_id,project_id,at,operation,origin,before_data,after_data) VALUES(?1,?2,?3,?4,'update','write',?5,?6)",params![if own.is_some(){"idea"}else{"proposal"},own.unwrap_or(id),change.project,at,serde_json::json!({"title":change.title,"archived":was_archived}).to_string(),serde_json::json!({"title":change.title,"archived":archived}).to_string()])?;
            }
            if let Some(task)=&change.task {
                if let Some(task_id)=task.id {
                    tx.execute("UPDATE tasks SET status=?2,revision=revision+1,data=?3 WHERE id=?1", params![task_id,task.status,task.data.to_string()])?;
                } else {
                    tx.execute("INSERT INTO tasks(status,source_key,data) VALUES(?1,?2,?3)",params![task.status,task.source_key,task.data.to_string()])?;
                }
            }
            let after=idea_snapshot(&tx,id)?;
            tx.commit()?;
            Ok(Some((change.archive.is_some() || change.task.is_some(),after)))
        })
    }
}

impl Store {
    pub fn tasks(&self) -> Result<Vec<Value>> {
        self.with(|db| {
            db.prepare("SELECT id, status, revision, data FROM tasks ORDER BY id DESC")?
                .query_map([], tracked_task)?
                .collect()
        })
    }

    pub fn task(&self, id: i64) -> Result<Option<Value>> {
        self.with(|db| {
            db.query_row(
                "SELECT id, status, revision, data FROM tasks WHERE id = ?1",
                [id],
                tracked_task,
            )
            .optional()
        })
    }

    /// The partial unique index makes repeated assignment atomic across processes.
    pub fn create_tracked_task(
        &self,
        source_key: Option<&str>,
        data: &Value,
    ) -> Result<(Value, bool)> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let created = tx.execute(
                "INSERT INTO tasks(status, source_key, data) VALUES('assigned', ?1, ?2) ON CONFLICT DO NOTHING",
                params![source_key, data.to_string()],
            )? > 0;
            let task = if created {
                tx.query_row(
                    "SELECT id, status, revision, data FROM tasks WHERE id = ?1",
                    [tx.last_insert_rowid()], tracked_task,
                )?
            } else {
                tx.query_row(
                    "SELECT id, status, revision, data FROM tasks WHERE source_key = ?1 AND status IN ('assigned', 'blocked', 'review')",
                    [source_key], tracked_task,
                )?
            };
            tx.commit()?;
            Ok((task, created))
        })
    }

    /// A stale agent report must not overwrite a simultaneous owner decision.
    pub fn replace_task(&self, id: i64, revision: i64, status: &str, data: &Value) -> Result<bool> {
        self.with(|db| {
            db.execute(
                "UPDATE tasks SET status = ?1, revision = revision + 1, data = ?2 WHERE id = ?3 AND revision = ?4",
                params![status, data.to_string(), id, revision],
            ).map(|count| count > 0)
        })
    }
}
