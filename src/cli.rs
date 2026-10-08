//! Readable commands for people and agents, with explicit `--json` output for programs.
//!
//! JSON mode: success {"ok": true, ...}; failures {"ok": false, "error": {"code", "message", "hint"}}.
//! Expected failures exit 1, wrong usage exits 2. Results and errors go to stdout.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};

use crate::cards::{self, Kind};
use crate::service::{Result, Watchtower, WatchtowerError, DEFAULT_MAX_AGE_DAYS};
use crate::{cli_output, system};

#[derive(Parser)]
#[command(
    name = "watchtower",
    bin_name = "watchtower",
    version,
    about = "Morning overview of all your agent-built projects.",
    after_help = "Commands show readable text. Use --json when a program parses the output. Exit codes: 0 success, 1 failed or incomplete, 2 invalid call."
)]
struct Cli {
    /// Print one complete JSON object for programs
    #[arg(long, global = true)]
    json: bool,
    /// Output format: text or json. Omit for readable text.
    #[arg(long = "format", aliases = ["output"], short = 'o', global = true, value_enum, ignore_case = true)]
    format: Option<OutputFormat>,
    /// Data folder for this call. Omit to use WATCHTOWER_HOME or the installed data folder.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Portable instance locator printed by continuation commands.
    #[arg(long, global = true, hide = true, value_parser = decode_instance, conflicts_with = "data_dir")]
    instance: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Where Watchtower keeps its data and how many projects it watches
    Status,
    /// Read the complete morning overview; --visit records a visit only when requested by the owner
    #[command(alias = "state")]
    Overview {
        #[arg(long)]
        visit: bool,
        /// Measure all projects before showing the overview. Omit to use stored facts.
        #[arg(long)]
        refresh: bool,
    },
    /// Register and manage the watched projects
    #[command(alias = "project")]
    Projects {
        #[command(subcommand)]
        action: ProjectsCmd,
    },
    /// Exact git facts, measured without a model
    Facts {
        #[command(subcommand)]
        action: FactsCmd,
    },
    /// Refresh facts and list the projects that need a new card, plus whether the portfolio brief is due
    Due {
        /// Rewrite unchanged cards older than this many days
        #[arg(long, default_value_t = DEFAULT_MAX_AGE_DAYS, value_parser = clap::value_parser!(i64).range(0..))]
        max_age_days: i64,
        /// Use the stored facts instead of measuring again
        #[arg(long)]
        no_refresh: bool,
    },
    /// Everything an agent needs before writing a project card
    Context { id: String },
    /// Read or write a project card
    Card {
        #[command(subcommand)]
        action: CardCmd,
    },
    /// The cross-project brief
    Portfolio {
        #[command(subcommand)]
        action: PortfolioCmd,
    },
    /// The owner's ratings of suggestions and ideas and replies to questions
    Feedback {
        /// Only reactions about this project
        #[arg(long)]
        project: Option<String>,
        #[command(subcommand)]
        action: Option<FeedbackCmd>,
    },
    /// Ideas the owner parked for agents to assess in the portfolio brief
    #[command(alias = "idea")]
    Ideas {
        #[command(subcommand)]
        action: IdeasCmd,
    },
    /// Track the completeness of a daily agent run
    #[command(alias = "run")]
    Runs {
        #[command(subcommand)]
        action: RunsCmd,
    },
    /// Read assigned work and report results for the owner to review
    #[command(alias = "task")]
    Tasks {
        #[command(subcommand)]
        action: TasksCmd,
    },
    /// Document fields and constraints (use --json for the complete JSON schema)
    Schema { kind: SchemaKind },
    /// Read measured warnings or record the owner's deliberate acknowledgement
    Warnings {
        #[command(subcommand)]
        action: WarningsCmd,
    },
    /// Read changes since the owner last marked the overview read
    Changes {
        #[command(subcommand)]
        action: ChangesCmd,
    },
    /// Read installation settings and change the owner's display preferences
    Settings {
        #[command(subcommand)]
        action: SettingsCmd,
    },
    /// Continue an immutable saved text output without repeating its command
    Output {
        token: String,
        /// Zero-based character offset from the previous page's Next command
        #[arg(long, default_value_t = 0)]
        start: usize,
    },
    /// Read or wait for a command that continued beyond the inline deadline
    Jobs {
        #[command(subcommand)]
        action: JobsCmd,
    },
    /// Local command usage; contains command and flag names, never their values
    Usage {
        /// Include calls on or after this UTC date, YYYY-MM-DD
        #[arg(long)]
        since: Option<String>,
    },
    /// Recent command outcomes for recovery after a lost response
    Activity,
    /// Undo one recorded owner change if its result has not changed again
    Undo { token: String },
}

#[derive(Subcommand)]
enum JobsCmd {
    /// Recorded long commands, including unfinished or unknown outcomes
    #[command(alias = "ls")]
    List,
    /// Current state without waiting
    #[command(aliases = ["show", "get"])]
    Status { token: String },
    /// Wait within the deadline and return the original result when complete
    Wait { token: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand)]
