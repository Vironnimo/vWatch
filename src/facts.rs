//! Exact repository facts, computed from git without any model.
//!
//! Everything here is read-only: no fetch, no checkout, nothing that changes a
//! repository. The GitHub CI lookup is optional and uses the `gh` CLI when it is
//! installed and the repository has a github.com remote.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

mod process;
mod statistics;
mod working_tree;

use process::{git, run};
use working_tree::{branches, collisions, dirty, night, worktrees};

pub const WEEKS: usize = 26;
const GH_TIMEOUT: Duration = Duration::from_secs(20);
/// Folders a build or package manager recreates; their size is space the owner could free.
const REBUILDABLE: &[&str] = &[
    "target",
    "node_modules",
    ".venv",
    "venv",
    "build",
    "dist",
    ".next",
    ".gradle",
    ".dart_tool",
];
const DOC_FILES: &[&str] = &[
    "README.md",
    "AGENTS.md",
    "TODO.md",
    "ROADMAP.md",
    "VISION.md",
    "IDEAS.md",
    "CHANGELOG.md",
    ".vorch/PROJECT.md",
    ".vorch/FLAGGED.md",
];

fn iso(t: SystemTime) -> String {
    DateTime::<Utc>::from(t)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn monday(d: NaiveDate) -> NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}

struct Commit {
    parents: Vec<String>,
    /// Committer time in seconds since the epoch.
    time: i64,
    /// Committer date in the committer's time zone.
    date: Option<NaiveDate>,
}

/// HEAD's whole ancestry from one `git log`, shared by the commit counts and the activity.
struct History(HashMap<String, Commit>);

impl History {
    fn read(repo: &Path, head: &str) -> Option<History> {
        let out = git(repo, &["log", "--format=%H%x09%P%x09%ct%x09%cs", head])?;
        let mut commits = HashMap::new();
        for line in out.lines() {
            let mut parts = line.split('\t');
            let (Some(hash), Some(parents), Some(time), Some(date)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            commits.insert(
                hash.to_owned(),
                Commit {
                    parents: parents.split_whitespace().map(str::to_owned).collect(),
                    time: time.parse().unwrap_or(i64::MIN),
                    date: NaiveDate::parse_from_str(date, "%Y-%m-%d").ok(),
                },
            );
        }
        Some(History(commits))
    }

    /// The commits reachable from `rev`, itself included; None when it is not in this history.
    fn ancestry(&self, rev: &str) -> Option<HashSet<&str>> {
        let (start, _) = self.0.get_key_value(rev)?;
        let mut seen = HashSet::from([start.as_str()]);
        let mut open = vec![start.as_str()];
        while let Some(hash) = open.pop() {
            for parent in &self.0[hash].parents {
                if let Some((key, _)) = self.0.get_key_value(parent) {
                    if seen.insert(key.as_str()) {
                        open.push(key.as_str());
                    }
                }
            }
        }
        Some(seen)
    }
}

fn commits(history: Option<&History>, today: NaiveDate) -> Value {
    let dates: Vec<NaiveDate> = history
        .into_iter()
        .flat_map(|h| h.0.values().filter_map(|c| c.date))
        .collect();
    let mut weekly = vec![0u32; WEEKS];
    let this_monday = monday(today);
    for d in dates.iter().filter(|d| **d <= today) {
        let weeks_ago = (this_monday - monday(*d)).num_days() / 7;
        if (0..WEEKS as i64).contains(&weeks_ago) {
            weekly[WEEKS - 1 - weeks_ago as usize] += 1;
        }
    }
    let within = |days: i64| {
        dates
            .iter()
            .filter(|d| (0..days).contains(&(today - **d).num_days()))
            .count()
    };
    json!({
        "total": dates.len(),
        "first": dates.iter().min().map(|d| d.to_string()),
        "last": dates.iter().max().map(|d| d.to_string()),
        "last_7d": within(7),
        "last_30d": within(30),
        "last_90d": within(90),
        "weekly": weekly,
    })
}

fn empty_commits() -> Value {
    json!({"total": 0, "first": null, "last": null, "last_7d": 0, "last_30d": 0, "last_90d": 0, "weekly": vec![0; WEEKS]})
}

/// Remove `user:token@` from URLs so credentials never reach the database.
fn strip_credentials(url: &str) -> String {
    if let Some(start) = url.find("//") {
        let rest = &url[start + 2..];
        let end = rest.find('/').unwrap_or(rest.len());
        if let Some(at) = rest[..end].rfind('@') {
            return format!("{}{}", &url[..start + 2], &rest[at + 1..]);
        }
    }
    url.to_owned()
}

fn remotes(repo: &Path) -> Vec<(String, String)> {
    let mut seen: Vec<(String, String)> = Vec::new();
    for line in git(repo, &["remote", "-v"]).unwrap_or_default().lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && !seen.iter().any(|(n, _)| n == parts[0]) {
            seen.push((parts[0].to_owned(), strip_credentials(parts[1])));
        }
    }
    seen
}

