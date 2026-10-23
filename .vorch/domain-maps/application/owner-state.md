# Owner State

## Identity and instructions

`Watchtower::update_project` validates identity edits; Store preserves the ID and rejects stale identity or duplicate paths. A folder change removes current facts so the old folder's measurements cannot describe the new one. A collector already in progress must also pass Store's identity check before saving. Removed projects and ideas stay restorable with their earlier documents, reactions and history. Tests: `tests/service.rs::project_updates_keep_identity_history_and_owner_state`, `::project_update_validation_is_atomic_and_old_databases_stay_current`, `tests/effects_recovery.rs`.

Owner status, notes, goals and replies become part of the evidence agents consume. A new card stores its owner basis so an answered question is distinguishable from an answer actually incorporated into interpretation. Goal and status changes must invalidate relevant context and freshness even when timestamps are equal. `owner_basis` and `owner_changes` in `src/service.rs` own these comparisons.

Feedback carries the item snapshot it referred to. Ratings do not assign work. An omitted note or snooze date must inherit the value current inside the write transaction, not a value read earlier by the service. Deferred items use a local calendar return date; note-only edits do not restart the snooze. Undo may restore an already expired date exactly. Tests: `tests/owner_controls.rs`, `tests/owner_undo.rs::concurrent_feedback_writers_preserve_omitted_notes_and_inherited_snooze_dates`.

## Warnings and assignments

`fact_warnings`, `acknowledged_warnings` and `warning_ack_applies` in `src/service.rs` derive warnings from facts and apply an owner's reason against the acknowledged basis. Acknowledging a warning changes its treatment, not its measured facts. A materially worsened condition invalidates the acknowledgement. `tests/owner_controls.rs` covers both persistence of raw facts and return of warnings.

Tasks keep their own source snapshot and history through later card rewrites. The service validates assignment and transitions; Store makes assignment uniqueness and revisions atomic. An agent report of `done` becomes `review`, with result and evidence; only owner confirmation completes it. Repeating the same report does not undo a later matching owner confirmation. Owner actions must use the displayed revision so a newer report cannot be confirmed unseen. `tests/tasks.rs` covers source snapshots, duplicate active work, stale reports, standalone-task freshness and guarded confirmation.

## Reading and visits

Visits establish re-entry context; they never acknowledge unread changes. `reading_content`, `reading_state` and `mark_changes_read` in `src/service.rs` own semantic report comparisons. Generated IDs, age counters, retired effort estimates and list reordering do not create unread content. Routine fact changes stay outside the digest. A disappearing open end means it is no longer listed, not that the work was completed.

The first visit also requires an explicit read action to establish a baseline. Project and event markers share that baseline. State captures the acknowledgement token before loading displayed cards; Store checks it again while saving the baseline. A newer write produces `stale_changes` and stays unread. Historical reads and facts refresh must not move this baseline. `tests/digest.rs` owns these contracts; transaction details are in `persistence/transactions-and-history.md`.

Durable undo preserves later edits through field-scoped receipts. Application callers translate `OwnerUndo` outcomes, not reconstruct an earlier whole project or preference object. Read `persistence/transactions-and-history.md` before changing recovery behavior.
