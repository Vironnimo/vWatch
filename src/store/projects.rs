//! Persistence for projects.
use super::*;
impl Store {
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.with(|db| {
            db.prepare("SELECT * FROM projects WHERE removed_at IS NULL ORDER BY added_at, id")?
                .query_map([], project)?
                .collect()
        })
    }

    pub fn project(&self, id: &str) -> Result<Option<Project>> {
        self.with(|db| {
            db.query_row(
                "SELECT * FROM projects WHERE id = ?1 AND removed_at IS NULL",
                [id],
                project,
            )
            .optional()
        })
    }

    pub fn add_project(
        &self,
        id: &str,
        name: &str,
        path: &str,
        at: &str,
    ) -> Result<ProjectRegistration> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let projects = tx
                .prepare("SELECT * FROM projects")?
                .query_map([], project)?
                .collect::<Result<Vec<_>>>()?;
            if let Some(existing) = projects.iter().find(|p| same_path(&p.path, path)) {
                return Ok(
                    if existing.id == id && existing.name == name && existing.removed_at.is_none() {
                        ProjectRegistration::Existing
                    } else {
                        ProjectRegistration::PathTaken(existing.id.clone())
                    },
                );
            }
            if projects.iter().any(|p| p.id == id) {
                return Ok(ProjectRegistration::IdTaken);
            }
            tx.execute(
                "INSERT INTO projects(id,name,path,added_at) VALUES(?1,?2,?3,?4)",
                params![id, name, path, at],
            )?;
            tx.commit()?;
            Ok(ProjectRegistration::Added)
        })
    }

    /// Update identity fields and record their undo in the same transaction.
    pub fn update_project(
        &self,
        before: &Project,
        name: &str,
        path: &str,
        fields: Vec<String>,
        at: &str,
    ) -> Result<ProjectUpdate> {
        let mut outcome = None;
        let receipt = self.owner_change(OwnerScope::Project { id: before.id.clone(), fields }, at, |db| {
            let current = db.query_row("SELECT * FROM projects WHERE id = ?1 AND removed_at IS NULL", [&before.id], project).optional()?;
            if current.is_none_or(|p| p.name != before.name || p.path != before.path || p.identity_revision != before.identity_revision) {
                outcome = Some(ProjectUpdate::Conflict);
                return Ok(());
            }
            let projects = db.prepare("SELECT * FROM projects WHERE id != ?1")?
                .query_map([&before.id], project)?.collect::<Result<Vec<_>>>()?;
            if let Some(other) = projects.iter().find(|p| same_path(&p.path, path)) {
                outcome = Some(ProjectUpdate::PathTaken(other.id.clone()));
                return Ok(());
            }
            db.execute("UPDATE projects SET name = ?2, path = ?3, identity_revision = identity_revision + 1 WHERE id = ?1", params![before.id, name, path])?;
            if before.path != path {
                db.execute("DELETE FROM facts WHERE project_id = ?1", [&before.id])?;
            }
            Ok(())
        })?;
        Ok(outcome.unwrap_or(ProjectUpdate::Updated(Box::new(receipt))))
    }

    pub fn set_status(
        &self,
        id: &str,
        status: &str,
        note: Option<&str>,
        at: &str,
    ) -> Result<OwnerReceipt> {
        self.owner_change(OwnerScope::Project { id:id.into(), fields: std::iter::once("status".into()).chain(note.map(|_| "status_note".into())).collect() },at, |db| {
            db.execute("UPDATE projects SET status = ?1, status_note = COALESCE(?2,status_note), status_changed_at = ?3 WHERE id = ?4 AND removed_at IS NULL", params![status, note, at, id]).map(drop)
        })
    }

    pub fn set_goal(
        &self,
        id: &str,
        goal: &str,
        due: Option<&str>,
        at: &str,
    ) -> Result<OwnerReceipt> {
        self.owner_change(OwnerScope::Project { id:id.into(), fields: vec!["goal".into(),"goal_due".into()] },at, |db| {
            db.execute("UPDATE projects SET goal = ?2, goal_due = ?3, status_changed_at = ?4 WHERE id = ?1 AND removed_at IS NULL", params![id, goal, due, at]).map(drop)
        })
    }

    pub fn set_opened(&self, id: &str, at: &str, previous: Option<&str>) -> Result<()> {
        self.with(|db| {
            db.execute(
                "UPDATE projects SET opened_at = ?2, previous_opened_at = ?3 WHERE id = ?1",
                params![id, at, previous],
            )
            .map(drop)
        })
    }

    pub fn project_including_removed(&self, id: &str) -> Result<Option<Project>> {
        self.with(|db| {
            db.query_row("SELECT * FROM projects WHERE id = ?1", [id], project)
                .optional()
        })
    }

    pub fn all_projects(&self) -> Result<Vec<Project>> {
        self.with(|db| {
            db.prepare("SELECT * FROM projects ORDER BY added_at, id")?
                .query_map([], project)?
                .collect()
        })
    }

    /// Removal only changes visibility. All identity and history rows remain intact.
    pub fn set_project_removed(&self, id: &str, at: Option<&str>) -> Result<bool> {
        self.with(|db| db.execute(
            "UPDATE projects SET removed_at = ?2, identity_revision = identity_revision + 1 WHERE id = ?1 AND ((?2 IS NULL AND removed_at IS NOT NULL) OR (?2 IS NOT NULL AND removed_at IS NULL))",
            params![id, at],
        ).map(|n| n > 0))
    }
    // facts
    pub fn save_facts(&self, id: &str, at: &str, fingerprint: &str, data: &Value) -> Result<()> {
        self.with(|db| {
            db.execute(
                "INSERT INTO facts(project_id, collected_at, fingerprint, data) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(project_id) DO UPDATE SET collected_at = excluded.collected_at,
                 fingerprint = excluded.fingerprint, data = excluded.data, revision = facts.revision + 1",
                params![id, at, fingerprint, data.to_string()],
            )
            .map(drop)
        })
    }

    /// A collector started before a relink must not replace the new folder's facts.
    pub fn save_project_facts(
        &self,
        project: &Project,
        expected_revision: Option<i64>,
        at: &str,
        fingerprint: &str,
        data: &Value,
    ) -> Result<FactsSave> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            let matches: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND path=?2 AND identity_revision=?3 AND removed_at IS NULL)", params![project.id, project.path, project.identity_revision], |row| row.get(0))?;
            if !matches { return Ok(FactsSave::ProjectChanged); }
            let revision: Option<i64> = tx.query_row("SELECT revision FROM facts WHERE project_id=?1", [&project.id], |row| row.get(0)).optional()?;
            if revision != expected_revision { return Ok(FactsSave::FactsChanged); }
            tx.execute(
                "INSERT INTO facts(project_id, collected_at, fingerprint, data)
                 VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(project_id) DO UPDATE SET collected_at = excluded.collected_at,
                 fingerprint = excluded.fingerprint, data = excluded.data, revision = facts.revision + 1",
                params![project.id, at, fingerprint, data.to_string()],
            )?;
            tx.commit()?;
            Ok(FactsSave::Saved)
        })
    }

    pub fn facts(&self, id: &str) -> Result<Option<StoredFacts>> {
        self.with(|db| {
            db.query_row(
                "SELECT collected_at, fingerprint, data, revision FROM facts WHERE project_id = ?1",
                [id],
                |r| {
                    Ok(StoredFacts {
                        collected_at: r.get(0)?,
                        fingerprint: r.get(1)?,
                        data: crate::facts::measurement_quality(json(r.get(2)?)),
                        revision: r.get(3)?,
                    })
                },
            )
            .optional()
        })
    }
    // cards
}
