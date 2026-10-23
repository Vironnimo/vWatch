# Desktop

The desktop presents Watchtower's shared state to the owner and connects explicit owner actions to the service and Windows.

## Overview

- `ui/index.html`, `ui/app.js` and `ui/app.css` form the vanilla JavaScript page, with no frontend build step. `src/app.rs` is its Tauri adapter; `src/bin/watchtower-ui.rs` starts the GUI executable. Keep service rules in `src/service.rs`; see `application.md`.
- `src/system.rs` owns executable-relative paths, the data-folder override, embedded Skill export, autostart and user PATH integration. Installer scripts belong to `development.md`; the operating Skill's content belongs to `agent-interface.md`.
- The GUI executable has the Windows GUI subsystem and remains independent of the console lifecycle. Do not move CLI dispatch, streams or exit-status handling into the window; see `agent-interface.md`.
- The page's four areas separate today's decisions, project re-entry, ideas and recommendations, and statistics/history. Changes should shorten the owner's read or make the next action clearer. UI text is German and addresses the owner as `du`.

## Interfaces

The page calls `window.__TAURI__.core.invoke` through `call` in `ui/app.js`. Commands in `src/app.rs` delegate to the shared `Watchtower` service using `spawn_blocking`. New owner capabilities need an intentional CLI counterpart or exclusion in `docs/agent-interface.md`; see `agent-interface.md`.

`state` returns the view with a `data_version`. The adapter combines SQLite's other-connection version with an own-write counter because SQLite alone does not notice writes on that same connection. `versioned_state` reads the counters before the state; `write` increments the counter even after failure so partial effects can be reloaded. Preserve that pairing when adding commands. The page compares versions and sequences state requests to prevent older results replacing newer accepted state.

Visits, opening a project and explicit read acknowledgement have different meanings. Render service-provided unread markers; only the explicit read action sends the displayed snapshot token to `mark_changes_read`. Refreshing, reopening, reading history or showing a project must not acknowledge new report content. Semantic digest rules belong to `application.md`.

Adding a document field requires its schema/default and guide in the owning domains plus a deliberate UI presentation here. Existing documents can lack new fields before the next agent run; render that state safely. Do not let UI fallbacks turn missing measurements into zero or a completed task.

## Conventions

`ui/app.js` keeps view state in its closure and renders HTML strings. Use `esc` for stored text in markup. Keep unsent drafts, selected targets and open disclosures independently of generated DOM: a background reload rebuilds the page. Event handlers generally delegate through document-level `data-*` targets; preserve their relationship to stable project and item IDs.

The service owns project identity, ratings, task transitions, warning decisions and undo guards. A successful save followed by a failed refresh or clipboard write is still saved. Display that distinction and reload before retrying an uncertain mutation. Feature-specific interaction contracts are in `desktop/interactions.md`.

Display preferences are persisted owner choices. Legacy values may need a presentation fallback without rewriting the saved preference. Card/window paths accept forward slashes and environment placeholders such as `%LOCALAPPDATA%`; `open_external` expands filesystem targets and opens URLs, folders or file locations rather than executing handbook commands.

## Constraints & Gotchas

- `tests/ui-helpers.cjs` executes the page in a Node VM with a small mock DOM. `tests/ui.cjs` covers shared behavior; the focused `ui-handoff`, `ui-project`, `ui-overview`, `ui-ideas` and `ui-statistics` files cover their interactions. These tests do not verify WebView layout, actual focus behavior or Windows integration.
- Verify UI changes in the running window using a task-specific `WATCHTOWER_HOME` under the checkout's `.tmp/`; see `development.md` for scratch and build setup. Do not change the owner's style/hidden-section preferences or leave test ratings, replies or completed ticks. `watchtower feedback` must show no test entries afterwards.
- Never toggle the owner's autostart or PATH choice for development verification. A scratch database does not isolate registry changes. The window requires WebView2; the optional Cargo `desktop` feature keeps Tauri dependencies out of the core/CLI build.

## Agent-facing text

`taskPrompt` in `ui/app.js` produces copied instructions for coding agents. Its reasons and the shared runtime-text change rules live in `agent-interface/agent-facing-text.md`. Read them before changing, adding or removing a sentence; update the corresponding reason with the text. `src/system.rs` exports the embedded Skill but does not own its wording.

## References

Read these only when your task matches - not by default.

- Changing refreshes, navigation, task handoff or owner interaction state -> `desktop/interactions.md`
- Changing startup, filesystem layout, Skill export or Windows settings -> `desktop/system-integration.md`
- Changing copied task instructions or embedded Skill wording -> `agent-interface/agent-facing-text.md`
- Changing service behavior or unread-content meaning -> `application.md`
- Adding/changing a Tauri owner capability or its CLI coverage -> `docs/agent-interface.md`
- Building, installing or verifying a live window -> `development.md`
