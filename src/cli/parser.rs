//! Command grammar and parser-owned command identity.
use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

use super::decode_instance;
use crate::service::DEFAULT_MAX_AGE_DAYS;

#[derive(Parser)]
#[command(
    name = "vwatch",
    bin_name = "vwatch",
    version,
    about = "Morning overview of all your agent-built projects.",
    after_help = "Commands show readable text. Use --json when a program parses the output. Exit codes: 0 success, 1 failed or incomplete, 2 invalid call."
)]
pub(super) struct Cli {
    /// Print one complete JSON object for programs. Omit for readable text.
    #[arg(long, global = true)]
    pub(super) json: bool,
    /// Output format: text or json. Omit for readable text.
    #[arg(long = "format", aliases = ["output"], short = 'o', global = true, value_enum, ignore_case = true)]
    pub(super) format: Option<OutputFormat>,
    /// Data folder for this call. Omit to use VWATCH_HOME or the installed data folder.
    #[arg(long, global = true)]
    pub(super) data_dir: Option<PathBuf>,
    /// Portable instance locator printed by continuation commands.
    #[arg(long, global = true, hide = true, value_parser = decode_instance, conflicts_with = "data_dir")]
    pub(super) instance: Option<PathBuf>,
    #[command(subcommand)]
    pub(super) command: Cmd,
}

#[derive(Subcommand)]
pub(super) enum Cmd {
    /// Read paths and project counts; export the bundled operating Skill
    Status,
    /// Read the complete morning overview; --visit records a visit only when requested by the owner
    #[command(alias = "state")]
    Overview {
        /// Record an owner-requested visit. Omit to leave visits unchanged.
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
        #[arg(long, default_value_t = DEFAULT_MAX_AGE_DAYS, value_parser = clap::value_parser!(i64).range(0..), help = format!("Mark unchanged cards due after this many days; does not rewrite them. Omit for {} days.", DEFAULT_MAX_AGE_DAYS))]
        max_age_days: i64,
        /// Use stored facts. Omit to measure all projects first.
        #[arg(long)]
        no_refresh: bool,
    },
    /// Read project evidence and the context token required to save its card
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
        /// Filter reads by project ID. Omit for all reactions; not valid with changes.
        #[arg(long)]
        project: Option<String>,
        #[command(subcommand)]
        action: Option<FeedbackCmd>,
    },
    /// Manage own ideas and the implementation/archive status of any idea or recommendation
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
    Schema {
        #[arg(ignore_case = true)]
        kind: SchemaKind,
    },
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
        /// Zero-based character offset from the previous Next command. Omit to restart the saved output.
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
        /// Include calls on or after this UTC date, YYYY-MM-DD. Omit for all retained usage.
        #[arg(long)]
        since: Option<String>,
    },
    /// Recent command outcomes for recovery after a lost response
    Activity,
    /// Read stored project statistics and comparisons; does not refresh facts
    Statistics {
        /// Project ID. Omit to compare all registered projects.
        #[arg(long)]
        project: Option<String>,
        /// First date, YYYY-MM-DD. Omit for the last 30 days ending at --until.
        #[arg(long)]
        from: Option<String>,
        /// Last date, YYYY-MM-DD. Omit for today.
        #[arg(long)]
        until: Option<String>,
    },
    /// Read the durable content journal, including removals and undo writes
    History {
        #[command(subcommand)]
        action: HistoryCmd,
    },
    /// Read independently retained agent proposals, including those absent from current reports
    Proposals {
        /// Project ID. Omit for project proposals and portfolio ideas together.
        #[arg(long)]
        project: Option<String>,
        /// idea: new capabilities; recommendation: next steps and upkeep. Omit for both.
        #[arg(long, value_parser = ["idea", "recommendation"])]
        category: Option<String>,
    },
    /// Read the questions agents left for the owner that still await an answer, longest waiting first
    Questions {
        /// Project ID. Omit for all projects.
        #[arg(long)]
        project: Option<String>,
    },
    /// Undo one recorded owner change if its result has not changed again
    Undo { token: String },
}

