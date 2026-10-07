mod common;

use chrono::NaiveDate;
use serde_json::Value;
use watchtower::facts;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()
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
    assert!(
        d["oldest_change"].as_str().unwrap() > "2026-09-29T12",
        "{}",
        d["oldest_change"]
    );
    // uncommitted edits count as work
    assert_eq!(
        f["last_work"].as_str().unwrap(),
        chrono::Local::now().date_naive().to_string()
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
    assert_ne!(
        facts::fingerprint(&facts::collect(path, today(), false)),
        before
    );
    let edited = facts::fingerprint(&facts::collect(path, today(), false));
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
}

#[test]
fn linked_checkout_excludes_itself_and_tracks_dirty_work_in_other_checkouts() {
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
    let before = facts::collect(linked_path, today(), false);
    assert_eq!(before["worktrees"]["count"], 1);
    assert_eq!(before["worktrees"]["items"][0]["branch"], "main");
    std::fs::write(repo.join("f0.txt"), "other checkout\n").unwrap();
    let after = facts::collect(linked_path, today(), false);
    assert_ne!(facts::fingerprint(&before), facts::fingerprint(&after));
    assert_eq!(after["night"]["uncommitted_touched"], 1);
    assert_eq!(after["collisions"], serde_json::json!([]));
    let second_before = facts::fingerprint(&after);
    std::fs::write(repo.join("f0.txt"), "next variation\n").unwrap();
    assert_ne!(
        second_before,
        facts::fingerprint(&facts::collect(linked_path, today(), false))
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
        &["commit", "-q", "-am", "feature"],
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
