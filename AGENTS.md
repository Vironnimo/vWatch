# AGENTS.md

Guidance for every coding agent working on Watchtower. Read it completely before changing anything, then read `README.md` for install and usage.

## What Watchtower is for

The owner builds about eight software projects at once, mostly through coding agents that work day and night. Without help they lose track: which project moved, what an agent left unfinished, which work exists only on this disk, what waits for their decision, and how to get back into a project they have not touched for weeks.

Watchtower answers that in a few minutes each morning. It is a local Windows app whose single exe is both the window the owner reads and a JSON CLI that agents write through. It serves these situations:

- **Morning overview:** what changed overnight, what is at risk, what to do first today.
- **Re-entry:** coming back to a project after days or weeks: what it is, how to start and use it, where things are, what happened since the owner last looked.
- **Decisions:** questions agents could not settle alone, collected in one place, answered with a click or a short reply.
- **Handoff:** turning a suggestion or open end into a ready-to-paste task for a coding agent.
- **Learning the owner's taste:** the owner rates suggestions and ideas; agents never bring back what was rejected and adapt to what was welcomed.

When you extend Watchtower, start from a situation like these and ask what the owner needs to see or decide in it. New features should make the morning read shorter or a decision easier, not add reading.

## How it works

Two kinds of data, kept strictly apart:

- **Facts** Watchtower measures itself, read-only, from git (and `gh` for CI): commits and the weekly activity curve, remote and unpushed commits, uncommitted work, extra worktrees, unmerged branches, CI state, releases, overnight work, lines of work that will collide, rebuildable disk space. Exact, free and instant. The window renders them directly.
- **Interpretation** written by coding agents through the CLI: one **card** per project (headline, phase, re-entry, open ends, questions for the owner, suggestions, handbook, …) and one cross-project **portfolio brief** (today's focus, patterns, synergies, new app ideas, park or revive, assessments of the owner's parked ideas, and once a week a review of the past week). Agents follow the Skill in `skill/watchtower/`.

The daily run, done by an agent with the Skill: `watchtower due` refreshes the facts and lists the projects whose card is due, with a reason (`no_card`, `changed`, `ci_changed`, `answered`, `status_changed`, `new_fields`, `stale`) → the agent studies each due repository read-only (`watchtower context <id>`, then the repository itself) → `watchtower card put` → `watchtower portfolio context` → `watchtower portfolio put`. `due` compares a fingerprint of the facts with the one the card was written against, so a daily run only re-analyses what actually changed.

The owner's side, written only by the window: which projects to watch, ratings and replies (`feedback`), status (`active`, `parked`, `done`, `archived`) with a note to agents, a goal with a due date, focus items ticked off, parked ideas (the next brief assesses each one in `idea_checks`), window preferences, and visits (used for "new since your last visit" and "since you last opened this project"). Owner changes such as a reply, status, note or goal make the affected card due, so agents react to them in the next run.

The window never calls a model and never writes to a repository. It picks up agent writes within seconds by watching the database's data version.

## Principles

- **Observe, never act on repositories.** Facts come from read-only git commands. Never add a git command that writes (commit, fetch, push, gc, checkout, …), and never add features that change, back up or clean the owner's repositories. Watchtower shows the state; the owner or their agents act.
- **Local and private.** Everything stays in one folder on this machine. No telemetry, no network except `gh` for CI state. Cards must not contain credentials; the Skill tells agents so.
- **Facts are exact, interpretation is honest.** The window shows measured numbers as measured. Agents cite evidence (file, function, fact field) and say when something is stale or uncertain.
- **Short reading, clear next step.** Word limits in the schemas keep cards readable. Every open end, question and suggestion should lead to a concrete action or decision.
- **The owner's word wins.** Ratings, replies, status notes and goals are instructions for agents. A rejected idea never returns.
- **Agents are the users of the CLI.** Its JSON output, error codes (with `hint`) and schemas are designed so a fresh agent with only the Skill can operate Watchtower without mistakes.

## Layout

