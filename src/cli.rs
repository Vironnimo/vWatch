//! Readable commands for people and agents, with explicit `--json` output for programs.
//!
//! JSON mode: success {"ok": true, ...}; failures {"ok": false, "error": {"code", "message", "hint"}}.
//! Expected failures exit 1, wrong usage exits 2. Results and errors go to stdout.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::CommandFactory;
use serde_json::{json, Value};

use crate::service::VWatchError;
use crate::{cli_output, system};

mod dispatch;
mod parser;

use dispatch::execute;
use parser::{parse_cli, Cli, Cmd, FactsCmd, OutputFormat, ProjectsCmd, RunsCmd};

/// Run one command. The stderr parameter remains available for exceptional transport failures.
pub fn run<I, T>(args: I, data_dir: &Path, stdin: &mut dyn Read, out: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    run_with_streams(args, data_dir, stdin, out, &mut std::io::stderr().lock())
}

// After a parse failure there is no Cli value to ask about the mode. Consult the
// same argument definitions, skipping actual option values and positional-only
// input after `--`. A missing value before `--json` still requests JSON errors.
fn value_options() -> HashMap<OsString, bool> {
    fn value_options(command: &clap::Command, options: &mut HashMap<OsString, bool>) {
        for arg in command.get_arguments() {
            if arg.get_action().takes_values() {
                let allow_hyphens = arg.is_allow_hyphen_values_set();
                if let Some(long) = arg.get_long() {
                    options.insert(format!("--{long}").into(), allow_hyphens);
                }
                if let Some(short) = arg.get_short() {
                    options.insert(format!("-{short}").into(), allow_hyphens);
                }
            }
        }
        for subcommand in command.get_subcommands() {
            value_options(subcommand, options);
        }
    }

    let mut command = Cli::command();
    command.build();
    let mut options = HashMap::new();
    value_options(&command, &mut options);
    options
}

fn requested_json(args: &[OsString]) -> bool {
    let options = value_options();
    let mut pending_value = None;
    let mut pending_format = false;
    for arg in args.iter().skip(1) {
        if let Some(allow_hyphens) = pending_value.take() {
            if allow_hyphens || !arg.as_encoded_bytes().starts_with(b"-") || arg == "-" {
                if pending_format && arg.to_string_lossy().eq_ignore_ascii_case("json") {
                    return true;
                }
                pending_format = false;
                continue;
            }
        }
        if arg == "--" {
            break;
        }
        if arg == "--json" {
            return true;
        }
        let text = arg.to_string_lossy();
        if ["--format=", "--output=", "-o="].iter().any(|prefix| {
            text.strip_prefix(prefix)
                .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        }) || text == "-ojson"
        {
            return true;
        }
        pending_format = matches!(text.as_ref(), "--format" | "--output" | "-o");
        pending_value = options.get(arg).copied();
        if pending_format {
            pending_value = Some(false);
        }
    }
    false
}

fn write_json(out: &mut dyn Write, body: &Value) {
    let text = serde_json::to_string_pretty(body).expect("JSON values are serializable");
    // Windows PowerShell 5.1 decodes native pipes using its active code page.
    // JSON escapes preserve Unicode content in every shell without changing values.
    let mut ascii = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii() {
            ascii.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]).iter() {
                use std::fmt::Write as _;
                let _ = write!(ascii, "\\u{unit:04x}");
            }
        }
    }
    let _ = writeln!(out, "{ascii}");
}

/// Metadata for long commands. Contains no user content except the selected instance path.
pub struct JobPlan {
    pub json: bool,
    pub command: &'static str,
    pub data_dir: Option<PathBuf>,
}

pub struct RuntimeMetadata {
    pub command: String,
    pub flags: Vec<String>,
    pub targets: usize,
    pub stdin_document: bool,
}

