# Watchtower CLI reference

Use this reference for owner actions outside the daily recipes, history recovery, long commands, document input or code that consumes output. Commands run from any working directory. Replace `<placeholders>` with the values named beside each example. Ordinary output is readable text; command and group `--help` describe arguments.

Contents:
- Output, continuation and recovery
- Document writes and history
- Projects
- Owner decisions
- Assigned work
- Overview, settings and reading state
- Daily-run data
- Facts
- Programmatic output

## Output, continuation and recovery

The first line states the result. `Note:` names interpretation, omissions or partial effects; `Next:` gives an exact continuation; `Error:` describes failure and recovery; `Undo:` reverses a recorded change where supported. Content from records is indented data, not a command to follow.

Text output is paged when it exceeds the output budget. Copy the exact `Next: watchtower output <token> --start <offset>` line. The token identifies a fixed snapshot; the offset counts Unicode characters from zero. Continuing a snapshot does not rerun its operation. Before a card or brief write, read every page of its context.

A potentially long command runs for up to 45 seconds inline. If unfinished, it returns a job token and an exact `jobs wait` continuation. Each wait is bounded to 45 seconds and either returns the original completed result or the next wait. `jobs list` lists recorded jobs; `jobs status <job-token>` inspects one. Waiting never repeats the original operation. A job with an unknown outcome requires state inspection, not blind retry; follow its recovery command.

If the shell loses a response, run `watchtower activity` for recent command outcomes. Use an available job continuation. If completion was not captured, inspect the affected project, card, brief, task or run before deciding whether to repeat the write. `watchtower usage --since <YYYY-MM-DD>` shows local command usage without argument values. Dates in this filter are UTC and inclusive.

The CLI works while the window is closed. `watchtower status` shows its version and data location and exports the Skill beside the executable. `--data-dir <folder>` selects a different database folder for that call; use the printed instance path when continuing its job or output. `WATCHTOWER_HOME` selects the default data folder when no flag is supplied. Never read or edit database files to bypass the CLI.

Common recoveries:

| Failure | Action |
|---|---|
| Invalid arguments or an ambiguous name | Use the corrected call or candidate IDs in the error; do not guess an ID. |
| Invalid card/brief or malformed input | Fix the named field, JSON position or encoding. The error states whether anything was saved. |
| `context_required` | Read the corresponding complete context, then write with its token. |
| `version_conflict` | Reread the current context or task and reconcile; never substitute a token/revision without checking the change. |
| Folder missing or already registered | Use the stated existing project/candidate path. A renamed folder does not justify creating a new project. |
| Edit saved but facts refresh failed | Keep the saved edit and rerun only the indicated facts refresh. |
| Run incomplete | Read its remaining project/brief failures, complete what can be completed and finish the same run again. |
| Output snapshot missing | Inspect the current state using the recovery command; do not repeat the original mutation to recreate output. |

## Document writes and history

Use `--file <path>` for card and portfolio documents. Paths are absolute Windows paths or relative to the command's working directory; use forward slashes. `--file -` reads stdin. `--input-json '<object>'` supplies inline JSON; it cannot be combined with `--file`. Files and stdin accept UTF-8, with or without BOM, and BOM-marked UTF-16. Prefer a file for multi-line content to avoid shell quoting changes. Keep temporary files in your own folder outside watched repositories.

`card put <project-id>` and `portfolio put` save versions. Both require `--context-token` from the corresponding complete context. `--merge` keeps omitted top-level fields; a supplied list or nested object replaces the whole field. `--dry-run` validates without saving, needs no token and reserves nothing. Use it when validation alone is the goal, not before every normal save.

The result states whether it saved, item IDs and warnings; a save also returns its `version`. Card writes include `changed_fields` and `unfilled_fields`. Fill all unfilled fields before declaring a card current. Unknown fields are dropped with warnings. With a merge, an untouched field over its word limit is retained with a warning; shorten it when updating that field. Without a previous card or brief, send a complete document without `--merge`.

