//! Persistence for documents.
use super::*;
impl Store {
    pub fn add_card(&self, id: &str, at: &str, basis: &CardBasis, data: &Value) -> Result<i64> {
        self.with(|db| {
            db.execute(
                "INSERT INTO cards(project_id, created_at, fingerprint, head, ci, basis, data, owner_basis) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, at, basis.fingerprint, basis.head, basis.ci, basis.parts.to_string(), data.to_string(), basis.owner.map(Value::to_string)],
            )?;
            db.query_row("SELECT COUNT(*) FROM cards WHERE project_id = ?1", [id], |r| r.get(0))
        })
    }

    pub fn latest_card(&self, id: &str) -> Result<Option<StoredCard>> {
        self.with(|db| {
            db.query_row("SELECT (SELECT COUNT(*) FROM cards WHERE project_id = ?1), created_at, fingerprint, head, ci, basis, data, owner_basis FROM cards WHERE project_id = ?1 ORDER BY id DESC LIMIT 1", [id], |r| {
                Ok(StoredCard {
                    version: r.get(0)?,
                    created_at: r.get(1)?,
                    fingerprint: r.get(2)?,
                    head: r.get(3)?,
                    ci: r.get(4)?,
                    basis: r.get::<_, Option<String>>(5)?.map(json),
                    data: json(r.get(6)?),
                    owner_basis: r.get::<_, Option<String>>(7)?.map(json),
                })
            })
            .optional()
        })
    }

    /// Every card version of a project, oldest first, as (created_at, data).
    pub fn card_history(&self, id: &str) -> Result<Vec<(String, Value)>> {
        self.with(|db| {
            let mut stmt =
                db.prepare("SELECT created_at, data FROM cards WHERE project_id = ?1 ORDER BY id")?;
            let rows = stmt.query_map([id], |r| Ok((r.get::<_, String>(0)?, json(r.get(1)?))))?;
            rows.collect()
        })
    }

    /// The owner-facing lists of each document saved after row `after`, oldest first:
    /// (row id, saved at, one JSON value per named list). Only the named lists are parsed.
    pub fn document_lists_after(
        &self,
        project: Option<&str>,
        after: i64,
        lists: &[&str],
    ) -> Result<Vec<(i64, String, Vec<Value>)>> {
        let columns: String = lists
            .iter()
            .map(|l| format!(",json_extract(data,'$.{l}')"))
            .collect();
        let sql = match project {
            Some(_) => format!("SELECT id,created_at{columns} FROM cards WHERE project_id=?1 AND id>?2 ORDER BY id"),
            None => format!("SELECT id,created_at{columns} FROM portfolio WHERE ?1 IS NULL AND id>?2 ORDER BY id"),
        };
        self.with(|db| {
            db.prepare(&sql)?
                .query_map(params![project, after], |r| {
                    let mut parsed = Vec::new();
                    for i in 0..lists.len() {
                        let text: Option<String> = r.get(i + 2)?;
                        parsed.push(text.map(json).unwrap_or(Value::Null));
                    }
                    Ok((r.get(0)?, r.get(1)?, parsed))
                })?
                .collect()
        })
    }

    /// Risk-relevant measurements of a project, newest first: every saved facts
    /// sample plus the facts basis each card was written against.
    pub fn risk_observations(&self, project: &str) -> Result<Vec<RiskObservation>> {
        self.with(|db| {
            db.prepare("SELECT at,ok,files,added,ahead,ci FROM (
                    SELECT created_at AS at,json_extract(basis,'$.readable') AS ok,
                        json_extract(basis,'$.uncommitted[0]') AS files,json_extract(basis,'$.uncommitted[1]') AS added,
                        json_extract(basis,'$.unpushed') AS ahead,ci,id AS seq,0 AS src
                    FROM cards WHERE project_id=?1 AND basis IS NOT NULL
                    UNION ALL
                    SELECT at,json_extract(data,'$.ok'),json_extract(data,'$.dirty.files'),json_extract(data,'$.dirty.lines_added'),
                        json_extract(data,'$.ahead'),json_extract(data,'$.ci.state'),id,1
                    FROM fact_samples WHERE project_id=?1)
                ORDER BY at DESC,src DESC,seq DESC")?
                .query_map([project], |r| {
                    Ok(RiskObservation {
                        at: r.get(0)?,
                        ok: r.get::<_, Option<bool>>(1)?.unwrap_or(false),
                        dirty_files: r.get(2)?,
                        lines_added: r.get(3)?,
                        ahead: r.get(4)?,
                        ci: r.get(5)?,
                    })
                })?
                .collect()
        })
    }

