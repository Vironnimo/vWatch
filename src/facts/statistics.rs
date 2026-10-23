//! Bounded, read-only measurements. Failed or partial scans never mean zero code.
use super::*;
use std::cell::OnceCell;
use std::collections::BTreeMap;
use tokei::{CodeStats, Config, LanguageType};

const METHOD: &str = "tokei-15/worktree-v1";
const ACTIVITY_METHOD: &str = "git-numstat-v2";
const ACTIVITY_DAYS: i64 = 365;
const HOTSPOT_DAYS: i64 = 90;
const HOTSPOTS: usize = 20;
const EXCLUDED: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "vendor",
    "third_party",
    "third-party",
    "dist",
    "build",
    ".next",
    "coverage",
    "generated",
    "__generated__",
    ".gradle",
    ".dart_tool",
];

fn excluded(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.split('/').any(|part| EXCLUDED.contains(&part))
        || lower.ends_with(".lock")
        || lower.ends_with(".min.js")
        || lower.ends_with(".min.css")
        || lower.ends_with(".g.cs")
        || lower.ends_with(".generated.ts")
        || lower.ends_with("package-lock.json")
        || lower.ends_with("pnpm-lock.yaml")
}

fn category(language: &str) -> &'static str {
    match language {
        "Markdown" | "Plain Text" | "Text" | "reStructuredText" | "AsciiDoc" | "LaTeX" => {
            "documentation"
        }
        "JSON" | "JSON5" | "TOML" | "YAML" | "XML" | "INI" | "SVG" | "Csv" | "CSV" => {
            "configuration"
        }
        _ => "source",
    }
}

fn add_stats(
    totals: &mut BTreeMap<String, (usize, CodeStats)>,
    language: LanguageType,
    stats: &CodeStats,
) {
    if category(&language.to_string()) != "source" {
        let row = totals.entry(language.to_string()).or_default();
        row.0 += 1;
        row.1 += stats.summarise();
        return;
    }
    let row = totals.entry(language.to_string()).or_default();
    row.0 += 1;
    row.1.code += stats.code;
    row.1.comments += stats.comments;
    row.1.blanks += stats.blanks;
    for (child, stats) in &stats.blobs {
        add_stats(totals, *child, stats);
    }
}

/// The working tree's tracked and non-ignored untracked files, listed at most once per measurement.
pub(super) struct Files<'a> {
    repo: &'a Path,
    list: OnceCell<Option<String>>,
}

impl<'a> Files<'a> {
    pub(super) fn new(repo: &'a Path) -> Self {
        Files {
            repo,
            list: OnceCell::new(),
        }
    }

    fn list(&self) -> Option<&str> {
        self.list
            .get_or_init(|| {
                git(
                    self.repo,
                    &[
                        "ls-files",
                        "-z",
                        "--cached",
                        "--others",
                        "--exclude-standard",
                    ],
                )
            })
            .as_deref()
    }
}

/// How the code scan treats one listed path.
enum Scan {
    Missing,
    Unread,
    Excluded,
    Unsupported,
    Counted(LanguageType, Vec<u8>),
}

/// Classify one listed path; also returns the bytes read, which count toward the scan budget.
fn scan_file(repo: &Path, path: &str, config: &Config) -> (Scan, u64) {
    if excluded(path) {
        return (Scan::Excluded, 0);
    }
    let full = repo.join(path);
    let meta = match std::fs::symlink_metadata(&full) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Scan::Missing, 0),
        Err(_) => return (Scan::Unread, 0),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return (Scan::Excluded, 0);
    }
    if meta.len() > 4_000_000 {
        return (Scan::Unread, 0);
    }
    let Some(language) = LanguageType::from_path(&full, config) else {
        return (Scan::Unsupported, 0);
    };
    let Ok(content) = std::fs::read(&full) else {
        return (Scan::Unread, 0);
    };
    let read = content.len() as u64;
    if content.contains(&0) {
        return (Scan::Unsupported, read);
    }
    let header = String::from_utf8_lossy(&content[..content.len().min(1024)]).to_lowercase();
    if header.contains("@generated")
        || header.contains("code generated")
        || header.contains("auto-generated")
    {
        return (Scan::Excluded, read);
    }
    (Scan::Counted(language, content), read)
}