/// (upstream, ahead, behind, date of the oldest unpushed commit)
fn upstream(repo: &Path) -> (Value, Value, Value, Value) {
    let Some(up) = git(
        repo,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    ) else {
        return (Value::Null, Value::Null, Value::Null, Value::Null);
    };
    let counts = git(
        repo,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    )
    .unwrap_or_default();
    let nums: Vec<u64> = counts
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    let (behind, ahead) = if nums.len() == 2 {
        (json!(nums[0]), json!(nums[1]))
    } else {
        (Value::Null, Value::Null)
    };
    let oldest = git(
        repo,
        &["log", "--reverse", "--format=%cs", "@{upstream}..HEAD"],
    )
    .and_then(|out| out.lines().next().map(str::to_owned));
    (json!(up.trim()), ahead, behind, json!(oldest))
}

/// The newest tag reachable from HEAD, its date and how many commits came after it.
fn release(repo: &Path) -> Value {
    let Some(tag) = git(repo, &["describe", "--tags", "--abbrev=0"])
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
    else {
        return Value::Null;
    };
    let date = git(repo, &["log", "-1", "--format=%cs", &tag]).map(|d| d.trim().to_owned());
    let since = git(repo, &["rev-list", "--count", &format!("{tag}..HEAD")])
        .and_then(|n| n.trim().parse::<u64>().ok());
    json!({"tag": tag, "date": date, "commits_since": since})
}

fn dir_size(
    dir: &Path,
    read_dir: &impl Fn(&Path) -> std::io::Result<std::fs::ReadDir>,
) -> std::io::Result<u64> {
    let mut bytes = 0;
    for entry in read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            continue;
        }
        bytes += if kind.is_dir() {
            dir_size(&entry.path(), read_dir)?
        } else if kind.is_file() {
            entry.metadata()?.len()
        } else {
            0
        };
    }
    Ok(bytes)
}

/// Space in folders a build or package manager recreates, at the top of the repository and one level
/// below. Walking them is slow, so Watchtower measures this once a day.
pub fn space(path: &str, today: NaiveDate) -> Value {
    space_with(path, today, &|dir| std::fs::read_dir(dir))
}