    /// Saved card and brief versions an incremental reader has not seen yet: rows after
    /// `cards_after`/`briefs_after`, the rows it asks for again, and which rows are still
    /// the newest of their project or of the briefs. Only those can still be rewritten
    /// in place; older versions never change. One read transaction keeps it consistent.
    pub fn document_rows(
        &self,
        cards_after: i64,
        cards_again: &[i64],
        briefs_after: i64,
        briefs_again: &[i64],
    ) -> Result<DocumentRows> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Deferred)?;
            let row = |r: &Row| {
                Ok(DocumentRow {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    created_at: r.get(2)?,
                    data: r.get(3)?,
                })
            };
            let rows = |table: &str, scope: &str, after: i64, again: &[i64]| {
                let columns = format!("SELECT id, {scope}, created_at, data FROM {table}");
                let mut found = tx
                    .prepare(&format!("{columns} WHERE id > ?1 ORDER BY id"))?
                    .query_map([after], row)?
                    .collect::<Result<Vec<_>>>()?;
                let mut by_id = tx.prepare(&format!("{columns} WHERE id = ?1"))?;
                for id in again {
                    found.extend(by_id.query_row([id], row).optional()?);
                }
                Ok::<_, rusqlite::Error>(found)
            };
            let cards = rows("cards", "project_id", cards_after, cards_again)?;
            let briefs = rows("portfolio", "NULL", briefs_after, briefs_again)?;
            let latest_cards = tx
                .prepare("SELECT max(id) FROM cards GROUP BY project_id")?
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<i64>>>()?;
            let latest_brief = tx.query_row("SELECT max(id) FROM portfolio", [], |r| r.get(0))?;
            tx.commit()?;
            Ok(DocumentRows {
                cards,
                briefs,
                latest_cards,
                latest_brief,
            })
        })
    }

    /// One saved version's content by row id, from `cards` or from the briefs.
    pub fn document_data(&self, brief: bool, id: i64) -> Result<Option<Value>> {
        let table = if brief { "portfolio" } else { "cards" };
        self.with(|db| {
            db.query_row(
                &format!("SELECT data FROM {table} WHERE id = ?1"),
                [id],
                |r| r.get(0),
            )
            .optional()
            .map(|data| data.map(json))
        })
    }

    pub fn card_version(&self, id: &str, version: i64) -> Result<Option<StoredCard>> {
        if version < 1 {
            return Ok(None);
        }
        self.with(|db| db.query_row(
            "SELECT created_at,fingerprint,head,ci,basis,data,owner_basis FROM cards WHERE project_id=?1 ORDER BY id LIMIT 1 OFFSET ?2",
            params![id,version-1], |r| Ok(StoredCard {
                version,created_at:r.get(0)?,fingerprint:r.get(1)?,head:r.get(2)?,ci:r.get(3)?,
                basis:r.get::<_,Option<String>>(4)?.map(json),data:json(r.get(5)?),owner_basis:r.get::<_,Option<String>>(6)?.map(json)
            })).optional())
    }

    pub fn portfolio_history(&self) -> Result<Vec<StoredPortfolio>> {
        self.with(|db| {
            db.prepare("SELECT id,created_at,data,basis FROM portfolio ORDER BY id")?
                .query_map([], |r| {
                    Ok(StoredPortfolio {
                        version: r.get(0)?,
                        created_at: r.get(1)?,
                        data: json(r.get(2)?),
                        basis: r.get::<_, Option<String>>(3)?.map(json),
                    })
                })?
                .collect()
        })
    }
}

