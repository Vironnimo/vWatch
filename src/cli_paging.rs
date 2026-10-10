//! Immutable output snapshots: continuing a long result never repeats its command.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::service::{Result, WatchtowerError};

/// Leave room for the dispatcher's instance note within the 10,000-character contract.
pub const PAGE_LIMIT: usize = 8_500;
const BODY_LIMIT: usize = 7_500;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
struct Snapshot {
    captured_at: String,
    text: String,
}

pub fn first(data_dir: &Path, text: &str) -> Result<String> {
    if text.chars().count() <= PAGE_LIMIT {
        return Ok(text.to_owned());
    }
    let snapshot = Snapshot {
        captured_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        text: text.to_owned(),
    };
    let bytes = serde_json::to_vec(&snapshot).expect("output snapshot is serializable");
    let token = format!("{:x}", Sha1::digest(&bytes));
    save(data_dir, &token, &bytes)?;
    page(data_dir, &snapshot, &token, 0)
}

pub fn resume(data_dir: &Path, token: &str, start: usize) -> Result<String> {
    let path = snapshot_path(data_dir, token)?;
    let bytes = fs::read(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            WatchtowerError::new("output_missing", "The saved output is unavailable in this instance. No command was repeated.")
                .hint("Use the same Watchtower instance that printed the continuation. If the snapshot was removed, read the current state with the original command's show or status command before repeating a change.")
        } else {
            io_error("read", error)
        }
    })?;
    if format!("{:x}", Sha1::digest(&bytes)) != token {
        return Err(WatchtowerError::new("output_changed", "The saved output changed on disk. No command was repeated.")
            .hint("Read the current state with the original command's show or status command. Do not repeat a change just to recover its output."));
    }
    let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(|_| {
        WatchtowerError::new("output_invalid", "The saved output cannot be read. No command was repeated.")
            .hint("Read the current state with the original command's show or status command before repeating a change.")
    })?;
    page(data_dir, &snapshot, token, start)
}

fn snapshot_path(data_dir: &Path, token: &str) -> Result<PathBuf> {
    if token.len() != 40
        || !token
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(WatchtowerError::new("invalid_output_token", "The output token must be the 40 lowercase hexadecimal characters from a continuation. No command was repeated.")
            .hint("Copy the full command from the earlier Next: line; the token is not a file path."));
    }
    Ok(data_dir.join("output").join(format!("{token}.json")))
}

fn save(data_dir: &Path, token: &str, bytes: &[u8]) -> Result<()> {
    let path = snapshot_path(data_dir, token)?;
    let dir = path.parent().expect("snapshot has parent");
    fs::create_dir_all(dir).map_err(|error| io_error("save", error))?;
    if path.exists() {
        return check_existing(&path, bytes);
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".{token}-{}-{sequence}.tmp", std::process::id()));
    let saved = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // All writers for this path have the same complete content hash. On Windows
        // rename refuses an existing target; inspect it rather than replacing it.
        fs::rename(&temp, &path)
    })();
    if let Err(error) = saved {
        let _ = fs::remove_file(&temp);
        if path.exists() {
            return check_existing(&path, bytes);
        }
        return Err(io_error("save", error));
    }
    Ok(())
}

fn check_existing(path: &Path, bytes: &[u8]) -> Result<()> {
    if fs::read(path).map_err(|error| io_error("read", error))? == bytes {
        return Ok(());
    }
    Err(WatchtowerError::new("output_conflict", "The output snapshot could not be saved without replacing different content. The original command has already run.")
        .hint("Read the current state with the original command's show or status command; do not repeat a change to recover its output."))
}

fn io_error(action: &str, error: std::io::Error) -> WatchtowerError {
    let cause = match error.kind() {
        std::io::ErrorKind::PermissionDenied => "access to the output folder was denied",
        std::io::ErrorKind::StorageFull => "the output disk is full",
        _ => "the output folder could not be accessed",
    };
    WatchtowerError::new("output_unavailable", format!("Could not {action} the saved output: {cause}. The original command has already run; this does not undo its changes."))
        .hint("Restore access and available space in the instance's data folder. Read the current state with the original command's show or status command before repeating a change.")
}

