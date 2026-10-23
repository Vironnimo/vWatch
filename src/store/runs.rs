//! Persistence for runs.
use super::*;
/// A tracked agent run keeps its starting point and final evidence independently of later edits.
pub struct StoredAgentRun {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub initial: Value,
    pub failures: Value,
    pub result: Option<Value>,
}

fn agent_run(row: &Row) -> Result<StoredAgentRun> {
    Ok(StoredAgentRun {
        id: row.get("id")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        initial: json(row.get("initial")?),
        failures: json(row.get("failures")?),
        result: row.get::<_, Option<String>>("result")?.map(json),
    })
}

impl Store {
    pub fn start_agent_run(&self, at: &str, initial: &Value) -> Result<(i64, bool)> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let existing = tx
                .query_row(
                    "SELECT id FROM agent_runs WHERE finished_at IS NULL ORDER BY id DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                return Ok((id, false));
            }
            tx.execute(
                "INSERT INTO agent_runs(started_at, initial) VALUES(?1, ?2)",
                params![at, initial.to_string()],
            )?;
            let id = tx.last_insert_rowid();
            tx.commit()?;
            Ok((id, true))
        })
    }

    pub fn agent_run(&self, id: i64) -> Result<Option<StoredAgentRun>> {
        self.with(|db| {
            db.query_row("SELECT * FROM agent_runs WHERE id = ?1", [id], agent_run)
                .optional()
        })
    }

    pub fn agent_runs(&self) -> Result<Vec<StoredAgentRun>> {
        self.with(|db| {
            db.prepare("SELECT * FROM agent_runs ORDER BY id DESC LIMIT 20")?
                .query_map([], agent_run)?
                .collect()
        })
    }

    pub fn last_successful_agent_run(&self) -> Result<Option<StoredAgentRun>> {
        self.with(|db| db.query_row("SELECT * FROM agent_runs WHERE outcome = 'complete' ORDER BY finished_at DESC, id DESC LIMIT 1", [], agent_run).optional())
    }

    pub fn fail_agent_run(&self, id: i64, scope: &str, failure: &Value) -> Result<bool> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let mut failures = json(tx.query_row("SELECT failures FROM agent_runs WHERE id = ?1", [id], |row| row.get::<_, String>(0))?);
            if failures[scope]["error"] == failure["error"] { return Ok(true); }
            let version: i64 = if scope == "@portfolio" {
                tx.query_row("SELECT COALESCE(MAX(id), 0) FROM portfolio", [], |row| row.get(0))?
            } else {
                tx.query_row("SELECT COUNT(*) FROM cards WHERE project_id = ?1", [scope], |row| row.get(0))?
            };
            let mut failure = failure.clone();
            failure["version"] = Value::from(version);
            failures[scope] = failure;
            let changed = tx.execute("UPDATE agent_runs SET failures = ?2 WHERE id = ?1 AND (outcome IS NULL OR outcome != 'complete')", params![id, failures.to_string()])?;
            tx.commit()?;
            Ok(changed > 0)
        })
    }

    pub fn agent_run_token(&self, id: i64) -> Result<String> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let token = agent_run_token(&tx, id)?;
            tx.commit()?;
            Ok(token)
        })
    }

    pub fn finish_agent_run(
        &self,
        id: i64,
        at: &str,
        result: &Value,
        expected: &str,
    ) -> Result<bool> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            if agent_run_token(&tx, id)? != expected {
                return Ok(false);
            }
            tx.execute("UPDATE agent_runs SET finished_at = ?2, result = ?3, outcome = ?4 WHERE id = ?1 AND (outcome IS NULL OR outcome != 'complete')", params![id, at, result.to_string(), result["state"].as_str()])?;
            tx.commit()?;
            Ok(true)
        })
    }
}
/// Finish against the same project evidence and failures that were inspected.
pub(super) fn agent_run_token(db: &Connection, id: i64) -> Result<String> {
    use sha1::{Digest, Sha1};
    let run = db.query_row("SELECT * FROM agent_runs WHERE id = ?1", [id], agent_run)?;
    let basis = serde_json::json!([
        context_token(db, None)?,
        run.initial,
        run.failures,
        run.result,
        run.finished_at
    ]);
    Ok(format!(
        "run1-{:x}",
        Sha1::digest(basis.to_string().as_bytes())
    ))
}
// Context tokens cover the values agents read, independent of wall-clock precision.