fn space_with(
    path: &str,
    today: NaiveDate,
    read_dir: &impl Fn(&Path) -> std::io::Result<std::fs::ReadDir>,
) -> Value {
    let repo = Path::new(path);
    let mut found: Vec<Value> = Vec::new();
    let issue = |path: &str, error: &std::io::Error| {
        json!({"path":path,"reason":match error.kind() {
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            std::io::ErrorKind::NotFound => "not_found",
            _ => "read_failed",
        }})
    };
    let mut look = |dir: &Path, prefix: &str| {
        let mut errors = Vec::new();
        for name in REBUILDABLE {
            let candidate = dir.join(name);
            let relative = format!("{prefix}{name}");
            let measured = match std::fs::symlink_metadata(&candidate) {
                Ok(meta) if meta.is_dir() && !meta.is_symlink() => dir_size(&candidate, read_dir),
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => Err(error),
            };
            match measured {
                Ok(bytes) if bytes > 0 => {
                    found.push(json!({"path":relative,"bytes":bytes,"ok":true}))
                }
                Ok(_) => {}
                Err(error) => {
                    errors.push(issue(&relative, &error));
                    found.push(json!({"path":relative,"bytes":null,"ok":false}));
                }
            }
        }
        errors
    };
    let mut errors = look(repo, "");
    match read_dir(repo) {
        Err(error) => errors.push(issue(".", &error)),
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Err(error) => errors.push(issue(".", &error)),
                    Ok(entry) => {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        match entry.file_type() {
                            Ok(kind)
                                if kind.is_dir()
                                    && !kind.is_symlink()
                                    && !name.starts_with('.')
                                    && !REBUILDABLE.contains(&name.as_str()) =>
                            {
                                errors.extend(look(&entry.path(), &format!("{name}/")))
                            }
                            Err(error) => errors.push(issue(&name, &error)),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f["bytes"].as_u64()));
    let total: u64 = found.iter().filter_map(|f| f["bytes"].as_u64()).sum();
    json!({"measured_on": today.to_string(), "ok":errors.is_empty(), "partial":!errors.is_empty(), "errors":errors,
        "total_bytes":errors.is_empty().then_some(total), "dirs":found})
}

/// Older observations have no evidence that these submeasurements completed. Adapt reads without
/// rewriting retained history or treating their previous partial-looking totals as measured zero.
pub(crate) fn measurement_quality(mut facts: Value) -> Value {
    if let Some(dirty) = facts.get_mut("dirty").filter(|value| value.is_object()) {
        if dirty["line_counts"]["ok"] != true {
            dirty["lines_added"] = Value::Null;
            dirty["lines_removed"] = Value::Null;
            if !dirty["line_counts"].is_object() {
                dirty["line_counts"] =
                    json!({"ok":false,"partial":false,"errors":["not_measured"]});
            }
        }
    }
    if let Some(space) = facts.get_mut("space").filter(|value| value.is_object()) {
        if space["ok"] != true {
            space["total_bytes"] = Value::Null;
            if !space["ok"].is_boolean() {
                space["ok"] = json!(false);
                space["partial"] = json!(false);
                space["errors"] = json!([{"path":".","reason":"not_measured"}]);
            }
        }
        if let Some(dirs) = space["dirs"].as_array_mut() {
            for dir in dirs {
                if dir["ok"] != true {
                    dir["bytes"] = Value::Null;
                    dir["ok"] = json!(false);
                }
            }
        }
    }
    facts
}

/// "owner/repo" of a github.com remote URL (https or ssh form).
fn github_slug(url: &str) -> Option<String> {
    let (host, rest) = if let Some((scheme, rest)) = url.split_once("://") {
        if !matches!(scheme, "https" | "http" | "ssh" | "git") {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?.split(':').next()?;
        (host, path)
    } else {
        let (authority, path) = url.split_once(':')?;
        (authority.rsplit('@').next()?, path)
    };
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty() && !repo.is_empty())
        .then(|| format!("{owner}/{repo}"))
}

/// CI state of the default branch (the remote's HEAD, else the checked-out branch):
/// the newest run of every workflow counts, and the worst of them decides.
fn ci(repo: &Path, remotes: &[(String, String)], branch: Option<&str>) -> Value {
    let mut ordered: Vec<&(String, String)> = remotes.iter().collect();
    ordered.sort_by_key(|(name, _)| name != "origin");
    let Some((remote, slug)) = ordered
        .iter()
        .find_map(|(name, url)| github_slug(url).map(|s| (name.as_str(), s)))
    else {
        return json!({"state": "none"});
    };
    let default_branch = git(
        repo,
        &[
            "symbolic-ref",
            "--short",
            &format!("refs/remotes/{remote}/HEAD"),
        ],
    )
    .and_then(|r| {
        r.trim()
            .strip_prefix(&format!("{remote}/"))
            .map(str::to_owned)
    })
    .or_else(|| branch.filter(|b| *b != "HEAD").map(str::to_owned));
    let mut args = vec![
        "run",
        "list",
        "-R",
        slug.as_str(),
        "-L",
        "40",
        "--json",
        "status,conclusion,createdAt,displayTitle,workflowName,url,event",
    ];
    if let Some(b) = default_branch.as_deref() {
        args.splice(4..4, ["-b", b]);
    }
    let (ok, out) = run("gh", &args, repo, GH_TIMEOUT);
    if !ok {
        let error: String = out.trim().chars().take(200).collect();
        return json!({"state": "unknown", "repo": slug, "error": error});
    }
    let Ok(Value::Array(runs)) = serde_json::from_str::<Value>(&out) else {
        return json!({"state": "unknown", "repo": slug});
    };
    let mut result = ci_state(&runs);
    result["repo"] = json!(slug);
    result["branch"] = json!(default_branch);
    result
}

/// Newest run per workflow, newest first as `gh run list` returns them; the worst state decides.
/// Dependabot's own update jobs (event "dynamic") say nothing about the code and are skipped.
fn ci_state(runs: &[Value]) -> Value {
    let state_of = |run: &Value| {
        if run["status"] != "completed" {
            "running"
        } else {
            match run["conclusion"].as_str() {
                Some("success") => "green",
                Some("failure" | "cancelled" | "timed_out" | "startup_failure") => "red",
                _ => "unknown",
            }
        }
    };
    let mut seen: Vec<&str> = Vec::new();
    let mut latest: Vec<(&Value, &str)> = Vec::new();
    for run in runs.iter().filter(|r| r["event"] != "dynamic") {
        let workflow = run["workflowName"].as_str().unwrap_or("");
        if !seen.contains(&workflow) {
            seen.push(workflow);
            latest.push((run, state_of(run)));
        }
    }
    let rank = |state: &str| {
        ["red", "running", "unknown", "green"]
            .iter()
            .position(|s| *s == state)
            .unwrap_or(2)
    };
    let Some(&(run, state)) = latest.iter().min_by_key(|(_, state)| rank(state)) else {
        return json!({"state": "none"});
    };
    let failing: Vec<&str> = latest
        .iter()
        .filter(|(_, s)| *s == "red")
        .map(|(r, _)| r["workflowName"].as_str().unwrap_or(""))
        .collect();
    let workflows: Vec<Value> = latest
        .iter()
        .map(|(r, s)| json!({"name": r["workflowName"], "state": s, "conclusion": r["conclusion"], "at": r["createdAt"], "url": r["url"]}))
        .collect();
    json!({
        "state": state, "workflow": run["workflowName"], "title": run["displayTitle"],
        "at": run["createdAt"], "url": run["url"], "failing": failing, "workflows": workflows,
    })
}

fn docs_mtime(repo: &Path) -> Option<SystemTime> {
    let mut stamps: Vec<SystemTime> = DOC_FILES
        .iter()
        .filter_map(|rel| {
            std::fs::metadata(repo.join(rel))
                .and_then(|m| m.modified())
                .ok()
        })
        .collect();
    if let Ok(entries) = std::fs::read_dir(repo.join(".vorch").join("plans")) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "md") {
                if let Ok(t) = entry.metadata().and_then(|m| m.modified()) {
                    stamps.push(t);
                }
            }
        }
    }
    stamps.into_iter().max()
}

