mod common;

use chrono::NaiveDate;
use serde_json::Value;
use std::path::{Path, PathBuf};
use vwatch::facts;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()
}

#[test]
fn dirty_line_totals_require_every_text_measurement_to_complete() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(tmp.path(), "limits", &["2026-09-29T10:00:00"]);
    std::fs::write(repo.join("f0.txt"), "changed\n").unwrap();
    let untracked = repo.join("draft.txt");
    std::fs::write(&untracked, vec![b'x'; 2_000_001]).unwrap();
    let measured = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(measured["ok"], true);
    assert_eq!(measured["dirty"]["files"], 2);
    assert!(measured["dirty"]["lines_added"].is_null());
    assert!(measured["dirty"]["lines_removed"].is_null());
    assert_eq!(measured["dirty"]["line_counts"]["partial"], true);
    assert_eq!(
        measured["dirty"]["line_counts"]["errors"],
        serde_json::json!(["untracked_size_limit"])
    );

    std::fs::write(&untracked, "one\ntwo\n").unwrap();
    let measured = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(measured["dirty"]["line_counts"]["ok"], true);
    assert_eq!(measured["dirty"]["lines_added"], 3);
    assert_eq!(measured["dirty"]["lines_removed"], 1);

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&untracked)
            .unwrap();
        let measured = facts::collect(repo.to_str().unwrap(), today(), false);
        assert_eq!(measured["dirty"]["files"], 2);
        assert!(measured["dirty"]["lines_added"].is_null());
        assert_eq!(
            measured["dirty"]["line_counts"]["errors"],
            serde_json::json!(["untracked_read_failed"])
        );
    }
}

#[test]
fn facts_describe_history_backup_and_unsaved_work() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(
        tmp.path(),
        "demo",
        &[
            "2026-04-01T10:00:00",
            "2026-09-10T10:00:00",
            "2026-09-29T10:00:00",
        ],
    );
    std::fs::write(repo.join("f0.txt"), "changed\nand more\n").unwrap();
    let edited_at = chrono::DateTime::parse_from_rfc3339("2026-09-30T14:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    std::fs::File::options()
        .write(true)
        .open(repo.join("f0.txt"))
        .unwrap()
        .set_modified(edited_at.into())
        .unwrap();
    std::fs::write(repo.join("new.txt"), "a\nb\nc\n").unwrap();
    // a file copied in from an old archive keeps its ancient mtime
    let archived =
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(repo.join("new.txt"))
        .unwrap()
        .set_modified(archived)
        .unwrap();

    let f = facts::collect(repo.to_str().unwrap(), today(), false);

    assert_eq!(
        (
            f["ok"].as_bool(),
            f["is_git"].as_bool(),
            f["branch"].as_str()
        ),
        (Some(true), Some(true), Some("main"))
    );
    let c = &f["commits"];
    assert_eq!(c["total"], 3);
    assert_eq!(
        (c["first"].as_str(), c["last"].as_str()),
        (Some("2026-04-01"), Some("2026-09-29"))
    );
    assert_eq!(
        (
            c["last_7d"].as_u64(),
            c["last_30d"].as_u64(),
            c["last_90d"].as_u64()
        ),
        (Some(1), Some(2), Some(2))
    );
    let weekly: Vec<u64> = c["weekly"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(
        (
            weekly.len(),
            weekly[facts::WEEKS - 1],
            weekly.iter().sum::<u64>()
        ),
        (facts::WEEKS, 1, 2)
    );
    assert_eq!(
        (f["has_remote"].as_bool(), f["upstream"].is_null()),
        (Some(false), true)
    );
    let d = &f["dirty"];
    assert_eq!(
        (
            d["files"].as_u64(),
            d["untracked"].as_u64(),
            d["modified"].as_u64()
        ),
        (Some(2), Some(1), Some(1))
    );
    assert_eq!(
        (d["lines_added"].as_u64(), d["lines_removed"].as_u64()),
        (Some(2 + 3), Some(1))
    );
    // only files edited after the last commit (2026-09-29 10:00) date the unsaved work
    assert_eq!(
        d["oldest_change"],
        edited_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    );
    // uncommitted edits count as work
    assert_eq!(
        f["last_work"].as_str().unwrap(),
        edited_at
            .with_timezone(&chrono::Local)
            .date_naive()
            .to_string()
    );

    // with an upstream that has only the first commit, the two later ones are unpushed
    common::git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/demo.git",
        ],
    );
    common::git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD~2"]);
    common::git(&repo, &["branch", "-u", "origin/main"]);
    let f = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(
        (
            f["ahead"].as_u64(),
            f["behind"].as_u64(),
            f["unpushed_since"].as_str()
        ),
        (Some(2), Some(0), Some("2026-09-10"))
    );

    // A clock set ahead must not put tomorrow's work into today's activity windows.
    common::git_at(
        &repo,
        &["commit", "-q", "--allow-empty", "-m", "future"],
        "2026-10-01T10:00:00",
    );
    let future = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(future["commits"]["total"], 4);
    for field in ["last_7d", "last_30d", "last_90d", "weekly"] {
        assert_eq!(future["commits"][field], f["commits"][field], "{field}");
    }
    assert_eq!(future["night"], f["night"]);
}

