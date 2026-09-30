# Watchtower

A local morning overview of all your software projects: where each one stands, what is at risk, what waits for your decision, and what to do next.

Watchtower holds two kinds of data:

- **Facts** it measures itself from git: activity, backup (remote), uncommitted and unpushed work, leftover worktrees and branches, GitHub CI state. Exact and free.
- **Interpretation** written by coding agents through the CLI: one card per project and one cross-project brief with focus items, patterns, synergies and ideas. `watchtower due` tells an agent which projects actually changed, so a daily run only re-analyses those.

You rate every suggestion in the window (gut / später / Quatsch), optionally with a short reason, answer or dismiss the questions a card asks you, and tick off focus items. Agents read these reactions with the rated item itself, never bring back a rejected idea, learn your taste across projects and carry your answers into the next card.

## One folder, one program

```
%LOCALAPPDATA%\Programs\Watchtower\
  Watchtower.exe        the window (double-click) and the CLI (with arguments)
  data\                 database and window cache
  skill\watchtower\     the Skill for agents, written by Watchtower on start
```

- Double-click `Watchtower.exe` (or the Start menu entry "Watchtower") opens the window. On first start you pick your projects folder and tick the repositories to watch.
- The gear opens the settings: add or remove projects, "Beim Anmelden öffnen" (autostart), "Für Agents aufrufbar" (puts the folder on your user PATH so agents can run `watchtower`), and the command that installs the Skill in vBot.
- `Watchtower.exe <command>` is the CLI: every command prints one JSON object. The window picks up whatever an agent writes within a few seconds.
- `WATCHTOWER_HOME` moves the data folder elsewhere.

Requires git on the PATH (`gh` optional, for CI state) and the WebView2 runtime that ships with Windows 11.

## Install

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

Builds the release exe (needs Rust), copies it to `%LOCALAPPDATA%\Programs\Watchtower` and creates the Start menu entry. Reinstalling keeps `data\` and the settings.

## Agents

The Skill (`skill/watchtower/`, embedded in the exe) teaches an agent to operate Watchtower completely: the daily run (`due` → study repositories read-only → `card put` → `portfolio put`), changing a project's status, and answering questions from the stored data.

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
cargo build --release
```

Run the window from source with `cargo run`; set `WATCHTOWER_HOME` to a scratch folder to keep test data out of the installed app.