/// Only parser-owned names are logged. Argument values never leave this helper.
pub fn runtime_metadata(args: &[OsString]) -> RuntimeMetadata {
    let definition = Cli::command();
    let matches = definition.clone().try_get_matches_from(args);
    let mut metadata = RuntimeMetadata {
        command: "invalid_arguments".into(),
        flags: Vec::new(),
        targets: 0,
        stdin_document: false,
    };
    fn collect(
        definition: &clap::Command,
        matches: &clap::ArgMatches,
        metadata: &mut RuntimeMetadata,
        path: &mut Vec<String>,
    ) {
        for arg in definition.get_arguments() {
            if matches.value_source(arg.get_id().as_str())
                != Some(clap::parser::ValueSource::CommandLine)
            {
                continue;
            }
            if let Some(name) = arg.get_long() {
                metadata.flags.push(format!("--{name}"));
            } else if arg.is_positional() {
                metadata.targets += matches
                    .get_raw(arg.get_id().as_str())
                    .map(|values| values.len())
                    .unwrap_or(0);
            }
            if arg.get_id() == "file"
                && matches
                    .get_raw("file")
                    .is_some_and(|mut values| values.any(|v| v == "-"))
            {
                metadata.stdin_document = true;
            }
        }
        if let Some((name, child_matches)) = matches.subcommand() {
            path.push(name.into());
            if let Some(child) = definition.find_subcommand(name) {
                collect(child, child_matches, metadata, path);
            }
        }
    }
    if let Ok(matches) = matches {
        let mut path = Vec::new();
        collect(&definition, &matches, &mut metadata, &mut path);
        metadata.command = path.join(".");
    }
    metadata.flags.sort();
    metadata.flags.dedup();
    metadata
}

fn decode_instance(value: &str) -> std::result::Result<PathBuf, String> {
    let hex = value
        .strip_prefix("p_")
        .ok_or("copy the complete instance locator from the earlier command")?;
    if hex.is_empty() || !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("the instance locator is incomplete; copy it from the earlier command".into());
    }
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII checked"), 16)
                .expect("hex checked")
        })
        .collect();
    let path = PathBuf::from(
        String::from_utf8(bytes).map_err(|_| "the instance locator is not a UTF-8 path")?,
    );
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err("the instance locator must identify an absolute data folder".into());
    }
    Ok(path)
}

/// A self-contained locator is safe to copy in both PowerShell and bash, even
/// when the selected path contains quotes, dollar signs or command delimiters.
pub(crate) fn scoped_command(data_dir: &Path, command: &str) -> String {
    if !nondefault_instance(data_dir) || command.contains(" --instance ") {
        return command.to_owned();
    }
    let Some(rest) = command.strip_prefix("vwatch ") else {
        return command.to_owned();
    };
    let absolute = if data_dir.is_absolute() {
        data_dir.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(data_dir)
    };
    let hex: String = absolute
        .to_string_lossy()
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("vwatch --instance p_{hex} {rest}")
}

pub fn job_plan(args: &[OsString]) -> Option<JobPlan> {
    let cli = parse_cli(args).ok()?;
    let slow = matches!(
        &cli.command,
        Cmd::Overview { refresh: true, .. }
            | Cmd::Facts {
                action: FactsCmd::Refresh { .. } | FactsCmd::Show { .. }
            }
            | Cmd::Due {
                no_refresh: false,
                ..
            }
            | Cmd::Runs {
                action: RunsCmd::Start { no_refresh: false }
            }
            | Cmd::Projects {
                action: ProjectsCmd::Add { .. }
                    | ProjectsCmd::Update { path: Some(_), .. }
                    | ProjectsCmd::Discover { .. }
                    | ProjectsCmd::Visit { .. }
            }
            | Cmd::Context { .. }
    );
    slow.then(|| JobPlan {
        json: cli.json || matches!(cli.format, Some(OutputFormat::Json)),
        command: cli.command.output_key(),
        data_dir: cli.data_dir.or(cli.instance),
    })
}

