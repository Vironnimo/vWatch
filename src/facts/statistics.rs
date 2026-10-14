//! Bounded, read-only measurements. Failed or partial scans never mean zero code.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tokei::{CodeStats, Config, LanguageType};

const METHOD: &str = "tokei-15/worktree-v1";
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

pub(super) fn code(repo: &Path) -> Value {
    let Some(paths) = git(
        repo,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    ) else {
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
        if excluded(path) {
            excluded_files += 1;
            continue;
        }
        let full = repo.join(path);
        let meta = match std::fs::symlink_metadata(&full) {
            Ok(meta) => meta,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    incomplete += 1;
                }
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            excluded_files += 1;
            continue;
        }
        if meta.len() > 4_000_000 {
            incomplete += 1;
            continue;
        }
        let Some(language) = LanguageType::from_path(&full, &config) else {
            unsupported += 1;
            continue;
        };
        let Ok(content) = std::fs::read(&full) else {
            incomplete += 1;
            continue;
        };
        bytes += content.len() as u64;
        if content.contains(&0) {
            unsupported += 1;
            continue;
        }
        let header = String::from_utf8_lossy(&content[..content.len().min(1024)]).to_lowercase();
        if header.contains("@generated")
            || header.contains("code generated")
            || header.contains("auto-generated")
        {
            excluded_files += 1;
            continue;
        }
        let stats = language.parse_from_slice(&content, &config);
        add_stats(&mut totals, language, &stats);
        files.push(json!({"path":path,"language":language.to_string(),"category":category(&language.to_string()),"code":stats.summarise().code}));
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

pub(super) fn activity(repo: &Path, today: NaiveDate, head: Option<&str>) -> Value {
    let from = (today - chrono::Duration::days(365)).to_string();
    let Some(log) = head.map_or_else(
        || Some(String::new()),
        |head| {
            git(
                repo,
                &[
                    "log",
                    "--no-renames",
                    "--numstat",
                    "--format=WT:%cs",
                    &format!("--since-as-filter={from}T00:00:00"),
                    head,
                ],
            )
        },
    ) else {
        return json!({"ok":false,"error":"git_log_failed","from":from});
    };
    let mut daily = BTreeMap::<String, (u64, u64, u64)>::new();
    let mut hotspots = BTreeMap::<String, (u64, u64, u64)>::new();
    let mut date = None;
    for line in log.lines() {
        if let Some(value) = line.strip_prefix("WT:") {
            date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .filter(|d| *d <= today && d.to_string() >= from)
                .map(|d| d.to_string());
            if let Some(day) = &date {
                daily.entry(day.clone()).or_default().0 += 1;
            }
        } else if let Some(day) = &date {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() != 3 || excluded(parts[2]) {
                continue;
            }
            if let (Ok(added), Ok(removed)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                let row = daily.entry(day.clone()).or_default();
                row.1 += added;
                row.2 += removed;
                let file = hotspots.entry(parts[2].to_owned()).or_default();
                file.0 += 1;
                file.1 += added;
                file.2 += removed;
            }
        }
    }
    let days:Vec<Value>=daily.into_iter().map(|(date,(commits,added,removed))|json!({"date":date,"commits":commits,"added":added,"removed":removed})).collect();
    let mut hotspots:Vec<Value>=hotspots.into_iter().map(|(path,(changes,added,removed))|json!({"path":path,"changes":changes,"added":added,"removed":removed})).collect();
    hotspots.sort_by_key(|f| std::cmp::Reverse(f["changes"].as_u64().unwrap_or(0)));
    hotspots.truncate(20);
    json!({"ok":true,"from":from,"through":today.to_string(),"head":head,"scope":"HEAD ancestry; merge commits count, merge diffs excluded; binary files excluded",
        "days":days,"hotspots":hotspots,"method":"git-numstat-v1","origin":"reconstructed"})
}