#[test]
fn fingerprint_changes_with_new_work() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(tmp.path());
    let path = repo.to_str().unwrap();
    let before = facts::fingerprint(&facts::collect(path, today(), false));
    assert_eq!(
        facts::fingerprint(&facts::collect(path, today(), false)),
        before
    );
    std::fs::write(repo.join("f0.txt"), "edited\n").unwrap();
    let edited = facts::fingerprint(&facts::collect(path, today(), false));
    assert_ne!(edited, before);
    let modified = std::fs::metadata(repo.join("f0.txt"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::write(repo.join("f0.txt"), "newest\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(repo.join("f0.txt"))
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_ne!(
        facts::fingerprint(&facts::collect(path, today(), false)),
        edited,
        "changing an already dirty file must count even when line counts stay equal"
    );
}

#[test]
fn fingerprint_tracks_staged_versions_with_unchanged_working_file() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(tmp.path());
    let path = repo.to_str().unwrap();
    let modified = std::fs::metadata(repo.join("f0.txt"))
        .unwrap()
        .modified()
        .unwrap();
    // The working file and status stay the same while its staged version changes.
    let stage = |contents: &str| {
        std::fs::write(repo.join("f0.txt"), contents).unwrap();
        common::git(&repo, &["add", "f0.txt"]);
        std::fs::write(repo.join("f0.txt"), "working\n").unwrap();
        std::fs::File::options()
            .write(true)
            .open(repo.join("f0.txt"))
            .unwrap()
            .set_modified(modified)
            .unwrap();
    };
    stage("staged one\n");
    let first_stage = facts::collect(path, today(), false);
    stage("staged two\n");
    let second_stage = facts::collect(path, today(), false);
    assert_ne!(
        first_stage["dirty"]["fingerprint"],
        second_stage["dirty"]["fingerprint"]
    );
    assert_ne!(
        facts::fingerprint(&first_stage),
        facts::fingerprint(&second_stage)
    );
}

#[test]
fn fingerprint_tracks_unresolved_index_stages() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(tmp.path(), "conflict", &[]);
    commit(
        &repo,
        &[("conflict.txt", Some("base\n"))],
        "2026-09-27T10:00:00",
    );
    common::git(&repo, &["checkout", "-q", "-b", "side"]);
    commit(
        &repo,
        &[("conflict.txt", Some("side\n"))],
        "2026-09-28T10:00:00",
    );
    common::git(&repo, &["checkout", "-q", "main"]);
    commit(
        &repo,
        &[("conflict.txt", Some("main\n"))],
        "2026-09-29T10:00:00",
    );
    assert!(!Command::new("git")
        .args(["merge", "side"])
        .current_dir(&repo)
        .output()
        .unwrap()
        .status
        .success());
    let path = repo.to_str().unwrap();
    let first = facts::collect(path, today(), false);
    assert_eq!(first["ok"], true);
    let output = Command::new("git")
        .args(["rev-parse", "HEAD~1:conflict.txt"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(output.status.success());
    let base = String::from_utf8(output.stdout).unwrap();
    let mut update = Command::new("git")
        .args(["update-index", "--index-info"])
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        update.stdin.take().unwrap(),
        "100644 {} 2\tconflict.txt",
        base.trim()
    )
    .unwrap();
    assert!(update.wait().unwrap().success());
    let second = facts::collect(path, today(), false);
    assert_eq!(second["ok"], true);
    assert_ne!(
        first["dirty"]["fingerprint"],
        second["dirty"]["fingerprint"]
    );
}

#[test]
fn facts_count_staged_and_unterminated_lines_before_the_first_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(tmp.path(), "empty", &[]);
    std::fs::write(repo.join("staged.txt"), "one\ntwo").unwrap();
    common::git(&repo, &["add", "staged.txt"]);
    std::fs::write(repo.join("untracked.txt"), "three").unwrap();
    let f = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(f["dirty"]["files"], 2);
    assert_eq!(f["dirty"]["lines_added"], 3);
    assert_eq!(f["dirty"]["lines_removed"], 0);
    let subdir = repo.join("subdir");
    std::fs::create_dir(&subdir).unwrap();
    let nested = facts::collect(subdir.to_str().unwrap(), today(), false);
    assert_eq!(
        nested["dirty"], f["dirty"],
        "repository-relative paths stay valid when registered from a subdirectory"
    );
}

