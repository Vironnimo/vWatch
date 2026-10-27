# Transactions and History

## Context and document recovery

`context_token` in `src/store/documents.rs` hashes the values an agent's interpretation depends on, not their collection time. It includes complete current facts, current document identity/content, relevant owner state and the effective report language (English when unset). Language participates in both scopes, so a switch cannot admit a write based on the earlier language choice. Project contexts also include ratings across projects so the owner's taste is shared. An unrelated project's card alone does not invalidate a project context. Portfolio context includes the previous brief, active owner ideas and whether the owner has opened vWatch since the previous brief; repeat visits after that boundary do not keep invalidating it.

Context coverage is deliberately broader than the facts fingerprint or portfolio due basis. An informational fact change can invalidate a pending write without requiring a new card in the ordinary daily due check. When adding context-visible state, decide explicitly whether it belongs in the token and in freshness comparisons; they answer different questions.

`add_card_guarded` and `add_portfolio_guarded` use an immediate transaction to recheck context, check item ownership, insert the document and save its request receipt. A receipt matches scope, expected token and request identity. `replay_document` accepts it only while the current context still equals the state immediately after that save. Preserve this restriction: a lost response can be recovered without duplicating a version, but a later owner edit cannot be silently treated as accepted.

The same `item_conflicts` query serves service preflight/dry runs and the final transactional guard. It protects IDs even when their earlier proposal disappeared from a report or survives only in feedback. Concurrent writers cannot transfer reactions to another project, portfolio scope or item kind. Tests: `tests/concurrency.rs` covers scope, simultaneous writers and item claims; `tests/statistics.rs::proposals_survive_report_omission_and_card_replays_do_not_duplicate_history` covers replay/history.

## Owner operations and task revisions

`owner_change` in `src/store/undo.rs` returns a typed `OwnerReceipt` with an opaque token and records before/after snapshots and field versions in the same transaction as the owner edit. The service formats the compatible `undo` command and exposes `undo_token` for desktop consumers. A no-op rolls back timestamp changes and may return the existing applicable receipt. `undo_owner_change` checks both visible state and field versions, preserving unrelated later edits and detecting change-away/change-back cases. Repeated undo is harmless only while the restored state is still current. Path restoration rechecks ownership and clears facts from the replaced folder.

Feedback resolves omitted notes and return dates while holding the owner-change transaction. Moving that inheritance into a prior service read loses concurrent edits. A failure writing a receipt rolls back the edit; a failure during undo rolls back its restoration. Tests: `tests/owner_undo.rs`.

Task assignment uses the partial `tasks_active_source` unique index for active statuses, returning the existing active assignment on repetition. `replace_task` uses the stored revision in its update predicate so an agent report cannot overwrite a simultaneous owner decision. Business transitions and the meaning of review are in `application/owner-state.md`; transaction tests are in `tests/tasks.rs`.

Project Facts saves also compare the stored Facts revision captured before collection. If another refresh saves first, the service remeasures against its latest cached inputs, with bounded retries. Identity changes reject the measurement separately. Tests: `tests/concurrency.rs::refresh_retries_when_another_refresh_changes_facts` and `::refresh_reports_sustained_facts_contention_without_overwriting_it`.

## Explicit read acknowledgement

`changes_snapshot` reads current report inputs and the saved baseline consistently without creating a baseline. `acknowledge_changes` rechecks the viewed token and saves the supplied snapshot inside an immediate transaction. A write arriving after the window's snapshot must remain unread. This guard is independent of document context tokens and owner undo receipts. Tests: `tests/digest.rs::concurrent_writes_cannot_be_silently_included_in_read_acknowledgement` and `::report_and_event_markers_share_the_explicit_read_snapshot`.

## Durable observations and migrations

`src/store/history.rs::migrate` installs the history schema and trigger families with named migration markers. It backfills the document versions and last facts still available in an older database; it cannot recover overwritten earlier measurements. Add migration behavior to this path and keep repeated opens idempotent.

Facts triggers record a sample when content, payload, fingerprint or identity changes, or on the first observation of a new UTC day. Identical same-day refreshes add no sample. Code and activity objects share immutable payload rows rather than being duplicated in every observation. Failed measurements are evidence too. Do not replace unknown observations with fabricated zeroes.

Document and owner-state triggers journal the same write that changes current state. Proposal triggers retain earlier suggestions, synergies and new-app ideas when omitted later; first appearance stays tied to the stable ID. Triggers forbid updates/deletes of saved content history, samples and payloads. Changes to any current-state write path must preserve those trigger effects.

Legacy documents can receive missing IDs in place during `vWatch::give_items_ids`. `rewrite_card_if_current` and `rewrite_portfolio_if_current` compare both version and previous content. The document-update migration journals that normalization and repairs retained proposals on databases normalized before those triggers existed. It does not increment the document version or rewrite older evidence. Normal restore instead appends a version with its original basis.

`period_samples` returns a preceding observation plus period samples and overlapping Git reconstructions; service comparison rules live in `application/statistics.md`. Keep sample retention distinct from reconstructed Git coverage.

Contracts: `tests/statistics.rs` covers repeat suppression, UTC-day boundaries, relinking, migration idempotence, normalization repair and trigger-failure rollback; `tests/concurrency.rs::legacy_document_rewrites_cannot_overwrite_newer_versions_or_content` and `::simultaneous_first_opens_create_one_complete_database` cover startup races.