enum FeedbackCmd {
    /// Read the owner's ratings and replies
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        project: Option<String>,
    },
    /// Record the owner's explicit rating or answer; never invent a reaction
    #[command(aliases = ["update", "rate"])]
    Set {
        id: String,
        #[arg(long, value_parser = ["up", "later", "down", "answered", "done"], ignore_case = true)]
        value: String,
        /// The owner's answer or reason. Omit to keep the existing note.
        #[arg(long)]
        note: Option<String>,
        #[arg(long, help = format!("Local date YYYY-MM-DD for later. Omit to keep the date or return in {} days.", crate::service::SNOOZE_DAYS))]
        until: Option<String>,
    },
    /// Withdraw a reaction when the owner asks
    #[command(aliases = ["remove", "delete", "rm"])]
    Clear { id: String },
    /// Restore a previously captured reaction snapshot, or JSON null for no reaction
    Restore {
        id: String,
        #[command(flatten)]
        input: InputArgs,
    },
}

#[derive(Subcommand)]
enum WarningsCmd {
    /// Current measured warnings and stored acknowledgements for one project
    #[command(aliases = ["show", "ls"])]
    List { id: String },
    /// Record the owner's reason for an intentional measured condition
    #[command(alias = "ack")]
    Acknowledge {
        id: String,
        #[arg(long, value_parser = ["no_remote", "ci_none", "worktrees"])]
        key: String,
        #[arg(long)]
        note: String,
    },
    /// Withdraw an acknowledgement when the owner asks
    Clear {
        id: String,
        #[arg(long, value_parser = ["no_remote", "ci_none", "worktrees"])]
        key: String,
    },
}

#[derive(Subcommand)]
enum ChangesCmd {
    /// Changes and the token of the exact snapshot displayed
    #[command(aliases = ["list", "get", "view"])]
    Show,
    /// Mark the displayed snapshot read on the owner's explicit request
    #[command(alias = "acknowledge")]
    Read {
        #[arg(long)]
        token: String,
    },
}

#[derive(Subcommand)]
enum SettingsCmd {
    /// Installation paths, autostart/PATH state and display preferences
    #[command(aliases = ["list", "get", "view"])]
    Show,
    /// Set display preferences only when the owner asks; preserves omitted values
    #[command(alias = "set")]
    Update {
        #[arg(long, value_parser = ["handover", "board", "handbook"], ignore_case = true)]
        style: Option<String>,
        /// Collapse these module IDs (comma-separated); headers stay visible. Empty expands all.
        #[arg(long)]
        hidden_modules: Option<String>,
    },
}

#[derive(Subcommand)]
enum IdeasCmd {
    /// All parked ideas
    #[command(alias = "ls")]
    List,
    /// Park an idea
    #[command(aliases = ["create", "new"])]
    Add { text: String },
    /// Remove a parked idea
    #[command(aliases = ["delete", "rm", "del"])]
    Remove { id: i64 },
    /// Removed ideas available to restore
    Trash,
    /// Restore a removed idea with its original ID
    Restore { id: i64 },
}

#[derive(Subcommand)]
enum RunsCmd {
    /// Start a tracked daily run, refreshing facts first
    Start {
        #[arg(long)]
        no_refresh: bool,
    },
    /// Latest run, pending work and last fully successful run
    Status,
    /// Record a project failure (omit --project for the portfolio brief)
    Fail {
        id: i64,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        error: String,
    },
    /// Close the run; success requires current cards and a current brief
    Finish { id: i64 },
}

#[derive(Subcommand)]
enum TasksCmd {
    /// Assigned work, reported results and completed tasks
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        project: Option<String>,
    },
    /// Read a task's snapshot, result, revision and history
    #[command(aliases = ["get", "view"])]
    Show { id: i64 },
    /// Assign work only when the owner asks; reports still require owner confirmation
    #[command(aliases = ["add", "assign"])]
    Create {
        /// Project IDs; repeat the flag or separate IDs by commas
        #[arg(long, value_delimiter = ',')]
        project: Vec<String>,
        #[arg(long, required_unless_present = "file")]
        title: Option<String>,
        #[arg(long, default_value = "")]
        why: String,
        #[arg(long, default_value = "")]
        evidence: String,
        #[arg(long, default_value = "")]
        done_when: String,
        /// The existing suggestion or focus item ID. Omit for independent work.
        #[arg(long)]
        source_id: Option<String>,
        /// Complete task document for longer text; accepts UTF-8 or UTF-16 with BOM
        #[arg(long, conflicts_with_all = ["project", "title", "why", "evidence", "done_when", "source_id"])]
        file: Option<String>,
    },
    /// Confirm a reviewed report only on the owner's explicit instruction
    Confirm {
        id: i64,
        #[arg(long)]
        revision: i64,
    },
    /// Reopen a task only on the owner's explicit instruction
    Reopen {
        id: i64,
        #[arg(long)]
        revision: i64,
    },
    /// Cancel a task only on the owner's explicit instruction
    Cancel {
        id: i64,
        #[arg(long)]
        revision: i64,
    },
    /// Report an outcome; done awaits the owner's confirmation
    Report {
        id: i64,
        #[arg(long, value_parser = ["done", "blocked"], ignore_case = true)]
        status: String,
        #[arg(long)]
        result: String,
        #[arg(long, default_value = "")]
        evidence: String,
    },
}