#[derive(Subcommand)]
pub(super) enum HistoryCmd {
    /// Read up to 30 saved events, newest first, with their IDs and changed fields
    List {
        /// Project ID, including a removed project. Omit for all content.
        #[arg(long)]
        project: Option<String>,
        /// Kind: card, portfolio, project, idea, proposal, task, feedback or warning. Omit for all kinds.
        #[arg(long)]
        entity: Option<String>,
        /// Exact entity ID from a history event. Omit for all entries of the selected kind.
        #[arg(long)]
        id: Option<String>,
        /// Event ID from next_before. Omit for the newest page.
        #[arg(long)]
        before: Option<i64>,
    },
    /// Read one event's full before/after content and changed fields
    Show { id: i64 },
}

#[derive(Subcommand)]
pub(super) enum JobsCmd {
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
pub(super) enum OutputFormat {
    Text,
    Json,
}

#[derive(Subcommand)]
pub(super) enum FeedbackCmd {
    /// Read the owner's ratings and replies
    #[command(alias = "ls")]
    List {
        /// Project ID to filter reactions. Omit for all projects and portfolio reactions.
        #[arg(long)]
        project: Option<String>,
    },
    /// Record the owner's explicit rating or answer; never invent a reaction
    #[command(aliases = ["update", "rate"])]
    Set {
        id: String,
        /// Owner reaction: up (good), later, down (rejected), answered or done.
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
pub(super) enum WarningsCmd {
    /// Current measured warnings and stored acknowledgements for one project
    #[command(aliases = ["show", "ls"])]
    List { id: String },
    /// Record the owner's reason for an intentional measured condition
    #[command(alias = "ack")]
    Acknowledge {
        id: String,
        /// Measured warning key from warnings list for this project.
        #[arg(long, value_parser = ["no_remote", "ci_none", "worktrees"], ignore_case = true)]
        key: String,
        /// The owner's reason for accepting this measured condition.
        #[arg(long)]
        note: String,
    },
    /// Withdraw an acknowledgement when the owner asks
    Clear {
        id: String,
        /// Measured warning key from warnings list for this project.
        #[arg(long, value_parser = ["no_remote", "ci_none", "worktrees"], ignore_case = true)]
        key: String,
    },
}

#[derive(Subcommand)]
pub(super) enum ChangesCmd {
    /// Changes and the token of the exact snapshot displayed
    #[command(aliases = ["list", "get", "view"])]
    Show,
    /// Mark the displayed snapshot read on the owner's explicit request
    #[command(alias = "acknowledge")]
    Read {
        /// Exact snapshot token from changes show or overview.
        #[arg(long)]
        token: String,
    },
}

#[derive(Subcommand)]
pub(super) enum SettingsCmd {
    /// Installation paths, autostart/PATH state and display preferences
    #[command(aliases = ["list", "get", "view"])]
    Show,
    /// Set display preferences only when the owner asks; preserves omitted values
    #[command(alias = "set")]
    Update {
        /// Initial view: handover (Today), board (Projects), ideas or statistics; handbook opens Projects. Omit to keep it.
        #[arg(long, value_parser = ["handover", "board", "ideas", "handbook", "statistics"], ignore_case = true)]
        style: Option<String>,
        /// Comma-separated section IDs to collapse; empty expands all. Omit to keep them.
        /// IDs: focus, projects, waiting, safety, recent, across, ideas, pulse, kpis, cards, tasks, changes, night, space, parking, week.
        #[arg(long)]
        hidden_modules: Option<String>,
        /// Window and new report language: en (English) or de (German). Omit to keep it; unset uses English. CLI output stays English.
        #[arg(long, value_parser = ["en", "de"], ignore_case = true)]
        language: Option<String>,
    },
}

#[derive(Subcommand)]
pub(super) enum IdeasCmd {
    /// Read an idea's status, or record the owner's implementation/archive decision; ratings stay unchanged
    Status {
        /// Item ID from proposals or item_id from ideas list.
        id: String,
        /// Omit to read. open reopens the idea; restore leaves the archive. No action starts an agent.
        #[arg(value_parser = ["open", "in_progress", "implemented", "archived", "restore"], ignore_case = true)]
        status: Option<String>,
        /// Revision from ideas status or overview idea_states; required for changes, protecting newer task results.
        #[arg(long, required_if_eq_any = [("status", "open"), ("status", "in_progress"), ("status", "implemented"), ("status", "archived"), ("status", "restore")], requires = "status")]
        revision: Option<String>,
    },
    /// Edit a parked idea, preserving its ID, history and previous text for undo
    Update {
        id: i64,
        text: String,
        /// Current revision from ideas list; protects newer edits.
        #[arg(long)]
        revision: i64,
    },
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
pub(super) enum RunsCmd {
    /// Start a tracked daily run, refreshing facts first
    Start {
        /// Use stored facts. Omit to measure all projects before starting the run.
        #[arg(long)]
        no_refresh: bool,
    },
    /// Latest run, pending work and last fully successful run
    Status,
    /// Record a project failure (omit --project for the portfolio brief)
    Fail {
        id: i64,
        /// Failed project ID. Omit to record a portfolio failure.
        #[arg(long)]
        project: Option<String>,
        /// What prevented completion and the next step needed to resolve it.
        #[arg(long)]
        error: String,
    },
    /// Close the run; success requires current cards and a current brief
    Finish { id: i64 },
}

#[derive(Subcommand)]
pub(super) enum TasksCmd {
    /// Assigned work, reported results and completed tasks
    #[command(alias = "ls")]
    List {
        /// Project ID to filter assignments. Omit for all tasks.
        #[arg(long)]
        project: Option<String>,
    },
    /// Read a task's snapshot, result, revision and history
    #[command(aliases = ["get", "view"])]
    Show { id: i64 },
    /// Assign work only when the owner asks; reports still require owner confirmation
    #[command(aliases = ["add", "assign"])]
    Create {
        /// Project IDs; repeat or separate by commas. Omit for work without a linked project.
        #[arg(long, value_delimiter = ',')]
        project: Vec<String>,
        /// Short task title. Omit only when --file supplies the task document.
        #[arg(long, required_unless_present = "file")]
        title: Option<String>,
        /// Why the task matters. Omit for no reason text.
        #[arg(long, default_value = "")]
        why: String,
        /// Files or observations supporting the task. Omit for no evidence text.
        #[arg(long, default_value = "")]
        evidence: String,
        /// Observable completion criterion. Omit for no criterion.
        #[arg(long, default_value = "")]
        done_when: String,
        /// ID of the item being assigned. Omit for independent work.
        #[arg(long)]
        source_id: Option<String>,
        /// JSON task file (projects, title, why, evidence, done_when, source_id); '-' reads stdin. Omit to use inline fields.
        #[arg(long, conflicts_with_all = ["project", "title", "why", "evidence", "done_when", "source_id"])]
        file: Option<String>,
    },
    /// Confirm a reviewed report only on the owner's explicit instruction
    Confirm {
        id: i64,
        /// Current task revision from tasks list, show or a saved task result.
        #[arg(long)]
        revision: i64,
    },
    /// Reopen a task only on the owner's explicit instruction
    Reopen {
        id: i64,
        /// Current task revision from tasks list, show or a saved task result.
        #[arg(long)]
        revision: i64,
    },
    /// Cancel a task only on the owner's explicit instruction
    Cancel {
        id: i64,
        /// Current task revision from tasks list, show or a saved task result.
        #[arg(long)]
        revision: i64,
    },
    /// Report an outcome; done awaits the owner's confirmation
    Report {
        id: i64,
        /// done requests owner review; blocked records unfinished work. Omit only with --file.
        #[arg(long, value_parser = ["done", "blocked"], ignore_case = true, required_unless_present = "file")]
        status: Option<String>,
        /// Outcome, or blocker and next step. Omit only when --file supplies the report.
        #[arg(long, required_unless_present = "file")]
        result: Option<String>,
        /// Checks or files supporting the result. Required for done; omit for blocked without evidence.
        #[arg(long, default_value = "")]
        evidence: String,
        /// JSON report file with status, result and evidence (required for done); '-' reads stdin. Omit to use inline fields.
        #[arg(long, conflicts_with_all = ["status", "result", "evidence"])]
        file: Option<String>,
    },
}

#[derive(Subcommand)]
pub(super) enum ProjectsCmd {
    /// All registered projects
    #[command(aliases = ["ls", "search", "find"])]
    List {
        /// Match ID, name, folder or card text (case-insensitive substring). Omit for all projects.
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
        /// Stable ID for one folder. Omit to derive it from the folder name.
        #[arg(long)]
        id: Option<String>,
        /// Display name for one folder. Omit to use the folder name.
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
    /// Change a project's name or folder; supply either option or both and keep its history
    Update {
        id: String,
        /// Display name. Omit to keep the current name.
        #[arg(long)]
        name: Option<String>,
        /// Existing folder after a rename or move. Omit to keep the path; never moves files.
        #[arg(long)]
        path: Option<String>,
    },
    /// Set the owner's goal for the project (an empty goal clears it)
    Goal {
        id: String,
        goal: String,
        /// Goal due date as YYYY-MM-DD. Omit to clear any previous due date.
        #[arg(long)]
        due: Option<String>,
    },
    /// Set what the owner intends with the project
    Status {
        id: String,
        #[arg(ignore_case = true)]
        status: Status,
        /// Owner's reason, shown to people and agents. Omit to keep the existing note.
        #[arg(long)]
        note: Option<String>,
    },
    /// Git repositories in a folder (or a repository and its neighbours) that are not registered yet
    Discover { root: String },
}

#[derive(Subcommand)]
pub(super) enum FactsCmd {
    /// All saved measurements for one project, including errors; does not refresh
    History { id: String },
    /// Measure again now (all projects when no id is given)
    Refresh {
        ids: Vec<String>,
        /// Skip the GitHub CI lookup. Omit to measure CI along with local git facts.
        #[arg(long)]
        no_ci: bool,
    },
    /// Facts of one project
    #[command(aliases = ["get", "view"])]
    Show { id: String },
}

#[derive(Subcommand)]
pub(super) enum CardCmd {
    /// Compare two saved card versions without changing the current card
    Compare {
        id: String,
        /// Earlier saved version number from card history.
        #[arg(long)]
        from: i64,
        /// Later saved version number from card history.
        #[arg(long)]
        to: i64,
    },
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
        /// Saved version number from card history. Omit for the current card.
        #[arg(long)]
        version: Option<i64>,
    },
    /// Saved card versions and the context token required to restore one
    History { id: String },
    /// Restore a saved version after reconciling the current project context
    Restore {
        id: String,
        /// Saved version number from card history to append as the current card.
        #[arg(long)]
        version: i64,
        /// Token from the reconciled project context.
        #[arg(long)]
        context_token: String,
    },
}

#[derive(Subcommand)]
pub(super) enum PortfolioCmd {
    /// Compare two saved brief versions without changing the current brief
    Compare {
        /// Earlier saved version number from portfolio history.
        #[arg(long)]
        from: i64,
        /// Later saved version number from portfolio history.
        #[arg(long)]
        to: i64,
    },
    /// All projects side by side, the previous brief and all ratings; projects
    /// unchanged since the previous brief appear as summaries
    Context {
        /// Project ID whose full card details to return instead of the whole context. Repeat for several.
        /// Omit for the whole context.
        #[arg(long)]
        project: Vec<String>,
    },
    /// Save a new brief
    #[command(aliases = ["set", "update"])]
    Put {
        #[command(flatten)]
        doc: DocArgs,
    },
    /// The current brief
    #[command(aliases = ["get", "view"])]
    Show {
        /// Saved version number from portfolio history. Omit for the current brief.
        #[arg(long)]
        version: Option<i64>,
    },
    /// Saved brief versions and the context token required to restore one
    History,
    /// Restore a saved version after reconciling the current portfolio context
    Restore {
        /// Saved version number from portfolio history to append as the current brief.
        #[arg(long)]
        version: i64,
        /// Token from the reconciled portfolio context.
        #[arg(long)]
        context_token: String,
    },
}

#[derive(Args)]
pub(super) struct InputArgs {
    /// JSON file; '-' reads stdin. Accepts UTF-8 or BOM-marked UTF-16. Omit only with --input-json.
    #[arg(long, conflicts_with = "input_json")]
    pub(super) file: Option<String>,
    /// Inline JSON. Omit when using --file, which preserves quotes and line breaks.
    #[arg(long)]
    pub(super) input_json: Option<String>,
}

#[derive(Args)]
pub(super) struct DocArgs {
    /// JSON file; '-' reads stdin. Accepts UTF-8 or BOM-marked UTF-16. Omit only with --input-json.
    #[arg(long, conflicts_with = "input_json")]
    pub(super) file: Option<String>,
    /// Inline JSON. Omit when using --file, which preserves quotes and line breaks.
    #[arg(long)]
    pub(super) input_json: Option<String>,
    /// Replace supplied top-level fields, keeping omitted fields; arrays are replaced whole. Omit for a complete document.
    #[arg(long)]
    pub(super) merge: bool,
    /// Validate and preview without saving. Omit to save the document.
    #[arg(long)]
    pub(super) dry_run: bool,
    /// Token from the complete context used to write this document. Omit only with --dry-run.
    #[arg(long)]
    pub(super) context_token: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum Status {
    Active,
    Parked,
    Done,
    Archived,
}

#[derive(Clone, Copy, ValueEnum)]
pub(super) enum SchemaKind {
    Card,
    Portfolio,
}

impl Cmd {
    pub(super) fn output_key(&self) -> &'static str {
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
            Cmd::Statistics { .. } => "statistics",
            Cmd::History { action } => match action {
                HistoryCmd::List { .. } => "history.list",
                HistoryCmd::Show { .. } => "history.show",
            },
            Cmd::Proposals { .. } => "proposals",
            Cmd::Questions { .. } => "questions",
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
                FactsCmd::History { .. } => "facts.history",
                FactsCmd::Refresh { .. } => "facts.refresh",
                FactsCmd::Show { .. } => "facts.show",
            },
            Cmd::Due { .. } => "due",
            Cmd::Context { .. } => "context",
            Cmd::Card { action } => match action {
                CardCmd::Compare { .. } => "card.compare",
                CardCmd::Put { .. } => "card.put",
                CardCmd::Show { .. } => "card.show",
                CardCmd::History { .. } => "card.history",
                CardCmd::Restore { .. } => "card.restore",
            },
            Cmd::Portfolio { action } => match action {
                PortfolioCmd::Compare { .. } => "portfolio.compare",
                PortfolioCmd::Context { .. } => "portfolio.context",
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
                IdeasCmd::Status { .. } => "ideas.status",
                IdeasCmd::Update { .. } => "ideas.update",
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
                    | "undo"
            ) || path == "projects status";
            let marker = match path {
                "due" => "refreshes facts unless --no-refresh",
                "overview" => "reads data; --refresh/--visit change data",
                "ideas status" => "reads data; supplied status changes data",
                "context" | "facts show" => "reads data; measures missing facts",
                "status" => "reads data; exports Skill",
                _ if changes => "changes data",
                _ => "reads data",
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

pub(super) fn parse_cli(args: &[OsString]) -> std::result::Result<Cli, clap::Error> {
    fn describe_output(command: &mut clap::Command) {
        for child in command.get_subcommands_mut() {
            *child = child.clone().after_help("Output: readable text with results or recovery; --json returns the complete object. Exit codes: 0 success, 1 failed or incomplete, 2 invalid call.");
            describe_output(child);
        }
    }
    let mut command = Cli::command();
    describe_output(&mut command);
    let index = command_index(&command);
    let matches = command.after_help(index).try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}