/// Collect the facts of one repository. Never fails for repository problems;
/// `ok: false` with an `error` code says why nothing could be measured.
pub fn collect(path: &str, today: NaiveDate, with_ci: bool) -> Value {
    collect_reusing(path, today, with_ci, None)
}

/// [`collect`], given `previous`, the last facts measured in the same folder. The code scan and the
/// activity reconstruction are carried over while their inputs are unchanged, and the activity is
/// extended when HEAD only moved forward; the result equals a full measurement.
pub fn collect_reusing(
    path: &str,
    today: NaiveDate,
    with_ci: bool,
    previous: Option<&Value>,
) -> Value {
    collect_at_checkpoint(path, today, with_ci, previous, || {})
}

/// The local checkpoint lets contract tests change repository identity after reading it,
/// without depending on process timing or exposing a production fault switch.
fn collect_at_checkpoint(
    path: &str,
    today: NaiveDate,
    with_ci: bool,
    previous: Option<&Value>,
    before_identity_check: impl FnOnce(),
) -> Value {
    let repo = Path::new(path);
    if !repo.is_dir() {
        return json!({"ok": false, "error": "path_missing", "is_git": false});
    }
    let Some(root) = git(repo, &["rev-parse", "--show-toplevel"]) else {
        return json!({"ok": false, "error": "not_a_git_repository", "is_git": false});
    };
    let repo = Path::new(root.trim());
    let trimmed = |v: Option<String>| v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let head = trimmed(git(repo, &["rev-parse", "HEAD"]));
    let branch = trimmed(git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]));
    let history = head.as_deref().and_then(|h| History::read(repo, h));
    if head.is_some() && history.is_none() {
        return json!({"ok": false, "error": "git_log_failed", "is_git": true});
    }
    let commits = if head.is_some() {
        commits(history.as_ref(), today)
    } else {
        empty_commits()
    };
    let remotes = remotes(repo);
    let Some(dirty) = dirty(repo, head.is_some()) else {
        return json!({"ok": false, "error": "git_status_failed", "is_git": true});
    };
    // The CI state comes over the network; it is read while the local measurements run.
    let ci = with_ci.then(|| {
        let (repo, remotes, branch) = (repo.to_owned(), remotes.clone(), branch.clone());
        std::thread::spawn(move || ci(&repo, &remotes, branch.as_deref()))
    });
    let (upstream, ahead, behind, unpushed_since) = upstream(repo);
    let newest_local = dirty
        .newest
        .map(|t| DateTime::<Local>::from(t).date_naive().to_string());
    let (worktrees, checkouts) = worktrees(repo);
    let branches = branches(repo, branch.as_deref());
    let night = night(repo, today, &dirty, &checkouts);
    let collisions = collisions(repo, branch.as_deref(), &dirty, &checkouts, &branches);
    let last_work = [commits["last"].as_str().map(str::to_owned), newest_local]
        .into_iter()
        .flatten()
        .max();
    let release = release(repo);
    let docs_mtime = docs_mtime(repo).map(iso);
    let previous = previous.filter(|p| p["ok"] == true);
    // The same HEAD and uncommitted state mean the same working-tree files.
    let unchanged = previous.is_some_and(|p| {
        p["head"] == json!(head) && p["dirty"]["fingerprint"] == dirty.value["fingerprint"]
    });
    let files = statistics::Files::new(repo);
    let code = statistics::code(
        repo,
        &files,
        previous.filter(|_| unchanged).map(|p| &p["code"]),
    );
    let activity = statistics::activity(
        repo,
        today,
        head.as_deref(),
        history.as_ref(),
        previous.filter(|p| p["branch"] == json!(branch)),
        unchanged,
        &files,
    );
    let ci = match ci {
        Some(lookup) => lookup
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        None => json!({"state": "skipped"}),
    };
    let mut measured = json!({
        "ok": true, "is_git": true, "head": head, "branch": branch,
        "commits": commits,
        "remotes": remotes.iter().map(|(n, u)| json!({"name": n, "url": u})).collect::<Vec<_>>(),
        "has_remote": !remotes.is_empty(),
        "upstream": upstream, "ahead": ahead, "behind": behind, "unpushed_since": unpushed_since,
        "dirty": dirty.value,
        "worktrees": worktrees,
        "branches": branches,
        "release": release,
        "night": night,
        "collisions": collisions,
        "ci": ci,
        "docs_mtime": docs_mtime,
        "last_work": last_work,
        "code": code,
        "activity": activity,
    });
    before_identity_check();
    if head != trimmed(git(repo, &["rev-parse", "HEAD"]))
        || branch != trimmed(git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]))
    {
        measured["ok"] = json!(false);
        measured["error"] = json!("repository_changed_during_measurement");
        measured["code"]["ok"] = json!(false);
        measured["code"]["partial"] = json!(true);
    }
    measured
}

