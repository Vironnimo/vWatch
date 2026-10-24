# Project Context

## Project

vWatch is a local Windows morning overview for an owner running roughly eight software projects through coding agents. It helps the owner see what changed, decide what needs attention, resume an older project and hand a concrete task to an agent. The window is for reading and decisions; a native console CLI lets people and agents use the same application service and SQLite database.

The installed folder contains `vWatch.exe`, `vWatch-UI.exe`, `data/` and `skill/vwatch/`. The window does not call a model. External agents read repository evidence and write project Cards and a portfolio brief in the selected language through guarded CLI operations.

## Architecture

**Stack:** Rust 2021, pinned toolchain 1.97.1; Tauri 2.12.0 with a static vanilla JavaScript/HTML/CSS frontend and no frontend build step. `Cargo.lock` pins rusqlite 0.40.2 (bundled SQLite), Tokei 15.0.0, Clap 4.6.7, Serde 1.0.229, serde_json 1.0.151 and Chrono 0.4.45. Node 24.15.0 runs the UI tests; Python 3.14.4 runs the dependency-free development tools. Git is required; `gh` is optional for CI measurements. Windows 11 supplies WebView2.

```text
src/main.rs -> cli_jobs / cli -> vWatch service
ui/ -> Tauri invoke -> src/app.rs -> vWatch service
src/service.rs + service/ -> cards, facts, store
src/cards.rs -> document definitions, validation, defaults, schema export
src/facts.rs + facts/ -> read-only Git / optional gh / bounded file scans
src/store.rs + store/ -> SQLite, migrations, guarded transactions, history
src/system.rs -> installation paths, embedded Skill, Windows integration
scripts/ + .githooks/ -> developer checks and checkout lifecycle
```

`VWatch` in `service.rs` owns application behavior; CLI and Tauri adapt it rather than implementing competing behavior. The optional `desktop` feature contains Tauri dependencies and the GUI binary; the console/core build works without it. The default `custom-protocol` feature serves embedded `ui/` assets, with no development HTTP server or port allocation.

Use 1,000 physical lines as a soft source-file review threshold, not an automatic refactor requirement. Split large owners into cohesive private units behind the same public interface when the task benefits; do not replace a large file with arbitrary chunks, pass-through layers or compressed formatting. The large owners use private units behind their existing interfaces; the maps locate those responsibilities.

**Observation boundary:** facts are measured, interpretation is authored. Repository measurements are read-only; unknown or incomplete values stay distinguishable from zero. Fingerprints decide whether interpretation needs renewal, not whether every informational measurement changed (repository-facts map).

**Owner authority:** feedback, project intent, goals and assignments are owner decisions. Daily Runs observe repositories and update interpretation; they cannot invent those decisions. Agent Task reports await owner confirmation. Explicit read acknowledgement, visits and data refresh are separate actions (application map).

**Consistency:** persisted identity survives renaming and relinking. Context Tokens and revision guards reject stale writes; Undo preserves later work. SQLite transactions and history triggers keep current state and its journal consistent (persistence map).

**Privacy:** local state stays on this machine; the only application network operation is optional `gh` CI measurement. Documents and history must not contain credentials. Usage metadata excludes argument values and document content; saved recovery files can contain private data (agent-interface map).

## Domain Maps

How to read the maps: `AGENTS.md` -> Load context for the task.

| Map | Domain | Covers |
|---|---|---|
| application.md | `src/service.rs`, `src/service/`, `src/cards.rs` | Shared behavior, report schemas, owner decisions, runs and read models |
| persistence.md | `src/store.rs`, `src/store/` | SQLite migrations, identity, atomic guards, history and Undo |
| repository-facts.md | `src/facts.rs`, `src/facts/` | Read-only repository observations, CI, bounded statistics and fingerprints |
| agent-interface.md | `src/main.rs`, `src/cli*.rs`, `src/cli/`, `src/cli_output/`, `skill/vwatch/`, `docs/agent-interface.md` | Console protocol, continuation, recovery, runtime instructions and capability parity |
| desktop.md | `ui/`, `src/app.rs`, `src/bin/vwatch-ui.rs`, `src/system.rs` | Owner window, Tauri lifecycle and Windows integration |
| development.md | `scripts/`, `.githooks/`, build configuration, test infrastructure | Pinned tools, static commit checks, worktrees, local verification and installation |

## Conventions

**Contracts and errors:** `VWatchError` is the service error boundary; the CLI owns presentation, codes and process exit status. Application CLI command names, output markers, JSON fields, due reasons and schemas are compatibility contracts; see agent-interface. Most service read models use `serde_json::Value`, so behavior and schema tests are essential at that boundary.

**Time:** service tests inject a fixed UTC clock and a fact collector through `VWatch::with_parts`. Facts distinguish local calendar activity from UTC observation history. Preserve the appropriate clock instead of replacing absent evidence with the wall clock.