impl Store {
    pub fn portfolio_version(&self, version: i64) -> Result<Option<StoredPortfolio>> {
        self.with(|db| {
            db.query_row(
                "SELECT id,created_at,data,basis FROM portfolio WHERE id=?1",
                [version],
                |r| {
                    Ok(StoredPortfolio {
                        version: r.get(0)?,
                        created_at: r.get(1)?,
                        data: json(r.get(2)?),
                        basis: r.get::<_, Option<String>>(3)?.map(json),
                    })
                },
            )
            .optional()
        })
    }

    /// The card the owner could see at `at`: the newest one written by then.
    pub fn card_at(&self, id: &str, at: &str) -> Result<Option<Value>> {
        self.with(|db| {
            db.query_row("SELECT data FROM cards WHERE project_id = ?1 AND created_at <= ?2 ORDER BY id DESC LIMIT 1", params![id, at], |r| r.get(0))
                .optional()
                .map(|d| d.map(json))
        })
    }

    pub fn cards_after(&self, id: &str, at: &str) -> Result<i64> {
        self.with(|db| {
            db.query_row(
                "SELECT COUNT(*) FROM cards WHERE project_id = ?1 AND created_at > ?2",
                params![id, at],
                |r| r.get(0),
            )
        })
    }

    /// Replaces the stored content of a project's latest card, keeping its version and date.
    pub fn rewrite_latest_card(&self, id: &str, data: &Value) -> Result<()> {
        self.with(|db| {
            db.execute(
                "UPDATE cards SET data = ?2 WHERE id = (SELECT MAX(id) FROM cards WHERE project_id = ?1)",
                params![id, data.to_string()],
            )
            .map(drop)
        })
    }

    /// Normalize the version that was read only while it is still the current content.
    pub fn rewrite_card_if_current(
        &self,
        id: &str,
        version: i64,
        previous: &Value,
        data: &Value,
    ) -> Result<bool> {
        self.with(|db| {
            db.execute(
                "UPDATE cards SET data=?4 WHERE id=(SELECT MAX(id) FROM cards WHERE project_id=?1)
                 AND (SELECT COUNT(*) FROM cards WHERE project_id=?1)=?2 AND data=?3",
                params![id, version, previous.to_string(), data.to_string()],
            )
            .map(|count| count > 0)
        })
    }

    /// Replaces the stored content of the latest brief, keeping its version and date.
    pub fn rewrite_latest_portfolio(&self, data: &Value) -> Result<()> {
        self.with(|db| {
            db.execute(
                "UPDATE portfolio SET data = ?1 WHERE id = (SELECT MAX(id) FROM portfolio)",
                [data.to_string()],
            )
            .map(drop)
        })
    }

    /// A startup migration must not replace a brief another process saved or normalized.
    pub fn rewrite_portfolio_if_current(
        &self,
        version: i64,
        previous: &Value,
        data: &Value,
    ) -> Result<bool> {
        self.with(|db| {
            db.execute(
                "UPDATE portfolio SET data=?3 WHERE id=?1 AND id=(SELECT MAX(id) FROM portfolio) AND data=?2",
                params![version, previous.to_string(), data.to_string()],
            )
            .map(|count| count > 0)
        })
    }
    // portfolio
    pub fn add_portfolio(&self, at: &str, data: &Value, basis: &Value) -> Result<i64> {
        self.with(|db| {
            db.execute(
                "INSERT INTO portfolio(created_at, data, basis) VALUES(?1, ?2, ?3)",
                params![at, data.to_string(), basis.to_string()],
            )?;
            Ok(db.last_insert_rowid())
        })
    }