/// Commit subjects after `rev`, newest first, as "YYYY-MM-DD subject"; None when rev is unknown.
pub fn commits_since(path: &str, rev: &str, limit: usize) -> Option<Vec<String>> {
    let out = git(
        Path::new(path),
        &[
            "log",
            &format!("--max-count={limit}"),
            "--format=%cs %s",
            &format!("{rev}..HEAD"),
        ],
    )?;
    Some(
        out.lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

pub fn count_since(path: &str, rev: &str) -> Option<u64> {
    git(
        Path::new(path),
        &["rev-list", "--count", &format!("{rev}..HEAD")],
    )?
    .trim()
    .parse()
    .ok()
}

/// Commits on the current branch made after an RFC 3339 time.
pub fn count_after(path: &str, at: &str) -> Option<u64> {
    git(
        Path::new(path),
        &["rev-list", "--count", &format!("--since={at}"), "HEAD"],
    )?
    .trim()
    .parse()
    .ok()
}

pub fn is_git_repo(path: &str) -> bool {
    Path::new(path).is_dir() && git(Path::new(path), &["rev-parse", "--show-toplevel"]).is_some()
}

pub fn last_commit_date(path: &str) -> Option<String> {
    let out = git(Path::new(path), &["log", "-1", "--format=%cs"])?;
    let out = out.trim();
    (!out.is_empty()).then(|| out.to_owned())
}

/// The facts that decide whether a card still holds, by what they mean: new commits, uncommitted
/// work, unpushed commits, edited docs, commits in extra worktrees or unmerged branches, readability.
pub fn basis(facts: &Value) -> Value {
    let d = &facts["dirty"];
    let mut uncommitted = vec![
        d["files"].clone(),
        d["lines_added"].clone(),
        d["lines_removed"].clone(),
    ];
    let side_dirty: Vec<Value> = facts["worktrees"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["dirty_fingerprint"].is_string())
        .map(|item| json!([item["path"], item["dirty_fingerprint"]]))
        .collect();
    if d["fingerprint"].is_string() || !side_dirty.is_empty() {
        uncommitted.push(json!([d["fingerprint"], side_dirty]));
    }
    let side_heads: Vec<&Value> = [&facts["worktrees"]["items"], &facts["branches"]["unmerged"]]
        .into_iter()
        .flat_map(|list| list.as_array().into_iter().flatten())
        .map(|item| &item["head"])
        .filter(|h| h.is_string())
        .collect();
    json!({
        "commits": facts["head"], "uncommitted": uncommitted, "unpushed": facts["ahead"],
        "docs": facts["docs_mtime"], "side_branches": side_heads, "readable": facts["ok"],
    })
}

/// The names of the basis parts that differ between two measurements.
pub fn changes(before: &Value, after: &Value) -> Vec<String> {
    after
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, v)| before.get(k.as_str()) != Some(*v))
        .map(|(k, _)| k.clone())
        .collect()
}