fn requested_data_dir(args: &[OsString], fallback: &Path) -> PathBuf {
    // Clap stops at the first invalid argument, which can precede a global
    // instance selector. Inspect selectors without executing the failed call,
    // and respect values and `--` so payload text never selects an instance.
    let options = value_options();
    let mut pending_value = None;
    let mut pending_selector = None;
    let mut data_dir = None;
    let mut instance = None;
    for arg in args.iter().skip(1) {
        let selector = pending_selector.take();
        if let Some(allow_hyphens) = pending_value.take() {
            if allow_hyphens || !arg.as_encoded_bytes().starts_with(b"-") || arg == "-" {
                match selector {
                    Some("--data-dir") => data_dir = Some(PathBuf::from(arg)),
                    Some("--instance") => {
                        instance = arg.to_str().and_then(|value| decode_instance(value).ok());
                    }
                    _ => {}
                }
                continue;
            }
        }
        if arg == "--" {
            break;
        }
        if let Some(value) = arg
            .to_str()
            .and_then(|value| value.strip_prefix("--data-dir="))
        {
            data_dir = Some(PathBuf::from(value));
        } else if let Some(value) = arg
            .to_str()
            .and_then(|value| value.strip_prefix("--instance="))
        {
            instance = decode_instance(value).ok();
        }
        pending_selector = match arg.to_str() {
            Some("--data-dir") => Some("--data-dir"),
            Some("--instance") => Some("--instance"),
            _ => None,
        };
        pending_value = options.get(arg).copied();
    }
    data_dir.or(instance).unwrap_or_else(|| fallback.to_owned())
}

fn usage_error(error: &clap::Error) -> VWatchError {
    let message = error.to_string().replacen("error:", "Error:", 1);
    let mut definition = Cli::command();
    definition.build();
    let usage = error
        .get(clap::error::ContextKind::Usage)
        .map(ToString::to_string)
        .unwrap_or_default();
    let mut current = &definition;
    // Only parser-owned command names select the explanation. Never interpret
    // an offending argument as a replacement target or executable recovery.
    for word in usage
        .split_whitespace()
        .skip_while(|word| *word != "vwatch")
        .skip(1)
    {
        if let Some(child) = current.find_subcommand(word) {
            current = child;
        } else {
            break;
        }
    }
    let flags: Vec<_> = current
        .get_arguments()
        .filter(|arg| !arg.is_hide_set())
        .filter_map(|arg| arg.get_long().map(|name| format!("--{name}")))
        .collect();
    let children: Vec<_> = current
        .get_subcommands()
        .filter(|child| !child.is_hide_set())
        .map(|child| child.get_name())
        .collect();
    let mut recovery = String::from("Correct the argument named above using the shown usage and accepted values, then repeat the command.");
    if !children.is_empty() {
        recovery.push_str(&format!("\nAvailable commands: {}.", children.join(", ")));
    }
    if !flags.is_empty() {
        recovery.push_str(&format!("\nAccepted flags here: {}.", flags.join(", ")));
    }
    if flags.iter().any(|flag| flag == "--file") {
        recovery.push_str("\nDocument input uses --file PATH");
        if flags.iter().any(|flag| flag == "--input-json") {
            recovery.push_str(" or --input-json JSON");
        }
        recovery.push_str(". --json only selects the output format; it does not take a document.");
    }
    VWatchError::new(
        "invalid_arguments",
        format!("{}\nNothing was changed.", message.trim()),
    )
    .hint(recovery)
}

