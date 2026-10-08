# Watchtower

A local morning overview of all your software projects: where each one stands, what is at risk, what waits for your decision, and what to do next.

Watchtower holds two kinds of data:

- **Facts** it measures itself from git: activity, backup (remote), uncommitted and unpushed work, leftover worktrees and branches, GitHub CI state. Exact and free.
- **Interpretation** written by coding agents through the CLI: one card per project and one cross-project brief with focus items, patterns, synergies and ideas. `watchtower due` tells an agent which projects actually changed, so a daily run only re-analyses those.

You rate every suggestion in the window (gut / später / Quatsch), optionally with a short reason, answer or dismiss the questions a card asks you, and tick off focus items. Agents read these reactions with the rated item itself, never bring back a rejected idea, learn your taste across projects and carry your answers into the next card.

The morning view distinguishes fresh git measurements from a completed agent run. It shows unfinished projects, failed updates and replies still waiting to be incorporated. "Seit deinem letzten Lesen" lists changes in project reports: progress, open ends, questions, documented decisions and incorporated replies. Routine Git changes (including uncommitted files, unpushed commits, worktrees, branches, remotes and CI) stay in the project facts and never generate digest entries. An open end disappearing means the card no longer lists it, not proof of completion. Changes remain unread until you explicitly mark them read.

Overview sections collapse in place: their headings and an expand button remain visible. Previously hidden sections return as collapsed headings. "Was passiert ist" shows recent events first; events on the same day prioritize the most recently updated project card. The list-size control at its top switches between the latest ten and all entries.

Copy a task and separately mark it assigned when you hand it to an agent. Watchtower keeps its completion criterion and reported result even when the original suggestion changes; you confirm the result or reopen the task. Deliberate choices such as local-only work or a needed worktree can be acknowledged with a reason. The measured fact remains visible, and the warning returns if the situation worsens. “Später” offers tomorrow, next week or a chosen date.

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
node --test tests/ui.cjs
cargo build --release
cargo test --no-default-features
cargo run --bin Watchtower -- due --no-refresh
```

Run the window from source with `cargo run`; set `WATCHTOWER_HOME` to a scratch folder to keep test data out of the installed app.

The UI behavior tests use Node's built-in test runner and need no browser packages.
