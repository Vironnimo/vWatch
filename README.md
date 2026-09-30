# Lookout

A local morning overview of all your software projects: where each one stands, what is at risk, what waits for your decision, and what to do next.

Lookout holds two kinds of data:

- **Facts** it measures itself from git: activity, backup (remote), uncommitted and unpushed work, leftover worktrees and branches, GitHub CI state. Exact and free.
- **Interpretation** written by coding agents through the CLI: one card per project and one cross-project brief with focus items, patterns, synergies and ideas. `lookout due` tells an agent which projects actually changed, so a daily run only re-analyses those.

You rate every suggestion in the window (gut / später / Quatsch); agents read the ratings and never bring back a rejected idea.

## One folder, one program

```
%LOCALAPPDATA%\Programs\Lookout\
  Lookout.exe        the window (double-click) and the CLI (with arguments)
  data\              database and window cache
  skill\lookout\     the Skill for agents, written by Lookout on start
```

- Double-click `Lookout.exe` (or the Start menu entry "Lookout") opens the window. On first start you pick your projects folder and tick the repositories to watch.
- The gear opens the settings: add or remove projects, "Beim Anmelden öffnen" (autostart), "Für Agents aufrufbar" (puts the folder on your user PATH so agents can run `lookout`), and the command that installs the Skill in vBot.
- `Lookout.exe <command>` is the CLI: every command prints one JSON object. The window picks up whatever an agent writes within a few seconds.
- `LOOKOUT_HOME` moves the data folder elsewhere.

Requires git on the PATH (`gh` optional, for CI state) and the WebView2 runtime that ships with Windows 11.

## Install

```powershell
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

Builds the release exe (needs Rust), copies it to `%LOCALAPPDATA%\Programs\Lookout` and creates the Start menu entry. Reinstalling keeps `data\` and the settings.

## Agents

The Skill (`skill/lookout/`, embedded in the exe) teaches an agent to operate Lookout completely: the daily run (`due` → study repositories read-only → `card put` → `portfolio put`), changing a project's status, and answering questions from the stored data.

In vBot, copy the install command from Lookout's settings:

```bash
vbot skill install "%LOCALAPPDATA%\Programs\Lookout\skill\lookout" --scope global
```

Then schedule a daily task such as "Führe den täglichen Lookout-Lauf durch." for an Agent that can use the Skill and has Bash access.

`lookout schema card` and `lookout schema portfolio` print the document schemas; `skill/lookout/references/cli.md` lists all commands.

## Development

```bash
cargo test
cargo clippy --all-targets
cargo build --release
```

Run the window from source with `cargo run`; set `LOOKOUT_HOME` to a scratch folder to keep test data out of the installed app.