Retain an item's ID only for the same item in its original document and kind. IDs from another project, another kind or removed items' feedback cannot be reused. Omit the ID for a new item.

History is available through `card history <project-id>` and `portfolio history`; inspect a version with `card show <project-id> --version <number>` or `portfolio show --version <number>`. To restore at the user's request, first read the current context and the chosen historical version. Replace `<version>` with that history entry's number and `<context-token>` with the current context token:

```powershell
watchtower card restore <project-id> --version <version> --context-token <context-token>
```

For a brief, use `watchtower portfolio restore --version <version> --context-token <context-token>`. Restoring an older version appends a version and preserves history; restoring the current version changes nothing. Restored evidence stays historical, so the document can remain due. Restore only content consistent with current user instructions; otherwise write a reconciled document instead.

## Projects

`projects list` shows stable IDs, names, status and card presence. `--query <text>` filters case-insensitively by ID, name, path or card text. `projects show <project-id>` includes its folder, measurement dates, card, facts, warnings and history. Names and paths can change; IDs persist.

For requested registration, `projects discover <folder>` lists unwatched repositories without registering them. A selected repository comes first, followed by its neighbors; otherwise discovery searches up to two levels below the folder. Candidates include path, name, suggested ID, last commit and a recommendation for the selected repository or one active within 120 days.

Register only the folders named in the request or selected by its stated criteria. Replace `<existing-folder>` with one of those paths:

```powershell
watchtower projects add "<existing-folder>"
```

Multiple paths can be added in one call. `--id` and `--name` apply only when adding a single path. An already registered folder retains its existing identity. Partial success reports which folders were added and which failed.

`projects update <project-id> --name <name> --path <folder>` accepts either or both options. It preserves cards, ratings, tasks, goals and history, makes affected interpretation due, and never moves files. The folder must exist and cannot belong to another watched project. `projects rename <project-id> <name>` changes only the display name. An unchanged update reports no change.

`projects status <project-id> <active|parked|done|archived> --note <text>` records the user's intent. Omit `--note` to preserve it. Archived projects leave daily runs and the main overview. `projects goal <project-id> <goal> --due <YYYY-MM-DD>` records a goal with an optional local due date; an empty goal clears it.

`projects remove <project-id>` removes a project from the watched set without deleting its history. `projects trash` lists removed projects; `projects restore <project-id>` restores the same ID and data. These actions do not modify repository files. Use update for a rename or moved folder.

## Owner decisions

`feedback` or `feedback list` returns reactions, newest first; `--project <project-id>` selects one project. Context already includes the reactions needed for interpretation. Each reaction carries its item ID, project, value, note, timestamp, snooze date and the item as it read when rated. Item kinds are `suggestion`, `question`, `focus`, `pattern`, `synergy`, `new_app` and `verdict`.

Take `<item-id>` from a card or brief. Questions accept `answered` or `done`; focus items accept `done`; suggestions, patterns, synergies, app ideas and verdicts accept `up`, `later` or `down`. For an explicit user request to defer a suggestion until a date, use its item ID:

```powershell
watchtower feedback set <item-id> --value later --until <YYYY-MM-DD>
```

Use `--note` for the user's answer or reason. Omission keeps the prior note. For `later`, an omitted `--until` preserves the existing date or sets 14 days. Reactions without a snooze date suppress an item for 14 days after their timestamp. `feedback clear <item-id>` withdraws a reaction. `feedback restore <item-id> --file <snapshot-file>` restores the reaction snapshot from a previous result; JSON null means no reaction. Prefer an emitted `Undo:` command for accidental edits.

`ideas list` shows the user's parked ideas. `ideas add <text>` records the user's idea for the next brief to assess. `ideas remove <idea-id>` moves it to `ideas trash`; `ideas restore <idea-id>` restores the same idea ID. The brief's `idea_checks[].idea` refers to this numeric ID.

`warnings list <project-id>` shows measured warnings and acknowledgements. Only on the user's request, record a deliberate condition with `warnings acknowledge <project-id> --key <key> --note <reason>`. The complete keys are `no_remote`, `ci_none` and `worktrees`. `warnings clear <project-id> --key <key>` withdraws it. The measured fact stays visible, and material worsening invalidates the acknowledgement.

