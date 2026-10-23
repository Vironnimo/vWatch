# Application

Watchtower's shared service turns measured repository state, agent documents and owner decisions into the morning overview and the next agent context.

## Overview

`src/service.rs` owns application behavior through `Watchtower`; both the console and desktop adapters call it. Add behavior here or in its existing child modules, rather than creating separate rules in `src/cli.rs` and `src/app.rs`. `src/cards.rs` owns the card/portfolio schema, defaults and validation, not persistence or project lookup.

Private units under `src/service/` keep the same `Watchtower` owner: `projects.rs` coordinates identity and measurements, `documents.rs` owns validation/writes/recovery, `context.rs` assembles agent evidence, `freshness.rs` compares bases, `owner_state.rs` validates feedback/preferences, `ideas.rs` projects idea status, `tasks.rs` owns assignment transitions, `runs.rs` records observation cycles, and `reading.rs` assembles indexes, unread changes and window state. Existing `signals.rs`, `statistics.rs` and `undo.rs` keep their focused derivation, history-comparison and recovery roles. The root owns construction, injected dependencies, errors and shared value conventions. Keep repository measurement in `repository-facts.md`, SQL and atomic guards in `persistence.md`, transport/output in `agent-interface.md`, and presentation in `desktop.md`.

## Data Model

Facts, agent interpretation and owner decisions have different authority. A new card describes the facts and owner instructions it consumed; it cannot silently change those instructions or confirm an assigned task. The saved facts basis, owner basis and portfolio basis support different freshness decisions. Do not substitute a document timestamp or the latest Git commit for those comparisons.

Documents carry stable item IDs that attach feedback, assignments and earlier appearances. Item text can change without creating a new identity. IDs cannot move to a different document scope or item kind. The persistence guard is the final authority on concurrent claims; service validation gives earlier, useful errors.

## Interfaces

- The service returns `serde_json::Value` read models and `WatchtowerError` with a stable code, message and optional recovery hint. CLI envelopes/rendering belong to the agent interface; the service must describe the actual saved effect. Changing a command-visible result, owner capability or recovery behavior also requires the capability mapping in `docs/agent-interface.md`.
- `Watchtower::with_parts` injects the Store, clock and repository collector. Extend the existing contract tests using `tests/common/mod.rs`; use this seam for deterministic time and measurement behavior rather than starting the production app against owner data.
- `state(visit)` and `open_project` distinguish owner visits from ordinary reads. The window consumes service state; it does not infer saved decisions from rendered content. Unread content, stale interpretation and measured warnings remain separate signals.
- `versioned_state` reads the Store's change version before assembling state. The cross-connection notification contract is owned by `persistence.md` -> Interfaces; preserve that ordering so a concurrent write cannot become invisible to the window.

## Constraints & Gotchas

`due` is a check; tracked runs separately preserve unfinished work and the last successful completion. Missing or failed measurements are problems, not evidence that a new interpretation is current. The details of document evolution, due reasons and run completion belong in `application/documents-and-runs.md`.

Project renames and folder changes update the existing identity. Removal is visibility with restoration, not deletion of history. The service owns validation and the owner's intended change; persistence owns the final race checks. Never implement rename as remove/add.

Public convenience document writers obtain their own fresh token for in-process callers. Agent adapters must use the guarded writers with the context token actually read by the agent. Otherwise they would accept an interpretation based on superseded instructions.

## Agent-facing text

This domain owns service error/recovery wording, document validation warnings and schema descriptions in `src/service.rs`, `src/service/*.rs` and `src/cards.rs`. Before changing their runtime wording, read `agent-interface/agent-facing-text.md` and `docs/agent-interface.md`; keep the reasons at that canonical home with the text change.

## References

Read these only when the task matches, not by default.

- Changing schemas, document writes, context, due detection or tracked runs -> `application/documents-and-runs.md`
- Changing feedback, visits, unread changes, warnings, project identity or assignments -> `application/owner-state.md`
- Changing statistics, history queries, document comparisons or coverage claims -> `application/statistics.md`
- Changing a concurrent write, receipt, migration or durable history -> `persistence.md`
- Changing service text shown to an agent -> `agent-interface/agent-facing-text.md`
- Changing an owner capability, CLI result or recovery contract -> `docs/agent-interface.md`