fn page(data_dir: &Path, snapshot: &Snapshot, token: &str, start: usize) -> Result<String> {
    let chars: Vec<char> = snapshot.text.chars().collect();
    let total = chars.len();
    if start >= total {
        return Err(WatchtowerError::new("invalid_output_start", format!("Output offset {start} is outside the saved text of {total} characters. No command was repeated."))
            .hint(format!("Use the zero-based offset from the continuation, or start again: {}", crate::cli::scoped_command(data_dir, &format!("watchtower output {token} --start 0")))));
    }
    let continuation = crate::cli::scoped_command(
        data_dir,
        &format!("watchtower output {token} --start {total}"),
    );
    let undo = snapshot
        .text
        .lines()
        .rev()
        .find(|line| line.starts_with("Undo: watchtower "))
        .map(|line| {
            format!(
                "Undo: {}",
                crate::cli::scoped_command(data_dir, line.trim_start_matches("Undo: "))
            )
        });
    let undo_size = undo
        .as_ref()
        .map_or(0, |command| command.chars().count().min(400) + 1);
    let next = snapshot
        .text
        .lines()
        .rev()
        .find(|line| line.starts_with("Next: watchtower "))
        .map(|line| {
            format!(
                "Next: {}",
                crate::cli::scoped_command(data_dir, line.trim_start_matches("Next: "))
            )
        });
    let next_size = next.as_ref().map_or(0, |command| command.chars().count());
    let framing = 600 + continuation.chars().count().max(next_size) + undo_size;
    let body_limit = PAGE_LIMIT.saturating_sub(framing).min(BODY_LIMIT);
    if body_limit < 128 {
        return Err(WatchtowerError::new("output_scope_too_long", "The instance path is too long for a safe continuation within the output limit. The original command has already run.")
            .hint("Use the same instance's show or status command with --json to inspect its complete current state. Do not repeat a change to recover its output."));
    }
    let mut end = start;
    let mut size = 2; // Every stored line is data, including stored marker-looking lines.
    let mut newline = None;
    while end < total {
        let cost = 1 + usize::from(chars[end] == '\n') * 2;
        if size + cost > body_limit {
            break;
        }
        size += cost;
        end += 1;
        if chars[end - 1] == '\n' {
            newline = Some(end);
        }
    }
    if end < total {
        if let Some(boundary) = newline.filter(|boundary| *boundary > start + (end - start) / 2) {
            end = boundary;
        }
    }
    let body: String = chars[start..end].iter().collect();
    let source: String = snapshot
        .text
        .lines()
        .next()
        .unwrap_or("Command output")
        .chars()
        .take(180)
        .collect();
    let mut out = if start == 0 {
        format!(
            "{source}\nNote: output is paged; characters 1 to {end} of {total}. Captured {}.\n",
            snapshot.captured_at
        )
    } else {
        format!("Saved output, characters {} to {end} of {total}.\nNote: snapshot captured {}; continuing does not repeat the command.\nSource: {source}\n", start + 1, snapshot.captured_at)
    };
    for part in body.split_inclusive('\n') {
        out.push_str("  ");
        out.push_str(part);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    // Undo is generated by the CLI, never by indented document content. Keep it
    // actionable even when a saved document needs several pages to display.
    if let Some(undo) = undo {
        if undo.chars().count() <= 400 {
            out.push_str(&undo);
            out.push('\n');
        } else {
            out.push_str("Note: the undo command appears in full in the saved output.\n");
        }
    }
    if end < total {
        out.push_str(&format!(
            "Next: {}",
            crate::cli::scoped_command(
                data_dir,
                &format!("watchtower output {token} --start {end}")
            )
        ));
    } else {
        out.push_str("End of saved output.");
        // The final page resumes the original workflow. Earlier pages expose
        // only the output continuation so context is read before the next step.
        if let Some(next) = next {
            out.push('\n');
            out.push_str(&next);
        }
    }
    debug_assert!(out.chars().count() <= PAGE_LIMIT);
    Ok(out)
}
