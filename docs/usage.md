# vWatch user guide

vWatch shows all your software projects in one window: where each one stands, what is at risk, what waits for your decision, and what to do next. The window's text is German; this guide quotes its labels as they appear.

- [What vWatch knows](#what-vwatch-knows)
- [The window](#the-window)
- [Ratings, questions and focus](#ratings-questions-and-focus)
- [Ideas and recommendations](#ideas-and-recommendations)
- [Handing work to an agent](#handing-work-to-an-agent)
- [Reading what changed](#reading-what-changed)
- [Statistics and history](#statistics-and-history)
- [Installation folder and settings](#installation-folder-and-settings)
- [Setting up your agent](#setting-up-your-agent)
- [The CLI](#the-cli)

## What vWatch knows

vWatch holds two kinds of data:

- **Facts** it measures itself from Git: activity, configured remotes, uncommitted and unpushed work, additional worktrees and branches, and GitHub CI state through `gh`. A remote is not proof of a backup; missing measurements remain unknown.
- **Interpretation** written by coding agents through the CLI: one Card per project and one cross-project portfolio brief with focus items, patterns, synergies and ideas. `vwatch due` tells an agent which projects actually changed, so a daily run only re-analyses those.

The window itself never calls a model and never writes to your repositories.

## The window

Four always-reachable areas organize the window:

- **Heute** brings together decisions, blocked work, results awaiting your confirmation, next steps and unread reports. Routine Git work does not determine the attention order. Unread content and an outdated agent report are different signals.
- **Projekte** shows project voices and tasks, and opens each project as a full page with its handbook, history and facts.
- **Ideen & Empfehlungen** separates product ideas from next steps and upkeep, with ratings for each.
- **Statistik** compares saved measurements and content changes.

A compact report-status control beside fact freshness shows the last check's completion time, which project reports changed and which were kept, plus any unfinished work. Failures and pending replies remain visible in its label.

Overview sections collapse in place: their headings and an expand button remain visible. "Was passiert ist" shows recent events first; events on the same day prioritize the most recently updated project card. The list-size control at its top switches between the latest ten and all entries.

### Project pages

A project page begins with purpose, current state, next step and its project voice. Below follow "Weiterarbeiten", "Starten & Handbuch" (each command on its own row with a copy button) and "Verlauf & Fakten" as one page; a jump bar reaches each part. The project list beside it marks open decisions, failed CI and unread reports; ↑/↓ moves through it and Esc returns to the overview. Status and goals have a compact summary with an edit control. Search opens the matching section and highlights the result. Commit curves and any retained agent progress estimate stay in project details, outside the morning overview.

### Warnings

Deliberate choices such as local-only work or a needed worktree can be acknowledged with a reason. The measured fact stays visible, and the warning returns if the situation worsens. "Später" offers tomorrow, next week or a chosen date.

## Ratings, questions and focus

You rate every suggestion in the window (gut / später / Quatsch), optionally with a short reason, answer or dismiss the questions a Card asks you, and tick off focus items. Agents read these reactions with the rated item itself, never bring back a rejected idea, learn your taste across projects and carry your answers into the next Card.

Tasks and ideas show the action, reason and completion criterion without agent estimates of duration or effort. Owner-set due dates and measured activity remain available.

## Ideas and recommendations

The collection **Ideen & Empfehlungen** opens with product ideas: new capabilities, simpler user workflows, new apps and your own ideas. **Empfehlungen** separately holds next steps, upkeep, repairs and safeguarding. Agents classify project suggestions and cross-project proposals explicitly and evaluate both directions independently, without a quota to invent content. Project cards allow up to four proposals per category so maintenance does not occupy the space for ideas.

Both categories have their own counts and rating rounds: 1 gut, 2 später, 3 Quatsch, with an optional reason. Filters separate "Neu", "Gut gefunden", "Später", "In Umsetzung", "Umgesetzt", "Archiviert", "Abgelehnt" and "Alle" within the selected category. A positive rating never creates an assignment. Reclassification preserves stable IDs, first appearance, feedback and tasks. Every proposal remains findable after a later report omits it. Deferred proposals return to "Neu" when their date arrives.

Ideas sort newest first by the first saved appearance of their stable ID, including on project pages. Rewriting or reintroducing an older idea does not make it new. Each project review includes reflection on useful improvements, followed by a synthesis across all projects, even when reports are current. Agents save worthwhile proposals, respect rejected or deferred ideas, and report what was newly added or why a considered direction did not qualify.

Your own ideas can be edited in place with guarded undo and preserved history; an edit requires a new agent assessment. Removing an idea moves it to restorable trash.

Ideas also offer direct "In Umsetzung", "Umgesetzt" and "Archivieren" actions, including in the rating round. Use "In Umsetzung" after manually pasting an idea to an agent; "Umgesetzt" records your confirmation without requiring an agent report. "Wieder offen" reverses the implementation status. "Archivieren" removes a proposal from open suggestions without changing its rating or cancelling running tasks; the "Archiviert" filter offers "Zurückholen". These decisions survive report rewrites and appear in agent contexts.

## Handing work to an agent

"Auftrag vorbereiten" opens a preview with the task, reason and completion criterion. "Nur kopieren" copies without assigning; "Beauftragen und kopieren" first saves the assignment, then copies its task number and reporting instructions. You paste that text into your agent. If copying fails after saving, the assignment remains available for another copy.

vWatch retains the task and its source even when a suggestion leaves the current report. The agent reports its result with `vwatch tasks report`; reporting `done` requests your review rather than confirming completion. You confirm a reported result or reopen the task.

## Reading what changed

"Seit deinem letzten Lesen" lists changes in project reports: progress, open ends, questions, documented decisions and incorporated replies. Routine Git changes stay in project facts and never generate digest entries. An open end disappearing means the Card no longer lists it, not proof of completion.

Project and activity markers share the explicit read snapshot. Even the first visit does not create a read baseline: use "Aktuellen Stand als gelesen markieren" after reading. Refreshing facts or reopening the window never acknowledges reports.

## Statistics and history

Statistics retain facts refreshes in SQLite, including failed measurements: every change and the first observation of each UTC day. They show source lines, language shares, files, commits, changed lines, frequently changed files, local work and confirmed task completions over a chosen period. Source counts describe the working tree, including uncommitted files, with documentation/configuration separate and common generated/dependency files excluded heuristically. Unknown or incomplete measurements never become zeroes; folder, branch or counting-method changes break size comparisons. The Git activity reconstruction has its own coverage and remains distinct from observed state.

Cards remain editable JSON documents through the guarded CLI. Each saved version and content change is recorded in the same database transaction, so the live UI and history agree. "Änderungsverlauf" shows previous and new values for reports, project intent, ideas, proposals, tasks, feedback and warning decisions; any two Card or brief versions can be compared. Unchanged large statistics payloads share storage, and a refresh that repeats the previous observation on the same UTC day is not saved again. Reading history never acknowledges unread reports.

## Installation folder and settings

```text
%LOCALAPPDATA%\Programs\vWatch\
  vWatch.exe        the console CLI for people and agents
  vWatch-UI.exe     the window
  data\             database and window cache
  skill\vwatch\     the Skill for agents, written by vWatch on start
```

- `vWatch-UI.exe` (or the Start menu entry "vWatch") opens the window. On first start you pick your projects folder and tick the repositories to watch.
- The gear opens the settings: add or remove projects, edit a project's name or folder with "Bearbeiten", "Beim Anmelden öffnen" (autostart), "Für Agents aufrufbar" (puts the folder on your user PATH so agents can run `vwatch`), and the command that installs the Skill in vBot. Editing preserves the project ID, Cards, ratings and history. Select the existing folder after moving or renaming it elsewhere.
- Reinstalling keeps `data\` and the settings. Installation refuses to replace the CLI while an agent is using it.

Both executables call the same service and share one SQLite database. vWatch needs Git on the PATH, `gh` optionally for CI state, and the WebView2 runtime that ships with Windows 11.

## Setting up your agent

The [operating Skill](../skill/vwatch/SKILL.md), embedded in the executables, teaches an agent the tracked daily run: `runs start` → `context` and repository study → `card put` → `portfolio context` → `portfolio put` → `runs finish`. Agents read ordinary text output and follow continuations. Card and brief writes require the `--context-token` from their complete context. A conflicting write is rejected until the agent rereads and reconciles the changed information.

Agents can also manage watched projects, ideas, ratings, replies, tasks, warnings and display preferences through the CLI when you request those changes. A daily run only observes repositories and updates vWatch's interpretation; it does not implement suggestions or invent your decisions. Autostart and PATH remain your choices in the window's settings.

To give an agent the Skill, point it at `%LOCALAPPDATA%\Programs\vWatch\skill\vwatch\SKILL.md`, or copy that folder into the agent's own skills folder, for Claude Code `%USERPROFILE%\.claude\skills\vwatch`. In vBot, give the install command from vWatch's settings to the Agent that should operate vWatch. It installs the Skill privately for that Agent:

```powershell
vbot skill install "$env:LOCALAPPDATA/Programs/vWatch/skill/vwatch" --scope own
```

`own` requires the calling vBot Agent's identity. From an external terminal, select that Agent explicitly with `--scope agent:<id>`.

Then schedule a daily task for the agent, for example "Führe den täglichen vWatch-Lauf durch." with shell access.

## The CLI

- `vwatch <command>` shows readable text for people and agents. Add `--json` when a program needs structured output, for example `vwatch projects list --json`. Document input uses `--file <path>`, `--file -` for stdin, or `--input-json '<object>'`.
- The CLI is a native console program independent of the window. A long operation returns within its 45-second wait budget with either its result or an exact `jobs wait` continuation. Output is complete when the console process exits.
- Long text results have an exact `Next:` command for the remaining pages. Errors name what failed, any partial effect and the recovery. `Undo:` identifies a supported reversal. `vwatch activity` helps recover after a lost response.
- `VWATCH_HOME` or `--data-dir <folder>` selects another data folder. Continuation commands retain the selected instance.
- `vwatch schema card` and `vwatch schema portfolio` describe the document fields; add `--json` for their complete JSON schemas. `vwatch --help` lists commands, and the [CLI reference](../skill/vwatch/references/cli.md) explains data, recovery and less common operations. The [agent interface specification](agent-interface.md) records capability coverage, guarantees and validation scope.
- To rename a watched project or reconnect its moved folder, use its stable ID with `vwatch projects update <id> --name "New name" --path "C:/projects/new-folder"`. Removed projects and ideas stay in their `trash` lists and can be restored with the same IDs.
- The CLI equivalent of the idea actions is `ideas status <item-id>` to read the current status and revision, then `ideas status <item-id> <status> --revision <revision>` for `in_progress`, `implemented`, `open`, `archived` or `restore`. Agents read either category with `vwatch proposals --category idea` or `--category recommendation`.