#[test]
fn measuring_a_repository_never_refreshes_its_index() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(tmp.path());
    let index = repo.join(".git/index");
    let before = std::fs::read(&index).unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(repo.join("f0.txt"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    let f = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(f["dirty"]["files"], 0);
    assert_eq!(std::fs::read(index).unwrap(), before);

    // A path named like a revision must not make the diff fail and report zero lines.
    std::fs::write(repo.join("HEAD"), "marker\n").unwrap();
    std::fs::write(repo.join("f0.txt"), "changed\nand more\n").unwrap();
    let f = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(f["dirty"]["files"], 2);
    assert_eq!(f["dirty"]["lines_added"], 3);
    assert_eq!(f["dirty"]["lines_removed"], 1);
}

#[test]
fn linked_checkout_excludes_itself_and_tracks_dirty_work_in_other_checkouts() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(tmp.path());
    let copy_source: String = (0..100)
        .map(|i| format!("line {i} original text\n"))
        .collect();
    std::fs::write(repo.join("source.txt"), &copy_source).unwrap();
    common::git(&repo, &["add", "source.txt"]);
    common::git(&repo, &["commit", "-q", "-m", "copy source"]);
    common::git(&repo, &["config", "status.renames", "copies"]);
    let wt = tmp.path().join("linked");
    common::git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    );
    let linked_path = wt.to_str().unwrap();
    let before = facts::collect(linked_path, today(), false);
    assert_eq!(before["worktrees"]["count"], 1);
    assert_eq!(before["worktrees"]["items"][0]["branch"], "main");
    common::git(&repo, &["mv", "f0.txt", "renamed.txt"]);
    std::fs::write(repo.join("renamed.txt"), "other checkout\n").unwrap();
    std::fs::write(repo.join("copied.txt"), &copy_source).unwrap();
    std::fs::write(
        repo.join("source.txt"),
        copy_source.replace("line 0 original", "line 0 changed"),
    )
    .unwrap();
    common::git(&repo, &["add", "source.txt", "copied.txt"]);
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(status.status.success());
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(status.contains("C  copied.txt\0source.txt\0"), "{status:?}");
    assert!(status.contains("RM renamed.txt\0f0.txt\0"), "{status:?}");
    let after = facts::collect(linked_path, today(), false);
    assert_ne!(facts::fingerprint(&before), facts::fingerprint(&after));
    assert_eq!(after["worktrees"]["items"][0]["dirty_files"], 3);
    let direct = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(direct["dirty"]["files"], 3);
    assert_eq!(direct["dirty"]["modified"], 3);
    assert_eq!(direct["dirty"]["untracked"], 0);
    assert_eq!(
        direct["dirty"]["fingerprint"],
        after["worktrees"]["items"][0]["dirty_fingerprint"]
    );
    assert_eq!(after["night"]["uncommitted_touched"], 3);
    assert_eq!(after["collisions"], serde_json::json!([]));
    let second_before = facts::fingerprint(&after);
    std::fs::write(repo.join("renamed.txt"), "next variation\n").unwrap();
    assert_ne!(
        second_before,
        facts::fingerprint(&facts::collect(linked_path, today(), false))
    );
}

