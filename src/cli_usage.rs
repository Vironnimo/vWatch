//! Local command metadata. Values, IDs, documents and result text never enter this log.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::service::{Result, WatchtowerError};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const RETAIN_EVENTS: usize = 20_000;

#[derive(Clone, Serialize, Deserialize)]
struct Event {
    call: String,
    at: String,
    command: String,
    flags: Vec<String>,
    targets: usize,
    process: u32,
    parent_process: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_characters: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
}

pub struct Ticket {
    path: PathBuf,
    started: Instant,
    event: Event,
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("cli-usage.jsonl")
}

/// Logging is best-effort and cannot turn a successful write into a reported failure.
/// The caller supplies parser-owned names, never tokens copied from user arguments.
pub fn begin(data_dir: &Path, command: &str, flags: &[String], targets: usize) -> Option<Ticket> {
    if !data_dir.is_dir() {
        return None;
    }
    let now = Utc::now();
    let event = Event {
        call: format!(
            "{}-{}-{}",
            std::process::id(),
            now.timestamp_nanos_opt().unwrap_or(0),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ),
        at: now.to_rfc3339(),
        command: command.to_owned(),
        flags: flags
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        targets,
        process: std::process::id(),
        parent_process: parent_process(),
        exit_code: None,
        duration_ms: None,
        output_characters: None,
        outcome: None,
    };
    let path = path(data_dir);
    append(&path, &event).ok()?;
    Some(Ticket {
        path,
        started: Instant::now(),
        event,
    })
}

pub fn finish(ticket: Option<Ticket>, exit_code: i32, output_characters: usize) {
    finish_outcome(ticket, exit_code, output_characters, None);
}

pub fn finish_outcome(
    ticket: Option<Ticket>,
    exit_code: i32,
    output_characters: usize,
    outcome: Option<&str>,
) {
    if let Some(mut ticket) = ticket {
        ticket.event.exit_code = Some(exit_code);
        ticket.event.duration_ms =
            Some(ticket.started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        ticket.event.output_characters = Some(output_characters);
        ticket.event.outcome = outcome
            .filter(|value| matches!(*value, "not_finished" | "unknown"))
            .map(str::to_owned);
        let _ = append(&ticket.path, &ticket.event);
    }
}

fn append(path: &Path, event: &Event) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    // No blocking lock: logging never extends an operation's deadline.
    file.try_lock().map_err(std::io::Error::other)?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    file.write_all(&line)
}

fn events(data_dir: &Path) -> Result<(Vec<Event>, usize, bool)> {
    let file = match File::open(path(data_dir)) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0, false)),
        Err(_) => return Err(WatchtowerError::new("usage_unreadable", "The local command metadata log cannot be read. No project or document was changed.")
            .hint("Check read access to the cli-usage.jsonl file in the selected data folder. The database remains separate from this optional log.")),
    };
    let mut retained = std::collections::VecDeque::new();
    let mut invalid = 0;
    let mut truncated = false;
    for line in BufReader::new(file).lines() {
        match line
            .ok()
            .and_then(|line| serde_json::from_str::<Event>(&line).ok())
        {
            Some(event) => {
                if retained.len() == RETAIN_EVENTS {
                    retained.pop_front();
                    truncated = true;
                }
                retained.push_back(event);
            }
            None => invalid += 1,
        }
    }
    let mut calls = BTreeMap::<String, Event>::new();
    for event in retained {
        calls.insert(event.call.clone(), event);
    }
    // A metadata read logs its start before reading this file. That read is
    // still in progress, not a lost mutation requiring recovery. Keep it in the
    // log, but leave it out of its own report until completion is recorded.
    let mut calls: Vec<_> = calls
        .into_values()
        .filter(|event| {
            !(event.process == std::process::id()
                && event.exit_code.is_none()
                && matches!(event.command.as_str(), "activity" | "usage"))
        })
        .collect();
    calls.sort_by(|a, b| a.at.cmp(&b.at).then(a.call.cmp(&b.call)));
    Ok((calls, invalid, truncated))
}