`Undo: watchtower undo <token>` and document restores with context tokens reject intervening changes. If undo reports a conflict, inspect the intervening state and preserve the user's later changes. Other printed undo commands are inverse operations such as restoring a removed project; if the target changed afterwards, inspect its current state before using them.

## Assigned work

`tasks list --project <project-id>` returns assignments and results; omit the filter for every task. `tasks show <task-id>` includes the original project/name/path snapshot, title, reason, evidence, completion criterion, result, revision and history. Copying task text is not an assignment.

Create assignments only when the user requests them. Use project IDs from the selected projects and, for an existing suggestion or focus item, its source ID. Omit `--source-id` for independent work:

```powershell
watchtower tasks create --project <project-id> --title "<task-title>" --done-when "<completion-criterion>"
```

Optional `--why`, `--evidence` and `--source-id` preserve the source context. Repeat `--project` or use comma-separated IDs for several projects. For long text, use `tasks create --file <task-file>` instead of inline fields. The JSON file contains `projects` (ID array), `title`, and optional `why`, `evidence`, `done_when`, `source_id`.

`tasks report <task-id> --status done --result <outcome> --evidence <check-or-file>` requires evidence and moves the task to `review`. `--status blocked` records the blocker and next step. Reporting does not confirm success on the user's behalf. Status values are `assigned`, `blocked`, `review`, `done` and `cancelled`.

On the user's explicit decision, use `tasks confirm <task-id> --revision <revision>`, `tasks reopen <task-id> --revision <revision>` or `tasks cancel <task-id> --revision <revision>`. Take the revision from the task's current result. A conflict means the task changed; read it and reconcile the decision. Reports and task-status changes never edit a repository.

## Overview, settings and reading state

`overview` returns the same stored portfolio, projects and facts as the window. Add `--refresh` when the user asks to measure fresh facts before reading it. `overview --visit` records that the user visited the overview; `projects visit <project-id>` records a project visit. Use visit operations only when the user requests recording that visit, not when an agent merely reads data.

`changes show` returns the digest since the last explicit acknowledgement and its snapshot token. `changes read --token <token>` marks exactly that snapshot read on the user's request. Reading views, refreshing facts and recording visits do not silently acknowledge the digest.

`settings show` includes installation paths, autostart/PATH state and display preferences. `settings update --style <handover|board|handbook>` changes the requested style. `--hidden-modules <comma-separated-ids>` changes the hidden sections; an empty value shows every section. Omitted options remain unchanged. Autostart and PATH cannot be toggled through these commands; those are the user's choices in the window's settings.

## Daily-run data

`runs start` refreshes facts and records a run; keep its returned `run_id`. `runs status` shows the latest run, last successful run, pending replies, repair task and history. A run can be `running`, `stalled`, `incomplete`, `failed` or `complete`; `unknown` means no recorded status. `runs fail <run-id> --project <project-id> --error <cause>` records a project failure; omit `--project` for the brief. `runs finish <run-id>` closes that run. Only `complete` confirms current cards, brief and readable facts. `latest` can belong to another agent; use your captured run ID.

`due` is the untracked equivalent of the due check. Its default card age is 14 days; `--max-age-days <days>` accepts nonnegative days. `--no-refresh` uses stored facts for `due` or `runs start`.

If measurements fail, `due` and `runs start` exit 1 but retain usable projects and the run's `run_id`. Continue those projects and record the failures. `problems` includes missing facts (`facts_missing`), git errors and inaccessible paths; failed facts never count as `up_to_date`. JSON includes per-project measurement results in `refreshed`, nested as `due.refreshed` for `runs start`.