#[test]
fn linked_checkout_fingerprint_tracks_staged_versions() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(tmp.path());
    let wt = tmp.path().join("linked");
    common::git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    );
    let linked_path = wt.to_str().unwrap();
    std::fs::write(repo.join("f0.txt"), "first staged\n").unwrap();
    common::git(&repo, &["add", "f0.txt"]);
    std::fs::write(repo.join("f0.txt"), "working version\n").unwrap();
    let modified = std::fs::metadata(repo.join("f0.txt"))
        .unwrap()
        .modified()
        .unwrap();
    let first = facts::collect(linked_path, today(), false);
    std::fs::write(repo.join("f0.txt"), "second staged\n").unwrap();
    common::git(&repo, &["add", "f0.txt"]);
    std::fs::write(repo.join("f0.txt"), "working version\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(repo.join("f0.txt"))
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let second = facts::collect(linked_path, today(), false);
    assert_ne!(facts::fingerprint(&first), facts::fingerprint(&second));
    let direct = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(
        direct["dirty"]["fingerprint"],
        second["worktrees"]["items"][0]["dirty_fingerprint"]
    );
}

#[test]
fn non_repositories_report_why() {
    let tmp = tempfile::tempdir().unwrap();
    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    assert_eq!(
        facts::collect(plain.to_str().unwrap(), today(), false)["error"],
        "not_a_git_repository"
    );
    assert_eq!(
        facts::collect(tmp.path().join("gone").to_str().unwrap(), today(), false)["error"],
        "path_missing"
    );
    let broken = common::demo_repo(tmp.path());
    std::fs::write(broken.join(".git/index"), "invalid index").unwrap();
    let f = facts::collect(broken.to_str().unwrap(), today(), false);
    assert_eq!(
        f["ok"], false,
        "a failed git status must never appear clean"
    );
    assert_eq!(f["is_git"], true);
    assert_eq!(f["error"], "git_status_failed");

    let broken_history = common::make_repo(tmp.path(), "broken-history", &["2026-09-29T10:00:00"]);
    let original = std::process::Command::new("git")
        .args(["cat-file", "commit", "HEAD"])
        .current_dir(&broken_history)
        .output()
        .unwrap();
    assert!(original.status.success());
    let commit = String::from_utf8(original.stdout).unwrap().replacen(
        '\n',
        &format!("\nparent {}\n", "1".repeat(40)),
        1,
    );
    std::fs::write(broken_history.join(".git/missing-parent-commit"), commit).unwrap();
    let object = std::process::Command::new("git")
        .args([
            "hash-object",
            "-w",
            "-t",
            "commit",
            ".git/missing-parent-commit",
        ])
        .current_dir(&broken_history)
        .output()
        .unwrap();
    assert!(object.status.success());
    let head = String::from_utf8(object.stdout).unwrap();
    common::git(
        &broken_history,
        &["update-ref", "refs/heads/main", head.trim()],
    );
    let f = facts::collect(broken_history.to_str().unwrap(), today(), false);
    assert_eq!(
        f["ok"], false,
        "unreadable history must never report zero commits"
    );
    assert_eq!(f["is_git"], true);
    assert_eq!(f["error"], "git_log_failed");
    assert!(f["commits"].is_null());
}

#[test]
fn facts_show_release_overnight_work_and_colliding_lines_of_work() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(
        tmp.path(),
        "demo",
        &["2026-09-01T10:00:00", "2026-09-29T21:00:00"],
    );
    common::git(&repo, &["tag", "v1.0", "HEAD~1"]);
    common::git_at(
        &repo,
        &["revert", "--no-edit", "HEAD"],
        "2026-09-29T23:00:00",
    );
    // an unmerged branch and a worktree both touch f0.txt, which main also has uncommitted
    common::git(&repo, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(repo.join("f0.txt"), "feature\n").unwrap();
    common::git_at(
        &repo,
        // Subjects are content, never a measured diff summary.
        &["commit", "-q", "-am", "1 file changed, 999 insertions(+)"],
        "2026-09-29T22:00:00",
    );
    common::git(&repo, &["checkout", "-q", "main"]);
    let wt = tmp.path().join("demo-wt");
    common::git(
        &repo,
        &["worktree", "add", "-q", "-b", "wt", wt.to_str().unwrap()],
    );
    std::fs::write(wt.join("f0.txt"), "worktree\n").unwrap();
    std::fs::write(repo.join("f0.txt"), "main\n").unwrap();

    let f = facts::collect(repo.to_str().unwrap(), today(), false);

    let r = &f["release"];
    assert_eq!(
        (
            r["tag"].as_str(),
            r["date"].as_str(),
            r["commits_since"].as_u64()
        ),
        (Some("v1.0"), Some("2026-09-01"), Some(2))
    );
    let n = &f["night"];
    assert_eq!(
        (n["commits"].as_u64(), n["reverts"].as_u64()),
        (Some(3), Some(1))
    );
    assert_eq!(
        (
            n["lines_added"].as_u64(),
            n["lines_removed"].as_u64(),
            n["uncommitted_touched"].as_u64()
        ),
        (Some(2), Some(2), Some(2))
    );
    let pairs: Vec<(String, String, Value)> = f["collisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                format!(
                    "{}:{}",
                    c["a"]["kind"].as_str().unwrap(),
                    c["a"]["name"].as_str().unwrap()
                ),
                format!(
                    "{}:{}",
                    c["b"]["kind"].as_str().unwrap(),
                    c["b"]["name"].as_str().unwrap()
                ),
                c["files"].clone(),
            )
        })
        .collect();
    let f0 = serde_json::json!(["f0.txt"]);
    assert_eq!(
        pairs,
        [
            ("main:main".into(), "worktree:wt".into(), f0.clone()),
            ("main:main".into(), "branch:feature".into(), f0.clone()),
            ("worktree:wt".into(), "branch:feature".into(), f0)
        ]
    );
}