pub fn report(data_dir: &Path, since: Option<&str>) -> Result<Value> {
    let since = since.map(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
        WatchtowerError::new("invalid_date", "The usage start date must be an ISO date (YYYY-MM-DD). No data was changed.")
            .hint("Supply an unambiguous calendar date, or omit --since for the retained metadata. Dates select recorded calls on or after that UTC date.")
    })).transpose()?;
    let (events, invalid, truncated) = events(data_dir)?;
    let selected: Vec<_> = events
        .iter()
        .filter(|event| {
            since.is_none_or(|since| {
                DateTime::parse_from_rfc3339(&event.at).is_ok_and(|at| at.date_naive() >= since)
            })
        })
        .collect();
    let mut commands = BTreeMap::<String, Value>::new();
    let mut sessions = BTreeMap::<u32, usize>::new();
    let mut invalid_flags = BTreeMap::<String, usize>::new();
    let mut repeats = 0;
    let mut list_then_change = 0;
    let mut show_after_change = 0;
    let mut previous = BTreeMap::<u32, &Event>::new();
    for event in &selected {
        let row = commands.entry(event.command.clone()).or_insert_with(|| json!({"command":event.command,"calls":0,"failed":0,"invalid":0,"unknown":0,"not_finished":0,"large_outputs":0}));
        let key = match event.outcome.as_deref() {
            Some("unknown") => Some("unknown"),
            Some("not_finished") => Some("not_finished"),
            _ => match event.exit_code {
                Some(0) => None,
                Some(2) => Some("invalid"),
                Some(_) => Some("failed"),
                None => Some("unknown"),
            },
        };
        increment(row, "calls");
        if let Some(key) = key {
            increment(row, key);
        }
        if event.output_characters.unwrap_or(0) > 8_000 {
            increment(row, "large_outputs");
        }
        if event.exit_code == Some(2) {
            for flag in &event.flags {
                *invalid_flags.entry(flag.clone()).or_default() += 1;
            }
        }
        if let Some(parent) = event.parent_process {
            *sessions.entry(parent).or_default() += 1;
            if let Some(prior) = previous.insert(parent, event) {
                if prior.command == event.command && prior.flags == event.flags {
                    repeats += 1;
                }
                if prior.command.ends_with(".list") && is_change(&event.command) {
                    list_then_change += 1;
                }
                if is_change(&prior.command) && event.command.ends_with(".show") {
                    show_after_change += 1;
                }
            }
        }
    }
    Ok(json!({
        "calls": selected.len(), "since": since.map(|d| d.to_string()),
        "commands": commands.into_values().collect::<Vec<_>>(),
        "parent_processes": sessions.into_iter().map(|(id,calls)| json!({"parent_process":id,"calls":calls})).collect::<Vec<_>>(),
        "invalid_call_flags":invalid_flags, "repeated_command_shapes":repeats,
        "list_then_change":list_then_change, "show_after_change":show_after_change,
        "notes": ["Metadata records command and flag names, counts and timing; never values, project IDs or document text.", "Parent processes are grouping hints, not confirmed agent sessions. Repeated shapes can have different targets or values."],
        "invalid_log_lines":invalid, "older_events_omitted":truncated, "retained_event_limit":RETAIN_EVENTS,
        "log": path(data_dir).to_string_lossy()
    }))
}

fn increment(row: &mut Value, key: &str) {
    row[key] = json!(row[key].as_u64().unwrap_or(0) + 1);
}

fn is_change(command: &str) -> bool {
    command.rsplit('.').next().is_some_and(|verb| {
        matches!(
            verb,
            "add"
                | "update"
                | "rename"
                | "remove"
                | "restore"
                | "put"
                | "set"
                | "clear"
                | "create"
                | "report"
                | "confirm"
                | "reopen"
                | "cancel"
                | "acknowledge"
                | "read"
                | "visit"
                | "start"
                | "finish"
                | "fail"
                | "refresh"
                | "goal"
        )
    })
}

pub fn activity(data_dir: &Path) -> Result<Value> {
    let (events, invalid, truncated) = events(data_dir)?;
    let total = events.len();
    let calls: Vec<_> = events.into_iter().rev().take(20).map(|event| {
        json!({"at":event.at,"command":event.command,"flags":event.flags,"targets":event.targets,
            "outcome":event.outcome.as_deref().unwrap_or(match event.exit_code {Some(0)=>"returned_success",Some(2)=>"invalid_call",Some(_)=>"returned_failure",None=>"unknown"}),
            "exit_code":event.exit_code,"duration_ms":event.duration_ms})
    }).collect();
    Ok(
        json!({"activity":calls,"total_recorded_calls":total,"order":"newest first",
        "note":"A returned exit code records that command's result, not completion of a whole agent run. An unfinished or missing record never proves that nothing changed. After a stopped command, inspect its job or the current project, document or run before repeating a write.",
        "next":"watchtower jobs list", "invalid_log_lines":invalid,"older_events_omitted":truncated}),
    )
}

#[cfg(windows)]
fn parent_process() -> Option<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = Process32FirstW(snapshot, &mut entry) != 0;
        let mut parent = None;
        while found {
            if entry.th32ProcessID == std::process::id() {
                parent = Some(entry.th32ParentProcessID);
                break;
            }
            found = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        parent
    }
}

#[cfg(not(windows))]
fn parent_process() -> Option<u32> {
    None
}
