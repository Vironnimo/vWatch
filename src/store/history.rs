//! Durable observations and content history, committed by the same SQLite write.
use super::*;
use serde_json::json;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS history_migrations(name TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS fact_payloads(id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL UNIQUE);
CREATE TABLE IF NOT EXISTS fact_samples(
 id INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL, at TEXT NOT NULL,
 fingerprint TEXT NOT NULL, path TEXT, identity_revision INTEGER, origin TEXT NOT NULL,
 data TEXT NOT NULL, code_payload INTEGER REFERENCES fact_payloads(id), activity_payload INTEGER REFERENCES fact_payloads(id));
CREATE INDEX IF NOT EXISTS samples_project_time ON fact_samples(project_id,at,id);
CREATE INDEX IF NOT EXISTS samples_activity ON fact_samples(project_id,activity_payload,id);
CREATE TABLE IF NOT EXISTS content_history(
 id INTEGER PRIMARY KEY AUTOINCREMENT, entity TEXT NOT NULL, entity_id TEXT NOT NULL,
 project_id TEXT, at TEXT, operation TEXT NOT NULL, origin TEXT NOT NULL,
 before_data TEXT, after_data TEXT);
CREATE INDEX IF NOT EXISTS history_project_time ON content_history(project_id,id);
CREATE INDEX IF NOT EXISTS history_entity ON content_history(entity,entity_id,id);
CREATE TABLE IF NOT EXISTS proposals(
 id TEXT PRIMARY KEY, scope TEXT NOT NULL, kind TEXT NOT NULL,
 first_seen TEXT NOT NULL, updated_at TEXT NOT NULL, in_report INTEGER NOT NULL,
 data TEXT NOT NULL);
";

fn proposal_sql(data: &str, scope: &str, at: &str, fields: &[(&str, &str)]) -> Vec<String> {
    let ids = fields.iter().map(|(field, _)| format!(
        "SELECT json_extract(value,'$.id') FROM json_each({data},'$.{field}') WHERE json_type(value,'$.id')='text'"
    )).collect::<Vec<_>>().join(" UNION ALL ");
    let mut sql = vec![format!("UPDATE proposals SET in_report=0,updated_at={at} WHERE scope={scope} AND in_report=1 AND id NOT IN ({ids})")];
    for (field, kind) in fields {
        sql.push(format!("INSERT INTO proposals(id,scope,kind,first_seen,updated_at,in_report,data)
            SELECT json_extract(value,'$.id'),{scope},'{kind}',{at},{at},1,json_set(value,'$.kind','{kind}')
            FROM json_each({data},'$.{field}') WHERE json_type(value,'$.id')='text'
            ON CONFLICT(id) DO UPDATE SET data=excluded.data,updated_at=excluded.updated_at,in_report=1
            WHERE proposals.scope=excluded.scope AND (proposals.data<>excluded.data OR proposals.in_report<>1)"));
    }
    sql
}

pub(super) fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(SCHEMA)?;
    let done: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM history_migrations WHERE name='durable-history-v1')",
        [],
        |r| r.get(0),
    )?;
    if done {
        return record_changed_samples(db);
    }
    // Preserve the one measurement old installations still possess; never invent older samples.
    db.execute_batch("INSERT OR IGNORE INTO fact_payloads(data) SELECT json_extract(data,'$.code') FROM facts WHERE json_type(data,'$.code')='object';
        INSERT OR IGNORE INTO fact_payloads(data) SELECT json_extract(data,'$.activity') FROM facts WHERE json_type(data,'$.activity')='object';
        INSERT INTO fact_samples(project_id,at,fingerprint,path,identity_revision,origin,data,code_payload,activity_payload)
        SELECT f.project_id,f.collected_at,f.fingerprint,p.path,p.identity_revision,'retained',json_remove(f.data,'$.code','$.activity'),
        (SELECT id FROM fact_payloads WHERE data=json_extract(f.data,'$.code')),
        (SELECT id FROM fact_payloads WHERE data=json_extract(f.data,'$.activity'))
        FROM facts f LEFT JOIN projects p ON p.id=f.project_id;")?;
    for (table, entity, scope, fields) in [
        (
            "cards",
            "card",
            "project_id",
            vec![("suggestions", "suggestion")],
        ),
        (
            "portfolio",
            "portfolio",
            "'@portfolio'",
            vec![("synergies", "synergy"), ("new_apps", "new_app")],
        ),
    ] {
        let mut previous = std::collections::HashMap::<String, String>::new();
        let rows = db
            .prepare(&format!(
                "SELECT {scope},created_at,data FROM {table} ORDER BY id"
            ))?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>>>()?;
        for (scope, at, data) in rows {
            db.execute("INSERT INTO content_history(entity,entity_id,project_id,at,operation,origin,before_data,after_data) VALUES(?1,?2,?3,?4,'saved','retained',?5,?6)",
                params![entity,scope,(entity=="card").then_some(&scope),at,previous.get(&scope),data])?;
            if serde_json::from_str::<Value>(&data).is_ok() {
                for sql in proposal_sql("?1", "?2", "?3", &fields) {
                    db.execute(&sql, params![data, scope, at])?;
                }
            }
            previous.insert(scope, data);
        }
        let project = if entity == "card" {
            "NEW.project_id"
        } else {
            "NULL"
        };
        let entity_id = if entity == "card" {
            "NEW.project_id"
        } else {
            "'@portfolio'"
        };
        let filter = if entity == "card" {
            "AND project_id=NEW.project_id"
        } else {
            ""
        };
        let mut body = format!("INSERT INTO content_history(entity,entity_id,project_id,at,operation,origin,before_data,after_data)
            VALUES('{entity}',{entity_id},{project},NEW.created_at,'saved','document',
            (SELECT data FROM {table} WHERE id<NEW.id {filter} ORDER BY id DESC LIMIT 1),NEW.data);");
        body.push_str(&proposal_sql("NEW.data", entity_id, "NEW.created_at", &fields).join(";"));
        db.execute_batch(&format!(
            "CREATE TRIGGER history_{table}_insert AFTER INSERT ON {table} BEGIN {body}; END;"
        ))?;
    }
    for action in ["INSERT", "UPDATE"] {
        db.execute_batch(&format!("CREATE TRIGGER samples_{} AFTER {action} ON facts BEGIN
            INSERT INTO fact_payloads(data) SELECT json_extract(NEW.data,'$.code') WHERE json_type(NEW.data,'$.code')='object' AND NOT EXISTS(SELECT 1 FROM fact_payloads WHERE data=json_extract(NEW.data,'$.code'));
            INSERT INTO fact_payloads(data) SELECT json_extract(NEW.data,'$.activity') WHERE json_type(NEW.data,'$.activity')='object' AND NOT EXISTS(SELECT 1 FROM fact_payloads WHERE data=json_extract(NEW.data,'$.activity'));
            INSERT INTO fact_samples(project_id,at,fingerprint,path,identity_revision,origin,data,code_payload,activity_payload)
            SELECT NEW.project_id,NEW.collected_at,NEW.fingerprint,p.path,p.identity_revision,'observed',json_remove(NEW.data,'$.code','$.activity'),
            (SELECT id FROM fact_payloads WHERE data=json_extract(NEW.data,'$.code')),
            (SELECT id FROM fact_payloads WHERE data=json_extract(NEW.data,'$.activity'))
            FROM (SELECT 1) LEFT JOIN projects p ON p.id=NEW.project_id; END;", action.to_lowercase()))?;
    }
    // Baselines retain existing content without claiming to know its previous edits or author.
    for (table, entity, id, project, columns, at) in [
        (
            "projects",
            "project",
            "id",
            "id",
            "id,name,path,status,status_note,goal,goal_due,removed_at",
            "NULL",
        ),
        (
            "ideas",
            "idea",
            "id",
            "NULL",
            "id,text,added_at,removed_at",
            "added_at",
        ),
        (
            "feedback",
            "feedback",
            "item_id",
            "project_id",
            "item_id,project_id,title,value,note,item,snoozed_until",
            "at",
        ),
        (
            "warning_acks",
            "warning",
            "project_id || ':' || kind",
            "project_id",
            "project_id,kind,note,basis",
            "at",
        ),
        ("tasks", "task", "id", "NULL", "data", "NULL"),
        (
            "proposals",
            "proposal",
            "id",
            "NULLIF(scope,'@portfolio')",
            "id,scope,kind,first_seen,updated_at,in_report,data",
            "updated_at",
        ),
    ] {
        let snapshot = |prefix: &str| -> String {
            if columns == "data" {
                return format!("{prefix}data");
            }
            format!(
                "json_object({})",
                columns
                    .split(',')
                    .map(|col| format!("'{col}',{prefix}{col}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let qualify = |expr: &str, prefix: &str| -> String {
            if expr == "NULL" {
                return "NULL".into();
            }
            if expr.starts_with("NULLIF") {
                return format!("NULLIF({prefix}scope,'@portfolio')");
            }
            if expr.contains(" || ") {
                return format!("{prefix}project_id || ':' || {prefix}kind");
            }
            format!("{prefix}{expr}")
        };
        db.execute_batch(&format!("INSERT INTO content_history(entity,entity_id,project_id,at,operation,origin,after_data)
            SELECT '{entity}',{id},{project},{at},'baseline','retained',{} FROM {table};", snapshot("")))?;
        for action in ["INSERT", "UPDATE", "DELETE"] {
            let prefix = if action == "DELETE" { "OLD." } else { "NEW." };
            let before = if action == "INSERT" {
                "NULL".into()
            } else {
                snapshot("OLD.")
            };
            let after = if action == "DELETE" {
                "NULL".into()
            } else {
                snapshot("NEW.")
            };
            let when = if action == "UPDATE" {
                format!("WHEN {before} IS NOT {after}")
            } else {
                String::new()
            };
            let time = if entity == "proposal" {
                format!("{prefix}updated_at")
            } else if entity == "task" {
                format!("COALESCE(json_extract({prefix}data,'$.updated_at'),json_extract({prefix}data,'$.created_at'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))")
            } else if entity == "idea" && action == "INSERT" {
                "NEW.added_at".into()
            } else {
                "strftime('%Y-%m-%dT%H:%M:%fZ','now')".into()
            };
            db.execute_batch(&format!("CREATE TRIGGER history_{table}_{} AFTER {action} ON {table} {when} BEGIN
                INSERT INTO content_history(entity,entity_id,project_id,at,operation,origin,before_data,after_data)
                VALUES('{entity}',{},{},{time},'{}','write',{before},{after}); END;",
                action.to_lowercase(),qualify(id,prefix),qualify(project,prefix),action.to_lowercase()))?;
        }
    }
    for table in ["content_history", "fact_samples", "fact_payloads"] {
        for action in ["UPDATE", "DELETE"] {
            db.execute_batch(&format!("CREATE TRIGGER immutable_{table}_{} BEFORE {action} ON {table} BEGIN SELECT RAISE(ABORT,'Saved history is immutable'); END;",action.to_lowercase()))?;
        }
    }
    db.execute(
        "INSERT INTO history_migrations VALUES('durable-history-v1')",
        [],
    )?;
    record_changed_samples(db)
}

/// A refresh that measures exactly the previous state adds no information. Keep the first
/// observation of each UTC day so durations stay measurable; saved samples stay untouched.
fn record_changed_samples(db: &Connection) -> Result<()> {
    let done: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM history_migrations WHERE name='changed-samples-v1')",
        [],
        |r| r.get(0),
    )?;
    if done {
        return Ok(());
    }
    db.execute_batch(
        "DROP TRIGGER IF EXISTS samples_insert; DROP TRIGGER IF EXISTS samples_update;",
    )?;
    for action in ["INSERT", "UPDATE"] {
        db.execute_batch(&format!("CREATE TRIGGER samples_{} AFTER {action} ON facts BEGIN
            INSERT INTO fact_payloads(data) SELECT json_extract(NEW.data,'$.code') WHERE json_type(NEW.data,'$.code')='object' AND NOT EXISTS(SELECT 1 FROM fact_payloads WHERE data=json_extract(NEW.data,'$.code'));
            INSERT INTO fact_payloads(data) SELECT json_extract(NEW.data,'$.activity') WHERE json_type(NEW.data,'$.activity')='object' AND NOT EXISTS(SELECT 1 FROM fact_payloads WHERE data=json_extract(NEW.data,'$.activity'));
            INSERT INTO fact_samples(project_id,at,fingerprint,path,identity_revision,origin,data,code_payload,activity_payload)
            SELECT n.project_id,n.at,n.fingerprint,n.path,n.identity_revision,'observed',n.data,n.code,n.activity
            FROM (SELECT NEW.project_id AS project_id,NEW.collected_at AS at,NEW.fingerprint AS fingerprint,p.path AS path,p.identity_revision AS identity_revision,
                json_remove(NEW.data,'$.code','$.activity') AS data,
                (SELECT id FROM fact_payloads WHERE data=json_extract(NEW.data,'$.code')) AS code,
                (SELECT id FROM fact_payloads WHERE data=json_extract(NEW.data,'$.activity')) AS activity
                FROM (SELECT 1) LEFT JOIN projects p ON p.id=NEW.project_id) n
            LEFT JOIN (SELECT * FROM fact_samples WHERE project_id=NEW.project_id ORDER BY at DESC,id DESC LIMIT 1) l ON 1
            WHERE l.id IS NULL OR substr(l.at,1,10)<substr(n.at,1,10) OR l.data IS NOT n.data
                OR l.code_payload IS NOT n.code OR l.activity_payload IS NOT n.activity OR l.fingerprint IS NOT n.fingerprint
                OR l.path IS NOT n.path OR l.identity_revision IS NOT n.identity_revision; END;", action.to_lowercase()))?;
    }
    db.execute(
        "INSERT INTO history_migrations VALUES('changed-samples-v1')",
        [],
    )?;
    Ok(())
}

/// The measured values statistics show for every observation. Lists, links and descriptive
/// text stay with the current and baseline views.
const SAMPLE_POINT: &str = "SELECT s.id,s.at,s.path,s.origin,
    json_object('ok',s.data->'$.ok','error',s.data->'$.error','branch',s.data->'$.branch','head',s.data->'$.head',
    'commits',json_object('total',s.data->'$.commits.total'),'dirty',s.data->'$.dirty','ahead',s.data->'$.ahead','behind',s.data->'$.behind',
    'unpushed_since',s.data->'$.unpushed_since','worktrees',json_remove(s.data->'$.worktrees','$.items'),'branches',s.data->'$.branches',
    'release',s.data->'$.release','ci',json_remove(s.data->'$.ci','$.workflows','$.url','$.title','$.repo'),
    'space',json_remove(s.data->'$.space','$.dirs'),
    'code',json_remove(c.data,'$.languages','$.largest_files','$.exclusions','$.percentage_basis'))
    FROM fact_samples s LEFT JOIN fact_payloads c ON c.id=s.code_payload";

fn sample_point(r: &Row) -> Result<Value> {
    Ok(
        json!({"id":r.get::<_,i64>(0)?,"at":r.get::<_,String>(1)?,"path":r.get::<_,Option<String>>(2)?,
        "origin":r.get::<_,String>(3)?,"facts":super::json(r.get(4)?)}),
    )
}

/// The observation a Git reconstruction run started with.
struct Run {
    id: i64,
    at: String,
    path: Option<String>,
    payload: i64,
}

/// One project's stored observations for a statistics period.
pub struct PeriodSamples {
    /// The last observation before the period.
    pub previous: Option<Value>,
    /// Observations in the period: the first of each UTC day, every change and the last one.
    pub samples: Vec<Value>,
    /// Frequently changed files of the latest complete Git reconstruction, with its range.
    pub hotspots: Option<Value>,
    /// Git reconstructions overlapping the period, oldest first, with days inside the period.
    pub reconstructions: Vec<Value>,
}

impl Store {
    /// One project's period through its time index: each UTC day's first observation and every
    /// change, without the large code lists or Git reconstructions of repeated observations.
    pub fn period_samples(&self, project: &str, from: &str, until: &str) -> Result<PeriodSamples> {
        self.with(|db| {
            let previous = db
                .prepare(&format!("{SAMPLE_POINT} WHERE s.project_id=?1 AND s.at<?2 ORDER BY s.at DESC,s.id DESC LIMIT 1"))?
                .query_row(params![project, from], sample_point)
                .optional()?;
            // A reconstruction made before the period ends before it; later ones can still cover it,
            // because saved Git windows stay valid after the rolling window has moved on. One ordered
            // pass compares neighbours; only kept observations are read as JSON.
            let mut points = Vec::<i64>::new();
            let mut point: Option<(String, String, Option<i64>, Option<String>)> = None;
            let mut last = None;
            let mut runs = Vec::<Run>::new();
            let mut run: Option<(String, i64, Option<String>)> = None;
            let mut scan = db.prepare("SELECT id,at,path,code_payload,activity_payload,data FROM fact_samples
                WHERE project_id=?1 AND at>=date(?2,'-1 day') ORDER BY at,id")?;
            let mut rows = scan.query(params![project, from])?;
            while let Some(r) = rows.next()? {
                let id: i64 = r.get(0)?;
                let at = r.get_ref(1)?.as_str()?;
                let path = r.get_ref(2)?.as_str_or_null()?;
                let code: Option<i64> = r.get(3)?;
                let data = r.get_ref(5)?.as_str()?;
                let day = at.get(..10).unwrap_or(at);
                if day >= from && day <= until {
                    let same = point.as_ref().is_some_and(|(d, data_before, code_before, path_before)| {
                        d == day && data_before == data && *code_before == code && path_before.as_deref() == path
                    });
                    if !same {
                        points.push(id);
                        point = Some((day.to_owned(), data.to_owned(), code, path.map(str::to_owned)));
                    }
                    last = Some(id);
                }
                if let Some(payload) = r.get::<_, Option<i64>>(4)? {
                    let same = run.as_ref().is_some_and(|(data_before, payload_before, path_before)| {
                        data_before == data && *payload_before == payload && path_before.as_deref() == path
                    });
                    if !same {
                        runs.push(Run { id, at: at.to_owned(), path: path.map(str::to_owned), payload });
                        run = Some((data.to_owned(), payload, path.map(str::to_owned)));
                    }
                }
            }
            drop(rows);
            if last.is_some() && points.last() != last.as_ref() {
                points.extend(last);
            }
            let mut read = db.prepare(&format!("{SAMPLE_POINT} WHERE s.id=?1"))?;
            let samples = points
                .iter()
                .map(|id| read.query_row([id], sample_point))
                .collect::<Result<Vec<_>>>()?;
            let hotspots = db.query_row("SELECT json_object('hotspots',a.data->'$.hotspots','from',a.data->'$.from','through',a.data->'$.through')
                FROM fact_samples s JOIN fact_payloads a ON a.id=s.activity_payload
                WHERE s.project_id=?1 AND a.data->>'$.ok'=1 ORDER BY s.at DESC,s.id DESC LIMIT 1", [project], |r| r.get(0))
                .optional()?.map(super::json);
            // Each reconstruction counts once per branch and folder, ordered by its latest run.
            let mut branch = db.prepare("SELECT data->>'$.branch' FROM fact_samples WHERE id=?1")?;
            let mut latest = std::collections::HashMap::<(i64, Option<String>, Option<String>), Run>::new();
            for run in runs {
                let branch: Option<String> = branch.query_row([run.id], |r| r.get(0))?;
                latest.insert((run.payload, run.path.clone(), branch), run);
            }
            let mut latest: Vec<_> = latest.into_iter().collect();
            latest.sort_by(|a, b| (&a.1.at, a.1.id).cmp(&(&b.1.at, b.1.id)));
            let mut window = db.prepare("SELECT json_object('from',a.data->'$.from','through',a.data->'$.through','days',
                (SELECT json_group_array(json(d.value)) FROM json_each(a.data,'$.days') d WHERE d.value->>'$.date' BETWEEN ?2 AND ?3))
                FROM fact_payloads a WHERE a.id=?1 AND a.data->>'$.ok'=1 AND a.data->>'$.from'<=?3 AND a.data->>'$.through'>=?2")?;
            let mut reconstructions = Vec::new();
            for ((payload, path, branch), run) in latest {
                if let Some(activity) = window.query_row(params![payload, from, until], |r| r.get::<_, String>(0)).optional()? {
                    reconstructions.push(json!({"at":run.at,"path":path,"branch":branch,"activity":super::json(activity)}));
                }
            }
            Ok(PeriodSamples { previous, samples, hotspots, reconstructions })
        })
    }

    /// One complete observation with its full code measurement, for the current and baseline views.
    pub fn fact_sample(&self, id: i64) -> Result<Option<Value>> {
        self.with(|db| {
            db.query_row("SELECT s.id,s.at,s.path,s.origin,s.data,c.data FROM fact_samples s
                LEFT JOIN fact_payloads c ON c.id=s.code_payload WHERE s.id=?1", [id], |r| {
                let mut facts = super::json(r.get(4)?);
                if let Some(code) = r.get::<_, Option<String>>(5)? {
                    facts["code"] = super::json(code);
                }
                Ok(json!({"id":r.get::<_,i64>(0)?,"at":r.get::<_,String>(1)?,"path":r.get::<_,Option<String>>(2)?,"origin":r.get::<_,String>(3)?,"facts":facts}))
            }).optional()
        })
    }
    pub fn fact_samples(
        &self,
        project: Option<&str>,
        since: Option<&str>,
        until: Option<&str>,
    ) -> Result<Vec<Value>> {
        self.with(|db| db.prepare("SELECT s.id,project_id,at,fingerprint,path,identity_revision,origin,s.data,c.data,a.data FROM fact_samples s LEFT JOIN fact_payloads c ON c.id=s.code_payload LEFT JOIN fact_payloads a ON a.id=s.activity_payload WHERE (?1 IS NULL OR project_id=?1) AND (?2 IS NULL OR substr(at,1,10)>=?2) AND (?3 IS NULL OR substr(at,1,10)<=?3) ORDER BY at,s.id")?
            .query_map(params![project,since,until], |r| {
                let mut facts=super::json(r.get(7)?);
                if let Some(code)=r.get::<_,Option<String>>(8)? { facts["code"]=super::json(code); }
                if let Some(activity)=r.get::<_,Option<String>>(9)? { facts["activity"]=super::json(activity); }
                Ok(json!({"id":r.get::<_,i64>(0)?,"project":r.get::<_,String>(1)?,"at":r.get::<_,String>(2)?,"fingerprint":r.get::<_,String>(3)?,"path":r.get::<_,Option<String>>(4)?,"identity_revision":r.get::<_,Option<i64>>(5)?,"origin":r.get::<_,String>(6)?,"facts":facts}))
            })?.collect())
    }

    pub fn content_history(
        &self,
        project: Option<&str>,
        entity: Option<&str>,
        id: Option<&str>,
        before: Option<i64>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        self.with(|db| db.prepare("SELECT id,entity,entity_id,project_id,at,operation,origin,before_data,after_data FROM content_history WHERE (?1 IS NULL OR project_id=?1 OR (entity='task' AND (EXISTS(SELECT 1 FROM json_each(after_data,'$.projects') WHERE value=?1) OR EXISTS(SELECT 1 FROM json_each(before_data,'$.projects') WHERE value=?1)) )) AND (?2 IS NULL OR entity=?2) AND (?3 IS NULL OR entity_id=?3) AND (?4 IS NULL OR id<?4) ORDER BY id DESC LIMIT ?5")?
            .query_map(params![project,entity,id,before,limit as i64], |r| Ok(json!({"id":r.get::<_,i64>(0)?,"entity":r.get::<_,String>(1)?,"entity_id":r.get::<_,String>(2)?,"project":r.get::<_,Option<String>>(3)?,"at":r.get::<_,Option<String>>(4)?,"operation":r.get::<_,String>(5)?,"origin":r.get::<_,String>(6)?,"before":r.get::<_,Option<String>>(7)?.map(super::json),"after":r.get::<_,Option<String>>(8)?.map(super::json)})))?.collect())
    }

    pub fn proposals(&self) -> Result<Vec<Value>> {
        self.with(|db| db.prepare("SELECT id,scope,kind,first_seen,updated_at,in_report,data FROM proposals ORDER BY first_seen DESC,id")?
            .query_map([], |r| Ok(json!({"id":r.get::<_,String>(0)?,"scope":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"first_seen":r.get::<_,String>(3)?,"updated_at":r.get::<_,String>(4)?,"in_report":r.get::<_,bool>(5)?,"item":super::json(r.get(6)?)})))?.collect())
    }
}
