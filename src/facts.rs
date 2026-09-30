//! Exact repository facts, computed from git without any model.
//!
//! Everything here is read-only: no fetch, no checkout, nothing that changes a
//! repository. The GitHub CI lookup is optional and uses the `gh` CLI when it is
//! installed and the repository has a github.com remote.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

pub const WEEKS: usize = 26;
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const GH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_UNTRACKED_FILES: usize = 20_000;
const MAX_UNTRACKED_BYTES: u64 = 2_000_000;
const DOC_FILES: &[&str] = &[
    "README.md", "AGENTS.md", "TODO.md", "ROADMAP.md", "VISION.md", "IDEAS.md", "CHANGELOG.md",
    ".vorch/PROJECT.md", ".vorch/FLAGGED.md",
];

/// Run a program without a console window. Returns (success, stdout or stderr).
fn run(program: &str, args: &[&str], cwd: &Path, timeout: Duration) -> (bool, String) {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, e.to_string()),
    };
    let reader = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    };
    let out = reader(Box::new(child.stdout.take().expect("piped stdout")));
    let err = reader(Box::new(child.stderr.take().expect("piped stderr")));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let err = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    match status {
        Some(s) if s.success() => (true, out),
        Some(_) => (false, err),
        None => (false, format!("{program} timed out")),
    }
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let mut full = vec!["-c", "core.quotepath=off", "-c", "i18n.logOutputEncoding=utf-8"];
    full.extend_from_slice(args);
    match run("git", &full, repo, GIT_TIMEOUT) {
        (true, out) => Some(out),
        _ => None,
    }
}

fn iso(t: SystemTime) -> String {
    DateTime::<Utc>::from(t).format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn monday(d: NaiveDate) -> NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn commits(repo: &Path, today: NaiveDate) -> Value {
    let out = git(repo, &["log", "--format=%cs", "HEAD"]).unwrap_or_default();
    let dates: Vec<NaiveDate> = out.split_whitespace().filter_map(|l| NaiveDate::parse_from_str(l, "%Y-%m-%d").ok()).collect();
    let mut weekly = vec![0u32; WEEKS];
    let this_monday = monday(today);
    for d in &dates {
        let weeks_ago = (this_monday - monday(*d)).num_days() / 7;
        if (0..WEEKS as i64).contains(&weeks_ago) {
            weekly[WEEKS - 1 - weeks_ago as usize] += 1;
        }
    }
    let within = |days: i64| dates.iter().filter(|d| (today - **d).num_days() < days).count();
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
    let Some(up) = git(repo, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]) else {
        return (Value::Null, Value::Null, Value::Null, Value::Null);
    };
    let counts = git(repo, &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"]).unwrap_or_default();
    let nums: Vec<u64> = counts.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    let (behind, ahead) = if nums.len() == 2 { (json!(nums[0]), json!(nums[1])) } else { (Value::Null, Value::Null) };
    let oldest = git(repo, &["log", "--reverse", "--format=%cs", "@{upstream}..HEAD"]).and_then(|out| out.lines().next().map(str::to_owned));
    (json!(up.trim()), ahead, behind, json!(oldest))
}

struct Dirty {
    value: Value,
    newest: Option<SystemTime>,
}

fn dirty(repo: &Path, has_head: bool) -> Dirty {
    let raw = git(repo, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).unwrap_or_default();
    let tokens: Vec<&str> = raw.split('\0').collect();
    let (mut modified, mut untracked, mut deleted) = (0u64, 0u64, 0u64);
    let mut paths: Vec<&str> = Vec::new();
    let mut untracked_paths: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        i += 1;
        if token.len() < 4 || !token.is_char_boundary(3) {
            continue;
        }
        let (xy, path) = (&token[..2], &token[3..]);
        if xy.contains('R') || xy.contains('C') {
            i += 1; // the original path of a rename or copy follows
        }
        if xy == "??" {
            untracked += 1;
            untracked_paths.push(path);
        } else if xy.contains('D') {
            deleted += 1;
        } else {
            modified += 1;
        }
        paths.push(path);
    }

    let (mut added, mut removed) = (0u64, 0u64);
    if has_head {
        for line in git(repo, &["diff", "HEAD", "--numstat"]).unwrap_or_default().lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if let (Some(Ok(a)), Some(Ok(r))) = (parts.first().map(|p| p.parse::<u64>()), parts.get(1).map(|p| p.parse::<u64>())) {
                added += a;
                removed += r;
            }
        }
    }
    for path in untracked_paths.iter().take(MAX_UNTRACKED_FILES) {
        let full = repo.join(path);
        let Ok(meta) = std::fs::metadata(&full) else { continue };
        if meta.len() > MAX_UNTRACKED_BYTES {
            continue;
        }
        if let Ok(data) = std::fs::read(&full) {
            if !data.contains(&0) {
                added += data.iter().filter(|b| **b == b'\n').count() as u64;
            }
        }
    }

    // Files copied in keep far older mtimes, so only edits after the last commit date the work;
    // when every file is older, the work is at risk since that commit.
    let last_commit = if has_head {
        git(repo, &["log", "-1", "--format=%ct"]).and_then(|s| s.trim().parse::<u64>().ok()).map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    } else {
        None
    };
    let mtimes: Vec<SystemTime> = paths.iter().filter_map(|p| std::fs::metadata(repo.join(p)).and_then(|m| m.modified()).ok()).collect();
    let after_commit = mtimes.iter().copied().filter(|t| last_commit.is_none_or(|c| *t > c));
    let (oldest, newest) = match (after_commit.clone().min(), after_commit.max()) {
        (Some(oldest), Some(newest)) => (Some(oldest), Some(newest)),
        _ if mtimes.is_empty() => (None, None),
        _ => (last_commit, last_commit),
    };
    Dirty {
        value: json!({
            "files": paths.len(), "modified": modified, "untracked": untracked, "deleted": deleted,
            "lines_added": added, "lines_removed": removed,
            "oldest_change": oldest.map(iso), "newest_change": newest.map(iso),
        }),
        newest,
    }
}

