# Watchtower

A local morning overview of all your software projects: where each one stands, what is at risk, what waits for your decision, and what to do next.

Watchtower holds two kinds of data:

- **Facts** it measures itself from git: activity, backup (remote), uncommitted and unpushed work, leftover worktrees and branches, GitHub CI state. Exact and free.
- **Interpretation** written by coding agents through the CLI: one card per project and one cross-project brief with focus items, patterns, synergies and ideas. `watchtower due` tells an agent which projects actually changed, so a daily run only re-analyses those.

You rate every suggestion in the window (gut / später / Quatsch), optionally with a short reason, answer or dismiss the questions a card asks you, and tick off focus items. Agents read these reactions with the rated item itself, never bring back a rejected idea, learn your taste across projects and carry your answers into the next card.

The morning view distinguishes fresh git measurements from a completed agent run. It shows unfinished projects, failed updates and replies still waiting to be incorporated. A changes digest lists new and resolved open ends since you last marked it read.

Copy a task and separately mark it assigned when you hand it to an agent. Watchtower keeps its completion criterion and reported result even when the original suggestion changes; you confirm the result or reopen the task. Deliberate choices such as local-only work or a needed worktree can be acknowledged with a reason. The measured fact remains visible, and the warning returns if the situation worsens. “Später” offers tomorrow, next week or a chosen date.

## One folder, two entry points

```
%LOCALAPPDATA%\Programs\Watchtower\
  Watchtower.exe        the console CLI for agents
  Watchtower-UI.exe     the window for the owner
  data\                 database and window cache
  skill\watchtower\     the Skill for agents, written by Watchtower on start
```

- Double-click `Watchtower-UI.exe` (or the Start menu entry "Watchtower") opens the window. On first start you pick your projects folder and tick the repositories to watch.
- The gear opens the settings: add or remove projects, "Beim Anmelden öffnen" (autostart), "Für Agents aufrufbar" (puts the folder on your user PATH so agents can run `watchtower`), and the command that installs the Skill in vBot.
- `watchtower <command>` is a native console program: it completes its work and JSON output before returning an exit code. It runs independently of the window, with ordinary stdin, stdout and stderr; no GUI automation or polling for late output is involved. No arguments show CLI usage. The window picks up agent writes within a few seconds.
- `WATCHTOWER_HOME` moves the data folder elsewhere.

Both entry points call the same service and share one SQLite database. The desktop adapter and its Tauri dependencies are optional; the CLI can be built with `cargo build --no-default-features --bin Watchtower`.

Requires git on the PATH (`gh` optional, for CI state) and the WebView2 runtime that ships with Windows 11.

## Install

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

Builds both release executables (needs Rust), copies them to `%LOCALAPPDATA%\Programs\Watchtower` and creates the Start menu entry for the window. Reinstalling keeps `data\` and the settings. An existing Watchtower autostart entry is redirected to the window executable without changing whether autostart is enabled. Installation refuses to replace a CLI executable while an agent is using it.

## Agents

The Skill (`skill/watchtower/`, embedded in the exe) teaches the tracked daily run (`runs start` → study due repositories read-only → `context` → `card put` → `portfolio context` → `portfolio put` → `runs finish`), task reports, project intent and questions from stored data. Card and portfolio writes require the `--context-token` returned by their context command. If another agent or the owner changes that context, the write is rejected until the agent rereads and reconciles it.

In vBot, copy the install command from Watchtower's settings:

```bash
vbot skill install "%LOCALAPPDATA%\Programs\Watchtower\skill\watchtower" --scope global
```

Then schedule a daily task such as "Führe den täglichen Watchtower-Lauf durch." for an Agent that can use the Skill and has Bash access.

`watchtower schema card` and `watchtower schema portfolio` print the document schemas; `skill/watchtower/references/cli.md` lists all commands.

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
