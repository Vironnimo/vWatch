//! Working-tree status, side checkouts, overnight work and collision measurements.

use super::{git, iso};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

const MAX_UNTRACKED_FILES: usize = 20_000;
const MAX_UNTRACKED_BYTES: u64 = 2_000_000;
const MAX_FINGERPRINT_BYTES: u64 = 20_000_000;
/// The overnight view starts at this local hour of the previous day.
const EVENING_HOUR: u32 = 18;

pub(super) struct Dirty {
    pub(super) value: Value,
    pub(super) newest: Option<SystemTime>,
    paths: Vec<String>,
    mtimes: Vec<SystemTime>,
}

/// Porcelain v1's two status bytes and destination path, in Git's original order.
/// A rename/copy's following source token belongs to the same entry, not another file.
fn status_entries(raw: &str) -> impl Iterator<Item = ([u8; 2], &str)> {
    let mut tokens = raw.split('\0');
    std::iter::from_fn(move || loop {
        let token = tokens.next()?;
        if token.len() < 4 || !token.is_char_boundary(3) {
            continue;
        }
        let status = [token.as_bytes()[0], token.as_bytes()[1]];
        if status.contains(&b'R') || status.contains(&b'C') {
            tokens.next();
        }
        return Some((status, &token[3..]));
    })
}

/// Notice edits even when file and line counts stay the same. Content reads are bounded;
/// larger files still contribute their full size and high-resolution modification time.
/// Also returns each path's modification time, following symlinks, so callers need no second pass.
fn dirty_fingerprint<'a>(
    repo: &Path,
    raw: &str,
    paths: impl Iterator<Item = &'a str>,
) -> (Value, Vec<Option<SystemTime>>) {
    if raw.is_empty() {
        return (Value::Null, Vec::new());
    }
    let mut hash = Sha1::new();
    hash.update(raw.as_bytes());
    let mut remaining = MAX_FINGERPRINT_BYTES;
    let mut mtimes = Vec::new();
    for path in paths {
        hash.update(path.as_bytes());
        let full = repo.join(path);
        let mut mtime = None;
        if let Ok(meta) = std::fs::symlink_metadata(&full) {
            hash.update(meta.len().to_le_bytes());
            if let Ok(at) = meta.modified() {
                mtime = Some(at);
                let at = DateTime::<Utc>::from(at);
                hash.update(at.timestamp().to_le_bytes());
                hash.update(at.timestamp_subsec_nanos().to_le_bytes());
            }
            if meta.is_symlink() {
                mtime = std::fs::metadata(&full).and_then(|m| m.modified()).ok();
                if let Ok(target) = std::fs::read_link(&full) {
                    hash.update(target.to_string_lossy().as_bytes());
                }
            } else if meta.is_file() && meta.len() <= MAX_UNTRACKED_BYTES && meta.len() <= remaining
            {
                if let Ok(file) = std::fs::File::open(full) {
                    let mut data = Vec::new();
                    if file.take(remaining).read_to_end(&mut data).is_ok() {
                        remaining -= data.len() as u64;
                        hash.update(data);
                    }
                }
            }
        }
        mtimes.push(mtime);
    }
    (json!(format!("{:x}", hash.finalize())), mtimes)
}

