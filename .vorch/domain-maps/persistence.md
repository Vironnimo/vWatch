# Persistence

The Store keeps Watchtower's shared SQLite state, durable evidence and concurrency guarantees consistent across console and desktop processes.

## Overview

`src/store.rs` owns database opening, the base schema, column migrations and shared storage types. Private `store/projects.rs`, `documents.rs`, `owner_state.rs`, `tasks.rs`, `runs.rs` and `reading.rs` keep each SQL responsibility behind Store methods; transaction bodies stay intact. `src/store/history.rs` owns observation/content history, retained proposals and their migrations. `src/store/undo.rs` owns field-scoped owner receipts and guarded undo. Application policy and read models remain in `application.md`; this domain enforces the atomic conditions they rely on.

`Store` serializes its connection with a mutex. That mutex does not coordinate other processes: conflicts must be checked in the transaction or SQL statement that saves the result. Use the existing guarded operations instead of composing a read-check-write sequence in an adapter.

## Data Model

Project IDs are durable identity. Names and paths can change while cards, feedback, tasks and history remain attached. Removal preserves rows for restoration, and removed identities and paths remain reserved. Current facts describe the registered folder; relinking discards those current facts, while historical observations retain their original folder.

Card versions count saves within one project; portfolio versions follow their stored row IDs. A version is not a wall-clock timestamp. Owner feedback and task snapshots can outlive the current report. Retained proposals are stored independently of whether the latest report lists them; omission is not deletion.

Facts read models apply the repository-facts owner's completeness adapter to uncertified line/space totals, including period and detailed historical reads. Raw observations and their stored fingerprints remain unchanged; see `repository-facts/measurements.md`. Risk queries do not certify line counts from historical Card bases that lack measurement provenance.

Content history and facts observations are append-only evidence. Normal current-state mutation, legacy normalization and restoration have different semantics; see `persistence/transactions-and-history.md` before changing their relationship. Store queries often return structured JSON, but schema validation and business transitions stay in the application service.

## Interfaces

`Store::open` configures WAL and a bounded busy timeout, then applies base schema, missing-column migrations and history migrations inside an immediate transaction. Existing databases must remain readable and migrations must be safe on repeated or concurrent opens. Extend the migration path rather than expecting a new database.

`GuardedWrite` distinguishes saved, already replayed, stale-context and item-identity conflicts. Return those distinctions to the service; a conflict must not become a fresh unguarded save. Owner operations likewise return receipts or guarded outcomes so callers can report the actual effect and available recovery.

`data_version` uses SQLite `PRAGMA data_version`. It changes for commits made by other connections, not writes on this Store's connection. The desktop refresh loop depends on this distinction; application `versioned_state` samples it before assembling state. Do not replace it with a local write counter or assume it globally identifies a database revision. Contract: `tests/service.rs::reading::the_data_version_moves_only_for_writes_of_other_connections`.

## Constraints & Gotchas

An operation that must be consistent with its receipt, history or acknowledgement needs one database transaction. Trigger failure must roll back the originating mutation too. A process losing its response does not justify blindly replaying a mutation; document receipts and field-scoped owner undo encode narrower, verifiable recovery.

`save_project_facts` rejects results whose registered path, identity revision or removal state changed while measurement ran. This guard belongs at persistence even though the repository collector runs outside it. Test helpers can use `save_facts` directly; production refresh must retain the guarded route.

Item ownership is rechecked against current documents, retained proposals and feedback inside guarded document writes. `cards::Kind` supplies the item kinds; this dependency protects the schema's global identity contract, not general document validation in SQL.

Tests for transaction races live in `tests/concurrency.rs`, field-scoped recovery in `tests/owner_undo.rs`, and durable evidence/migrations in `tests/statistics.rs`. Extend those behavior owners rather than testing private SQL construction.

## References

Read these only when the task matches, not by default.

- Changing context guards, replay, undo, read acknowledgement, task revisions, migration or history retention -> `persistence/transactions-and-history.md`
- Changing business transitions or deciding what makes interpretation current -> `application.md`
- Changing period statistics or document comparisons -> `application/statistics.md`
- Changing a storage failure's service wording or recovery instructions -> `agent-interface/agent-facing-text.md`
- Changing an externally visible recovery contract -> `docs/agent-interface.md`