#[derive(Subcommand)]
enum ProjectsCmd {
    /// All registered projects
    #[command(aliases = ["ls", "search", "find"])]
    List {
        /// Match ID, name, folder or card text (case-insensitive substring)
        #[arg(long, aliases = ["search", "filter"], short = 'q', conflicts_with = "text")]
        query: Option<String>,
        #[arg(hide = true)]
        text: Option<String>,
    },
    /// Project card, measured facts, warnings and complete card history
    #[command(aliases = ["get", "view"])]
    Show { id: String },
    /// Record the owner's visit and return changes since the previous visit
    Visit { id: String },
    /// Register a project folder (usually a git repository)
    #[command(aliases = ["create", "new"])]
    Add {
        #[arg(required = true)]
        paths: Vec<String>,
        /// Short id; defaults to the folder name
        #[arg(long)]
        id: Option<String>,
        /// Display name; defaults to the folder name
        #[arg(long)]
        name: Option<String>,
    },
    /// Stop watching a project; its cards and history remain available to restore
    #[command(aliases = ["delete", "rm", "del"])]
    Remove { id: String },
    /// Removed projects available to restore
    Trash,
    /// Restore a removed project with its original cards and ID
    Restore { id: String },
    /// Change the display name
    Rename { id: String, name: String },
    /// Change a project's display name or registered folder without losing its history
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        /// Existing folder after a rename or move; does not move the folder itself
        #[arg(long)]
        path: Option<String>,
    },
    /// Set the owner's goal for the project (an empty goal clears it)
    Goal {
        id: String,
        goal: String,
        /// Due date as YYYY-MM-DD
        #[arg(long)]
        due: Option<String>,
    },
    /// Set what the owner intends with the project
    Status {
        id: String,
        #[arg(ignore_case = true)]
        status: Status,
        /// Short reason, shown to people and agents
        #[arg(long)]
        note: Option<String>,
    },
    /// Git repositories in a folder (or a repository and its neighbours) that are not registered yet
    Discover { root: String },
}

#[derive(Subcommand)]
enum FactsCmd {
    /// Measure again now (all projects when no id is given)
    Refresh {
        ids: Vec<String>,
        /// Skip the GitHub CI lookup
        #[arg(long)]
        no_ci: bool,
    },
    /// Facts of one project
    #[command(aliases = ["get", "view"])]
    Show { id: String },
}

#[derive(Subcommand)]
enum CardCmd {
    /// Save a new card version
    #[command(aliases = ["set", "update"])]
    Put {
        id: String,
        #[command(flatten)]
        doc: DocArgs,
    },
    /// The current card
    #[command(aliases = ["get", "view"])]
    Show {
        id: String,
        #[arg(long)]
        version: Option<i64>,
    },
    /// Saved card versions and the context token required to restore one
    History { id: String },
    /// Restore a saved version after reconciling the current project context
    Restore {
        id: String,
        #[arg(long)]
        version: i64,
        #[arg(long)]
        context_token: String,
    },
}

#[derive(Subcommand)]
enum PortfolioCmd {
    /// All projects side by side, the previous brief and all ratings
    Context,
    /// Save a new brief
    #[command(aliases = ["set", "update"])]
    Put {
        #[command(flatten)]
        doc: DocArgs,
    },
    /// The current brief
    #[command(aliases = ["get", "view"])]
    Show {
        #[arg(long)]
        version: Option<i64>,
    },
    /// Saved brief versions and the context token required to restore one
    History,
    /// Restore a saved version after reconciling the current portfolio context
    Restore {
        #[arg(long)]
        version: i64,
        #[arg(long)]
        context_token: String,
    },
}

#[derive(Args)]
struct InputArgs {
    /// Document file; '-' reads stdin. Accepts UTF-8 or UTF-16 with a byte-order mark.
    #[arg(long, conflicts_with = "input_json")]
    file: Option<String>,
    /// JSON document inline; use --file for text with quotes or line breaks
    #[arg(long)]
    input_json: Option<String>,
}