    pub fn latest_portfolio(&self) -> Result<Option<StoredPortfolio>> {
        self.newest_portfolio("basis")
    }

    /// The newest brief as shown, leaving its large basis unread (`None`).
    pub fn latest_portfolio_content(&self) -> Result<Option<StoredPortfolio>> {
        self.newest_portfolio("NULL")
    }

    fn newest_portfolio(&self, basis: &str) -> Result<Option<StoredPortfolio>> {
        self.with(|db| {
            db.query_row(
                &format!(
                    "SELECT id, created_at, data, {basis} FROM portfolio ORDER BY id DESC LIMIT 1"
                ),
                [],
                |r| {
                    Ok(StoredPortfolio {
                        version: r.get(0)?,
                        created_at: r.get(1)?,
                        data: json(r.get(2)?),
                        basis: r.get::<_, Option<String>>(3)?.map(json),
                    })
                },
            )
            .optional()
        })
    }
    // feedback
}

fn token_rows(db: &Connection, sql: &str, scope: Option<&str>) -> Result<Value> {
    let mut stmt = db.prepare(sql)?;
    let columns = stmt.column_count();
    let mut rows = stmt.query([scope])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut values = Vec::new();
        for i in 0..columns {
            values.push(match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(n) => Value::from(n),
                rusqlite::types::ValueRef::Real(n) => Value::from(n),
                rusqlite::types::ValueRef::Text(s) => {
                    Value::from(String::from_utf8_lossy(s).into_owned())
                }
                rusqlite::types::ValueRef::Blob(s) => Value::from(format!("{s:?}")),
            });
        }
        out.push(Value::Array(values));
    }
    Ok(Value::Array(out))
}

pub(super) fn context_token(db: &Connection, scope: Option<&str>) -> Result<String> {
    use sha1::{Digest, Sha1};
    let mut basis = serde_json::json!({"scope": scope});
    basis["projects"] = token_rows(db,
        "SELECT id,name,path,status,status_note,goal,goal_due,added_at,identity_revision FROM projects WHERE removed_at IS NULL AND ((?1 IS NULL AND status != 'archived') OR id = ?1) ORDER BY id", scope)?;
    basis["cards"] = token_rows(db,
        "SELECT c.project_id,c.id,c.data FROM cards c JOIN projects p ON p.id=c.project_id WHERE c.id=(SELECT MAX(id) FROM cards WHERE project_id=c.project_id) AND p.removed_at IS NULL AND ((?1 IS NULL AND p.status != 'archived') OR p.id=?1) ORDER BY p.id", scope)?;
    basis["facts"] = token_rows(db,
        "SELECT f.project_id,f.fingerprint,f.data FROM facts f JOIN projects p ON p.id=f.project_id WHERE p.removed_at IS NULL AND ((?1 IS NULL AND p.status != 'archived') OR p.id=?1) ORDER BY p.id", scope)?;
    basis["feedback"] = token_rows(db,
        "SELECT item_id,project_id,value,note,item,at,snoozed_until FROM feedback WHERE ?1 IS NULL OR project_id=?1 OR value IN ('up','later','down') ORDER BY item_id", scope)?;
    if scope.is_none() {
        basis["portfolio"] = token_rows(
            db,
            "SELECT id,data FROM portfolio WHERE ?1 IS NULL ORDER BY id DESC LIMIT 1",
            scope,
        )?;
        basis["ideas"] = token_rows(
            db,
            "SELECT id,text FROM ideas WHERE ?1 IS NULL AND removed_at IS NULL ORDER BY id",
            scope,
        )?;
        // The brief only needs to know whether the owner opened Watchtower since
        // its previous version. Later visits do not change that interpretation.
        basis["previous_portfolio_opened"] = Value::Bool(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM prefs WHERE key='last_visit' AND json_extract(value, '$') >= (SELECT created_at FROM portfolio ORDER BY id DESC LIMIT 1))",
            [],
            |row| row.get(0),
        )?);
    }
    // These owner-facing features are absent in databases from before their migration.
    for (table, query) in [
        ("tasks", "SELECT id,status,revision,data FROM tasks WHERE ?1 IS NULL OR EXISTS(SELECT 1 FROM json_each(data,'$.projects') WHERE value=?1) ORDER BY id"),
        ("idea_archives", "SELECT item_id,archived,revision FROM idea_archives WHERE ?1 IS NULL OR project_id=?1 OR project_id IS NULL ORDER BY item_id"),
        ("warning_acks", "SELECT project_id,kind,note,basis FROM warning_acks WHERE ?1 IS NULL OR project_id=?1 ORDER BY project_id,kind"),
    ] {
        let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |r| r.get(0))?;
        if exists { basis[table] = token_rows(db, query, scope)?; }
    }
    Ok(format!(
        "ct1-{:x}",
        Sha1::digest(basis.to_string().as_bytes())
    ))
}

