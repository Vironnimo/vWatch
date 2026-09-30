# AGENTS.md

Lookout is a local web app plus CLI that gives the owner a morning overview of their software projects. Read `README.md` first.

## Layout

- `lookout/facts.py` measures a repository with read-only git (and `gh run list`). Never add a writing git command.
- `lookout/cards.py` defines and validates the agent-written documents (card, portfolio brief) and assigns ids to rated items.
- `lookout/store.py` is the SQLite persistence; `lookout/service.py` owns all behavior (registration, due detection, context, merge writes, feedback, visits). The CLI (`cli.py`) and the web server (`server.py`) are thin adapters over `Lookout` in `service.py`.
- `lookout/web/` is the vanilla JS page; it reads `/api/state` and writes only owner choices (feedback, prefs, project status).
- `skill/lookout/` is the Skill agents use to operate Lookout. It is runtime Agent-facing text: self-contained, no references to this repository's internals. Keep `references/cli.md` in sync with `cli.py`.

## Rules

- The CLI's JSON output, the error codes and the card/portfolio schemas are contracts for agents. Change them deliberately and update the Skill in the same commit.
- UI text is German and addresses the owner as "du"; code, docs and the Skill are English.
- Tests guard contracts (service behavior, CLI JSON, web API, facts), not individual changes. Keep them fast: use the fixed clock and temporary repositories from `tests/conftest.py`. Run `python -m pytest -q`.
- Commit finished work to `main` with conventional commits (`feat(scope): what`, lowercase, ≤72 chars). The repository is local only; do not add a remote.