#[derive(Args)]
struct DocArgs {
    /// JSON file with the document; '-' reads stdin
    #[arg(long, conflicts_with = "input_json")]
    file: Option<String>,
    /// The document as a JSON string
    #[arg(long)]
    input_json: Option<String>,
    /// Send only changed top-level fields; the rest is kept from the previous version
    #[arg(long)]
    merge: bool,
    /// Check the document and show what would be saved, without saving it
    #[arg(long)]
    dry_run: bool,
    /// Token from the context used to write this document; required when saving
    #[arg(long)]
    context_token: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Status {
    Active,
    Parked,
    Done,
    Archived,
}

#[derive(Clone, Copy, ValueEnum)]
enum SchemaKind {
    Card,
    Portfolio,
}

impl Cmd {
    fn output_key(&self) -> &'static str {
        match self {
            Cmd::Status => "status",
            Cmd::Overview { .. } => "overview",
            Cmd::Output { .. } => "output",
            Cmd::Jobs { action } => match action {
                JobsCmd::List => "jobs.list",
                JobsCmd::Status { .. } => "jobs.status",
                JobsCmd::Wait { .. } => "jobs.wait",
            },
            Cmd::Usage { .. } => "usage",
            Cmd::Activity => "activity",
            Cmd::Undo { .. } => "undo",
            Cmd::Projects { action } => match action {
                ProjectsCmd::List { .. } => "projects.list",
                ProjectsCmd::Show { .. } => "projects.show",
                ProjectsCmd::Visit { .. } => "projects.visit",
                ProjectsCmd::Add { .. } => "projects.add",
                ProjectsCmd::Remove { .. } => "projects.remove",
                ProjectsCmd::Trash => "projects.trash",
                ProjectsCmd::Restore { .. } => "projects.restore",
                ProjectsCmd::Rename { .. } => "projects.rename",
                ProjectsCmd::Update { .. } => "projects.update",
                ProjectsCmd::Goal { .. } => "projects.goal",
                ProjectsCmd::Status { .. } => "projects.status",
                ProjectsCmd::Discover { .. } => "projects.discover",
            },
            Cmd::Facts { action } => match action {
                FactsCmd::Refresh { .. } => "facts.refresh",
                FactsCmd::Show { .. } => "facts.show",
            },
            Cmd::Due { .. } => "due",
            Cmd::Context { .. } => "context",
            Cmd::Card { action } => match action {
                CardCmd::Put { .. } => "card.put",
                CardCmd::Show { .. } => "card.show",
                CardCmd::History { .. } => "card.history",
                CardCmd::Restore { .. } => "card.restore",
            },
            Cmd::Portfolio { action } => match action {
                PortfolioCmd::Context => "portfolio.context",
                PortfolioCmd::Put { .. } => "portfolio.put",
                PortfolioCmd::Show { .. } => "portfolio.show",
                PortfolioCmd::History => "portfolio.history",
                PortfolioCmd::Restore { .. } => "portfolio.restore",
            },
            Cmd::Feedback { action, .. } => match action {
                None | Some(FeedbackCmd::List { .. }) => "feedback",
                Some(FeedbackCmd::Set { .. }) => "feedback.set",
                Some(FeedbackCmd::Clear { .. }) => "feedback.clear",
                Some(FeedbackCmd::Restore { .. }) => "feedback.restore",
            },
            Cmd::Ideas { action } => match action {
                IdeasCmd::List => "ideas.list",
                IdeasCmd::Add { .. } => "ideas.add",
                IdeasCmd::Remove { .. } => "ideas.remove",
                IdeasCmd::Trash => "ideas.trash",
                IdeasCmd::Restore { .. } => "ideas.restore",
            },
            Cmd::Runs { action } => match action {
                RunsCmd::Start { .. } => "runs.start",
                RunsCmd::Status => "runs.status",
                RunsCmd::Fail { .. } => "runs.fail",
                RunsCmd::Finish { .. } => "runs.finish",
            },
            Cmd::Tasks { action } => match action {
                TasksCmd::List { .. } => "tasks.list",
                TasksCmd::Report { .. } => "tasks.report",
                TasksCmd::Show { .. } => "tasks.show",
                TasksCmd::Create { .. } => "tasks.create",
                TasksCmd::Confirm { .. } => "tasks.confirm",
                TasksCmd::Reopen { .. } => "tasks.reopen",
                TasksCmd::Cancel { .. } => "tasks.cancel",
            },
            Cmd::Schema { kind } => match kind {
                SchemaKind::Card => "schema.card",
                SchemaKind::Portfolio => "schema.portfolio",
            },
            Cmd::Warnings { action } => match action {
                WarningsCmd::List { .. } => "warnings.list",
                WarningsCmd::Acknowledge { .. } => "warnings.acknowledge",
                WarningsCmd::Clear { .. } => "warnings.clear",
            },
            Cmd::Changes { action } => match action {
                ChangesCmd::Show => "changes.show",
                ChangesCmd::Read { .. } => "changes.read",
            },
            Cmd::Settings { action } => match action {
                SettingsCmd::Show => "settings.show",
                SettingsCmd::Update { .. } => "settings.update",
            },
        }
    }
}