/// What must change before a project needs a fresh interpretation: a digest of the basis.
/// Without commits in worktrees or unmerged branches the key keeps its original form.
pub fn fingerprint(facts: &Value) -> String {
    let b = basis(facts);
    let u = &b["uncommitted"];
    let mut key = json!([
        b["commits"],
        u[0],
        u[1],
        u[2],
        b["unpushed"],
        b["docs"],
        b["readable"]
    ]);
    if b["side_branches"].as_array().is_some_and(|s| !s.is_empty()) {
        key.as_array_mut()
            .expect("the key is a list")
            .push(b["side_branches"].clone());
    }
    if let Some(dirty) = u.get(3) {
        key.as_array_mut()
            .expect("the key is a list")
            .push(dirty.clone());
    }
    let digest = Sha1::digest(key.to_string().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_totals_require_complete_discovery_and_directory_walks() {
        let tmp = tempfile::tempdir().unwrap();
        let denied = tmp.path().join("target/denied");
        std::fs::create_dir_all(&denied).unwrap();
        std::fs::create_dir_all(tmp.path().join("app/node_modules")).unwrap();
        std::fs::write(denied.join("payload"), b"abc").unwrap();
        std::fs::write(tmp.path().join("app/node_modules/package"), b"12345").unwrap();
        let day = NaiveDate::from_ymd_opt(2026, 10, 23).unwrap();
        let full = space(tmp.path().to_str().unwrap(), day);
        assert_eq!(full["ok"], true);
        assert_eq!(full["total_bytes"], 8);
        // Inject only the OS enumeration failure; the real discovery, other directory walk,
        // aggregation and public payload path run normally without changing machine ACLs.
        let partial = space_with(tmp.path().to_str().unwrap(), day, &|path| {
            if path == denied {
                Err(std::io::ErrorKind::PermissionDenied.into())
            } else {
                std::fs::read_dir(path)
            }
        });
        assert_eq!(partial["ok"], false);
        assert_eq!(partial["partial"], true);
        assert!(partial["total_bytes"].is_null());
        assert_eq!(
            partial["dirs"],
            json!([{"path":"app/node_modules","bytes":5,"ok":true},{"path":"target","bytes":null,"ok":false}])
        );
        assert_eq!(
            partial["errors"],
            json!([{"path":"target","reason":"permission_denied"}])
        );
        let missing = space(tmp.path().join("missing").to_str().unwrap(), day);
        assert!(missing["total_bytes"].is_null());
        assert_eq!(missing["errors"][0]["path"], ".");
        let empty = tempfile::tempdir().unwrap();
        let empty = space(empty.path().to_str().unwrap(), day);
        assert_eq!(empty["total_bytes"], 0);
        assert_eq!(empty["ok"], true);
    }

    #[test]
    fn collection_rejects_a_head_or_branch_changed_during_measurement() {
        let dir = tempfile::tempdir().unwrap();
        let write_git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=t@example.com",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(dir.path())
                .env("GIT_AUTHOR_DATE", "2026-09-29T10:00:00Z")
                .env("GIT_COMMITTER_DATE", "2026-09-29T10:00:00Z")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        write_git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        write_git(&["add", "main.rs"]);
        write_git(&["commit", "-q", "-m", "initial"]);
        let today = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
        let path = dir.path().to_str().unwrap();
        let initial = collect(path, today, false);
        assert_eq!(initial["ok"], true);
        assert_eq!(initial["code"]["ok"], true);
        assert_eq!(initial["code"]["source_code"], 1);
        for change in [
            vec!["commit", "-q", "--allow-empty", "-m", "new head"],
            vec!["checkout", "-q", "-b", "other"],
        ] {
            let measured = collect_at_checkpoint(path, today, false, None, || {
                write_git(&change);
            });
            assert_eq!(measured["ok"], false, "{change:?}");
            assert_eq!(measured["is_git"], true);
            assert_eq!(measured["error"], "repository_changed_during_measurement");
            assert_eq!(measured["code"]["ok"], false);
            assert_eq!(measured["code"]["partial"], true);
            assert_eq!(
                measured["code"]["source_code"],
                initial["code"]["source_code"]
            );
        }
    }

    #[test]
    fn remote_urls_lose_credentials_and_yield_github_slugs() {
        assert_eq!(
            strip_credentials("https://user:tok@github.com/a/b.git"),
            "https://github.com/a/b.git"
        );
        assert_eq!(
            strip_credentials("git@github.com:a/b.git"),
            "git@github.com:a/b.git"
        );
        assert_eq!(
            github_slug("git@github.com:owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            github_slug("https://github.com/owner/repo/").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(github_slug("https://gitlab.com/owner/repo"), None);
        assert_eq!(github_slug("https://notgithub.com/owner/repo"), None);
        assert_eq!(
            github_slug("https://github.com.attacker.test/owner/repo"),
            None
        );
        assert_eq!(github_slug("C:/mirrors/github.com/owner/repo"), None);
        assert_eq!(
            github_slug("ssh://git@github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
    }

    #[test]
    fn ci_state_is_the_worst_newest_run_per_workflow() {
        let r = |wf: &str, status: &str, conclusion: &str, event: &str| json!({"workflowName": wf, "status": status, "conclusion": conclusion, "event": event, "displayTitle": wf});
        let runs = [
            r("Dependabot Updates", "completed", "failure", "dynamic"),
            r("docs", "completed", "success", "push"),
            r("quality", "completed", "failure", "push"),
            r("docs", "completed", "failure", "push"), // older run, superseded
        ];
        let ci = ci_state(&runs);
        assert_eq!(
            (ci["state"].as_str(), ci["workflow"].as_str()),
            (Some("red"), Some("quality"))
        );
        assert_eq!(ci["failing"], json!(["quality"]));
        let states: Vec<(&str, &str)> = ci["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| (w["name"].as_str().unwrap(), w["state"].as_str().unwrap()))
            .collect();
        assert_eq!(states, [("docs", "green"), ("quality", "red")]);
        assert_eq!(ci_state(&runs[..2])["state"], "green");
        assert_eq!(
            ci_state(&[r("build", "in_progress", "", "push")])["state"],
            "running"
        );
        assert_eq!(ci_state(&runs[..1])["state"], "none");
    }
}
