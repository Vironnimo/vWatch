# Historical Read Models

`src/service/statistics.rs` owns statistics and document comparison semantics. Repository scanning and reconstruction inputs belong to `repository-facts.md`; stored observations, retention and query mechanics belong to `persistence/transactions-and-history.md`.

`statistics` reads saved observations only. It neither refreshes repositories nor acknowledges unread reports. A selected removed project remains available historically through `historical_project`; an ordinary unscoped query uses current project registrations.

Working-tree size comparison needs two complete measurements. From its chosen baseline through the current sample, measurements must remain valid and keep the same folder, branch and counting method. A gap or incompatible observation makes the delta unknown; do not subtract numbers merely because both endpoints contain values. Summary totals count only known measurements and expose their coverage.

Git activity is reconstructed evidence, separate from observations of working-tree state. Historical reconstruction windows can cover dates before Watchtower first observed a project. A newer reconstruction replaces the dates it covers, including days absent after a rebase. Totals are available only when merged coverage spans the entire requested period; gaps are not zero-activity days. Hotspots retain their own coverage dates.

Confirmed task completions count owner `done` transitions in task history, not agent reports requesting review. Card and portfolio comparisons use saved raw document versions; they must not apply today's reading projection to rewrite what was recorded. Detailed history, facts history and proposals are reads, not implicit visits or owner actions.

`Store::period_samples` returns the preceding baseline, bounded observations and overlapping reconstructions. Only baseline/current detail views need the large language and file payloads. Preserve that split when extending the read model so routine period queries do not rehydrate every historical payload.

Contract evidence: `tests/statistics.rs::observations_record_changes_deduplicate_payloads_and_compare_known_measurements`, `::statistics_skip_repeated_observations_without_changing_their_results`, `::archived_git_windows_still_cover_old_periods_and_do_not_turn_gaps_into_zeroes`, and `::history_filters_and_pages_keep_task_project_membership_and_exact_boundaries`.
