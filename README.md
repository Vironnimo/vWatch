# Lookout

A local morning overview of all your software projects: where each one stands, what is at risk, what waits for your decision, and what to do next.

Lookout holds two kinds of data:

- **Facts** it measures itself from git: activity, backup (remote), uncommitted and unpushed work, leftover worktrees and branches, GitHub CI state. Exact and free.
- **Interpretation** written by coding agents through the CLI: one card per project and one cross-project brief with focus items, patterns, synergies and ideas. `lookout due` tells an agent which projects actually changed, so a daily run only re-analyses those.

You rate every suggestion in the web app (gut / später / Quatsch); agents read the ratings and never bring back a rejected idea.

## Setup

Requires Python 3.11+ and git on the PATH (`gh` optional, for CI state).

```bash
python -m pip install -e .
lookout projects discover C:\Development\projects
lookout projects add C:\Development\projects\some-app
lookout open
```

- `lookout open` starts the web app in the background and opens http://127.0.0.1:8455.
- `lookout autostart on` starts it at Windows sign-in.
- `lookout serve` runs it in the foreground.
- Data lives in `~/.lookout` (`LOOKOUT_HOME` overrides it; `LOOKOUT_PORT` changes the port).

## Agents

The Skill in `skill/lookout/` teaches an agent to operate Lookout completely: the daily run (`due` → study repositories read-only → `card put` → `portfolio put`), adding projects, changing a project's status, and answering questions from the stored data.

In vBot:

```bash
vbot skill install C:\Development\projects\lookout\skill\lookout --scope global
```

Then schedule a daily task such as "Führe den täglichen Lookout-Lauf durch." for an Agent that can use the Skill and has Bash access.

Every CLI command prints JSON (`{"ok": true, ...}` or `{"ok": false, "error": {...}}`). `lookout schema card` and `lookout schema portfolio` print the document schemas; `skill/lookout/references/cli.md` lists all commands.

## Development

```bash
python -m pip install -e ".[dev]"
python -m pytest -q
```
