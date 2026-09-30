mod common;

use chrono::NaiveDate;
use lookout::facts;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()
}

#[test]
fn facts_describe_history_backup_and_unsaved_work() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::make_repo(tmp.path(), "demo", &["2026-04-01T10:00:00", "2026-09-10T10:00:00", "2026-09-29T10:00:00"]);
    std::fs::write(repo.join("f0.txt"), "changed\nand more\n").unwrap();
    std::fs::write(repo.join("new.txt"), "a\nb\nc\n").unwrap();
    // a file copied in from an old archive keeps its ancient mtime
    let archived = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options().write(true).open(repo.join("new.txt")).unwrap().set_modified(archived).unwrap();

    let f = facts::collect(repo.to_str().unwrap(), today(), false);

    assert_eq!((f["ok"].as_bool(), f["is_git"].as_bool(), f["branch"].as_str()), (Some(true), Some(true), Some("main")));
    let c = &f["commits"];
    assert_eq!(c["total"], 3);
    assert_eq!((c["first"].as_str(), c["last"].as_str()), (Some("2026-04-01"), Some("2026-09-29")));
    assert_eq!((c["last_7d"].as_u64(), c["last_30d"].as_u64(), c["last_90d"].as_u64()), (Some(1), Some(2), Some(2)));
    let weekly: Vec<u64> = c["weekly"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    assert_eq!((weekly.len(), weekly[facts::WEEKS - 1], weekly.iter().sum::<u64>()), (facts::WEEKS, 1, 2));
    assert_eq!((f["has_remote"].as_bool(), f["upstream"].is_null()), (Some(false), true));
    let d = &f["dirty"];
    assert_eq!((d["files"].as_u64(), d["untracked"].as_u64(), d["modified"].as_u64()), (Some(2), Some(1), Some(1)));
    assert_eq!((d["lines_added"].as_u64(), d["lines_removed"].as_u64()), (Some(2 + 3), Some(1)));
    // unsaved work is at risk since the last commit at the earliest
    assert!(d["oldest_change"].as_str().unwrap() >= "2026-09-29", "{}", d["oldest_change"]);
    // uncommitted edits count as work
    assert_eq!(f["last_work"].as_str().unwrap(), chrono::Local::now().date_naive().to_string());
}

#[test]
fn fingerprint_changes_with_new_work() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = common::demo_repo(tmp.path());
    let path = repo.to_str().unwrap();
    let before = facts::fingerprint(&facts::collect(path, today(), false));
    assert_eq!(facts::fingerprint(&facts::collect(path, today(), false)), before);
    std::fs::write(repo.join("f0.txt"), "edited\n").unwrap();
    assert_ne!(facts::fingerprint(&facts::collect(path, today(), false)), before);
}

#[test]
fn non_repositories_report_why() {
    let tmp = tempfile::tempdir().unwrap();
    let plain = tmp.path().join("plain");
    std::fs::create_dir(&plain).unwrap();
    assert_eq!(facts::collect(plain.to_str().unwrap(), today(), false)["error"], "not_a_git_repository");
    assert_eq!(facts::collect(tmp.path().join("gone").to_str().unwrap(), today(), false)["error"], "path_missing");
}
