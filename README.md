# Watchtower

A local morning overview of all your software projects: where each one stands, what is at risk, what waits for your decision, and what to do next.

Watchtower holds two kinds of data:

- **Facts** it measures itself from git: activity, configured remotes, uncommitted and unpushed work, additional worktrees and branches, GitHub CI state. A remote is not proof of a backup; missing measurements remain unknown.
- **Interpretation** written by coding agents through the CLI: one card per project and one cross-project brief with focus items, patterns, synergies and ideas. `watchtower due` tells an agent which projects actually changed, so a daily run only re-analyses those.

You rate every suggestion in the window (gut / später / Quatsch), optionally with a short reason, answer or dismiss the questions a card asks you, and tick off focus items. Agents read these reactions with the rated item itself, never bring back a rejected idea, learn your taste across projects and carry your answers into the next card.

Four always-reachable areas organize the window: **Heute** brings together decisions, blocked work, results awaiting your confirmation, next steps and unread reports; **Projekte** shows project voices and tasks, and opens each project as a full page with its handbook, history and facts; **Ideen & Empfehlungen** separates product ideas from next steps and upkeep, with ratings for each; **Statistik** compares saved measurements and content changes. Routine Git work does not determine the attention order. Unread content and an outdated agent report are different signals.

Statistics retain facts refreshes in SQLite, including failed measurements: every change and the first observation of each UTC day. They show source lines, language shares, files, commits, changed lines, frequently changed files, local work and confirmed task completions over a chosen period. Source counts describe the working tree, including uncommitted files, with documentation/configuration separate and common generated/dependency files excluded heuristically. Unknown or incomplete measurements never become zeroes; folder, branch or counting-method changes break size comparisons. The Git activity reconstruction has its own coverage and remains distinct from observed state.

Cards remain editable JSON documents through the guarded CLI. Each saved version and content change is recorded in the same database transaction, so the live UI and history agree. "Änderungsverlauf" shows previous and new values for reports, project intent, ideas, proposals, tasks, feedback and warning decisions; any two card or portfolio versions can be compared. The migration retains existing document versions and the last available facts; earlier overwritten measurements cannot be recovered. Unchanged large statistics payloads share storage, and a refresh that repeats the previous observation on the same UTC day is not saved again. Reading history never acknowledges unread reports.

"Seit deinem letzten Lesen" lists changes in project reports: progress, open ends, questions, documented decisions and incorporated replies. Routine Git changes stay in project facts and never generate digest entries. An open end disappearing means the card no longer lists it, not proof of completion. Project and activity markers share the explicit read snapshot. Even the first visit does not create a read baseline: use "Aktuellen Stand als gelesen markieren" after reading. Refreshing facts or reopening the window never acknowledges reports.

A compact report-status control beside fact freshness shows the last check's completion time, which project reports changed and which were kept, plus any unfinished work. Failures and pending replies remain visible in its label. The overview summary uses ordinary reading size so project content comes first.

Overview sections collapse in place: their headings and an expand button remain visible. Previously hidden sections return as collapsed headings. "Was passiert ist" shows recent events first; events on the same day prioritize the most recently updated project card. The list-size control at its top switches between the latest ten and all entries.

"Auftrag vorbereiten" opens a preview with the task, reason and completion criterion. "Nur kopieren" copies without assigning; "Beauftragen und kopieren" first saves the assignment, then copies its task number and reporting instructions. You paste that text into your agent. If copying fails after saving, the assignment remains available for another copy. Watchtower retains the task and its source even when a suggestion leaves the current report; you confirm a reported result or reopen the task.

A project page begins with purpose, current state, next step and its project voice. Below follow "Weiterarbeiten", "Starten & Handbuch" (each command on its own row with a copy button) and "Verlauf & Fakten" as one page; a jump bar reaches each part. The project list beside it marks open decisions, failed CI and unread reports; ↑/↓ moves through it and Esc returns to the overview. Status and goals have a compact summary with an edit control. Search opens the matching section and highlights the result. Commit curves and any retained agent progress estimate stay in project details, outside the morning overview.