fn worktrees(repo: &Path) -> Value {
    let mut items: Vec<Value> = Vec::new();
    for line in git(repo, &["worktree", "list", "--porcelain"]).unwrap_or_default().lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            items.push(json!({"path": path, "branch": null, "head": null, "prunable": false}));
        } else if let Some(current) = items.last_mut() {
            if let Some(head) = line.strip_prefix("HEAD ") {
                current["head"] = json!(head);
            } else if let Some(branch) = line.strip_prefix("branch ") {
                current["branch"] = json!(branch.strip_prefix("refs/heads/").unwrap_or(branch));
            } else if line.starts_with("prunable") {
                current["prunable"] = json!(true);
            }
        }
    }
    let mut extra: Vec<Value> = items.into_iter().skip(1).collect(); // the first entry is the main checkout
    for item in &mut extra {
        let path = Path::new(item["path"].as_str().unwrap_or(""));
        let dirty = if path.is_dir() { git(path, &["status", "--porcelain", "--untracked-files=all"]).map(|s| s.lines().count()) } else { None };
        item["dirty_files"] = json!(dirty);
    }
    let prunable = extra.iter().filter(|w| w["prunable"] == json!(true)).count();
    json!({"count": extra.len(), "prunable": prunable, "items": extra})
}

fn branches(repo: &Path, current: Option<&str>) -> Value {
    let fmt = "--format=%(refname:short)\t%(committerdate:short)\t%(objectname)";
    let merged = git(repo, &["branch", "--merged", "HEAD", fmt]).unwrap_or_default();
    let merged_count = merged.lines().filter_map(|l| l.split('\t').next()).filter(|n| !n.is_empty() && Some(*n) != current).count();
    let mut unmerged = Vec::new();
    for line in git(repo, &["branch", "--no-merged", "HEAD", fmt]).unwrap_or_default().lines() {
        let mut parts = line.split('\t');
        let (name, last, head) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let text = |s: &str| if s.is_empty() { Value::Null } else { json!(s) };
        if !name.is_empty() {
            unmerged.push(json!({"name": name, "last": text(last), "head": text(head)}));
        }
    }
    json!({"merged_count": merged_count, "unmerged": unmerged})
}