/// Decode a document: UTF-8 with or without BOM, or UTF-16 with BOM (Windows PowerShell's default).
fn decode(bytes: &[u8], origin: &str) -> Result<String> {
    let invalid_encoding = || {
        WatchtowerError::new("invalid_json", format!("{origin} has invalid text encoding. Nothing was saved."))
            .hint("Save the complete JSON document as UTF-8, or UTF-16 with a byte-order mark, and retry.")
    };
    let utf16 = |be: bool| {
        if !(bytes.len() - 2).is_multiple_of(2) {
            return Err(invalid_encoding());
        }
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| {
                if be {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16(&units).map_err(|_| invalid_encoding())
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => std::str::from_utf8(rest)
            .map(str::to_owned)
            .map_err(|_| invalid_encoding()),
        _ => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| invalid_encoding()),
    }
}

fn read_document(doc: &DocArgs, stdin: &mut dyn Read) -> Result<Value> {
    read_input(doc.file.as_deref(), doc.input_json.as_deref(), stdin)
}

fn read_input(file: Option<&str>, input_json: Option<&str>, stdin: &mut dyn Read) -> Result<Value> {
    let (text, origin) = match (input_json, file) {
        (Some(json), _) => (json.to_owned(), "--input-json".to_owned()),
        (None, Some("-")) => {
            let mut buf = Vec::new();
            stdin.read_to_end(&mut buf).map_err(|e| {
                input_error("stdin", &e)
            })?;
            (decode(&buf, "stdin")?, "stdin".to_owned())
        }
        (None, Some(file)) => {
            if Path::new(file).is_dir() {
                return Err(WatchtowerError::new("file_unreadable", format!("{file} is a folder, not a document file. Nothing was saved."))
                    .hint("Pass the JSON document file inside that folder with --file."));
            }
            let bytes = std::fs::read(file).map_err(|e| {
                input_error(file, &e)
            })?;
            (decode(&bytes, file)?, file.to_owned())
        }
        (None, None) => {
            return Err(
                WatchtowerError::new("missing_document", "No document was supplied. Nothing was saved.")
                    .hint("Write the document to a UTF-8 file, then repeat this command with --file followed by that file's path. Use --file - only when JSON is piped into stdin."),
            )
        }
    };
    if text.trim().is_empty() {
        return Err(WatchtowerError::new("invalid_json", format!("{origin} is empty. Nothing was saved."))
            .hint("Write the complete JSON document to a UTF-8 file and repeat this command with --file followed by its path. Closed stdin supplies no document."));
    }
    serde_json::from_str(&text).map_err(|e| {
        WatchtowerError::new("invalid_json", format!("{origin} is not valid JSON at line {}, column {}: {e}. Nothing was saved.", e.line(), e.column()))
            .hint("Fix the JSON at that location and repeat this command. Use a file-writing tool to preserve quotes and backslashes, and pass the file with --file.")
    })
}

fn input_error(origin: &str, error: &std::io::Error) -> WatchtowerError {
    use std::io::ErrorKind;
    let (cause, recovery) = match error.kind() {
        ErrorKind::NotFound => ("does not exist", "Write the document at that path or correct --file, then repeat this command."),
        ErrorKind::PermissionDenied => ("cannot be read with this account", "Use a readable document file and pass its path with --file."),
        ErrorKind::IsADirectory => ("is a folder, not a document file", "Pass the JSON file inside that folder with --file."),
        _ => ("could not be read", "Ensure the document is a readable file, close any exclusive writer, then repeat this command."),
    };
    WatchtowerError::new(
        "file_unreadable",
        format!("{origin} {cause}. Nothing was saved."),
    )
    .hint(recovery)
}

fn reject_feedback_filter(project: Option<&str>) -> Result<()> {
    if project.is_some() {
        return Err(WatchtowerError::new("invalid_arguments", "--project filters feedback reads; it cannot select a reaction to change. Nothing was changed.")
            .hint("Remove --project from this command. The explicit item ID selects the reaction to change."));
    }
    Ok(())
}

fn text_matches(value: &Value, query: &str) -> bool {
    match value {
        Value::String(text) => text.to_lowercase().contains(query),
        Value::Array(values) => values.iter().any(|value| text_matches(value, query)),
        Value::Object(values) => values.values().any(|value| text_matches(value, query)),
        _ => false,
    }
}

fn execute(cmd: Cmd, data_dir: &Path, stdin: &mut dyn Read) -> Result<Value> {
    if let Cmd::Output { token, start } = cmd {
        return Ok(json!({"page": crate::cli_paging::resume(data_dir, &token, start)?}));
    }
    if let Cmd::Jobs { action } = cmd {
        return match action {
            JobsCmd::List => crate::cli_jobs::command("list", None, data_dir),
            JobsCmd::Status { token } => crate::cli_jobs::command("status", Some(&token), data_dir),
            JobsCmd::Wait { token } => crate::cli_jobs::command("wait", Some(&token), data_dir),
        };
    }
    if let Cmd::Usage { since } = cmd {
        return crate::cli_usage::report(data_dir, since.as_deref());
    }
    if let Cmd::Activity = cmd {
        return crate::cli_usage::activity(data_dir);
    }
    if let Cmd::Schema { kind } = cmd {
        let kind = match kind {
            SchemaKind::Card => Kind::Card,
            SchemaKind::Portfolio => Kind::Portfolio,
        };
        return Ok(json!({"schema": cards::json_schema(kind)}));
    }
    std::fs::create_dir_all(data_dir).map_err(|e| {
        WatchtowerError::new(
            "storage_error",
            format!("Cannot create data folder {}: {e}.", data_dir.display()),
        )
        .hint("Check that Watchtower can write to its data folder.")
    })?;
    let db = system::db_path(data_dir);
    let svc = Watchtower::open(&db)?;
    Ok(match cmd {
        Cmd::Schema { .. }
        | Cmd::Output { .. }
        | Cmd::Jobs { .. }
        | Cmd::Usage { .. }
        | Cmd::Activity => unreachable!("handled above"),
        Cmd::Overview { visit, refresh } => {
            let refreshed = if refresh {
                Some(svc.refresh_facts(None, true)?)
            } else {
                None
            };
            let mut state = svc.state(visit)?;
            if let Some(refreshed) = refreshed {
                state["refreshed"] = json!(refreshed);
            }
            state
        }
        Cmd::Undo { token } => svc.undo(&token)?,
        Cmd::Status => {
            // A freshly installed exe hands out its own Skill before the window ever starts.
            let skill = system::export_skill().ok();
            let projects = svc.list_projects()?;
            let list = projects.as_array().cloned().unwrap_or_default();
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "now": chrono::Local::now().to_rfc3339(),
                "exe": system::exe_path().to_string_lossy(),
                "data_dir": data_dir.to_string_lossy(),
                "database": db.to_string_lossy(),
                "logs": {"usage": crate::cli_usage::path(data_dir).to_string_lossy()},
                "skill_dir": skill.map(|d| d.to_string_lossy().into_owned()),
                "projects": list.len(),
                "with_card": list.iter().filter(|p| p["has_card"] == json!(true)).count(),
                "hint": if list.is_empty() { json!("No projects yet. Register an existing repository folder with projects add when the owner asks to watch it. Use the folder's actual path.") } else { Value::Null },
            })
        }
        Cmd::Projects { action } => match action {
            ProjectsCmd::List { query, text } => {
                let query = query.or(text);
                let mut projects = svc.list_projects()?;
                if let Some(query) = query.as_ref() {
                    let query = query.to_lowercase();
                    let state = svc.state(false)?;
                    let matches: Vec<&Value> = state["projects"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|p| {
                            ["id", "name", "path", "card"]
                                .iter()
                                .any(|field| text_matches(&p[field], &query))
                        })
                        .collect();
                    if let Some(list) = projects.as_array_mut() {
                        list.retain(|p| matches.iter().any(|other| other["id"] == p["id"]));
                    }
                }
                json!({"projects": projects, "query": query})
            }
            ProjectsCmd::Show { id } => {
                let state = svc.state(false)?;
                let project = state["projects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| p["id"] == id)
                    .cloned();
                match project {
                    Some(project) => json!({"project": project}),
                    None => {
                        svc.context(&id)?;
                        unreachable!("context rejects an unknown project")
                    }
                }
            }
            ProjectsCmd::Visit { id } => {
                let mut result = svc.open_project(&id)?;
                result["project"] = json!(id);
                result
            }
            ProjectsCmd::Add { paths, id, name } => {
                if paths.len() == 1 {
                    json!({"project": svc.add_project(&paths[0], id.as_deref(), name.as_deref())?})
                } else if id.is_some() || name.is_some() {
                    return Err(WatchtowerError::new("invalid_arguments", "--id and --name apply to one folder. No projects were added.")
                        .hint("Add these folders together without --id and --name, or run a separate projects add command for each custom ID and name."));
                } else {
                    svc.add_projects(&paths)?
                }
            }
            ProjectsCmd::Remove { id } => svc.remove_project(&id)?,
            ProjectsCmd::Trash => svc.trash_projects()?,
            ProjectsCmd::Restore { id } => svc.restore_project(&id)?,
            ProjectsCmd::Rename { id, name } => json!({"project": svc.rename_project(&id, &name)?}),
            ProjectsCmd::Update { id, name, path } => {
                svc.update_project(&id, name.as_deref(), path.as_deref())?
            }
            ProjectsCmd::Status { id, status, note } => {
                let status = status
                    .to_possible_value()
                    .expect("every status has a name")
                    .get_name()
                    .to_owned();
                json!({"project": svc.set_status(&id, &status, note.as_deref())?})
            }
            ProjectsCmd::Goal { id, goal, due } => {
                json!({"project": svc.set_goal(&id, &goal, due.as_deref())?})
            }
            ProjectsCmd::Discover { root } => svc.discover(&root)?,
        },
        Cmd::Facts { action } => match action {
            FactsCmd::Refresh { ids, no_ci } => {
                json!({"refreshed": svc.refresh_facts(Some(&ids), !no_ci)?})
            }
            FactsCmd::Show { id } => json!({"facts": svc.facts(&id)?}),
        },
        Cmd::Due {
            max_age_days,
            no_refresh,
        } => svc.due(max_age_days, !no_refresh)?,
        Cmd::Context { id } => svc.context(&id)?,
        Cmd::Card { action } => match action {
            CardCmd::Put { id, doc } => svc.put_card_guarded(
                &id,
                read_document(&doc, stdin)?,
                doc.merge,
                doc.dry_run,
                doc.context_token.as_deref(),
            )?,
            CardCmd::Show { id, version } => match version {
                Some(version) => svc.card_version(&id, version)?,
                None => svc.card(&id)?,
            },
            CardCmd::History { id } => svc.card_history(&id)?,
            CardCmd::Restore {
                id,
                version,
                context_token,
            } => svc.restore_card(&id, version, Some(&context_token))?,
        },
        Cmd::Portfolio { action } => match action {
            PortfolioCmd::Context => svc.portfolio_context()?,
            PortfolioCmd::Put { doc } => svc.put_portfolio_guarded(
                read_document(&doc, stdin)?,
                doc.merge,
                doc.dry_run,
                doc.context_token.as_deref(),
            )?,
            PortfolioCmd::Show { version } => match version {
                Some(version) => svc.portfolio_version(version)?,
                None => svc.portfolio()?,
            },
            PortfolioCmd::History => svc.portfolio_history()?,
            PortfolioCmd::Restore {
                version,
                context_token,
            } => svc.restore_portfolio(version, Some(&context_token))?,
        },
        Cmd::Feedback { project, action } => match action {
            None => json!({"feedback": svc.feedback(project.as_deref())?}),
            Some(FeedbackCmd::List { project: selected }) => {
                if project.is_some() && selected.is_some() && project != selected {
                    return Err(WatchtowerError::new(
                        "invalid_arguments",
                        "Two different --project filters were supplied. Nothing was changed.",
                    )
                    .hint("Pass one --project value to select the reactions for that project."));
                }
                json!({"feedback": svc.feedback(selected.as_deref().or(project.as_deref()))?})
            }
            Some(FeedbackCmd::Set {
                id,
                value,
                note,
                until,
            }) => {
                reject_feedback_filter(project.as_deref())?;
                svc.set_feedback_until(
                    &id,
                    Some(&value.to_lowercase()),
                    note.as_deref(),
                    until.as_deref(),
                )?
            }
            Some(FeedbackCmd::Clear { id }) => {
                reject_feedback_filter(project.as_deref())?;
                svc.set_feedback(&id, None, None)?
            }
            Some(FeedbackCmd::Restore { id, input }) => {
                reject_feedback_filter(project.as_deref())?;
                let snapshot =
                    read_input(input.file.as_deref(), input.input_json.as_deref(), stdin)?;
                svc.restore_feedback(&id, Some(&snapshot))?
            }
        },
        Cmd::Ideas { action } => match action {
            IdeasCmd::List => json!({"ideas": svc.ideas()?}),
            IdeasCmd::Add { text } => json!({"idea": svc.add_idea(&text)?}),
            IdeasCmd::Remove { id } => svc.remove_idea(id)?,
            IdeasCmd::Trash => json!({"ideas": svc.ideas_trash()?}),
            IdeasCmd::Restore { id } => svc.restore_idea(id)?,
        },
        Cmd::Runs { action } => match action {
            RunsCmd::Start { no_refresh } => svc.run_start(!no_refresh)?,
            RunsCmd::Status => svc.run_status()?,
            RunsCmd::Fail { id, project, error } => svc.run_fail(id, project.as_deref(), &error)?,
            RunsCmd::Finish { id } => svc.run_finish(id)?,
        },
        Cmd::Tasks { action } => match action {
            TasksCmd::List { project } => svc.tasks(project.as_deref())?,
            TasksCmd::Show { id } => {
                let tasks = svc.tasks(None)?;
                let task = tasks["tasks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|task| task["id"] == id)
                    .cloned()
                    .ok_or_else(|| {
                        let choices: Vec<String> = tasks["tasks"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .take(8)
                            .map(|t| format!("{} ({})", t["id"], t["title"].as_str().unwrap_or("")))
                            .collect();
                        WatchtowerError::new(
                            "unknown_task",
                            format!("Task {id} does not exist. No task was changed."),
                        )
                        .hint(if choices.is_empty() {
                            "There are no tasks.".to_owned()
                        } else {
                            format!("Available task IDs: {}.", choices.join(", "))
                        })
                    })?;
                json!({"task": task})
            }
            TasksCmd::Create {
                project,
                title,
                why,
                evidence,
                done_when,
                source_id,
                file,
            } => {
                let payload = if let Some(file) = file {
                    read_input(Some(&file), None, stdin)?
                } else {
                    let mut payload = json!({"projects": project, "title": title, "why": why, "evidence": evidence, "done_when": done_when});
                    if let Some(source_id) = source_id {
                        payload["source_id"] = json!(source_id);
                    }
                    payload
                };
                svc.create_task(payload)?
            }
            TasksCmd::Confirm { id, revision } => {
                svc.set_task_status_guarded(id, "done", revision)?
            }
            TasksCmd::Reopen { id, revision } => {
                svc.set_task_status_guarded(id, "assigned", revision)?
            }
            TasksCmd::Cancel { id, revision } => {
                svc.set_task_status_guarded(id, "cancelled", revision)?
            }
            TasksCmd::Report {
                id,
                status,
                result,
                evidence,
            } => svc.report_task(id, &status.to_lowercase(), &result, &evidence)?,
        },
        Cmd::Warnings { action } => match action {
            WarningsCmd::List { id } => {
                json!({"project": id, "warnings": svc.project_warnings(&id)?, "acknowledgements": svc.warning_acknowledgements(&id)?})
            }
            WarningsCmd::Acknowledge { id, key, note } => {
                svc.set_warning_ack(&id, &key, Some(&note))?
            }
            WarningsCmd::Clear { id, key } => svc.set_warning_ack(&id, &key, None)?,
        },
        Cmd::Changes { action } => match action {
            ChangesCmd::Show => json!({"changes": svc.state(false)?["changes"]}),
            ChangesCmd::Read { token } => svc.mark_changes_read(&token)?,
        },
        Cmd::Settings { action } => {
            let mut result = if let SettingsCmd::Update {
                style,
                hidden_modules,
            } = action
            {
                if style.is_none() && hidden_modules.is_none() {
                    return Err(WatchtowerError::new("invalid_arguments", "No preference was supplied. Nothing was changed.")
                        .hint("Pass --style handover, --style board or --style handbook, or pass --hidden-modules with comma-separated module IDs."));
                }
                let mut updates = Vec::new();
                if let Some(style) = style {
                    updates.push(("style", json!(style.to_lowercase())));
                }
                if let Some(modules) = hidden_modules {
                    let modules: Vec<&str> = modules
                        .split(',')
                        .map(str::trim)
                        .filter(|m| !m.is_empty())
                        .collect();
                    updates.push(("hidden_modules", json!(modules)));
                }
                svc.update_prefs(&updates)?
            } else {
                json!({"prefs": svc.state(false)?["prefs"]})
            };
            let mut settings = system::settings();
            settings["data_dir"] = json!(data_dir.to_string_lossy());
            result["settings"] = settings;
            result
        }
    })
}

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
fn requested_json(args: &[OsString]) -> bool {
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
    let Some(rest) = command.strip_prefix("watchtower ") else {
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
    format!("watchtower --instance p_{hex} {rest}")
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

fn command_index(command: &clap::Command) -> String {
    fn append(command: &clap::Command, path: &str, lines: &mut Vec<String>) {
        let children: Vec<_> = command
            .get_subcommands()
            .filter(|sub| sub.get_name() != "help")
            .collect();
        if children.is_empty() {
            let purpose = command
                .get_about()
                .map(ToString::to_string)
                .unwrap_or_default();
            let verb = path.split_whitespace().last().unwrap_or("");
            let changes = matches!(
                verb,
                "add"
                    | "create"
                    | "update"
                    | "remove"
                    | "restore"
                    | "rename"
                    | "goal"
                    | "refresh"
                    | "put"
                    | "start"
                    | "fail"
                    | "finish"
                    | "report"
                    | "confirm"
                    | "reopen"
                    | "cancel"
                    | "set"
                    | "clear"
                    | "acknowledge"
                    | "read"
                    | "visit"
            ) || path == "projects status";
            let marker = if changes {
                "changes data"
            } else {
                "reads data"
            };
            lines.push(format!("  {path}: {purpose} [{marker}]"));
        } else {
            for child in children {
                append(child, format!("{path} {}", child.get_name()).trim(), lines);
            }
        }
    }
    let mut lines = Vec::new();
    append(command, "", &mut lines);
    format!("Complete command index:\n{}\n\nReadable text is the default. --json selects full JSON and takes precedence over --format.\nExit codes: 0 success; 1 failed or incomplete; 2 invalid call.", lines.join("\n"))
}

fn parse_cli(args: &[OsString]) -> std::result::Result<Cli, clap::Error> {
    let command = Cli::command();
    let index = command_index(&command);
    let matches = command.after_help(index).try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

fn requested_data_dir(args: &[OsString], fallback: &Path) -> PathBuf {
    // Parse only recognized options for diagnostics after a failed call. This
    // reads no database and cannot turn a malformed command into an execution.
    Cli::command()
        .ignore_errors(true)
        .try_get_matches_from(args)
        .ok()
        .and_then(|matches| {
            matches
                .get_one::<PathBuf>("data_dir")
                .or_else(|| matches.get_one::<PathBuf>("instance"))
                .cloned()
        })
        .unwrap_or_else(|| fallback.to_owned())
}

fn usage_error(error: &clap::Error) -> WatchtowerError {
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
        .skip_while(|word| *word != "watchtower")
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
        recovery.push_str("\nDocument input uses --file PATH or --input-json JSON. --json only selects the output format; it does not take a document.");
    }
    WatchtowerError::new(
        "invalid_arguments",
        format!("{}\nNothing was changed.", message.trim()),
    )
    .hint(recovery)
}

pub(crate) fn write_error(
    out: &mut dyn Write,
    error: &WatchtowerError,
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
            let hint = hint.replace("watchtower --json ", "watchtower ");
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
    let prefix = scoped_command(data_dir, "watchtower ");
    let mut parts = hint.split("watchtower ");
    let mut result = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        if part.starts_with("--instance ") || part.starts_with("--data-dir ") {
            result.push_str("watchtower ");
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
            scoped_lines(&cli_output::human(key, &value), data_dir)
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
            let mut error = WatchtowerError::new(
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
            let error = WatchtowerError::new("job_result_unreadable", "The saved command result is incomplete. Its outcome is unknown.")
                .hint("Read watchtower activity and the affected item before deciding whether to repeat a change.");
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
    } else if std::env::var_os("WATCHTOWER_HOME").is_some() {
        "WATCHTOWER_HOME"
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
    let ticket = if std::env::var_os("WATCHTOWER_INTERNAL_JOB_WORKER").is_none() {
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
        let error = WatchtowerError::new(
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