/// Commit file contents (None deletes the file) at `date`.
fn commit(repo: &Path, files: &[(&str, Option<&str>)], date: &str) {
    for (path, content) in files {
        let full = repo.join(path);
        match content {
            Some(text) => {
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, text).unwrap();
            }
            None => std::fs::remove_file(full).unwrap(),
        }
    }
    common::git(repo, &["add", "-A"]);
    common::git_at(repo, &["commit", "-q", "-m", date], date);
}

fn measurement_reuse_repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(
        tmp.path(),
        "demo",
        &["2025-09-30T12:00:00", "2026-07-01T10:00:00"],
    );
    commit(
        &repo,
        &[("app.rs", Some("fn main() {}\n"))],
        "2026-09-29T10:00:00",
    );
    (tmp, repo)
}

// Marked values can only come from the previous measurement.
fn marked_measurements(facts: &Value) -> Value {
    let mut facts = facts.clone();
    facts["code"]["source_code"] = 999.into();
    facts["activity"]["days"][1]["added"] = 777.into();
    facts
}

#[test]
fn measurement_reuse_keeps_same_head_and_worktree() {
    let (_tmp, repo) = measurement_reuse_repo();
    let path = repo.to_str().unwrap();
    let first = facts::collect(path, today(), false);
    assert_eq!(first["activity"]["days"][0]["date"], "2025-09-30");
    assert_eq!(first["activity"]["days"][1]["date"], "2026-07-01");

    // the same HEAD and working tree on the same day
    assert_eq!(
        facts::collect_reusing(path, today(), false, Some(&first)),
        first
    );
    let reused = facts::collect_reusing(path, today(), false, Some(&marked_measurements(&first)));
    assert_eq!(
        (
            reused["code"]["source_code"].as_u64(),
            reused["activity"]["days"][1]["added"].as_u64()
        ),
        (Some(999), Some(777))
    );
}

#[test]
fn measurement_reuse_trims_elapsed_days() {
    let (_tmp, repo) = measurement_reuse_repo();
    let path = repo.to_str().unwrap();
    let day = |offset| today() + chrono::Duration::days(offset);
    let first = facts::collect(path, today(), false);
    // later days drop the days that left the window and keep the rest
    for offset in [1, 40] {
        let full = facts::collect(path, day(offset), false);
        assert_eq!(full["activity"]["days"][0]["date"], "2026-07-01");
        assert_eq!(
            facts::collect_reusing(path, day(offset), false, Some(&first)),
            full
        );
        let carried =
            facts::collect_reusing(path, day(offset), false, Some(&marked_measurements(&first)));
        assert_eq!(carried["activity"]["days"][0]["added"], 777);
    }
}

#[test]
fn measurement_reuse_extends_forward_head() {
    let (_tmp, repo) = measurement_reuse_repo();
    let path = repo.to_str().unwrap();
    let day = |offset| today() + chrono::Duration::days(offset);
    let first = facts::collect(path, today(), false);
    // HEAD moved forward: only the new commits are read and the code is counted again
    commit(
        &repo,
        &[("app.rs", Some("fn main() {}\nfn more() {}\n"))],
        "2026-09-30T06:00:00",
    );
    let moved = facts::collect(path, today(), false);
    assert_eq!(moved["activity"]["days"].as_array().unwrap().len(), 4);
    assert_eq!(
        facts::collect_reusing(path, today(), false, Some(&first)),
        moved
    );
    assert_eq!(
        facts::collect_reusing(path, day(1), false, Some(&first)),
        facts::collect(path, day(1), false)
    );
    let extended = facts::collect_reusing(path, today(), false, Some(&marked_measurements(&first)));
    assert_eq!(extended["activity"]["days"][1]["added"], 777);
    assert_eq!(extended["code"], moved["code"]);
}