/// Count the working tree's code. `previous` is a scan of the same HEAD and uncommitted state;
/// a complete one is returned as it is, because the files it read are unchanged.
pub(super) fn code(repo: &Path, files: &Files, previous: Option<&Value>) -> Value {
    if let Some(code) = previous.filter(|c| c["ok"] == true && c["method"] == METHOD) {
        return code.clone();
    }
    let Some(paths) = files.list() else {
        return json!({"ok":false,"error":"file_list_failed","method":METHOD});
    };
    let started = Instant::now();
    let mut totals = BTreeMap::<String, (usize, CodeStats)>::new();
    let mut files = Vec::new();
    let mut excluded_files = 0;
    let mut unsupported = 0;
    let mut incomplete = 0;
    let mut bytes = 0u64;
    let config = Config::default();
    let paths: BTreeSet<&str> = paths.split('\0').filter(|p| !p.is_empty()).collect();
    for (index, path) in paths.iter().enumerate() {
        if started.elapsed() > Duration::from_secs(8) || bytes > 100_000_000 || index >= 50_000 {
            incomplete += paths.len() - index;
            break;
        }
        let (scan, read) = scan_file(repo, path, &config);
        bytes += read;
        match scan {
            Scan::Missing => {}
            Scan::Unread => incomplete += 1,
            Scan::Excluded => excluded_files += 1,
            Scan::Unsupported => unsupported += 1,
            Scan::Counted(language, content) => {
                let stats = language.parse_from_slice(&content, &config);
                add_stats(&mut totals, language, &stats);
                files.push(json!({"path":path,"language":language.to_string(),"category":category(&language.to_string()),"code":stats.summarise().code}));
            }
        }
    }
    let languages: Vec<Value> = totals.into_iter().map(|(name,(files,stats))| json!({
        "language":name,"category":category(&name),"files":files,"code":stats.code,"comments":stats.comments,"blanks":stats.blanks,
    })).collect();
    let source_code: u64 = languages
        .iter()
        .filter(|l| l["category"] == "source")
        .filter_map(|l| l["code"].as_u64())
        .sum();
    files.sort_by_key(|f| std::cmp::Reverse(f["code"].as_u64().unwrap_or(0)));
    let counted_files = files.len();
    files.truncate(20);
    json!({"ok":incomplete==0,"partial":incomplete>0,"method":METHOD,"scope":"working_tree",
        "source_code":source_code,"files":counted_files,"languages":languages,"largest_files":files,
        "excluded_files":excluded_files,"unsupported_files":unsupported,"unread_files":incomplete,
        "exclusions":"gitignored untracked files; dependency/build/generated directories, lockfiles, minified files and generated headers; symlinks",
        "percentage_basis":"source code lines; documentation and configuration excluded"})
}

/// Commits, added and removed lines per committer date.
type Days = BTreeMap<String, (u64, u64, u64)>;

fn day(value: &Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.as_str()?, "%Y-%m-%d").ok()
}

/// `git log --numstat` over `revs`, limited the way the activity window is.
fn numstat(repo: &Path, from: NaiveDate, revs: &[&str]) -> Option<String> {
    let since = format!("--since-as-filter={from}T00:00:00");
    let mut args = vec![
        "log",
        "--no-renames",
        "--numstat",
        "--root",
        "--diff-merges=off",
        "--no-ext-diff",
        "--no-textconv",
        "--format=WT:%cs",
        &since,
    ];
    args.extend_from_slice(revs);
    git(repo, &args)
}

/// Add the commits of a `numstat` output whose committer date lies in [from, today].
fn add_days(days: &mut Days, log: &str, from: NaiveDate, today: NaiveDate) {
    let mut date = None;
    for line in log.lines() {
        if let Some(value) = line.strip_prefix("WT:") {
            date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .filter(|d| *d <= today && *d >= from)
                .map(|d| d.to_string());
            if let Some(day) = &date {
                days.entry(day.clone()).or_default().0 += 1;
            }
        } else if let Some(day) = &date {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() != 3 || excluded(parts[2]) {
                continue;
            }
            if let (Ok(added), Ok(removed)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                let row = days.entry(day.clone()).or_default();
                row.1 += added;
                row.2 += removed;
            }
        }
    }
}