Deliberate choices such as local-only work or a needed worktree can be acknowledged with a reason. The measured fact stays visible, and the warning returns if the situation worsens. “Später” offers tomorrow, next week or a chosen date.

Tasks and ideas show the action, reason and completion criterion without agent estimates of duration or effort. Existing estimates are hidden throughout the window, and new cards and briefs no longer request or store them. Owner-set due dates and measured activity remain available.

The collection **Ideen & Empfehlungen** opens with product ideas: new capabilities, simpler user workflows, new apps and your own ideas. **Empfehlungen** separately holds next steps, upkeep, repairs and safeguarding. Agents classify project suggestions and cross-project proposals explicitly and evaluate both directions independently, without a quota to invent content. Project cards allow up to four proposals per category so maintenance does not occupy the space for ideas. Older feature suggestions remain ideas; other unclassified proposals initially appear as recommendations until reviewed.

Both categories have their own counts and rating rounds: 1 gut, 2 später, 3 Quatsch, with an optional reason. Filters separate "Neu", "Gut gefunden", "Später", "Beauftragt", "Abgeschlossen", "Abgelehnt" and "Alle" within the selected category. A positive rating never creates an assignment. Reclassification preserves stable IDs, first appearance, feedback and tasks. Every proposal remains findable after a later report omits it. Your own ideas can be edited in place with guarded undo and preserved history; an edit requires a new agent assessment. Removing an idea moves it to restorable trash. Deferred proposals return to "Neu" when their date arrives. Agents can read either category with `watchtower proposals --category idea` or `--category recommendation`.

Ideas sort newest first by the first saved appearance of their stable ID, including on project pages. Rewriting or reintroducing an older idea does not make it new. Each project review includes reflection on useful improvements, followed by a synthesis across all projects, even when reports are current. Existing suggestions do not block new ones. Agents save worthwhile proposals, respect rejected or deferred ideas, and report what was newly added or why a considered direction did not qualify.

## One folder, two entry points

```
%LOCALAPPDATA%\Programs\Watchtower\
  Watchtower.exe        the console CLI for people and agents
  Watchtower-UI.exe     the window for the owner
  data\                 database and window cache
  skill\watchtower\     the Skill for agents, written by Watchtower on start
```

- Double-click `Watchtower-UI.exe` (or the Start menu entry "Watchtower") opens the window. On first start you pick your projects folder and tick the repositories to watch.
- The gear opens the settings: add or remove projects, edit a project's name or folder with "Bearbeiten", "Beim Anmelden öffnen" (autostart), "Für Agents aufrufbar" (puts the folder on your user PATH so agents can run `watchtower`), and the command that installs the Skill in vBot. Editing preserves the project ID, cards, ratings and history. Select the existing folder after moving or renaming it elsewhere.
- `watchtower <command>` shows readable text for people and agents. Add `--json` when a program needs structured output, for example `watchtower projects list --json`. Document input uses `--file <path>`, `--file -` for stdin, or `--input-json '<object>'`.
- The CLI is a native console program independent of the window. A long operation returns within its 45-second wait budget with either its result or an exact `jobs wait` continuation. A command that is still running says so; follow its continuation instead of starting it again. Output is complete when the console process exits; there is no late GUI output to poll for.
- Long text results have an exact `Next:` command for the remaining pages. Errors name what failed, any partial effect and the recovery. `Undo:` identifies a supported reversal. `watchtower activity` helps recover after a lost response.
- Task reports also accept `watchtower tasks report <id> --file <path>` for multiline results and evidence. The file contains `status`, `result` and `evidence`; reporting `done` requests your review rather than confirming completion.
- `WATCHTOWER_HOME` or `--data-dir <folder>` selects another data folder. Continuation commands retain the selected instance.