/// "owner/repo" of a github.com remote URL (https or ssh form).
fn github_slug(url: &str) -> Option<String> {
    let rest = &url[url.find("github.com")? + "github.com".len()..];
    let rest = rest.trim_start_matches([':', '/']).trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && !owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

/// CI state of the default branch (the remote's HEAD, else the checked-out branch):
/// the newest run of every workflow counts, and the worst of them decides.
fn ci(repo: &Path, remotes: &[(String, String)], branch: Option<&str>) -> Value {
    let mut ordered: Vec<&(String, String)> = remotes.iter().collect();
    ordered.sort_by_key(|(name, _)| name != "origin");
    let Some((remote, slug)) = ordered.iter().find_map(|(name, url)| github_slug(url).map(|s| (name.as_str(), s))) else {
        return json!({"state": "none"});
    };
    let default_branch = git(repo, &["symbolic-ref", "--short", &format!("refs/remotes/{remote}/HEAD")])
        .and_then(|r| r.trim().strip_prefix(&format!("{remote}/")).map(str::to_owned))
        .or_else(|| branch.filter(|b| *b != "HEAD").map(str::to_owned));
    let mut args = vec!["run", "list", "-R", slug.as_str(), "-L", "40", "--json", "status,conclusion,createdAt,displayTitle,workflowName,url,event"];
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
    let rank = |state: &str| ["red", "running", "unknown", "green"].iter().position(|s| *s == state).unwrap_or(2);
    let Some(&(run, state)) = latest.iter().min_by_key(|(_, state)| rank(state)) else {
        return json!({"state": "none"});
    };
    let failing: Vec<&str> = latest.iter().filter(|(_, s)| *s == "red").map(|(r, _)| r["workflowName"].as_str().unwrap_or("")).collect();
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
    let mut stamps: Vec<SystemTime> = DOC_FILES.iter().filter_map(|rel| std::fs::metadata(repo.join(rel)).and_then(|m| m.modified()).ok()).collect();
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
    let repo = Path::new(path);
    if !repo.is_dir() {
        return json!({"ok": false, "error": "path_missing", "is_git": false});
    }
    if git(repo, &["rev-parse", "--show-toplevel"]).is_none() {
        return json!({"ok": false, "error": "not_a_git_repository", "is_git": false});
    }
    let trimmed = |v: Option<String>| v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let head = trimmed(git(repo, &["rev-parse", "HEAD"]));
    let branch = trimmed(git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]));
    let commits = if head.is_some() { commits(repo, today) } else { empty_commits() };
    let remotes = remotes(repo);
    let dirty = dirty(repo, head.is_some());
    let (upstream, ahead, behind, unpushed_since) = upstream(repo);
    let newest_local = dirty.newest.map(|t| DateTime::<Local>::from(t).date_naive().to_string());
    let last_work = [commits["last"].as_str().map(str::to_owned), newest_local].into_iter().flatten().max();
    json!({
        "ok": true, "is_git": true, "head": head, "branch": branch,
        "commits": commits,
        "remotes": remotes.iter().map(|(n, u)| json!({"name": n, "url": u})).collect::<Vec<_>>(),
        "has_remote": !remotes.is_empty(),
        "upstream": upstream, "ahead": ahead, "behind": behind, "unpushed_since": unpushed_since,
        "dirty": dirty.value,
        "worktrees": worktrees(repo),
        "branches": branches(repo, branch.as_deref()),
        "ci": if with_ci { ci(repo, &remotes, branch.as_deref()) } else { json!({"state": "skipped"}) },
        "docs_mtime": docs_mtime(repo).map(iso),
        "last_work": last_work,
    })
}