pub(super) fn dirty(repo: &Path, has_head: bool) -> Option<Dirty> {
    let raw = git(
        repo,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let (mut modified, mut untracked, mut deleted) = (0u64, 0u64, 0u64);
    let mut paths: Vec<&str> = Vec::new();
    let mut untracked_paths: Vec<&str> = Vec::new();
    for (status, path) in status_entries(&raw) {
        if status == *b"??" {
            untracked += 1;
            untracked_paths.push(path);
        } else if status.contains(&b'D') {
            deleted += 1;
        } else {
            modified += 1;
        }
        paths.push(path);
    }

    let (mut added, mut removed) = (0u64, 0u64);
    let mut errors = BTreeSet::new();
    // A clean status leaves no lines to count. An unborn branch has no HEAD, but its index still
    // contains staged additions.
    let baseline = if modified + deleted == 0 {
        None
    } else if has_head {
        Some("HEAD".to_owned())
    } else {
        git(repo, &["hash-object", "-t", "tree", "--stdin"]).map(|out| out.trim().to_owned())
    };
    if modified + deleted > 0 && baseline.as_ref().is_none_or(String::is_empty) {
        errors.insert("diff_failed");
    }
    if let Some(baseline) = baseline.filter(|value| !value.is_empty()) {
        let diff = git(
            repo,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                &baseline,
                "--numstat",
                "--",
            ],
        );
        match diff {
            None => {
                errors.insert("diff_failed");
            }
            Some(diff) => {
                for line in diff.lines() {
                    let parts: Vec<&str> = line.splitn(3, '\t').collect();
                    match parts.as_slice() {
                        ["-", "-", _] => {} // Git excludes binary changes from text line counts.
                        [a, r, _] => match (a.parse::<u64>(), r.parse::<u64>()) {
                            (Ok(a), Ok(r)) => {
                                added += a;
                                removed += r;
                            }
                            _ => {
                                errors.insert("diff_invalid");
                            }
                        },
                        _ => {
                            errors.insert("diff_invalid");
                        }
                    }
                }
            }
        }
    }
    if untracked_paths.len() > MAX_UNTRACKED_FILES {
        errors.insert("untracked_file_limit");
    }
    for path in untracked_paths.iter().take(MAX_UNTRACKED_FILES) {
        let full = repo.join(path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            errors.insert("untracked_read_failed");
            continue;
        };
        if !meta.is_file() {
            errors.insert("untracked_unsupported_type");
            continue;
        }
        if meta.len() > MAX_UNTRACKED_BYTES {
            errors.insert("untracked_size_limit");
            continue;
        }
        let mut data = Vec::new();
        match std::fs::File::open(&full)
            .and_then(|file| file.take(MAX_UNTRACKED_BYTES + 1).read_to_end(&mut data))
        {
            Err(_) => {
                errors.insert("untracked_read_failed");
            }
            Ok(_) if data.len() as u64 > MAX_UNTRACKED_BYTES => {
                errors.insert("untracked_size_limit");
            }
            Ok(_) if !data.contains(&0) => {
                added += data.iter().filter(|b| **b == b'\n').count() as u64;
                added += u64::from(!data.is_empty() && !data.ends_with(b"\n"));
            }
            Ok(_) => {}
        }
    }

    // Files copied in keep far older mtimes, so only edits after the last commit date the work;
    // when every file is older, the work is at risk since that commit.
    let last_commit = if has_head && !paths.is_empty() {
        git(repo, &["log", "-1", "--format=%ct"])
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    } else {
        None
    };
    let (fingerprint, mtimes) = dirty_fingerprint(repo, &raw, paths.iter().copied());
    let mtimes: Vec<SystemTime> = mtimes.into_iter().flatten().collect();
    let after_commit = mtimes
        .iter()
        .copied()
        .filter(|t| last_commit.is_none_or(|c| *t > c));
    let (oldest, newest) = match (after_commit.clone().min(), after_commit.max()) {
        (Some(oldest), Some(newest)) => (Some(oldest), Some(newest)),
        _ if mtimes.is_empty() => (None, None),
        _ => (last_commit, last_commit),
    };
    Some(Dirty {
        value: json!({
            "files": paths.len(), "modified": modified, "untracked": untracked, "deleted": deleted,
            "lines_added": errors.is_empty().then_some(added), "lines_removed": errors.is_empty().then_some(removed),
            "line_counts": {"ok":errors.is_empty(), "partial":!errors.is_empty(), "errors":errors},
            "oldest_change": oldest.map(iso), "newest_change": newest.map(iso),
            "fingerprint": fingerprint,
        }),
        newest,
        paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        mtimes,
    })
}

