# Watchtower

A local morning overview of all your software projects: where each one stands, what is at risk, what waits for your decision, and what to do next.

Watchtower holds two kinds of data:

- **Facts** it measures itself from git: activity, configured remotes, uncommitted and unpushed work, additional worktrees and branches, GitHub CI state. A remote is not proof of a backup; missing measurements remain unknown.
- **Interpretation** written by coding agents through the CLI: one card per project and one cross-project brief with focus items, patterns, synergies and ideas. `watchtower due` tells an agent which projects actually changed, so a daily run only re-analyses those.

You rate every suggestion in the window (gut / später / Quatsch), optionally with a short reason, answer or dismiss the questions a card asks you, and tick off focus items. Agents read these reactions with the rated item itself, never bring back a rejected idea, learn your taste across projects and carry your answers into the next card.

Four always-reachable areas organize the window: **Heute** brings together decisions, blocked work, results awaiting your confirmation, next steps and unread reports; **Projekte** keeps project voices, current state, tasks and expandable history and facts; **Ideen** collects suggestions and your own ideas; **Handbuch** helps you start and use each project. Routine Git work does not determine the attention order. Unread content and an outdated agent report are different signals.

"Seit deinem letzten Lesen" lists changes in project reports: progress, open ends, questions, documented decisions and incorporated replies. Routine Git changes stay in project facts and never generate digest entries. An open end disappearing means the card no longer lists it, not proof of completion. Project and activity markers share the explicit read snapshot. Even the first visit does not create a read baseline: use "Aktuellen Stand als gelesen markieren" after reading. Refreshing facts or reopening the window never acknowledges reports.

A compact report-status control beside fact freshness shows the last check's completion time, which project reports changed and which were kept, plus any unfinished work. Failures and pending replies remain visible in its label. The overview summary uses ordinary reading size so project content comes first.

Overview sections collapse in place: their headings and an expand button remain visible. Previously hidden sections return as collapsed headings. "Was passiert ist" shows recent events first; events on the same day prioritize the most recently updated project card. The list-size control at its top switches between the latest ten and all entries.

"Auftrag vorbereiten" opens a preview with the task, reason and completion criterion. "Nur kopieren" copies without assigning; "Beauftragen und kopieren" first saves the assignment, then copies its task number and reporting instructions. You paste that text into your agent. If copying fails after saving, the assignment remains available for another copy. Watchtower retains the task and its source even when a suggestion leaves the current report; you confirm a reported result or reopen the task.

The project drawer begins with purpose, current state, next step and its project voice. "Weiterarbeiten", "Starten & Handbuch" and "Verlauf & Fakten" provide direct access to the rest. Status and goals have a compact summary with an edit control. Search opens the matching section and highlights the result. Commit curves and any retained agent progress estimate stay in project details, outside the morning overview.

Deliberate choices such as local-only work or a needed worktree can be acknowledged with a reason. The measured fact stays visible, and the warning returns if the situation worsens. “Später” offers tomorrow, next week or a chosen date.

Tasks and ideas show the action, reason and completion criterion without agent estimates of duration or effort. Existing estimates are hidden throughout the window, and new cards and briefs no longer request or store them. Owner-set due dates and measured activity remain available.

The ideas collection combines project suggestions, app ideas, cross-project possibilities and your own ideas. Filters separate "Neu", "Gut gefunden", "Später", "Beauftragt", "Abgeschlossen", "Abgelehnt" and "Alle". A positive rating never creates an assignment. Rated and assigned proposals remain findable after a later report omits them; removing your own idea still moves it to restorable trash. Deferred ideas return to "Neu" when their date arrives.

Ideas sort newest first by the first saved appearance of their stable ID, including in project drawers. Rewriting or reintroducing an older idea does not make it new. Each project review includes reflection on useful improvements, followed by a synthesis across all projects, even when reports are current. Existing suggestions do not block new ones. Agents save worthwhile proposals, respect rejected or deferred ideas, and report what was newly added or why a considered direction did not qualify.

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
node --test tests/ui.cjs tests/ui-drawer.cjs tests/ui-handoff.cjs tests/ui-ideas.cjs tests/ui-overview.cjs
cargo build --release
cargo test --no-default-features
cargo run --bin Watchtower -- due --no-refresh
```

Run the window from source with `cargo run`; set `WATCHTOWER_HOME` to a scratch folder to keep test data out of the installed app.

The UI behavior tests use Node's built-in test runner and need no browser packages.