/// Commit subjects after `rev`, newest first, as "YYYY-MM-DD subject"; None when rev is unknown.
pub fn commits_since(path: &str, rev: &str, limit: usize) -> Option<Vec<String>> {
    let out = git(Path::new(path), &["log", &format!("--max-count={limit}"), "--format=%cs %s", &format!("{rev}..HEAD")])?;
    Some(out.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

pub fn count_since(path: &str, rev: &str) -> Option<u64> {
    git(Path::new(path), &["rev-list", "--count", &format!("{rev}..HEAD")])?.trim().parse().ok()
}

/// Commits on the current branch made after an RFC 3339 time.
pub fn count_after(path: &str, at: &str) -> Option<u64> {
    git(Path::new(path), &["rev-list", "--count", &format!("--since={at}"), "HEAD"])?.trim().parse().ok()
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
    let side_heads: Vec<&Value> = [&facts["worktrees"]["items"], &facts["branches"]["unmerged"]]
        .into_iter()
        .flat_map(|list| list.as_array().into_iter().flatten())
        .map(|item| &item["head"])
        .filter(|h| h.is_string())
        .collect();
    json!({
        "commits": facts["head"], "uncommitted": [d["files"], d["lines_added"], d["lines_removed"]], "unpushed": facts["ahead"],
        "docs": facts["docs_mtime"], "side_branches": side_heads, "readable": facts["ok"],
    })
}

/// The names of the basis parts that differ between two measurements.
pub fn changes(before: &Value, after: &Value) -> Vec<String> {
    after.as_object().into_iter().flatten().filter(|(k, v)| before.get(k.as_str()) != Some(*v)).map(|(k, _)| k.clone()).collect()
}

/// What must change before a project needs a fresh interpretation: a digest of the basis.
/// Without commits in worktrees or unmerged branches the key keeps its original form.
pub fn fingerprint(facts: &Value) -> String {
    let b = basis(facts);
    let u = &b["uncommitted"];
    let mut key = json!([b["commits"], u[0], u[1], u[2], b["unpushed"], b["docs"], b["readable"]]);
    if b["side_branches"].as_array().is_some_and(|s| !s.is_empty()) {
        key.as_array_mut().expect("the key is a list").push(b["side_branches"].clone());
    }
    let digest = Sha1::digest(key.to_string().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_urls_lose_credentials_and_yield_github_slugs() {
        assert_eq!(strip_credentials("https://user:tok@github.com/a/b.git"), "https://github.com/a/b.git");
        assert_eq!(strip_credentials("git@github.com:a/b.git"), "git@github.com:a/b.git");
        assert_eq!(github_slug("git@github.com:owner/repo.git").as_deref(), Some("owner/repo"));
        assert_eq!(github_slug("https://github.com/owner/repo/").as_deref(), Some("owner/repo"));
        assert_eq!(github_slug("https://gitlab.com/owner/repo"), None);
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
        assert_eq!((ci["state"].as_str(), ci["workflow"].as_str()), (Some("red"), Some("quality")));
        assert_eq!(ci["failing"], json!(["quality"]));
        let states: Vec<(&str, &str)> = ci["workflows"].as_array().unwrap().iter().map(|w| (w["name"].as_str().unwrap(), w["state"].as_str().unwrap())).collect();
        assert_eq!(states, [("docs", "green"), ("quality", "red")]);
        assert_eq!(ci_state(&runs[..2])["state"], "green");
        assert_eq!(ci_state(&[r("build", "in_progress", "", "push")])["state"], "running");
        assert_eq!(ci_state(&runs[..1])["state"], "none");
    }
}