pub(crate) fn write_error(
    out: &mut dyn Write,
    error: &VWatchError,
    json_mode: bool,
    data_dir: &Path,
) {
    let hint = error
        .hint
        .as_deref()
        .map(|hint| scoped_hint(data_dir, hint));
    if json_mode {
        let mut body = json!({"ok": false, "error": error.to_json()});
        if let Some(hint) = &hint {
            body["error"]["hint"] = json!(hint);
        }
        if nondefault_instance(data_dir) {
            body["instance"] = json!({"data_dir":data_dir.to_string_lossy(),"default":false});
        }
        write_json(out, &body);
    } else {
        let message = error
            .message
            .strip_prefix("Error: ")
            .unwrap_or(&error.message);
        let mut lines = message.lines();
        let first = visible_line(lines.next().unwrap_or("The command failed."));
        let mut text = format!("Error: {first} ({})\n", error.code);
        for line in lines {
            text.push_str(&format!("  {}\n", visible_line(line)));
        }
        if let Some(hint) = &hint {
            let hint = hint.replace("vwatch --json ", "vwatch ");
            // A recovery explanation is not an executable continuation. In
            // particular, examples and user-supplied IDs cannot create markers.
            for line in hint.lines() {
                text.push_str(&format!("  {}\n", visible_line(line)));
            }
        }
        let page = if data_dir.is_dir() {
            crate::cli_paging::first(data_dir, &text).ok()
        } else {
            None
        };
        let text = page.unwrap_or_else(|| {
            if text.chars().count() <= crate::cli_paging::PAGE_LIMIT { return text; }
            let short: String = text.chars().take(7_900).collect();
            format!("{short}\nNote: remaining error details could not be saved in this data folder. Check folder access. Read current state before repeating a write; a shortened message does not prove no effect.")
        });
        let _ = writeln!(out, "{}", with_instance_note(&text, data_dir));
    }
}

// Service hints contain prose around generated commands. Preserve the selected
// database even when a command is offered as an explanation instead of Next:.
fn scoped_hint(data_dir: &Path, hint: &str) -> String {
    if !nondefault_instance(data_dir) {
        return hint.to_owned();
    }
    let prefix = scoped_command(data_dir, "vwatch ");
    let mut parts = hint.split("vwatch ");
    let mut result = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        if part.starts_with("--instance ") || part.starts_with("--data-dir ") {
            result.push_str("vwatch ");
        } else {
            result.push_str(&prefix);
        }
        result.push_str(part);
    }
    result
}

fn visible_line(text: &str) -> String {
    text.chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
}

fn nondefault_instance(data_dir: &Path) -> bool {
    let default = system::app_dir().join("data");
    std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_owned())
        != std::fs::canonicalize(&default).unwrap_or(default)
}

fn instance_note(data_dir: &Path) -> String {
    if !nondefault_instance(data_dir) {
        return String::new();
    }
    let path = visible_line(&data_dir.to_string_lossy());
    let short: String = path.chars().take(1_000).collect();
    format!(
        "Note: non-default data folder: {}{}. Keep this instance selected for every continuation.",
        short,
        if path.chars().count() > 1_000 {
            "... (full path in status --json)"
        } else {
            ""
        }
    )
}

fn with_instance_note(text: &str, data_dir: &Path) -> String {
    let note = instance_note(data_dir);
    if note.is_empty() {
        return text.to_owned();
    }
    match text.split_once('\n') {
        Some((first, rest)) => format!("{first}\n{note}\n{rest}"),
        None => format!("{text}\n{note}"),
    }
}