/// Measure `items` in order on a few threads. Starting git dominates per-checkout measurements, so
/// repositories with many worktrees would otherwise wait on one process after another.
fn parallel<T: Sync, R: Send>(items: &[T], measure: impl Fn(&T) -> R + Sync) -> Vec<R> {
    const THREADS: usize = 4;
    let size = items.len().div_ceil(THREADS).max(1);
    std::thread::scope(|scope| {
        let parts: Vec<_> = items
            .chunks(size)
            .map(|chunk| scope.spawn(|| chunk.iter().map(&measure).collect::<Vec<R>>()))
            .collect();
        parts
            .into_iter()
            .flat_map(|part| {
                part.join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    })
}

/// An extra checkout's uncommitted paths and their modification times, read once per measurement.
pub(super) struct Checkout {
    dir: PathBuf,
    branch: Option<String>,
    paths: Vec<String>,
    mtimes: Vec<Option<SystemTime>>,
}

/// The extra worktrees, plus the status of each one that exists for the overnight and collision views.
pub(super) fn worktrees(repo: &Path) -> (Value, Vec<Checkout>) {
    let mut items: Vec<Value> = Vec::new();
    for line in git(repo, &["worktree", "list", "--porcelain"])
        .unwrap_or_default()
        .lines()
    {
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
    let current = std::fs::canonicalize(repo).ok();
    let mut extra: Vec<Value> = items
        .into_iter()
        .filter(|item| {
            let path = item["path"].as_str().unwrap_or("");
            current.is_none() || std::fs::canonicalize(path).ok() != current
        })
        .collect();
    let statuses = parallel(&extra, |item| {
        let path = PathBuf::from(item["path"].as_str().unwrap_or(""));
        let exists = path.is_dir();
        let raw = if exists {
            git(
                &path,
                &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
            )
        } else {
            None
        };
        let paths = raw.as_deref().map(|raw| {
            status_entries(raw)
                .map(|(_, path)| path.to_owned())
                .collect::<Vec<_>>()
        });
        let (fingerprint, mtimes) = match (raw.as_deref(), paths.as_ref()) {
            (Some(raw), Some(paths)) => {
                dirty_fingerprint(&path, raw, paths.iter().map(String::as_str))
            }
            _ => (Value::Null, Vec::new()),
        };
        (path, exists, paths, fingerprint, mtimes)
    });
    let mut checkouts = Vec::new();
    for (item, (path, exists, paths, fingerprint, mtimes)) in extra.iter_mut().zip(statuses) {
        item["dirty_files"] = json!(paths.as_ref().map(Vec::len));
        item["dirty_fingerprint"] = fingerprint;
        if exists {
            checkouts.push(Checkout {
                branch: item["branch"].as_str().map(str::to_owned),
                dir: path,
                paths: paths.unwrap_or_default(),
                mtimes,
            });
        }
    }
    let prunable = extra
        .iter()
        .filter(|w| w["prunable"] == json!(true))
        .count();
    (
        json!({"count": extra.len(), "prunable": prunable, "items": extra}),
        checkouts,
    )
}

pub(super) fn branches(repo: &Path, current: Option<&str>) -> Value {
    let fmt = "--format=%(refname:short)\t%(committerdate:short)\t%(objectname)";
    let merged = git(repo, &["branch", "--merged", "HEAD", fmt]).unwrap_or_default();
    let merged_count = merged
        .lines()
        .filter_map(|l| l.split('\t').next())
        .filter(|n| !n.is_empty() && Some(*n) != current)
        .count();
    let mut unmerged = Vec::new();
    for line in git(repo, &["branch", "--no-merged", "HEAD", fmt])
        .unwrap_or_default()
        .lines()
    {
        let mut parts = line.split('\t');
        let (name, last, head) = (
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
            parts.next().unwrap_or(""),
        );
        let text = |s: &str| if s.is_empty() { Value::Null } else { json!(s) };
        if !name.is_empty() {
            unmerged.push(json!({"name": name, "last": text(last), "head": text(head)}));
        }
    }
    json!({"merged_count": merged_count, "unmerged": unmerged})
}

/// What happened since yesterday evening: commits on every local branch (worktrees included),
/// their size, reverts among them, and uncommitted files edited in that time.
pub(super) fn night(repo: &Path, today: NaiveDate, dirty: &Dirty, checkouts: &[Checkout]) -> Value {
    let evening = today
        .pred_opt()
        .and_then(|d| d.and_hms_opt(EVENING_HOUR, 0, 0))
        .and_then(|t| Local.from_local_datetime(&t).earliest());
    let Some(evening) = evening else {
        return Value::Null;
    };
    let since = SystemTime::from(evening);
    let log = git(
        repo,
        &[
            "log",
            "--branches",
            &format!("--since-as-filter={}", evening.to_rfc3339()),
            &format!("--until={today}T23:59:59"),
            "--format=%x1e%s",
            "--shortstat",
            "--root",
            "--diff-merges=off",
            "--no-ext-diff",
            "--no-textconv",
        ],
    )
    .unwrap_or_default();
    let (mut commits, mut reverts, mut added, mut removed) = (0u64, 0u64, 0u64, 0u64);
    for entry in log.split('\u{1e}').filter(|e| !e.trim().is_empty()) {
        commits += 1;
        if entry.trim_start().starts_with("Revert") {
            reverts += 1;
        }
        for part in entry
            .lines()
            .skip(1) // The commit subject can itself look like a shortstat line.
            .filter(|l| l.contains("changed"))
            .flat_map(|l| l.split(','))
        {
            let n: u64 = part
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            if part.contains("insertion") {
                added += n;
            } else if part.contains("deletion") {
                removed += n;
            }
        }
    }
    let touched = dirty.mtimes.iter().filter(|t| **t >= since).count()
        + checkouts
            .iter()
            .flat_map(|c| c.mtimes.iter().flatten())
            .filter(|t| **t >= since)
            .count();
    json!({"since": iso(since), "commits": commits, "lines_added": added, "lines_removed": removed, "reverts": reverts, "uncommitted_touched": touched})
}

/// Files that two lines of work change at once: the main checkout's uncommitted files, each extra
/// worktree (uncommitted plus committed on its branch) and each unmerged branch without a worktree.
/// These shared files may need coordination before merging.
pub(super) fn collisions(
    repo: &Path,
    base: Option<&str>,
    dirty: &Dirty,
    checkouts: &[Checkout],
    branches: &Value,
) -> Value {
    let base = base.filter(|b| *b != "HEAD");
    let diff = |dir: &Path, range: String| -> Vec<String> {
        git(
            dir,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--name-only",
                "-z",
                &range,
            ],
        )
        .map(|o| {
            o.split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
    };
    let mut lines: Vec<(Value, BTreeSet<String>)> = Vec::new();
    lines.push((
        json!({"kind": "main", "name": base.unwrap_or("HEAD")}),
        dirty.paths.iter().cloned().collect(),
    ));
    let committed = match base {
        Some(b) => parallel(checkouts, |c| diff(&c.dir, format!("{b}...HEAD"))),
        None => Vec::new(),
    };
    let mut committed = committed.into_iter();
    let mut in_worktree: Vec<String> = Vec::new();
    for checkout in checkouts {
        let dir = checkout.dir.as_path();
        let branch = checkout.branch.clone();
        let mut files: BTreeSet<String> = checkout.paths.iter().cloned().collect();
        files.extend(committed.next().into_iter().flatten());
        let name = branch.clone().unwrap_or_else(|| {
            dir.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        in_worktree.extend(branch);
        lines.push((json!({"kind": "worktree", "name": name}), files));
    }
    if let Some(b) = base {
        let names: Vec<&str> = branches["unmerged"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|br| br["name"].as_str())
            .filter(|name| !name.is_empty() && !in_worktree.iter().any(|w| w == name))
            .collect();
        let changed = parallel(&names, |name| diff(repo, format!("{b}...{name}")));
        for (name, files) in names.into_iter().zip(changed) {
            lines.push((
                json!({"kind": "branch", "name": name}),
                files.into_iter().collect(),
            ));
        }
    }
    let mut out = Vec::new();
    for (i, (a, fa)) in lines.iter().enumerate() {
        for (b, fb) in &lines[i + 1..] {
            let shared: Vec<&String> = fa.intersection(fb).collect();
            if !shared.is_empty() {
                out.push(json!({"a": a, "b": b, "count": shared.len(), "files": shared.iter().take(5).collect::<Vec<_>>()}));
            }
        }
    }
    json!(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_tracked_diff_keeps_status_counts_but_cannot_claim_zero_lines() {
        let repo = tempfile::tempdir().unwrap();
        let setup = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(repo.path())
                .output()
                .unwrap()
                .status
                .success());
        };
        setup(&["init", "-q"]);
        std::fs::write(repo.path().join("staged.txt"), "one\n").unwrap();
        setup(&["add", "staged.txt"]);
        // Model HEAD disappearing after the caller discovered it: status still succeeds,
        // while the actual Git diff against HEAD fails. Collection's final guard is separate.
        let measured = dirty(repo.path(), true).unwrap().value;
        assert_eq!(measured["files"], 1);
        assert_eq!(measured["modified"], 1);
        assert!(measured["lines_added"].is_null());
        assert!(measured["lines_removed"].is_null());
        assert_eq!(measured["line_counts"]["errors"], json!(["diff_failed"]));
    }

    #[test]
    fn porcelain_entries_preserve_status_order_and_rename_copy_destinations() {
        // Source names can look like another status entry. Consume exactly one
        // token for either status column without decoding or sorting the paths.
        let raw = " M first\0R  renamed with spaces\0?? old source\0 C copied ü.rs\0 M source.rs\0?? last\0x\0éé\0";
        assert_eq!(
            status_entries(raw).collect::<Vec<_>>(),
            vec![
                (*b" M", "first"),
                (*b"R ", "renamed with spaces"),
                (*b" C", "copied ü.rs"),
                (*b"??", "last"),
            ]
        );
        assert_eq!(
            status_entries("C  copy\0source\0 R rename\0original\0 D removed\0")
                .collect::<Vec<_>>(),
            vec![(*b"C ", "copy"), (*b" R", "rename"), (*b" D", "removed")]
        );
    }
}
