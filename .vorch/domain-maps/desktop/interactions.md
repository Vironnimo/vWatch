# Desktop interactions

Use this when changing a page flow, rendering state, asynchronous loading or task handoff. Service transitions and persistence remain owned by `application.md` and `persistence.md`.

## Live state and rendering

State reads, explicit Task recovery and Facts refreshes in `ui/app.js` share monotonically increasing request numbers allocated before dispatch. Only a response from a later request than the accepted response replaces `S` and its version. A delayed refresh cannot relabel an older snapshot as newer on arrival. A failed background read keeps the last usable display, does not acknowledge the failed version, and schedules a retry; this also recovers an initial load failure. Accepted snapshots invalidate statistics' basis without discarding shown results or drafts. Display preference writes are ordered per preference; pending choices survive reloads, and a failed final save restores the last successful choice. Reads dispatched before a save settles cannot replace that choice.

`watch` checks `data_version` every 2.5 seconds, avoiding overlapping loads and active text/select editing. Automatic fact refresh is separate: it runs while the page is visible, observes a 20-minute interval and skips measurement when all nonarchived projects have fresh facts. Focus can record a visit; the background version watcher calls `load(false)`. Neither is a read acknowledgement.

`render` replaces the main markup and rebuilds open secondary surfaces. Draft objects/maps, `openFolds`, input focus/cursor restoration and stable search destinations are therefore part of the interaction contract. Do not treat the current input DOM as the only copy of unsaved owner text. A background source rewrite must not silently discard an open answer or note.

Statistics have separate query and detail request counters. Keep the displayed statistics until their relevant facts/content basis changes, preserve unsubmitted filters, and reject results from an obsolete query or closed detail. A saved before/after comparison is an immutable selection, not a request to follow the newest versions automatically. See `tests/ui-statistics.cjs`.

## Reading and navigation

`Heute` emphasizes owner decisions, blocked/review work, focus and unread reports. Ordinary dirty files, local commits and missing remotes do not determine the attention order. Fact freshness and report-run completion are different signals; failures and replies awaiting incorporation remain visible. See `tests/ui-overview.cjs` and report-status tests in `tests/ui.cjs`.

`Projekte` keeps a compact list beside the opened project page. That page is a continuous read with work, startup/handbook and history/facts sections, reached through a jump bar. Arrow keys move through the project list; Escape returns. Handbook commands have individual copy actions. Search must open the exact matched section/item and retain its identity through insertions or reordering, refusing removed or ambiguous targets. See `tests/ui-project.cjs`.

The read action uses the token of the displayed semantic snapshot. Newer changes must remain unread if they arrived before the click completes. Project and recent-activity unread markers come from the same explicit reading state; even the first visit is not an acknowledgement. See `tests/ui.cjs` and `tests/digest.rs`.

## Tasks, ratings and recovery

`openTaskPreview` copies the selected task into a reviewable snapshot. Copy-only neither assigns nor changes task state. Assign-and-copy waits for storage and then uses the returned task ID and reporting instructions. Repeated clicks are guarded while saving. Clipboard failure leaves the saved assignment available; an unknown save result requires checking current tasks before any retry. Removed projects block new assignments while retained task project snapshots keep old assignments usable. See `tests/ui-handoff.cjs` and `tests/tasks.rs`.

Agent completion reports await the owner's confirmation. Reopening or confirming uses the displayed task revision, and task/source history remains available after a card rewrite. Warning acknowledgement leaves measured facts visible and expires when the service judges a material worsening. These are service rules rendered here, not transitions for the UI to invent.

Ideas and recommendations have independent category filters and rating rounds. Positive ratings do not assign work. Deferred items return on their saved local calendar date; rejected/deferred items and open note editors remain distinct. Stable IDs, retained proposals and first appearance keep feedback and task history attached after source rewrites. See `tests/ui-ideas.cjs`.

Idea progress and selected Task details come from `idea_states[id]`, including the full idea revision and enriched `task` from that same service snapshot. Do not reselect from the separate Task list or derive progress in the page. Local rating/snooze filters and pinned editors remain presentation decisions. The selected Task snapshot also drives the handoff preview, so its displayed revision cannot silently switch to a different list entry.

Guarded Undo receipts carry an opaque `undo_token`; feedback, owner-idea edits and project verdict adoption send it unchanged to `undo`. The GUI does not parse the separate CLI `undo` command string. Archive/restore and removal/restore actions retain their distinct inverse commands.

## References

- Changing copied task wording or its data boundaries -> `agent-interface/agent-facing-text.md`
- Changing service-side task, feedback, warning or read semantics -> `application.md`
- Checking real rendering and owner-data isolation -> `development.md`