fn item_conflicts(
    db: &Connection,
    kind: crate::cards::Kind,
    project: Option<&str>,
    doc: &Value,
) -> Result<Vec<String>> {
    use crate::cards::Kind;
    let mut owners: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    let mut add = |kind: Kind, project: Option<String>, doc: Value| {
        for (list, _, item_kind) in kind.items() {
            for item in doc[*list].as_array().into_iter().flatten() {
                if let Some(id) = item["id"].as_str() {
                    owners.push((
                        id.to_owned(),
                        project.clone(),
                        Some((*item_kind).to_owned()),
                    ));
                }
            }
        }
    };
    let mut stmt = db.prepare("SELECT project_id,data FROM cards c WHERE id=(SELECT MAX(id) FROM cards WHERE project_id=c.project_id)")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, json(r.get(1)?))))? {
        let (project, data) = row?;
        add(Kind::Card, Some(project), data);
    }
    if let Some(data) = db
        .query_row(
            "SELECT data FROM portfolio ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        add(Kind::Portfolio, None, json(data));
    }
    let mut stmt = db.prepare("SELECT id,NULLIF(scope,'@portfolio'),kind FROM proposals")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })? {
        owners.push(row?);
    }
    let mut stmt = db.prepare("SELECT item_id,project_id,item FROM feedback")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?.map(json),
        ))
    })? {
        let (id, project, item) = row?;
        let item_kind = item.as_ref().and_then(|i| i["kind"].as_str());
        let scope = match item_kind {
            Some("suggestion" | "question") | None => project,
            Some(_) => None,
        };
        owners.push((id, scope, item_kind.map(str::to_owned)));
    }
    let mut conflicts = Vec::new();
    for (list, _, item_kind) in kind.items() {
        for (index, item) in doc[*list].as_array().into_iter().flatten().enumerate() {
            let Some(id) = item["id"].as_str() else {
                continue;
            };
            if id.starts_with("idea:") {
                conflicts.push(format!(
                    "{list}[{index}].id: '{id}' uses the reserved owner-idea namespace"
                ));
            } else if owners.iter().any(|(other, scope, other_kind)| {
                other == id
                    && (scope.as_deref() != project
                        || other_kind.as_deref().is_some_and(|k| k != *item_kind))
            }) {
                conflicts.push(format!(
                    "{list}[{index}].id: '{id}' belongs to another card, brief or item kind"
                ));
            }
        }
    }
    Ok(conflicts)
}
/// Receipts and the written document share one transaction. A receipt is usable
/// only while every context input still equals the state immediately after that write.
fn replay_document(
    db: &Connection,
    scope: Option<&str>,
    expected: &str,
    request: &str,
) -> Result<Option<(i64, String)>> {
    let receipt = db.query_row(
        "SELECT version,after_token FROM document_requests WHERE scope=?1 AND expected=?2 AND request=?3",
        params![scope.unwrap_or("@portfolio"), expected, request],
        |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)),
    ).optional()?;
    match receipt {
        Some((version, token)) if context_token(db, scope)? == token => Ok(Some((version, token))),
        _ => Ok(None),
    }
}

