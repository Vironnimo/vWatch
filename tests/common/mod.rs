#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::{json, Value};
use watchtower::facts;
use watchtower::store::Store;
use watchtower::Watchtower;

pub fn git_at(repo: &Path, args: &[&str], date: &str) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn git(repo: &Path, args: &[&str]) {
    git_at(repo, args, "2026-09-29T10:00:00");
}

/// A repository with one file and commit per date.
pub fn make_repo(root: &Path, name: &str, commits: &[&str]) -> PathBuf {
    let repo = root.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.email", "t@example.com"),
        ("user.name", "Test"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", key, value]);
    }
    for (i, date) in commits.iter().enumerate() {
        std::fs::write(repo.join(format!("f{i}.txt")), format!("line {i}\n")).unwrap();
        git(&repo, &["add", "."]);
        git_at(&repo, &["commit", "-q", "-m", &format!("commit {i}")], date);
    }
    repo
}

pub fn demo_repo(root: &Path) -> PathBuf {
    make_repo(root, "demo", &["2026-09-29T10:00:00"])
}

#[derive(Clone)]
pub struct Clock(Arc<Mutex<DateTime<Utc>>>);

impl Clock {
    pub fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

/// A service on a temporary database with a fixed clock (2026-09-30 08:00 UTC) and no CI lookups.
pub fn service(dir: &Path) -> (Watchtower, Clock) {
    let clock = Clock(Arc::new(Mutex::new(
        Utc.with_ymd_and_hms(2026, 9, 30, 8, 0, 0).unwrap(),
    )));
    let tick = clock.clone();
    let store = Store::open(&dir.join("watchtower.db")).unwrap();
    let svc = Watchtower::with_parts(
        store,
        Box::new(move || *tick.0.lock().unwrap()),
        Box::new(|path, today, _ci| facts::collect(path, today, false)),
    );
    (svc, clock)
}

pub fn card() -> Value {
    json!({
        "one_liner": "Ein Testprojekt",
        "phase": "active",
        "headline": "Alles ruhig",
        "reentry": {"next_step": "Tests laufen lassen"},
        "suggestions": [{"type": "feature", "title": "Dark Mode", "why": "Weil nachts."}],
    })
}

pub fn card_with(overrides: Value) -> Value {
    let mut c = card();
    for (k, v) in overrides.as_object().unwrap() {
        c[k] = v.clone();
    }
    c
}