Both entry points call the same service and share one SQLite database. The desktop adapter and its Tauri dependencies are optional; the CLI can be built with `cargo build --no-default-features --bin Watchtower`.

Requires git on the PATH (`gh` optional, for CI state) and the WebView2 runtime that ships with Windows 11.

## Install

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

Builds both release executables (needs Rust), copies them to `%LOCALAPPDATA%\Programs\Watchtower` and creates the Start menu entry for the window. Reinstalling keeps `data\` and the settings. An existing Watchtower autostart entry is redirected to the window executable without changing whether autostart is enabled. Installation refuses to replace a CLI executable while an agent is using it.

## Agents

The [operating Skill](skill/watchtower/SKILL.md), embedded in the executables, teaches the tracked daily run: `runs start` → `context` and repository study → `card put` → `portfolio context` → `portfolio put` → `runs finish`. Agents read ordinary text output and follow continuations. Card and portfolio writes require the `--context-token` from their complete context. A conflicting write is rejected until the agent rereads and reconciles the changed information.

Agents can also manage watched projects, ideas, ratings, replies, tasks, warnings and display preferences through the CLI when you request those changes. A daily run only observes repositories and updates Watchtower's interpretation; it does not implement suggestions or invent your decisions. Task reports remain subject to your confirmation. Autostart and PATH remain your choices in the window's settings.

In vBot, give the install command from Watchtower's settings to the Agent that should operate Watchtower. It installs the Skill privately for that Agent:

```powershell
vbot skill install "$env:LOCALAPPDATA/Programs/Watchtower/skill/watchtower" --scope own
```

`own` requires the calling vBot Agent's identity. From an external terminal, select that Agent explicitly with `--scope agent:<id>`. When updating an existing installation, keep its existing scope; install globally only when the owner explicitly requests global availability.

Then schedule a daily task such as "Führe den täglichen Watchtower-Lauf durch." for that Agent with Bash access.

`watchtower schema card` and `watchtower schema portfolio` describe the document fields as readable text; add `--json` for their complete JSON schemas. `watchtower --help` lists commands, and the [CLI reference](skill/watchtower/references/cli.md) explains data, recovery and less common operations. The [agent interface specification](docs/agent-interface.md) records capability coverage, guarantees and validation scope.

To rename a watched project or reconnect its moved folder, use its stable ID with `watchtower projects update <id> --name "New name" --path "C:/projects/new-folder"`. Either option can be used alone. If you do not know the ID, find it with `watchtower projects list`. Cards, ratings and history remain attached. Removed projects and ideas stay in their `trash` lists and can be restored with the same IDs; removal is not a rename operation.

## Development

```bash
cargo test
cargo clippy --all-targets
node --test tests/ui*.cjs
cargo build --release
cargo test --no-default-features
cargo run --bin Watchtower -- due --no-refresh
```

Run the window from source with `cargo run`; set `WATCHTOWER_HOME` to a task-specific folder under `.tmp/` to keep test data out of the installed app.

All development scratch files belong in the repository-root `.tmp/`, whose contents are ignored by Git. Cargo configures `TEMP`, `TMP` and `TMPDIR` there for builds, tests and `cargo run`. Run Cargo from this checkout so it loads `.cargo/config.toml`. For other tools, set the same variables in the current PowerShell process from the repository root:

```powershell
$repoTemp = (New-Item -ItemType Directory -Force -Path .tmp).FullName
$env:TEMP = $repoTemp
$env:TMP = $repoTemp
$env:TMPDIR = $repoTemp
$env:GIT_CEILING_DIRECTORIES = $repoTemp
$env:WATCHTOWER_HOME = Join-Path $repoTemp 'dev-instance'
```

Use task-specific subfolders for temporary scripts, logs, screenshots, payloads and database copies. `GIT_CEILING_DIRECTORIES` (also configured by Cargo) prevents temporary folders without their own repository from discovering Watchtower's parent Git repository. Keep build output in the existing `target/` folder.

The UI behavior tests use Node's built-in test runner and need no browser packages.