fn record_document_request(
    db: &Connection,
    scope: Option<&str>,
    expected: &str,
    request: &str,
    version: i64,
) -> Result<String> {
    let token = context_token(db, scope)?;
    db.execute("INSERT OR REPLACE INTO document_requests(scope,expected,request,after_token,version) VALUES(?1,?2,?3,?4,?5)",
        params![scope.unwrap_or("@portfolio"), expected,request,token,version])?;
    Ok(token)
}

impl Store {
    /// The same ownership rule is used by previews and transaction-time guards.
    pub(crate) fn item_conflicts(
        &self,
        kind: crate::cards::Kind,
        project: Option<&str>,
        doc: &Value,
    ) -> Result<Vec<String>> {
        self.with(|db| item_conflicts(db, kind, project, doc))
    }

    pub fn document_replay(
        &self,
        scope: Option<&str>,
        expected: &str,
        request: &str,
    ) -> Result<Option<(i64, String)>> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let result = replay_document(&tx, scope, expected, request)?;
            tx.commit()?;
            Ok(result)
        })
    }

    pub fn context_token(&self, project: Option<&str>) -> Result<String> {
        self.with(|db| {
            let tx = db.unchecked_transaction()?;
            let token = context_token(&tx, project)?;
            tx.commit()?;
            Ok(token)
        })
    }

    pub fn add_card_guarded(
        &self,
        id: &str,
        at: &str,
        basis: &CardBasis,
        data: &Value,
        expected: &str,
        request: &str,
    ) -> Result<GuardedWrite> {
        self.with(|db| {
            let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            if context_token(&tx, Some(id))? != expected {
                return Ok(match replay_document(&tx, Some(id), expected, request)? {
                    Some((version, token)) => GuardedWrite::Replayed(version, token),
                    None => GuardedWrite::Conflict,
                });
            }
            let conflicts = item_conflicts(&tx, crate::cards::Kind::Card, Some(id), data)?;
            if !conflicts.is_empty() { return Ok(GuardedWrite::ItemConflict(conflicts)); }
            tx.execute("INSERT INTO cards(project_id,created_at,fingerprint,head,ci,basis,data,owner_basis) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![id,at,basis.fingerprint,basis.head,basis.ci,basis.parts.to_string(),data.to_string(),basis.owner.map(Value::to_string)])?;
            let version = tx.query_row("SELECT COUNT(*) FROM cards WHERE project_id=?1", [id], |r| r.get(0))?;
            let after = record_document_request(&tx, Some(id), expected, request, version)?;
            tx.commit()?;
            Ok(GuardedWrite::Saved(version, after))
        })
    }

    pub fn add_portfolio_guarded(
        &self,
        at: &str,
        data: &Value,
        basis: &Value,
        expected: &str,
        request: &str,
    ) -> Result<GuardedWrite> {
        self.with(|db| {
            let tx =
                rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)?;
            if context_token(&tx, None)? != expected {
                return Ok(match replay_document(&tx, None, expected, request)? {
                    Some((version, token)) => GuardedWrite::Replayed(version, token),
                    None => GuardedWrite::Conflict,
                });
            }
            let conflicts = item_conflicts(&tx, crate::cards::Kind::Portfolio, None, data)?;
            if !conflicts.is_empty() {
                return Ok(GuardedWrite::ItemConflict(conflicts));
            }
            tx.execute(
                "INSERT INTO portfolio(created_at,data,basis) VALUES(?1,?2,?3)",
                params![at, data.to_string(), basis.to_string()],
            )?;
            let version = tx.last_insert_rowid();
            let after = record_document_request(&tx, None, expected, request, version)?;
            tx.commit()?;
            Ok(GuardedWrite::Saved(version, after))
        })
    }
}