- `src/main.rs` decides: no arguments → window (`app.rs`), arguments → CLI (`cli.rs`). The exe uses the Windows GUI subsystem; `system::attach_console` gives the CLI its stdout and stderr.
- `src/service.rs` (`Watchtower`) owns all behavior: registration, facts refresh, due detection, context, card and portfolio writes (with merge and dry run), feedback, visits, goals, prefs and the window state. `cli.rs` and the Tauri commands in `app.rs` are thin adapters over it; put new behavior here, not in the adapters.
- `src/facts.rs` measures one repository (`collect`) and computes the fingerprint (`fingerprint` over `basis()`; new informational facts fields stay out of the basis so they do not make cards due). Disk space is measured once a day by `space`, carried over in `service.rs`.
- `src/cards.rs` defines the agent-written documents (card, portfolio brief) as schema data: field types, word limits, defaults. It validates writes, merges, reports fields a card lacks (`missing_fields` → due reason `new_fields`), and `watchtower schema` prints the same definition.
- `src/store.rs` is the SQLite persistence (WAL). Schema changes go through the migration list; existing databases must keep working.
- `src/system.rs` owns the folder layout (exe folder, `data\`, `skill\watchtower\`), exports the embedded Skill, and the Windows integration (autostart Run key, user PATH).
- `ui/` is the vanilla JS page in the window (`index.html`, `app.js`, `app.css`, no build step). It calls `app.rs` through `window.__TAURI__.core.invoke`. Three styles share one right-hand drawer with each project's full record: "Übergabe" (handover, read top to bottom), "Leitstand" (card grid with a side column) and "Handbuch" (reference sheets). Also: search (Ctrl+K), the decision round (all open questions one after another), "Auftrag kopieren" (a ready-to-paste task for a coding agent), the idea parking lot, the week review, the overnight view and the space overview.
- `skill/watchtower/` is the Skill: `SKILL.md` (the workflow), `references/card-guide.md` (how to study a project and fill each card field), `references/portfolio-guide.md` (the brief), `references/cli.md` (every command and JSON field). It is embedded with `include_str!` and written next to the exe on start and by `watchtower status`.
- `tests/`: `service.rs` (behavior contracts through the service), `cli.rs` (CLI JSON), `facts.rs` (git measurements on temporary repositories); shared helpers with a fixed clock in `tests/common/mod.rs`.
- `scripts/install.ps1` builds the release exe and installs it to `%LOCALAPPDATA%\Programs\Watchtower`.

## Changing contracts

The CLI's JSON output, error codes, due and brief reasons, and the card and portfolio schemas are contracts for agents.

- Change them deliberately and update the Skill in the same commit: `references/cli.md` must match `cli.rs` and the JSON the service returns; `card-guide.md` and `portfolio-guide.md` must describe every field the schema has.
- Adding a card field: define it in `cards.rs` with a default, document it in the card guide, render it in the UI. Existing cards then become due with `new_fields`, so the next daily run fills the field everywhere.
- The portfolio brief is rewritten only when fields it builds on change (`BRIEF_FIELDS` in `service.rs`). Add a card field there only if the brief uses it.
- Skill text is runtime Agent-facing text: self-contained for a fresh agent that has never seen this repository, no references to its internals, only concepts the agent can observe or act on through the CLI. Keep it concise; every sentence costs every run.

## Conventions

- UI text is German and addresses the owner as "du". Code, comments, docs, the Skill and commit messages are English. Card and brief content is written by agents in German.
- Keep modules few and deep; extend the existing owner of a capability before adding a new one.
- Card and window paths use forward slashes or environment variables such as `%LOCALAPPDATA%` where the Skill asks for them.

## Build, run, test

```bash
cargo test                      # all tests, a few seconds
cargo clippy --all-targets      # must stay free of warnings
cargo run                       # the window from source
cargo run -- due --no-refresh   # the CLI from source
```

- Set `WATCHTOWER_HOME` to a scratch folder when running from source, so test data stays out of the installed app. Copy the installed `data\watchtower.db` there to try changes on real data.
- Install with `powershell -ExecutionPolicy Bypass -File scripts/install.ps1`. It closes the running window; start it again afterwards. Do not install while an agent is using the installed exe.
- Tests guard contracts (service behavior, CLI JSON, facts), not individual changes. Extend the test that owns a behavior before adding a new one. Keep them fast and deterministic with the fixed clock and temporary repositories.
- Verify UI changes in the running window. Do not change the owner's preferences (style, hidden sections) or leave ratings, replies or ticks behind while testing; `watchtower feedback` must show no test entries afterwards.
- Never toggle the owner's autostart or PATH setting; those are the owner's choices in the settings.

## Git

- Work on `main` and commit finished work with conventional commits (`feat(scope): what`, lowercase, at most 72 characters). One logical change per commit; never commit failing tests.
- The repository is local only. Do not add a remote.