/// Git's own reading of the window starts, as epoch seconds: `--since-as-filter` keeps commits
/// whose committer time is at least this.
fn thresholds(repo: &Path, a: NaiveDate, b: NaiveDate) -> Option<(i64, i64)> {
    let out = git(
        repo,
        &[
            "rev-parse",
            &format!("--since={a}T00:00:00"),
            &format!("--since={b}T00:00:00"),
        ],
    )?;
    let mut ages = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("--max-age=")?.parse().ok());
    Some((ages.next()?, ages.next()?))
}

/// The previous measurement's daily series carried forward to `head` and `today`, exactly as a full
/// scan would count it. None when that cannot be shown: another counting method, history that no
/// longer contains the old head the same way, or commits that enter or leave the window other than
/// as whole days at its start.
fn carried_days(
    repo: &Path,
    today: NaiveDate,
    head: &str,
    history: &History,
    previous: &Value,
) -> Option<Days> {
    let before = &previous["activity"];
    if before["ok"] != true
        || before["method"] != ACTIVITY_METHOD
        || before["head"] != previous["head"]
    {
        return None;
    }
    let old_head = before["head"].as_str()?;
    let (old_from, through) = (day(&before["from"])?, day(&before["through"])?);
    let from = today - chrono::Duration::days(ACTIVITY_DAYS);
    if today < through || old_from != through - chrono::Duration::days(ACTIVITY_DAYS) {
        return None;
    }
    // Rewritten history, grafts or a deepened shallow clone change what the old head reaches.
    let ancestry = history.ancestry(old_head)?;
    let dated = ancestry
        .iter()
        .filter(|h| history.0[**h].date.is_some())
        .count();
    if previous["commits"]["total"].as_u64() != Some(dated as u64) {
        return None;
    }
    let mut days = Days::new();
    for entry in before["days"].as_array()? {
        let counts = (
            entry["commits"].as_u64()?,
            entry["added"].as_u64()?,
            entry["removed"].as_u64()?,
        );
        days.insert(day(&entry["date"])?.to_string(), counts);
    }
    if today != through {
        let (old_since, since) = thresholds(repo, old_from, from)?;
        let counted = |c: &Commit, since: i64, until: NaiveDate| {
            c.time >= since && c.date.is_some_and(|d| d >= from && d <= until)
        };
        if ancestry.iter().any(|h| {
            let c = &history.0[*h];
            counted(c, old_since, through) != counted(c, since, today)
        }) {
            return None;
        }
        let first = from.to_string();
        days.retain(|date, _| *date >= first);
    }
    if old_head != head {
        let log = numstat(repo, from, &[head, &format!("^{old_head}")])?;
        add_days(&mut days, &log, from, today);
    }
    Some(days)
}