fn scoped_lines(text: &str, data_dir: &Path) -> String {
    text.lines()
        .map(|line| {
            for marker in ["Next: ", "Undo: "] {
                if let Some(command) = line.strip_prefix(marker) {
                    return format!("{marker}{}", scoped_command(data_dir, command));
                }
            }
            line.to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn result_exit_code(key: &str, value: &Value) -> i32 {
    let failed = match key {
        "facts.refresh" => value["refreshed"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["ok"] == false),
        "projects.add" => {
            value["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty())
                || value["failed"]
                    .as_array()
                    .is_some_and(|errors| !errors.is_empty())
                || value["refreshed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|row| row["ok"] == false)
                || value["project"]["facts"]["ok"] == false
        }
        "projects.update" => value["facts_refresh"]["ok"] == false,
        "runs.finish" => value["state"] != "complete",
        "due" => value["problems"]
            .as_array()
            .is_some_and(|problems| !problems.is_empty()),
        "runs.start" => value["due"]["problems"]
            .as_array()
            .is_some_and(|problems| !problems.is_empty()),
        "jobs.wait" | "jobs.status" => {
            if value["job"]["state"] == "completed" {
                value["result"]["exit_code"].as_i64().unwrap_or(1) != 0
            } else {
                true
            }
        }
        "overview" => value["refreshed"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|row| row["ok"] == false),
        _ => false,
    };
    i32::from(failed)
}

pub(crate) fn write_result(
    key: &str,
    value: Value,
    json_mode: bool,
    data_dir: &Path,
    out: &mut dyn Write,
) -> i32 {
    let code = result_exit_code(key, &value);
    if json_mode {
        let mut body = json!({"ok": code == 0});
        if let (Some(body), Value::Object(result)) = (body.as_object_mut(), value) {
            body.extend(result);
        }
        if code != 0 {
            body["ok"] = json!(false);
        }
        if nondefault_instance(data_dir) {
            body["instance"] = json!({"data_dir":data_dir.to_string_lossy(),"default":false});
        }
        for key in ["next", "undo"] {
            if let Some(command) = body[key].as_str() {
                body[key] = json!(scoped_command(data_dir, command));
            }
        }
        // Only service-owned receipt wrappers are commands. Never recursively
        // reinterpret document fields or text written by the owner or agents.
        for wrapper in ["project", "idea", "task"] {
            for key in ["next", "undo"] {
                if let Some(command) = body[wrapper][key].as_str() {
                    body[wrapper][key] = json!(scoped_command(data_dir, command));
                }
            }
        }
        write_json(out, &body);
    } else {
        let text = if key == "output" {
            value["page"].as_str().unwrap_or("").to_owned()
        } else {
            scoped_lines(&cli_output::human(key, &value, data_dir), data_dir)
        };
        let page = if key == "output" {
            Ok(text)
        } else {
            crate::cli_paging::first(data_dir, &text)
        };
        match page {
            Ok(page) => {
                let _ = writeln!(out, "{}", with_instance_note(&page, data_dir));
            }
            Err(error) => {
                write_error(out, &error, false, data_dir);
                return 1;
            }
        }
    }
    code
}

/// Replay a completed long command in the caller's requested output mode.
pub fn render_completed_job(
    command: &str,
    result: &Value,
    json_mode: bool,
    data_dir: &Path,
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> i32 {
    let code = result["exit_code"].as_i64().unwrap_or(1) as i32;
    let text = result["stdout"].as_str().unwrap_or("");
    let parsed = serde_json::from_str::<Value>(text);
    match parsed {
        Ok(body) if body["error"].is_object() && body["ok"] == false => {
            let mut error = VWatchError::new(
                body["error"]["code"].as_str().unwrap_or("job_failed"),
                body["error"]["message"]
                    .as_str()
                    .unwrap_or("The command failed."),
            );
            if let Some(hint) = body["error"]["hint"].as_str() {
                error = error.hint(hint);
            }
            write_error(out, &error, json_mode, data_dir);
        }
        Ok(body) => {
            let rendered = write_result(command, body, json_mode, data_dir, out);
            if rendered != 0 {
                return rendered;
            }
        }
        Err(_) => {
            let error = VWatchError::new("job_result_unreadable", "The saved command result is incomplete. Its outcome is unknown.")
                .hint("Read vwatch activity and the affected item before deciding whether to repeat a change.");
            write_error(out, &error, json_mode, data_dir);
            return 1;
        }
    }
    let _ = out.flush();
    code
}

/// Run one command with explicit streams. JSON results and errors use stdout;
/// readable results and errors also use stdout. Returns 0/1/2.
pub fn run_with_streams<I, T>(
    args: I,
    data_dir: &Path,
    stdin: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let metadata = runtime_metadata(&args);
    let cli = match parse_cli(&args) {
        Ok(cli) => cli,
        Err(e) => {
            let code = e.exit_code();
            if code == 0 {
                // Help and version are explicitly requested human-readable text,
                // including when combined with the machine-output switch.
                let _ = write!(out, "{e}");
            } else {
                let data_dir = requested_data_dir(&args, data_dir);
                let ticket = crate::cli_usage::begin(
                    &data_dir,
                    &metadata.command,
                    &metadata.flags,
                    metadata.targets,
                );
                let mut buffer = Vec::new();
                write_error(
                    &mut buffer,
                    &usage_error(&e),
                    requested_json(&args),
                    &data_dir,
                );
                crate::cli_usage::finish(
                    ticket,
                    code,
                    String::from_utf8_lossy(&buffer).chars().count(),
                );
                let _ = out.write_all(&buffer);
            }
            let _ = out.flush();
            let _ = err.flush();
            return code;
        }
    };
    let key = cli.command.output_key();
    let json_mode = cli.json || matches!(cli.format, Some(OutputFormat::Json));
    let instance_source = if cli.data_dir.is_some() {
        "--data-dir"
    } else if cli.instance.is_some() {
        "printed --instance locator"
    } else if std::env::var_os("VWATCH_HOME").is_some() {
        "VWATCH_HOME"
    } else if nondefault_instance(data_dir) {
        "caller-supplied data folder"
    } else {
        "default installed data folder"
    };
    let data_dir = cli
        .data_dir
        .as_deref()
        .or(cli.instance.as_deref())
        .unwrap_or(data_dir);
    if !matches!(key, "output" | "usage" | "activity")
        && !key.starts_with("schema.")
        && !key.starts_with("jobs.")
    {
        // Valid application calls already create this directory in execute.
        // Creating it here lets the metadata record precede any database write.
        let _ = std::fs::create_dir_all(data_dir);
    }
    let ticket = if std::env::var_os("VWATCH_INTERNAL_JOB_WORKER").is_none() {
        crate::cli_usage::begin(data_dir, key, &metadata.flags, metadata.targets)
    } else {
        None
    };
    let mut buffer = Vec::new();
    let mut outcome = None;
    let code = match execute(cli.command, data_dir, stdin) {
        Ok(mut result) => {
            if key == "status" {
                result["instance_source"] = json!(instance_source);
            }
            if key.starts_with("jobs.") {
                outcome = match result["job"]["state"].as_str() {
                    Some("running") => Some("not_finished"),
                    Some("unknown") => Some("unknown"),
                    _ => None,
                };
            }
            if key == "jobs.wait" && result["job"]["state"] == "completed" {
                render_completed_job(
                    result["job"]["command"].as_str().unwrap_or(""),
                    &result["result"],
                    json_mode,
                    data_dir,
                    &mut buffer,
                    err,
                )
            } else {
                write_result(key, result, json_mode, data_dir, &mut buffer)
            }
        }
        Err(e) => {
            write_error(&mut buffer, &e, json_mode, data_dir);
            1
        }
    };
    crate::cli_usage::finish_outcome(
        ticket,
        code,
        String::from_utf8_lossy(&buffer).chars().count(),
        outcome,
    );
    let _ = out.write_all(&buffer);
    let _ = out.flush();
    let _ = err.flush();
    code
}

/// Entry point used by the console executable.
pub fn main(args: Vec<OsString>) -> i32 {
    let data_dir = system::data_path();
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    use std::io::IsTerminal;
    if runtime_metadata(&args).stdin_document && std::io::stdin().is_terminal() {
        let error = VWatchError::new(
            "missing_document",
            "--file - needs piped input; terminal input is not read. Nothing was saved.",
        )
        .hint("Write the document to a UTF-8 file and pass its path with --file.");
        write_error(
            &mut lock,
            &error,
            requested_json(&args),
            &requested_data_dir(&args, &data_dir),
        );
        return 1;
    }
    run(args, &data_dir, &mut std::io::stdin(), &mut lock)
}