#[test]
fn measurement_reuse_rejects_rewritten_history() {
    let (_tmp, repo) = measurement_reuse_repo();
    let path = repo.to_str().unwrap();
    commit(
        &repo,
        &[("app.rs", Some("fn main() {}\nfn more() {}\n"))],
        "2026-09-30T06:00:00",
    );
    let moved = facts::collect(path, today(), false);
    // rewritten history is measured again in full
    common::git(&repo, &["reset", "-q", "--hard", "HEAD~2"]);
    commit(
        &repo,
        &[("other.rs", Some("fn other() {}\n"))],
        "2026-09-30T07:00:00",
    );
    assert_eq!(
        facts::collect_reusing(path, today(), false, Some(&marked_measurements(&moved))),
        facts::collect(path, today(), false)
    );
}

#[test]
fn measurement_reuse_rejects_grafted_history() {
    let (_tmp, repo) = measurement_reuse_repo();
    let path = repo.to_str().unwrap();
    // so is a history whose shape changed under the same HEAD
    let before = facts::collect(path, today(), false);
    common::git(&repo, &["replace", "--graft", "HEAD~1"]);
    let grafted = facts::collect_reusing(path, today(), false, Some(&marked_measurements(&before)));
    let full = facts::collect(path, today(), false);
    assert_ne!(full["activity"], before["activity"]);
    assert_eq!(grafted["activity"], full["activity"]);
}

#[test]
fn hotspots_rank_recent_changes_to_source_files_that_still_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(tmp.path(), "demo", &[]);
    // before the 90-day window
    commit(&repo, &[("main.rs", Some("a\n"))], "2026-07-01T10:00:00");
    commit(&repo, &[("main.rs", Some("a\nb\n"))], "2026-07-02T10:00:00");
    commit(
        &repo,
        &[
            ("main.rs", Some("a\nb\nc\n")),
            ("lib.rs", Some("x\n")),
            ("README.md", Some("# r\n")),
            ("config.toml", Some("a = 1\n")),
            ("gone.rs", Some("g\n")),
            ("node_modules/dep.js", Some("d();\n")),
        ],
        "2026-08-01T10:00:00",
    );
    commit(
        &repo,
        &[
            ("gone.rs", None),
            ("lib.rs", Some("y\n")),
            ("README.md", Some("# r2\n")),
            ("node_modules/dep.js", Some("e();\n")),
        ],
        "2026-09-01T10:00:00",
    );
    commit(&repo, &[("dropped.rs", Some("d\n"))], "2026-09-05T10:00:00");
    // a side line of work merged without its changes still counts
    common::git(&repo, &["checkout", "-q", "-b", "side"]);
    commit(
        &repo,
        &[("lib.rs", Some("y\nside\n"))],
        "2026-09-10T10:00:00",
    );
    common::git(&repo, &["checkout", "-q", "main"]);
    commit(
        &repo,
        &[("main.rs", Some("a\nb\nc\nd\n"))],
        "2026-09-11T10:00:00",
    );
    common::git_at(
        &repo,
        &["merge", "-q", "-s", "ours", "--no-edit", "side"],
        "2026-09-12T10:00:00",
    );
    commit(
        &repo,
        &[("main.rs", Some("a\nb\nc\nd\ne\n"))],
        "2026-09-20T10:00:00",
    );
    // deleted in the working tree, not yet committed
    std::fs::remove_file(repo.join("dropped.rs")).unwrap();

    let f = facts::collect(repo.to_str().unwrap(), today(), false);

    let a = &f["activity"];
    assert_eq!(a["hotspots_from"], "2026-07-02");
    assert_eq!(
        a["hotspots"],
        serde_json::json!([
            {"path":"main.rs","changes":4,"added":4,"removed":0},
            {"path":"lib.rs","changes":3,"added":3,"removed":1},
        ])
    );

    // User display preferences must not drop root diffs or count merge diffs twice.
    common::git(&repo, &["config", "log.showRoot", "false"]);
    common::git(&repo, &["config", "log.diffMerges", "separate"]);
    let configured = facts::collect(repo.to_str().unwrap(), today(), false);
    assert_eq!(configured["activity"], f["activity"]);
}
