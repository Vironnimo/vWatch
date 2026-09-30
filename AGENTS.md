# AGENTS.md

Lookout is a Windows desktop app (Tauri 2, WebView2) whose single exe is also a JSON CLI for agents. It gives the owner a morning overview of their software projects. Read `README.md` first.

## Layout

- `src/main.rs` decides: no arguments → window (`app.rs`), arguments → CLI (`cli.rs`). The exe uses the Windows GUI subsystem; `system::attach_console` gives the CLI its stdout/stderr.
- `src/facts.rs` measures a repository with read-only git (and `gh run list`). Never add a writing git command.
- `src/cards.rs` defines and validates the agent-written documents (card, portfolio brief) as schema data; `lookout schema` prints the same definition.
- `src/store.rs` is the SQLite persistence; `src/service.rs` (`Lookout`) owns all behavior: registration, due detection, context, merge writes, feedback, visits, prefs. `cli.rs` and the Tauri commands in `app.rs` are thin adapters over it.
- `src/system.rs` owns the folder layout (exe dir, `data\`, `skill\`) and the Windows integration (autostart Run key, user PATH).
- `ui/` is the vanilla JS page inside the window. It talks to `app.rs` through `window.__TAURI__.core.invoke` and writes only owner choices (projects, feedback, prefs, status, settings).
- `skill/lookout/` is the Skill agents use. It is embedded with `include_str!` and exported next to the exe on start, so rebuild after editing it. It is runtime Agent-facing text: self-contained, no references to this repository's internals. Keep `references/cli.md` in sync with `cli.rs`.

## Rules

- The CLI's JSON output, the error codes and the card/portfolio schemas are contracts for agents. Change them deliberately and update the Skill in the same commit.
- UI text is German and addresses the owner as "du"; code, docs and the Skill are English.
- Tests guard contracts (service behavior, CLI JSON, facts), not individual changes. Keep them fast: use the fixed clock and temporary repositories from `tests/common/mod.rs`. Run `cargo test` and `cargo clippy --all-targets`.
- Commit finished work to `main` with conventional commits (`feat(scope): what`, lowercase, ≤72 chars). The repository is local only; do not add a remote.