**Text and paths:** the window and newly authored report content share a saved English/German language choice, defaulting to English. CLI-generated text, code and developer/operating documentation are English. Card/window paths use forward slashes or environment placeholders as documented by the operating Skill. `.gitattributes` governs line endings: LF text and hooks, CRLF PowerShell/batch files.

## Development

**Setup:** install the pinned Python and Node versions, Git and Rust via rustup; the Windows desktop build also needs MSVC build tools and WebView2. Cargo resolves the application through its committed lock file. There is no Python package environment, npm install, generated frontend or local server.

```powershell
rustup toolchain install 1.97.1 --profile minimal --component rustfmt --component clippy
git config --local core.hooksPath .githooks
python scripts/check.py tools
```

**Source runs:** from the intended checkout, use the primary checkout's existing build directory and an isolated instance. This applies to worktrees too; never link directories between checkouts.

```powershell
$repoScratch = (New-Item -ItemType Directory -Force .tmp).FullName
$env:TEMP = $repoScratch
$env:TMP = $repoScratch
$env:TMPDIR = $repoScratch
$env:GIT_CEILING_DIRECTORIES = $repoScratch
$env:VWATCH_HOME = Join-Path $repoScratch 'dev-instance'
$primaryRepo = Split-Path (git rev-parse --path-format=absolute --git-common-dir).Trim() -Parent
cargo run --target-dir (Join-Path $primaryRepo 'target')
```

For the console use `cargo run --target-dir (Join-Path $primaryRepo 'target') --bin vWatch -- due --no-refresh`. `--data-dir` overrides `VWATCH_HOME`; continuations preserve the selected instance. Default application data belongs to the installed app, so keep the scratch override for development.

**Worktrees:** before creating, merging or deleting a task checkout, read `.vorch/workflows/worktree-workflow.md`. Use `python scripts/worktree.py create|list|merge|delete <task-name>`; task branches use `codex/`. Small changes can stay on `main`. Codex-managed checkouts use the app's attachment/archive lifecycle; do not delete one with a different worktree manager.

**Complete checks and push:** there is no CI. `python scripts/check.py all` checks the current working tree; before checking or pushing an immutable commit read `.vorch/workflows/push-workflow.md`.

**Installation and releases:** before installing a finished change or publishing a release, read `.vorch/workflows/release-workflow.md`. `scripts/install.ps1` builds and installs both executables; it preserves data and existing OS choices and refuses replacement while an agent is using the installed CLI.

## Testing

Rust uses libtest integration suites in `tests/*.rs` and unit tests beside their owners. Node's built-in runner executes `tests/ui*.cjs` against VM/DOM fixtures; it does not validate browser layout. Python's standard-library tests cover development tools where they protect staged work or checkout data. Read `.vorch/domain-maps/development/testing.md` before changing tests.

```powershell
python scripts/check.py static
python scripts/test.py
python scripts/test.py --no-default-features --suite rust
python scripts/test.py --suite ui
```

The test runner supplies isolated homes and repositories outside checkouts/the user profile, bounded cases and duration reports; logs remain under `.tmp/`. Normal runs omit opt-in stress cases; existing ignored cases remain visible. SQLite and saved Job results retain real synchronization; no production durability switch is introduced for speed. Direct Cargo tests are useful for targeted contracts, using `--target-dir` as above and the scratch environment. Node VM checks need no browser packages.

**Commit hook:** `pre-commit` and `pre-merge-commit` enforce authorship and run static checks on the staged snapshot. They never run suites or rewrite/restage files. The report begins with `Commit check`; unstaged working-tree content cannot affect the checked snapshot; every compiler error in that snapshot blocks. Details and failure recovery belong to development.

**Live testing:** before checking the native window, Unicode console streams or an installed change, read `.vorch/workflows/live-testing-workflow.md`.

## Context

The owner's morning read should get shorter and decisions easier. Re-entry, answering agent questions, handing off work and learning from ratings are the other core situations; additional dashboard information is not a goal by itself.

The repository is public on GitHub as `Vironnimo/vWatch` under the MIT License since 2026-10-24, by the owner's decision; the README, `.github/` and community files are its public face. Pushes and releases happen only on the owner's request. The owner approved Vorch task worktrees on 2026-10-23; this replaces the earlier main-only development rule. The shared build directory and checkout-local build/log/live-instance scratch remain explicit vWatch adaptations; test fixture data follows Vorch's external isolation convention. Author identity for development is Julian B. <virojb@web.de>, confirmed by the owner on the same date.

Cross-domain recovery and historical-data preservation are owned by persistence; output recovery is owned by agent-interface. Window polling is transport freshness, not acknowledgement of new reports. Read the relevant map before changing any of these boundaries.
