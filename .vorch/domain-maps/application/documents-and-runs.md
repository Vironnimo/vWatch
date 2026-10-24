# Documents and Runs

## Schema changes

`src/cards.rs` describes types, defaults, word limits and list limits once. `validate`, `Kind::missing_fields` and `json_schema` use that definition; readable schema output must stay aligned with it. Do not add an independent schema in an adapter.

For a new card field, define a default, document it in `skill/vwatch/references/card-guide.md`, and render it in the UI. Update `portfolio-guide.md` for portfolio fields. Existing documents missing schema fields become due with `new_fields`; the recursive check also examines fields missing from existing list items. Update the operating Skill in the same change when a document or command contract changes.

`BRIEF_FIELDS` in `src/service/freshness.rs` is the subset of card content the portfolio uses. Add a field only if the brief needs it. A handbook-only change must not force another cross-project analysis. Portfolio freshness also includes owner intent, idea revisions and tasks, including standalone tasks and tasks whose project was removed (`portfolio_basis`).

`Kind::current_view` hides retired effort estimates and derives legacy proposal categories without modifying stored evidence or explicitly requested history. New writes validate into the current schema. Changing the reading projection is not a migration of old documents. Contracts: `tests/cards.rs`, `tests/service.rs::documents::current_contexts_hide_legacy_estimates_without_rewriting_evidence_or_history`.

## Write semantics

`put_card_guarded` and `put_portfolio_guarded` own the complete write pipeline: check context, expand kept IDs, merge if requested, validate content and references, then delegate the atomic write to Store. The context check before validation is not sufficient against concurrent processes; preserve the second check inside Store.

Merge is shallow at the document's top level. Omitted fields remain; supplied lists and objects replace those fields in full. `cards::expand_kept_items` restores an item sent only as its ID from the latest known wording in that scope. Keep IDs when retaining an item so its feedback remains attached. Unsupplied inherited fields may retain old word-limit excess with a warning; submitted fields must satisfy current limits. Unknown fields are ignored with warnings, not persisted as extensions.

A schema-only dry run may omit a context token and never consumes one. If a token is supplied it still must be current. A real guarded write requires the original context token; recovery means rereading and reconciling the changed input, not replacing only the token. Document replay, collision checks and recovery receipts are owned by `persistence/transactions-and-history.md`.

Restoring a saved version appends its old content with its old evidence basis. It is not a new analysis and may remain due. Legacy ID assignment at service startup is the narrow exception that updates an existing latest document; persistence guards and journals it. Tests: `tests/service.rs::documents::merge_keeps_unsent_fields_and_rated_ids`, `::kept_items_need_only_their_id_and_receipts_name_each_change`, `tests/concurrency.rs::guarded_writes_require_context_but_dry_runs_do_not_consume_it`, `tests/effects_recovery.rs::history_restore_appends_old_content_and_keeps_its_stale_evidence`.

## Due detection and tracked runs

`due` compares a card's saved basis with current evidence. Its reasons are a public contract: no card, changed facts, settled CI change, incorporated owner input needed, stale age, scheduled idea review, or missing schema fields. Missing/unreadable facts go into problems rather than the normal due list. Owner replies remain pending even when an earlier-priority reason explains why the card is due.

CI only becomes interpretation evidence at settled `green`/`red` states. A previously unknown CI basis becomes due on red, rather than every transient state. The repository fingerprint is owned by `repository-facts.md`; informational measurements must not be added to its basis merely because they are available in context.

The portfolio is due on a new local day, an unchecked owner idea, missing fields or changed relevant evidence. Cards needing only new schema fields do not by themselves predict a changed brief. Quiet active projects take turns for idea review; a recorded run keeps its original review selection if the daily rotation advances.

`run_start` records work; `run_fail` retains failures; only explicit `run_finish` records success after all required scopes are current. A current card needs no rewrite merely to finish a run. `run_status` uses saved state without refreshing Git. An abandoned run can become stalled, but never automatically successful.

`RunCompletionStage` provides deterministic integration-test synchronization after Due evaluation and before the guarded save; normal construction installs no observer. Completion is assembled from several reads, so `run_finish` captures `agent_run_token`, builds the view, then asks Store to recheck before saving. It retries a changing snapshot at most three times and reports `run_conflict` rather than inventing completion. Preserve failures reported by another process and the prior successful completion when a later run fails.

Contracts: `tests/runs.rs` covers repair, unreadable facts, pending replies, abandoned runs, completion races and rotation; `tests/service.rs::freshness::a_ci_turning_red_or_green_makes_the_card_due` and `::quiet_active_projects_take_turns_for_an_idea_review` cover freshness decisions.