/// Within the last 90 days, the source files changed by the most commits that the code scan would
/// count in the working tree today; lines come from text diffs.
fn hotspots(repo: &Path, today: NaiveDate, head: &str, files: &Files) -> Option<Vec<Value>> {
    let from = today - chrono::Duration::days(HOTSPOT_DAYS);
    let since = format!("--since-as-filter={from}T00:00:00");
    let within = |value: &str| {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok_and(|d| d >= from && d <= today)
    };
    // Git quotes paths with control characters, so \x01 marks commit headers unambiguously.
    let log = git(
        repo,
        &[
            "log",
            "--no-renames",
            "--name-only",
            "--root",
            "--diff-merges=off",
            "--no-ext-diff",
            "--no-textconv",
            "--format=%x01%cs",
            &since,
            head,
        ],
    )?;
    let mut changes = BTreeMap::<&str, u64>::new();
    let mut counting = false;
    for line in log.lines() {
        if let Some(value) = line.strip_prefix('\u{1}') {
            counting = within(value);
        } else if counting && !line.is_empty() {
            *changes.entry(line).or_default() += 1;
        }
    }
    let listed: HashSet<&str> = files
        .list()?
        .split('\0')
        .filter(|p| !p.is_empty())
        .collect();
    let config = Config::default();
    let mut ranked: Vec<(&str, u64)> = changes.into_iter().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let top: Vec<(&str, u64)> = ranked
        .into_iter()
        .filter(|(path, _)| {
            listed.contains(path)
                && matches!(scan_file(repo, path, &config).0,
                    Scan::Counted(language, _) if category(&language.to_string()) == "source")
        })
        .take(HOTSPOTS)
        .collect();
    let mut lines = HashMap::<String, (u64, u64)>::new();
    if !top.is_empty() {
        // Full history keeps side-branch commits that path-limited simplification would hide.
        let mut args = vec![
            "--literal-pathspecs",
            "log",
            "--no-renames",
            "--full-history",
            "--numstat",
            "--root",
            "--diff-merges=off",
            "--no-ext-diff",
            "--no-textconv",
            "--format=%x01%cs",
            &since,
            head,
            "--",
        ];
        args.extend(top.iter().map(|(path, _)| *path));
        let mut counting = false;
        for line in git(repo, &args)?.lines() {
            if let Some(value) = line.strip_prefix('\u{1}') {
                counting = within(value);
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if let (true, [added, removed, path]) = (counting, parts.as_slice()) {
                if let (Ok(added), Ok(removed)) = (added.parse::<u64>(), removed.parse::<u64>()) {
                    let row = lines.entry((*path).to_owned()).or_default();
                    row.0 += added;
                    row.1 += removed;
                }
            }
        }
    }
    Some(
        top.into_iter()
            .map(|(path, changes)| {
                let (added, removed) = lines.get(path).copied().unwrap_or_default();
                json!({"path":path,"changes":changes,"added":added,"removed":removed})
            })
            .collect(),
    )
}

/// Reconstruct daily activity from HEAD's ancestry. `previous` holds the folder's last readable facts
/// on the same branch; `unchanged` says they measured the same HEAD and uncommitted state.
pub(super) fn activity(
    repo: &Path,
    today: NaiveDate,
    head: Option<&str>,
    history: Option<&History>,
    previous: Option<&Value>,
    unchanged: bool,
    files: &Files,
) -> Value {
    let from = today - chrono::Duration::days(ACTIVITY_DAYS);
    let hotspots_from = today - chrono::Duration::days(HOTSPOT_DAYS);
    let failed = || json!({"ok":false,"error":"git_log_failed","from":from.to_string()});
    let (days, hotspots) = match head {
        None => (Days::new(), Vec::new()),
        Some(head) => {
            let carried = previous
                .zip(history)
                .and_then(|(p, h)| carried_days(repo, today, head, h, p));
            // Hotspots also depend on which files exist, so they need the same tree and day.
            let kept = previous.map(|p| &p["activity"]).filter(|a| {
                carried.is_some()
                    && unchanged
                    && a["head"] == head
                    && a["through"] == today.to_string()
                    && a["hotspots_from"] == hotspots_from.to_string()
            });
            let Some(days) = carried.or_else(|| {
                let mut days = Days::new();
                add_days(&mut days, &numstat(repo, from, &[head])?, from, today);
                Some(days)
            }) else {
                return failed();
            };
            let hotspots = match kept.and_then(|a| a["hotspots"].as_array()) {
                Some(kept) => kept.clone(),
                None => match self::hotspots(repo, today, head, files) {
                    Some(hotspots) => hotspots,
                    None => return failed(),
                },
            };
            (days, hotspots)
        }
    };
    let days:Vec<Value>=days.into_iter().map(|(date,(commits,added,removed))|json!({"date":date,"commits":commits,"added":added,"removed":removed})).collect();
    json!({"ok":true,"from":from.to_string(),"through":today.to_string(),"head":head,
        "scope":"HEAD ancestry; merge commits count, merge diffs excluded; binary files excluded; hotspots: last 90 days, source files the code scan counts today",
        "days":days,"hotspots":hotspots,"hotspots_from":hotspots_from.to_string(),"method":ACTIVITY_METHOD,"origin":"reconstructed"})
}
