# Repository facts

This domain measures repository state and supplies the factual basis for Watchtower's reports without interpreting or changing the watched repositories.

## Overview

- `src/facts.rs` owns Git/process collection, CI lookup, local working-tree and side-work measurements, rebuildable-space measurement, and the basis/fingerprint used by due detection.
- `src/facts/statistics.rs` owns working-tree code/language counts and reconstructed daily activity/hotspots. It does not store observations or compare saved documents.
- `src/service.rs` selects and refreshes watched projects, supplies previous measurements, carries daily space measurements forward and persists results. `src/store/history.rs` retains observations; `src/service/statistics.rs` assembles history and coverage-aware comparisons. See `application.md` and `persistence.md` before changing those responsibilities.
- The desktop renders measurements directly; agents supply interpretation separately. A configured remote is not proof of a backup, and unavailable measurements must remain distinguishable from measured zero.

## Interfaces

`collect(path, today, with_ci)` is the full measurement entry point. `collect_reusing` additionally accepts the last facts from the same folder. It can reuse expensive measurements only when their inputs still hold; callers must not pass a different folder's prior state. Missing paths, unreadable Git history/status and a HEAD/branch change during collection produce unsuccessful facts rather than a current-looking successful snapshot.

The result has overall readability plus submeasurement state. Check both levels: successful repository collection does not guarantee a complete source scan or CI result. Collection is not an atomic repository snapshot; the final HEAD/branch guard catches those changes, not every concurrent filesystem edit.

`basis` selects changes that can invalidate a project report, and `fingerprint` hashes that basis. Head commits, uncommitted work, unpushed counts, documentation timestamps, side-work heads and readability participate. New informational fields stay outside the basis so adding a chart or measurement does not make all cards due. The service owns separate CI/due-reason rules; changing a reason is an agent-interface contract change.

`space` is separate from normal collection because directory walking is expensive. The service reuses it within the measurement day. It reports rebuildable directories for the owner to assess; it never removes them. Historical recording and retention are persistence responsibilities, not part of collection.

## Conventions

Use the existing `git`/`run` helpers. They disable prompts and pagers, avoid optional locks and index auto-refresh, capture both streams and impose deadlines. Diff reads disable external diff/text conversion where used. Do not introduce writes such as fetch, checkout, commit, push or cleanup into collection, including as a shortcut for stale remote state.

The collector represents unknown upstream comparisons with null values rather than assuming no unpushed work. Preserve unknown/partial states through service, history and UI consumers. Source and activity counting methods identify their semantics; a changed method invalidates reuse and can break size comparability. Measurement limits and activity reconstruction rules are in `repository-facts/measurements.md`.

## External Dependencies

Git supplies local facts; `gh` optionally supplies GitHub CI state. CI lookup runs alongside local collection and has its own deadline. It prefers an `origin` GitHub remote, reads the local remote HEAD for the default branch and falls back to the checked-out branch. Failed/unavailable `gh` remains unknown, and skipped CI is distinct from no workflows. No fetch updates remote-tracking information during observation.

Remote URL user-info is stripped before storing the remote list. Keep credentials out of facts and error payloads when extending external-command output handling.

## Constraints & Gotchas

`tests/facts.rs` owns real temporary-repository contracts, including unchanged index state, unborn branches, linked checkouts, same-sized dirty edits, collisions and equivalence between reused/full measurements. Unit tests in `src/facts.rs` cover subprocess/CI parsing details. Extend the owning behavior test rather than adding tests that only mirror helpers.

Use the fixed test date and isolated fixture setup from `tests/common/mod.rs` and `development.md`. Temporary non-repositories must not discover a parent checkout through Git traversal. Fixture Git writes create test inputs; they are not permission for the production collector to mutate watched repositories.

## References

Read these only when your task matches - not by default.

- Changing code/activity counting, cache reuse, worktree measurement or subprocess behavior -> `repository-facts/measurements.md`
- Changing refresh selection, due detection or statistics comparisons -> `application.md`
- Changing retained observations, deduplication or migration -> `persistence.md`
- Changing exposed fact meanings or due reasons agents consume -> `agent-interface.md`
- Building tests or creating isolated repository fixtures -> `development.md`
