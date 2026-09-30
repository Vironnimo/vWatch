//! The command line agents use. Every command prints one JSON object.
//!
//! Success: {"ok": true, ...}. Expected failure: {"ok": false, "error": {"code", "message", "hint"}}
//! with exit code 1. Wrong usage prints help on stderr with exit code 2.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};

use crate::cards::{self, Kind};
use crate::service::{Lookout, LookoutError, Result, DEFAULT_MAX_AGE_DAYS};
use crate::system;

#[derive(Parser)]
#[command(name = "lookout", bin_name = "lookout", version, about = "Morning overview of all your agent-built projects. Every command prints JSON.")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Where Lookout keeps its data and how many projects it watches
    Status,
    /// Register and manage the watched projects
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
        #[arg(long, default_value_t = DEFAULT_MAX_AGE_DAYS)]
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
    },
    /// JSON schema of a document
    Schema { kind: SchemaKind },
}

#[derive(Subcommand)]
enum ProjectsCmd {
    /// All registered projects
    List,
    /// Register a project folder (usually a git repository)
    Add {
        path: String,
        /// Short id; defaults to the folder name
        #[arg(long)]
        id: Option<String>,
        /// Display name; defaults to the folder name
        #[arg(long)]
        name: Option<String>,
    },
    /// Stop watching a project and delete its stored cards
    Remove { id: String },
    /// Change the display name
    Rename { id: String, name: String },
    /// Set what the owner intends with the project
    Status {
        id: String,
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
    Show { id: String },
}

#[derive(Subcommand)]
enum CardCmd {
    /// Save a new card version
    Put {
        id: String,
        #[command(flatten)]
        doc: DocArgs,
    },
    /// The current card
    Show { id: String },
}

#[derive(Subcommand)]
enum PortfolioCmd {
    /// All projects side by side, the previous brief and all ratings
    Context,
    /// Save a new brief
    Put {
        #[command(flatten)]
        doc: DocArgs,
    },
    /// The current brief
    Show,
}

#[derive(Args)]
struct DocArgs {
    /// JSON file with the document; '-' reads stdin
    #[arg(long, conflicts_with = "json")]
    file: Option<String>,
    /// The document as a JSON string
    #[arg(long)]
    json: Option<String>,
    /// Send only changed top-level fields; the rest is kept from the previous version
    #[arg(long)]
    merge: bool,
    /// Check the document and show what would be saved, without saving it
    #[arg(long)]
    dry_run: bool,
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

/// Decode a document: UTF-8 with or without BOM, or UTF-16 with BOM (Windows PowerShell's default).
fn decode(bytes: &[u8]) -> String {
    let utf16 = |be: bool| {
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, ..] => utf16(false),
        [0xFE, 0xFF, ..] => utf16(true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn read_document(doc: &DocArgs, stdin: &mut dyn Read) -> Result<Value> {
    let (text, origin) = match (&doc.json, doc.file.as_deref()) {
        (Some(json), _) => (json.clone(), "--json".to_owned()),
        (None, Some("-")) => {
            let mut buf = Vec::new();
            stdin.read_to_end(&mut buf).map_err(|e| LookoutError::new("file_unreadable", format!("Cannot read stdin: {e}.")))?;
            (decode(&buf), "stdin".to_owned())
        }
        (None, Some(file)) => {
            let bytes = std::fs::read(file).map_err(|e| LookoutError::new("file_unreadable", format!("Cannot read {file}: {e}.")))?;
            (decode(&bytes), file.to_owned())
        }
        (None, None) => {
            return Err(LookoutError::new("missing_document", "No document given.")
                .hint("Pass --file <path>, --file - for stdin, or --json '<object>'."))
        }
    };
    serde_json::from_str(&text).map_err(|e| LookoutError::new("invalid_json", format!("{origin} is not valid JSON: {e}.")))
}

fn execute(cmd: Cmd, data_dir: &Path, stdin: &mut dyn Read) -> Result<Value> {
    if let Cmd::Schema { kind } = cmd {
        let kind = match kind {
            SchemaKind::Card => Kind::Card,
            SchemaKind::Portfolio => Kind::Portfolio,
        };
        return Ok(json!({"schema": cards::json_schema(kind)}));
    }
    let db = system::db_path(data_dir);
    let svc = Lookout::open(&db)?;
    Ok(match cmd {
        Cmd::Schema { .. } => unreachable!("handled above"),
        Cmd::Status => {
            // A freshly installed exe hands out its own Skill before the window ever starts.
            let skill = system::export_skill().ok();
            let projects = svc.list_projects()?;
            let list = projects.as_array().cloned().unwrap_or_default();
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "exe": system::exe_path().to_string_lossy(),
                "data_dir": data_dir.to_string_lossy(),
                "database": db.to_string_lossy(),
                "skill_dir": skill.map(|d| d.to_string_lossy().into_owned()),
                "projects": list.len(),
                "with_card": list.iter().filter(|p| p["has_card"] == json!(true)).count(),
                "hint": if list.is_empty() { json!("No projects yet. The owner adds them in the Lookout app, or run `lookout projects add <path>` when the owner asks.") } else { Value::Null },
            })
        }
        Cmd::Projects { action } => match action {
            ProjectsCmd::List => json!({"projects": svc.list_projects()?}),
            ProjectsCmd::Add { path, id, name } => json!({"project": svc.add_project(&path, id.as_deref(), name.as_deref())?}),
            ProjectsCmd::Remove { id } => svc.remove_project(&id)?,
            ProjectsCmd::Rename { id, name } => json!({"project": svc.rename_project(&id, &name)?}),
            ProjectsCmd::Status { id, status, note } => {
                let status = status.to_possible_value().expect("every status has a name").get_name().to_owned();
                json!({"project": svc.set_status(&id, &status, note.as_deref())?})
            }
            ProjectsCmd::Discover { root } => svc.discover(&root)?,
        },
        Cmd::Facts { action } => match action {
            FactsCmd::Refresh { ids, no_ci } => json!({"refreshed": svc.refresh_facts(Some(&ids), !no_ci)?}),
            FactsCmd::Show { id } => json!({"facts": svc.facts(&id)?}),
        },
        Cmd::Due { max_age_days, no_refresh } => svc.due(max_age_days, !no_refresh)?,
        Cmd::Context { id } => svc.context(&id)?,
        Cmd::Card { action } => match action {
            CardCmd::Put { id, doc } => svc.put_card(&id, read_document(&doc, stdin)?, doc.merge, doc.dry_run)?,
            CardCmd::Show { id } => svc.card(&id)?,
        },
        Cmd::Portfolio { action } => match action {
            PortfolioCmd::Context => svc.portfolio_context()?,
            PortfolioCmd::Put { doc } => svc.put_portfolio(read_document(&doc, stdin)?, doc.merge, doc.dry_run)?,
            PortfolioCmd::Show => svc.portfolio()?,
        },
        Cmd::Feedback { project } => json!({"feedback": svc.feedback(project.as_deref())?}),
    })
}

/// Run one command. Output goes to `out`, usage errors to stderr. Returns the exit code.
pub fn run<I, T>(args: I, data_dir: &Path, stdin: &mut dyn Read, out: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let code = e.exit_code();
            let _ = e.print();
            return code;
        }
    };
    let (body, code) = match execute(cli.command, data_dir, stdin) {
        Ok(result) => {
            let mut body = json!({"ok": true});
            if let (Some(b), Value::Object(r)) = (body.as_object_mut(), result) {
                b.extend(r);
            }
            (body, 0)
        }
        Err(e) => (json!({"ok": false, "error": e.to_json()}), 1),
    };
    let text = serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{\"ok\": false}".into());
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
    code
}

/// Entry point of the exe when it is started with arguments.
pub fn main(args: Vec<OsString>) -> i32 {
    let data_dir = system::data_dir();
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    run(args, &data_dir, &mut std::io::stdin(), &mut lock)
}