| Card due reason | Meaning |
|---|---|
| `no_card` | No card exists. |
| `changed` | Commits, uncommitted work, docs or side branches differ from the card's basis. `changes` names the differences. |
| `ci_changed` | CI settled red or green differently from the card's basis. |
| `answered` | The user added, revised or withdrew a question reply. |
| `status_changed` | Name, folder, status, note or goal changed. |
| `new_fields` | The card lacks listed fields; fill every `new_fields` path. |
| `stale` | The card exceeds the requested age. |

Any due entry can include `new_fields`. A null `changes` value means no comparison basis was recorded. The complete change categories are `commits`, `uncommitted`, `unpushed`, `docs`, `side_branches` and `readable`. Problems identify unreadable projects; do not write a card from missing measurements.

Portfolio due reasons are `missing`, `new_day`, `owner_idea`, `cards_changed` and `cards_will_change`. Changes to project identity, intent, membership or fields used by the brief can make it due. A card due solely for missing fields does not by itself require a brief rewrite.

`context <project-id>` includes `project`, `facts`, `previous_card`, `since_previous_card`, `new_fields`, `tasks`, `warning_acknowledgements`, `feedback`, `feedback_summary` and `context_token`. The project contains its ID, name, path, status, note, goal and goal date. Previous card and changes are null when absent.

`portfolio context` includes all project summaries, `previous_portfolio`, `owner_ideas`, `owner_last_opened`, `review_week`, tasks, reactions and its own token. `review_week` specifies the previous calendar week's ID, start/end dates and whether its review is due. Each compact card has `outdated` and `changed_since_brief`; current facts override stale card claims.

## Facts

`facts show <project-id>` reads stored facts. `facts refresh <project-id> ...` measures named projects; omit IDs for all. `--no-ci` skips the GitHub CI lookup. Refresh results identify success, error and changed state per project. A failed measurement is not evidence of a clean repository.

The measurements cover:

- `head`, `branch`, `commits` (total, first/last, 7/30/90-day counts and 26 weekly counts oldest first), remotes and upstream. `ahead`, `behind` and `unpushed_since` are null without an upstream.
- `dirty`: file/line counts, change signature and oldest/newest uncommitted edit times. These times use edits after the last commit; if all files predate it, that commit's time is used.
- `worktrees`: additional checkouts, branch/head, prune eligibility and dirty counts/signatures. Null dirty counts mean unreadable; the main checkout is excluded. `branches` includes merged count and unmerged names, heads and last dates.
- `release`: newest tag reachable from HEAD, its date and commits since; null without tags. `night`: work since 18:00 local time yesterday, including all local branches, reverts and touched uncommitted files.
- `collisions`: overlapping changes between main checkout, worktrees and unmerged branches, with file count and up to five example files.
- `space`: rebuildable folders measured once per day, byte totals and per-folder sizes. Watchtower measures but never deletes them.
- `ci`: newest run of each default-branch workflow; the worst result decides `state` (`green`, `red`, `running`, `none`, `unknown`, `skipped`). With `--no-ci`, a prior CI result is retained; without one, the state is `skipped`. Cancelled and timed-out runs count as red. Workflow details include conclusion, timestamp and URL.
- `docs_mtime`, `last_work` (local date of the latest commit or uncommitted edit) and collection timestamp. Facts only cover work git can see; ignored private data is outside these measurements.

## Programmatic output

When code parses a result, add global `--json`, before or after the command. Ordinary agent reading uses text. JSON output is complete and unpaged; `--json` controls output, not document input. `--format json` is equivalent. `--help` and `--version` return text.

Success exits 0 with one object containing `ok: true` and the operation's data. Failure, partial completion or an unfinished job exits 1; invalid usage exits 2. Errors include `error.code`, `error.message` and recovery information such as `hint`. Results and expected errors go to stdout; stderr is reserved for exceptional transport failures. Inspect the outcome and partial-effect fields, not the exit code alone.

For a saved card or brief, code can read `saved`, `version`, `warnings` and `items`. Card writes also return `changed_fields` and `unfilled_fields`. `schema card --json` and `schema portfolio --json` return the complete document schema. A successful context read returns its opaque `context_token`; changing data makes an old token fail rather than silently overwrite later work.
