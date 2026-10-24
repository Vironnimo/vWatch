# Agent interface

vWatch's console CLI and desktop window call the same service. People and agents read the same ordinary text; programs explicitly request complete JSON. The operating Skill teaches the daily observation run and user-requested owner actions without database access, GUI automation or a mandatory discovery call.

Owner-change JSON receipts expose an opaque `undo_token` alongside the compatible `undo` command. Desktop callers pass the token directly to `undo`; readable receipts omit that metadata and retain the printed Undo command. Overview `idea_states` entries carry the selected Task content together with its guarded source revision, so consumers do not select independently from another Task list.

This document records capability coverage, call sequences, recovery contracts and validation scope. Update it with the CLI, desktop handlers and `skill/vwatch/` when those contracts change.

The saved `language` preference (`en` or `de`, English when unset) governs the window and newly authored Card and portfolio brief content. `settings update --language` changes it atomically with other supplied preferences and returns the existing guarded Undo receipt. Both contexts expose `report_language`, covered by their Context Tokens; switching languages rejects a pending stale write but does not rewrite historical content, reset read acknowledgement or make otherwise current reports due. CLI-generated help, labels, outcomes and diagnostics remain English regardless of this choice; stored content, names, paths and owner-supplied text retain their language. Generated digest entries expose `title_prefix`, `source_title` and `generated_detail` so the window can translate its own framing without translating authored content. OS diagnostics use English error-kind descriptions and numeric OS codes; measured subprocesses request the C locale.

## Entry points and test instances

The installed console entry point is `%LOCALAPPDATA%/Programs/vWatch/vWatch.exe`; `vWatch-UI.exe` is the independent desktop entry point. Production agents use shell execution in vBot or Codex. The operating Skill gives PowerShell and Git Bash forms. The CLI works with the window closed and never attaches a console to a GUI process.

Build the console without desktop dependencies:

```powershell
cargo build --no-default-features --bin vWatch
```

Use an absolute scratch folder under the repository's `.tmp/` for development calls. This example reads an isolated instance from the repository root:

```powershell
$env:VWATCH_HOME = Join-Path (Get-Location).Path '.tmp/vwatch-agent-interface'
& ./target/debug/vWatch.exe projects list
```

Git Bash equivalent:

```bash
VWATCH_HOME="$PWD/.tmp/vwatch-agent-interface" ./target/debug/vWatch.exe projects list
```

The global `--data-dir <folder>` overrides `VWATCH_HOME`. Non-default instances identify themselves in text and JSON. Printed continuations preserve the selected instance through an opaque `--instance` locator; copy them unchanged. The locator is an encoded path, not a secret or permission grant.

Job fault injection requires both a non-default data folder and `VWATCH_TEST_INSTANCE=1`. Tests can set `VWATCH_TEST_JOB_DEADLINE_MS` (20–45000 milliseconds), `VWATCH_TEST_JOB_DELAY_MS`, `VWATCH_TEST_JOB_CRASH=1` or `VWATCH_TEST_JOB_BLOCK=1`. The blocking switch waits for `.release-test-job` in that scratch folder, with a 30-second cap. Production's default instance ignores these switches. Tests in `tests/process.rs` and the job test module create isolated fixtures; do not enable faults for the installed owner's instance.

## Capability coverage

The table covers every registered Tauri command in `src/app.rs`. "Covered" means the CLI reaches the same service operation or exposes the same stored information; it does not mean the CLI imitates a window gesture.

| Desktop endpoint | CLI operation | Status |
|---|---|---|
| `state(visit)` | `overview`; `--visit` records an explicit owner visit | Covered |
| `refresh()` | `overview --refresh` | Covered; per-project refresh failures remain visible |
| `set_feedback` | `feedback set`, `feedback clear` | Covered |
| `restore_feedback` | `feedback restore --file` | Covered |
| `undo` | `undo <token>` | Covered; the window's feedback undo uses the same guarded receipt |
| `create_task` | `tasks create --file` or inline fields | Covered |
| `set_task_status` | `tasks confirm`, `tasks reopen`, `tasks cancel`, each with `--revision` | Covered |
| `set_warning_ack` | `warnings acknowledge`, `warnings clear` | Covered |
| `mark_changes_read` | `changes read --token` | Covered |
| `open_project` | `projects visit` | Covered; this records a visit, not Explorer navigation |
| `set_pref` | `settings update --style` / `--hidden-modules` / `--language` | Covered; view, collapsed sections and the shared window/report language |
| `set_status` | `projects status` | Covered |
| `set_goal` | `projects goal` | Covered |
| `add_idea` | `ideas add` | Covered |
| `update_idea` | `ideas update <id> <text> --revision <revision>` | Covered; guarded edit and undo preserve earlier text |
| `set_idea_status` | `ideas status <item-id> <status> --revision <revision>` | Covered; direct implementation and reversible archive for all idea sources; omit status to read |
| `statistics` | `statistics --project <id> --from <date> --until <date>` | Covered; stored measurements, optional filters |
| `history` | `history list` | Covered; bounded content journal with optional filters |
| `history_event` | `history show <id>` | Covered; complete old/new content |
| `document_versions` | `card history <id>`, `portfolio history` | Covered |
| `compare_documents` | `card compare <id> --from <version> --to <version>`, `portfolio compare` | Covered; read-only comparison |
| `remove_idea` | `ideas remove` | Covered; CLI also exposes trash and restore |
| `restore_idea` | `ideas restore` | Covered; the window's removal undo uses the same service |
| `rename_project` | `projects rename` or name-only `projects update` | Covered |
| `update_project` | `projects update --name` / `--path` | Covered |
| `remove_project` | `projects remove` | Covered; CLI also exposes trash and restore |
| `restore_project` | `projects restore` | Covered; the window's removal undo uses the same service |
| `discover` | `projects discover` | Covered |
| `add_projects` | `projects add <path>...` | Covered; a single path uses the single-registration service |
| `settings` | `settings show` | Covered; installation details and display preferences |
| `pick_folder` | Explicit paths in discover/add/update commands | Excluded native dialog; path selection is an input to the CLI |
| `set_autostart` | State readable through `settings show` | Excluded mutation: project instructions reserve this OS setting to the owner in the window |
| `set_agent_access` | State readable through `settings show` | Excluded mutation: project instructions reserve user PATH changes to the owner in the window |
| `open_external` | Paths and URLs readable through project/card views | Excluded native browser/Explorer navigation |
| `data_version` | Each CLI read returns current service state | Excluded UI reload counter; no agent polling loop is required |
| `ready` | None | Excluded window show/focus lifecycle signal |

Thirty of the thirty-six endpoints have CLI equivalents; six are explicit presentation or OS-setting exclusions. There is no uncovered persisted owner-state operation in this inventory.

The screen/event-handler pass covers behavior outside those endpoints:

| Screen or handler | CLI access | Boundary |
|---|---|---|
| Today, projects, ideas and project page | `overview`, `projects show`, `card show`, `facts show`, `ideas list`, `feedback`, `tasks list` | Stored content is available; layout, opening a project page and the idea round's order are presentation; the round's ratings use `feedback set` |
| Search and project navigation | `projects list --query`, project/card views | Search accepts ID, name, path or card text; CLI does not manipulate UI focus |
| Decision round and focus completion | Card/brief views plus feedback commands | Ratings/replies represent the owner's request, never an invented agent decision |
| Week review, overnight activity and space overview | `portfolio show`, `overview`, `facts show` | Same stored brief and measured facts; no cleanup operation |
| Task preview / copy / handbook command / settings text | Task, card, project and settings views; `tasks create` for assignment | Preview and clipboard remain presentation; assigning before copying uses the same persisted task operation |
| Export embedded Skill | `status` | Calls the same export as desktop startup; reports the exported folder |
| Skill installation command | `settings show` | Exposes the install command; installation itself belongs to the agent application's installer |

The CLI also exposes agent-only operations: measured refreshes, guarded card/brief writes, schema reads, run tracking, task reports, document history, trash restoration, saved output, jobs, usage and recovery activity.

The navigation selects purposes rather than competing dashboard designs: "Heute" (`handover`), "Projekte" (`board`), "Ideen" (`ideas`) and "Statistik" (`statistics`). The former handbook area is now part of each project page; the stored `handbook` style remains valid and opens "Projekte". The morning page prioritizes owner decisions, blocked tasks and results for confirmation, followed by focus and unread reports. Project voices stay visible in the projects area and on project pages. Routine local Git work no longer drives attention priority; concrete Git states and unknown measurements replace backup claims. Commit curves and agent progress percentages remain available only in project details, with percentages labeled as agent assessments. A project page replaces the former drawer: a compact project list stays beside one continuous page of work, start/handbook material and history/facts with a jump bar; owner status and goals open for editing on request, and search selects and highlights the matching section. Opening a project records a visit as the drawer did; re-rendering after a refresh does not.

The legacy preference name `hidden_modules` and CLI flag `--hidden-modules` now mean collapsed sections: their headers remain visible and can reopen them directly. Existing saved values require no migration. The window's recent-event list sorts by event date descending, then card update timestamp descending for same-day ties, preserving authored order within one card. The timestamp orders reports; it does not invent event times.

This behavior is covered by 30 passing UI checks, including legacy collapsed values, persistence, a failed preference save and list-size controls. The installed window was also exercised on a separate database copy: reopen a previously hidden activity section, show all 28 entries, return to the latest ten, and collapse while retaining the header. The owner's display preferences and feedback remained unchanged. The 133 Rust tests and strict Clippy passed; the display-label change was rechecked through the CLI/output contract suites.

The digest follow-up removes raw fact churn from both entries and acknowledgement tokens. Validation: 134 Rust tests, strict Clippy, formatting and 30 UI checks passed. On the same isolated owner-data copy, the real CLI changed four Git-only entries to zero while preserving the read baseline; the window's facts refresh also preserved the digest token. In the installed test window, three labeled report fixtures (progress, open end and question) appeared and were explicitly marked read. The normal window was reopened; production read baseline, display preferences and feedback remained unchanged. Installed binaries and the four Skill files match their sources, including Nabu's existing private Skill scope.

## Call sequences

Idea lifecycle: `ideas status <item-id>` returns the displayed status and revision; `ideas status <item-id> in_progress|implemented|open|archived|restore --revision <revision>` applies the owner's decision in one atomic transaction. A known revision from overview avoids the read. Progress uses the existing source-linked task and preserves its reports/history; direct completion is an owner event, never a fabricated agent report. Cancelled tasks return ideas to their rating filter instead of counting as implemented. Archiving is independent of progress and feedback, preserves active tasks, is recorded in content history and returns a guarded inverse command. Status revisions include task revisions and archive revisions, so an intervening report or decision rejects stale clicks, including undo. Context tokens include archives; the portfolio basis also changes on archiving/restoring. `idea_states` in overview and contexts tells agents which ideas are underway, implemented or archived. All actions are available on idea cards and in the rating round. The archive filter and search provide restoration. Agents still report results through `tasks report`; lifecycle mutations require the owner's instruction.

The `statistics` ("Statistik") area presents stored measurements and durable history alongside "Heute", "Projekte" and "Ideen", without changing the morning-page priority or read acknowledgement rules. `statistics` reads a selected date range in one call; `history list` returns at most 30 summaries and a continuation; `history show` retrieves one event's full before/after content. `facts history <id>` exposes complete immutable observations. `proposals` exposes independent retained agent proposals. Report omissions do not delete proposals; contexts include omitted retained proposals so agents can reconcile them without an extra mandatory discovery call. Own-idea edits use `ideas list` for the revision, then `ideas update`; they invalidate an earlier assessment and return guarded undo.

Facts history records a refresh atomically with the current facts, including errors, when it differs from the project's latest observation (facts, code or activity payload, folder or identity revision) or is its first of the UTC day; unchanged code/activity payloads share storage. The `changed-samples-v1` migration replaced the recording triggers and kept every earlier sample. `statistics` applies the same reduction to older history: `samples` holds the first observation of each UTC day, every change and the period's last observation, with measured values only (no language/file lists, worktree items, CI workflow lists or folder sizes), while current, baseline, comparison and activity are computed as before. It reads each project through its time index and parses JSON only for the points it returns. Code scans use Tokei with explicit exclusions and bounds; incomplete results stay unknown. Comparisons require consistent branch, folder and counting method and break across measurement gaps. Saved Git reconstruction windows remain usable after the rolling collection window advances; uncovered dates remain unknown. Migration preserves existing report versions and available facts without manufacturing earlier measurements or edit times. The content journal records reports, project identity/intent, own ideas, proposals, task changes, feedback and warning decisions; presentation preferences are outside this journal. Historical data remains local and can contain any document content agents submit, so the existing no-credentials boundary still applies.

Contract coverage is in `tests/statistics.rs` (migration/reopen, atomic rollback, failed observations, deduplicated payloads and repeated observations, unchanged results over repeated older history, historical coverage gaps, retained proposals, revisions/undo and real-console success/failure/continuation) and `tests/ui-statistics.cjs` (unknown values, chart gaps, safe history rendering, retained unrated ideas, edit drafts, stale asynchronous results, refetching only after measured facts or content changed while the shown numbers stay, change-only observation rows and sections rendered when opened). These deterministic checks do not claim a fresh-model evaluation. Validation: both full Rust configurations passed 153 tests; the final history/statistics contracts were rerun after the migration-baseline fix. Strict Clippy and 78 UI checks passed. The native statistics overview was inspected against an isolated copy of the eight-project database. Production projects, ideas, feedback, task state, warning decisions and preferences matched their pre-test snapshots. Nabu's existing private Skill received only the new capability documentation, preserving its unrelated adaptations.

Counts below are protocol design targets confirmed by command/contract tests, not measurements of fresh-model behavior. They exclude reading repository evidence and Skill references. A page or unfinished job adds its necessary continuation; it does not repeat the underlying operation.

| Goal | Sequence | Calls without pagination or waiting |
|---|---|---|
| Read the morning overview | `overview`; add `--refresh` when fresh measurement is requested | 1 |
| Rename/reconnect a known project | `projects update <id> ...` | 1 |
| Find and rename a project | `projects list --query <text>` → `projects update <id> ...` | 2; selection is needed only when the ID is unknown |
| Register several selected folders | `projects add <path>...` | 1 |
| List questions awaiting the owner | `questions`; `--project <id>` for one project | 1 |
| Answer a known question | `feedback set <item-id> --value answered --note ...` | 1 |
| Write one current card | `context <id>` → study evidence → `card put <id> --context-token ...` | 2 |
| Daily run with N due cards and a due brief | `runs start` → N context/save pairs → portfolio context/save → `runs finish` | `2N + 4` |
| Daily run with nothing due | `runs start` → portfolio context and synthesis → save only if changed → `runs finish` | 3 without a save |
| Report assigned work | `tasks report <id> --status done --result ... --evidence ...` | 1; owner review remains separate |
| Confirm a known current task result | `tasks confirm <id> --revision <revision>` | 1 |
| Correct an immediately preceding owner edit | Printed `Undo:` command | 1 where the result provides undo |
| Recover a lost response | `activity` → relevant saved job or current-state read | Depends on whether a completed result was captured; never a blind replay |

There is no mandatory status/preflight call, dry run before a save, or verification read after a successful mutation. A save returns its result and warnings. First cards use a complete document without `--merge`; incremental updates use `--merge` and preserve whole nested fields and list item IDs.

Common noun/verb aliases and case-insensitive enumerated values tolerate clear spelling variations. Unknown or ambiguous targets fail with candidate/recovery information instead of mutating a guessed project. The parser owns accepted aliases; `--help` is the command index rather than a second hand-maintained list in the Skill.

## Output and error contracts

Repository readability and optional measurement completeness are separate. Dirty file counts come from successful Git status; text-line totals are nullable and certified by `dirty.line_counts.ok`, with `partial` and `errors` describing an incomplete attempt. Limits, read failures and failed/invalid tracked numstat output invalidate both line totals; binary files are excluded from text-line counts. `space.ok` certifies `total_bytes`, and each directory has its own `ok`/nullable `bytes`. One failed directory or incomplete discovery makes the total unknown without hiding complete directories. Missing completeness flags are uncertified, including saved observations from earlier collectors: current and historical read models expose null totals without rewriting retained rows or their fingerprints. Only a complete same-day space result is reused; incomplete or uncertified results are retried on the next refresh. Refresh receipts expose `incomplete_measurements` (`dirty_lines`, `rebuildable_space`); these optional gaps do not turn readable Git Facts into an overall failure. Risk trends compare lines only when both observations have certified counts, otherwise file counts; they never compare different units or treat an unknown count as zero.

Plain output is the default regardless of redirection. Its first line states the outcome; lists state their scope, order and total. `Note:` explains limitations or partial effects, `Next:` is an executable continuation, `Error:` identifies failure, and `Undo:` names a supported reversal. Stored content is escaped and indented so text inside an item cannot forge a top-level marker.

Table values are complete; padding is bounded independently of value length. Stored `note` fields are labeled `Saved note` to distinguish owner content from CLI notices. Task receipts put ID, status and revision before their history, so the next owner decision does not require paging through old events. Successful fact measurements omit empty error/warning slots. Project remove/restore receipts describe the resulting removal state.

Card and brief writes return concise readable receipts: saved version or replay/preview state, context token, warnings, card field changes/omissions, item IDs and guarded Undo. They do not echo the submitted document. Explicit JSON and document `show` retain the full content; large sets of item receipts still use normal paging.

Portfolio context carries each card's version and actionable finding evidence (details, sources, question options and completion criteria). Measurement validity, errors and collection time remain visible; missing branch counts remain null. Per-project `review_week_evidence` separates strictly week-bounded changes/milestones from current card summaries. The review's commit count is selected relative to the stored measurement's calendar week and remains null when that sample cannot establish the completed week's count. These are additive JSON fields except for correcting previously fabricated zero or misdated counts.

Agent estimates of task duration or effort are removed from reentry, suggestions, focus, synergies, new apps and idea assessments. Neither schema advertises or requires `effort`; legacy payload fields follow normal unknown-field handling (accepted, dropped and warned). Old stored documents/history are not rewritten. The window ignores legacy estimates in all styles and on project pages; the portfolio's suggestion projection omits them. The operating Skill forbids estimates in prose too. Observable completion criteria, owner-set deadlines and measured activity remain unchanged.

Document validation enforces the published nullable types: omission applies defaults on full writes or retains fields in merges; explicit null is accepted only for nullable fields. Duplicate explicit item IDs reject the write instead of silently acquiring a different identity. Missing-field detection examines every list item. Portfolio merge errors identify inherited brief fields as such. These changes do not rewrite stored documents; a malformed inherited field requires reconciliation before the next save.

Run status is a compact, persistent toolbar disclosure beside Git fact freshness, not a leading content panel. Details show recorded completion time and project reports rewritten versus retained, derived from initial/current versions; missing historical metadata is explicitly unknown. Failures, pending replies, causes and continuation prompts remain available. Opening the disclosure survives data refresh and changes no preference. The greeting uses reading size. "Git-Fakten messen" identifies the refresh operation accurately; it does not claim to renew agent reports. The Skill explicitly asks reviews to consider useful new ideas even when older suggestions remain.

The `ideas` view is labeled "Ideen & Empfehlungen" and opens with product ideas. A separate recommendation category contains next steps, upkeep and repairs. Card `suggestions[]` and portfolio `synergies[]` accept `category: idea|recommendation`; app ideas and owner ideas remain ideas. Missing categories default to recommendation, except existing `type: feature` suggestions infer idea. Existing stored documents are not rewritten: missing fields make cards and briefs due with `new_fields`, listed in their respective contexts. Project suggestions allow eight total, at most four per category. Both categories have separate counts and rating rounds, with the same new, positively rated, deferred, assigned, completed/closed, rejected and all filters. Search opens the appropriate category and status filter. Project pages separate both sections.

`proposals --category idea|recommendation` reads the same classification; omission returns both. Receipts expose the selected filter, and every retained proposal and its item expose their effective category. Portfolio context preserves the category of project suggestions. Reclassification retains IDs, first appearance, feedback and assignments. An expired deferral returns to new; rating, agent assessment and assignment remain separate decisions. Rated proposals and recognized task sources remain accessible after leaving a current report. Removed owner ideas stay absent until restored, even when old feedback or tasks retain them. Open input forms survive filtering, category switches and background rewrites. The operating Skill requires separate selection outcomes for ideas and recommendations; a hygiene recommendation does not satisfy product reflection.

Ideas sort by first saved appearance of their stable ID, newest first; project pages use the same ordering. Window state exposes each project's `suggestion_first_seen` and top-level `new_app_first_seen` and `synergy_first_seen` mappings for current IDs. Owner ideas use `added_at`. Historical versions establish these timestamps without changing agent documents or schemas. Rewording, reordering, temporary removal and later restoration retain the original appearance date. Unknown dates sort last, and ties preserve authored order. No date is inferred from the latest card rewrite or supplied by the agent.

`overview`/window state adds the following owner-facing metadata without changing card or portfolio schemas:

- Project `card_unread` compares semantic report content against the explicit read snapshot; generated IDs, whitespace, list order, elapsed ages and legacy effort estimates do not create another unread report. `unread_recent_changes` contains zero-based indices into that card's current `recent_changes`, compared independently so unrelated report edits do not mark old activities new. Both are null without a read baseline; archived projects do not receive unread markers. `pending_owner_replies` counts replies not yet reflected in the card's owner basis, retaining the existing timestamp fallback only for legacy cards without that basis.
- Each `feedback[id]` retains its existing reaction fields and adds `item`, `project_id`, `title` and `first_seen`. The item is the rated snapshot with its kind, not a newly invented proposal; unknown original content or dates remain null.
- New tasks persist a recognized `source_snapshot`, including kind, at assignment. State adds `source_kind` and `source_first_seen`; for older tasks without snapshots it projects available feedback or historical source information without rewriting the task. Original task title, reason, evidence and completion criterion remain authoritative for the assignment. Unrecognized sources remain unknown rather than becoming ideas.

Owner ideas use the reserved identity `idea:<numeric-id>` for feedback and task sources. Their snapshot kind is `owner_idea`, with text, first creation time and any available assessment. `idea_checks[].idea`, idea removal and restoration retain their existing numeric IDs. Future card/brief writes cannot use the reserved `idea:` prefix, preventing owner reactions from attaching to agent-created items.

The task preview shows the proposed action, reason and completion criterion before either operation. "Nur kopieren" performs no assignment. "Beauftragen und kopieren" saves the task first, then copies a prompt containing its ID and reporting route; already active assignments are reused. A clipboard failure preserves the saved assignment and offers another copy. An unconfirmed save outcome requires inspecting saved tasks before retrying, rather than assuming no effect. Sending the copied prompt to an agent remains the owner's action.

Earlier recency rollout validation: 136 Rust tests, 35 UI tests, strict Clippy and formatting passed. The then-current window on an isolated database copy showed the six newest ideas with their original dates, expanded to all eighteen and returned to six. Release binary hashes matched the installed copies. Production display preferences, explicit read baseline and feedback were preserved; opening the normal window remained an ordinary recorded visit. The four operating Skill files were unchanged in that rollout and still matched Nabu's private installation.

Earlier overview rollout validation: 135 Rust tests, strict Clippy, formatting and 34 UI tests passed. Real CLI saves on a separate owner-data copy accepted cards/briefs without estimates, dropped legacy estimates while keeping content and IDs, and preserved historical versions. The then-current test window displayed the smaller greeting and estimate-free tasks; its disclosure showed three updated and five retained reports with the recorded completion time. The ideas list expanded from eight to all eighteen stored proposals and returned to one per project. This verified display and compatibility, not the quality or novelty of future generated ideas.

Ordinary responses, including oversized errors, fit within 10,000 Unicode characters including instance framing. The pager limits each saved page to 8,500 characters, reserves room for the instance note and bounds the page body at 7,500 characters. A complete immutable snapshot is stored locally; `output <token> --start <offset>` reads it by zero-based Unicode character offset. Snapshots never rerun writes or remeasure facts. Failure to persist output reports that the original operation has already run and directs the caller to current-state inspection.

Intermediate pages expose the saved-output continuation; the last page restores the original generated workflow `Next:` with the same instance. Stored content cannot supply that command. The worker reader rechecks a durable result after an inactive lock, covering completion between its first result check and its liveness check instead of falsely reporting an unknown outcome.

Potentially slow operations use a 45-second inline wait budget, then return an explicit unfinished job and exact continuation. Each `jobs wait` uses the same bounded wait and returns the original result when complete. Workers use the captured instance and working directory; they do not retain the shell's output pipe. A completed job can be reread without rerunning a mutation. Lost-worker outcomes are reported as unknown, not rolled back or successful.

`--json` or `--format json` explicitly requests a complete, unpaged JSON object. This is an intentional exception to text character budgets because programs require the full structure. `--json` wins over another output format. `--help` and `--version` remain text. `--input-json` is document input and never selects an output mode. UTF-8 files and BOM-marked UTF-16 are accepted; output JSON escapes preserve Unicode through Windows PowerShell's native pipes.

Exit codes are 0 for success, 1 for failed or incomplete work, and 2 for invalid usage. Expected results and errors are on stdout so an agent receives the recovery with the result; stderr is reserved for exceptional transport failures. Partial registration/refresh success and an incomplete run are not reported as full success. A malformed call changes nothing. Error text distinguishes invalid fields, missing targets, stale context, inaccessible files, database locks, corruption and storage failures.

Measured real-process samples in an isolated instance:

| Sample | Unicode characters | Exit code |
|---|---:|---:|
| Top-level help | 7,605 | 0 |
| Project list | 313 | 0 |
| Unknown project | 295 | 1 |
| Invalid option | 738 | 2 |
| Invalid option containing 25,000 characters, first saved page | 8,127 | 2 |
| Project context | 2,435 | 0 |

Stderr was empty in every sample. The capture is `%TEMP%/vwatch-rename-cli-work/agent-interface-output-samples.json`; these measurements characterize the sampled fixtures, while the contract tests exercise larger and adversarial content.

## Decisions and rejected alternatives

| Decision | Rejected alternative and reason |
|---|---|
| Text for ordinary calls; explicit complete JSON for programs | Automatically selecting JSON for agents or redirected output violates the requested human-readable default. Truncating explicit JSON prevents reliable programmatic parsing. |
| Restorable project/idea trash with no automatic purge | Permanent removal or timed purging loses history and makes a reversible owner request destructive. Renaming by remove/re-add also breaks identity. |
| Known aliases and clear formats; unknown flags/targets fail | Guessing the intended semantics of an unknown flag or nearby project can change the wrong data. Errors provide correction without executing a speculative mutation. |
| Reread and reconcile after a context conflict | Silently obtaining a fresh token for an unchanged payload would overwrite owner replies, facts or another agent's work. Identical durable replay is allowed only while the original post-write context still matches. |
| Finite workers with saved outcomes and bounded waits | A persistent daemon adds a separate lifecycle without improving these local operations. Waiting indefinitely reintroduces shell hangs; starting the original operation again risks duplicate effects. |
| No `jobs stop` command | Forced termination cannot promise rollback after a partial write. Finite work completes under read-subprocess timeouts: git has 30 seconds per call and `gh` has 20 seconds. If a worker disappears, recovery reports an unknown outcome and inspects state instead of claiming cancellation was safe. |
| Field-scoped guarded undo | Restoring an entire earlier owner record could erase unrelated later edits. An old receipt is rejected when its affected values have changed. |
| Explicit OS/presentation exclusions | Agent-driven autostart/PATH changes conflict with the owner's project instructions. Clipboard, browser/folder launch and window focus are presentation actions; their underlying data remains accessible. |

## Error and recovery test inventory

| Failure or edge case | Contract checked | Tests |
|---|---|---|
| Invalid arguments, unknown flags, wrong input/output flag | Exit 2, actionable correction, no mutation, explicit instance retained | `tests/cli.rs`, `tests/recovery_contract.rs`, `tests/process.rs` |
| Missing/unreadable file, encoding, malformed JSON, closed stdin | Name the input problem; preserve the prior document; no prompt or blocking read | `tests/cli_parity.rs`, `tests/process.rs` |
| Unknown project/task, removed project and trash recovery | Identify the missing target or restore route before unrelated validation; never create replacement identity | `tests/service.rs`, `tests/effects_recovery.rs`, `tests/cli_parity.rs` |
| Missing or stale context, concurrent writers | Preserve current owner instructions and facts; reject stale payloads; one successful version for an identical concurrent request | `tests/concurrency.rs`, `tests/effects_recovery.rs` |
| Database lock, access, corruption or receipt-write failure | Distinguish storage causes; preserve damaged files; roll back a failed atomic edit/undo | `tests/service.rs`, `tests/owner_undo.rs` |
| Partial registration/refresh and incomplete runs | Keep successful effects visible; exit 1; failed measurements cannot count as current cards | `tests/cli_parity.rs`, `tests/output_contract.rs`, `tests/effects_recovery.rs`, `tests/runs.rs` |
| Deadline, stopped caller, disappeared worker, failed job storage | Return bounded continuation or unknown outcome; capture/replay original result; do not report rollback or replay work | `tests/jobs.rs`, `tests/recovery_contract.rs`, `tests/process.rs` |
| Long output/error, Unicode offset, changed data, wrong instance | Preserve snapshot content, bounded pages and original exit status; continuations target the same data folder | `tests/output_contract.rs`, `tests/recovery_contract.rs` |
| Paging storage fails after a saved mutation | Name the delivery failure and saved effect; direct recovery to current state without replaying the mutation | `tests/output_contract.rs` |
| Repeated write/report, trash restoration, stale undo | Avoid duplicate effects, preserve confirmed task results and history, reject intervening affected-field changes | `tests/effects_recovery.rs`, `tests/owner_undo.rs`, `tests/cli_parity.rs`, `tests/ui.cjs` |
| Metadata privacy, unavailable log and incomplete activity | Omit values/content from usage; preserve successful writes when logging fails; distinguish unfinished or unknown from failed/unchanged | `tests/usage.rs`, `tests/recovery_contract.rs` |

## Effects, replay and recovery

- Registration changes retain stable project IDs. Renames and path updates preserve cards, reactions, tasks and history and do not move repository files. Invalid combined updates change neither field. A saved path edit can outlive a failed facts refresh; output names that partial effect and the refresh recovery.
- Removed projects and ideas are hidden, not purged. Trash and restore preserve original IDs and records. Card/brief history restoration appends a version with its original evidence basis; restoring wording does not claim a fresh analysis.
- Card/brief writes require the context that was studied. A changed document, fact or owner instruction rejects a stale payload. An identical request with its previous token replays a durable saved result only while the post-write context still matches; a different payload or intervening change fails. Agents reread and reconcile, not merely replace a token.
- Owner edits to names/paths, status/notes, goals/dates, feedback, warning acknowledgements and display preferences return guarded undo receipts. Undo restores exactly the affected fields, rejects intervening changes to them and preserves unrelated changes. Folder undo discards facts from the other folder and returns a refresh continuation. Ordinary project/idea trash inverses are not blanket revision guards; preserve later owner changes before applying them.
- Repeated registration of the same path retains the existing record when supplied identity details agree. Identical active ideas and tasks are deduplicated. Run start coalesces an unfinished run; an identical failure report retains its original failure basis. No-op changes do not emit a destructive inverse.
- Task reports record evidence and request review. Only an explicit owner decision confirms completion, reopens or cancels a task; those transitions require the current revision. Daily observation does not execute assignments or fabricate ratings and answers.
- `tasks report <id> --file <path>` accepts a UTF-8 or BOM-marked UTF-16 JSON object containing `status`, `result` and optional `evidence` (required for `done`). `--file -` reads stdin. File input and inline report fields are mutually exclusive. The service rejects unknown report keys or invalid types without changing the task; the same reporting behavior serves both routes. `tasks show` and report/status errors use the service's shared lookup, including available IDs for a missing task.
- Reads and visits never establish or move the read baseline, including the first visit. Only `changes read` or its window action acknowledges the displayed snapshot. Without a baseline, read status stays unknown and the window offers an explicit initial acknowledgement. The digest compares authored progress, open ends, questions, decisions and incorporated replies; other changed report text can mark a project unread without adding a digest row. All such markers share the same acknowledgement. Measured facts (dirty files, ahead counts, branches, worktrees, remotes and CI) generate no digest entries and are excluded from the snapshot/token. Existing baselines remain usable without migration. The token is captured before loading displayed reports and rejects later card/owner writes, including handbook or voice edits; facts-only refreshes do not invalidate it. Facts and warning acknowledgements remain separate, and material worsening still invalidates an acknowledgement.

## Privacy and metadata

All state, snapshots, worker files and usage metadata stay in the selected local data folder. There is no telemetry. Repository observation remains read-only; `gh` reads CI state over the network.

`cli-usage.jsonl` records parser-owned command/flag names, target counts, UTC timestamps, process/parent IDs, elapsed milliseconds, exit status and output character counts. It never records argument values, project IDs, paths from arguments, document text or output content. Logging is best-effort and does not turn a successful write into a reported failure. Usage reports inspect the last 20,000 log events; the append-only file is not an automatic deletion policy. `usage --since` filters inclusive UTC dates.

`activity` returns the most recent 20 recorded calls. A recorded exit code describes that command, not the completeness of a daily agent run. Missing completion metadata does not prove that a mutation had no effect. Parent process IDs are grouping hints, not verified model sessions; repeated command shapes can involve different values or targets.

Metadata reads omit their own in-progress `activity`/`usage` entry from their result. The log still records these reads and later reports include their completion; interrupted mutations remain visible as unknown.

Job request/result files and output snapshots are different from usage metadata: they retain arguments, instance/working-directory paths and result text needed for recovery. Treat those files like the private database. Do not attach unredacted snapshots, worker requests or owner documents to an external report. No model-session corpus is collected by vWatch.

## Runtime text and its reasons

| Surface | Constraint and reason |
|---|---|
| Skill description | Select operating vWatch, not coding its repository; avoids taking over unrelated tasks |
| Skill calling/output sections | Default text, exact shell fallback and continuations; avoids forced JSON, late-output polling and repeated mutations |
| Skill authority rules | Daily observation versus explicit owner changes; prevents invented decisions and unintended repository edits |
| Skill card/brief recipes | Context reconciliation, first-card behavior and whole-field merge semantics; prevents stale overwrites and missing list entries |
| Run start and repair instructions | Reflect before each reviewed card and synthesize across all projects, including when reports are current; a freshness check must not become an ideation skip |
| Field guides | Every schema field, evidence and word limits; avoids invented payloads and generic filler |
| Feedback/task rules | Stable item IDs, rejection/snooze semantics and reported versus confirmed completion; prevents repeated unwanted suggestions and false completion |
| Errors and partial-effect notices | What failed, what remains saved and the available recovery; prevents blind retries and database recreation |
| Output data framing | Stored text cannot forge `Next:`/`Undo:` instructions; prevents records from acquiring command authority |
| Generated task prompt | Reports results through the CLI and leaves completion confirmation to the owner |

The complete source Skill measured 12,887 → 11,033 `o200k_base` tokens, a reduction of 1,854 tokens (14.4%). The main body changed from 3,407 to 2,806 tokens while adding recovery and owner-operation guidance. `AGENTS.md` changed from 2,668 to 2,908 tokens: the 240-token increase records owner authority, recovery ownership and the on-demand interface reference. README changed from 1,289 to 1,480 tokens to explain continuations, explicit JSON and restoration. Keep shared rules in their owning surface rather than repeating them in every prompt.

Static trigger review used five requests that require this Skill: a vWatch daily run, a card update, a watched-project rename/reconnection, rating a displayed suggestion and reporting a vWatch assignment. Five neighboring requests do not require it: implementing vWatch Rust code, fixing another repository's CI, unsaved brainstorming, renaming a Codex project and generic Windows PATH configuration. These are static scope checks, not model trigger measurements.

The exact before/after Skill text is preserved in the repository, with review baseline `479455e27c59a77c54ddbfcc29c86f324cbe8c7c`. The after text is the source in `skill/vwatch/`; do not duplicate the four complete documents in this report. Review their changes from the repository root:

```powershell
git diff 479455e -- skill/vwatch/SKILL.md skill/vwatch/references/cli.md skill/vwatch/references/card-guide.md skill/vwatch/references/portfolio-guide.md
```

## Validation and release evidence

Contract suites cover service behavior, readable/JSON output, real native process streams, PowerShell/CMD/Git Bash behavior, Unicode input, closed stdin, output continuation, deadlines, recovery, schema errors, history preservation and guarded undo. `tests/skill_commands.rs` extracts command lines from every Skill Markdown code block and runs them with isolated real fixture values so new examples enter the checks automatically.

Run from the repository root:

```powershell
cargo test --no-default-features
cargo test
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
node --test tests/ui.cjs tests/ui-project.cjs tests/ui-handoff.cjs tests/ui-ideas.cjs tests/ui-overview.cjs
cargo fmt --check
```

The Skill review verified 63 actual command-help calls, and its extracted PowerShell recipes passed against isolated data. The Bash fallback is normalized to the same executable in that recipe test; the process suite separately checks actual Git Bash argument and Unicode behavior. Independent static review corrected the first-card merge recipe, per-kind feedback values, snooze default, response-field distinctions, undo scope and CI-state coverage. The provided Skill checker returned: `vwatch: description 218 characters, body 159 lines, 3 other files (308 reference lines); no findings`.

Completed validation at this report update:

- `cargo test --no-default-features`: 130 tests passed.
- `cargo test` with desktop features: 130 tests passed (`final-desktop-tests.log`).
- Clippy with `-D warnings`: passed for both default and no-default-feature configurations.
- Node UI suite: 25 tests passed.
- The Windows window rendered and a rating write was verified against an isolated database copy. Live Undo was not verified: the input helper's guard blocked the follow-up interaction. Service and UI contract tests cover guarded Undo and intervening-change conflicts; those deterministic results are not described as a live Undo test.

Release verification completed on 2026-10-07:

- Both release executables were installed with `scripts/install.ps1` and match the built files byte for byte. The CLI uses Windows console subsystem 3; the window uses GUI subsystem 2.
- The initial rollout incorrectly created and updated a global vBot package while Nabu's pre-existing private package stayed stale. This was corrected: all four Skill files now match the repository, installed export and Nabu's private package byte for byte. The unintended global copy was removed into vBot's recoverable archive. The private package is available only to Nabu and has no sharing grants. Future updates must retain the package's existing scope.
- Installed-process checks passed for readable and explicit JSON project lists and missing-target errors, with the expected exit codes and empty stderr.
- The renamed `vUtility` project retains ID `vutility2` and all five original card versions, with their exact content hash unchanged. Owner preferences, autostart and user PATH are unchanged; installed feedback contains no test entries.
- After the final UI restoration adapters and readable undo-conflict text, desktop compilation, 11 focused recovery/undo tests, all 25 UI tests and both strict Clippy configurations passed again.

No `agent-eval` Skill was read or run, as requested. Deterministic tests establish command behavior and text contracts; they do not establish that every model follows the Skill or reaches the ideal call count. No fresh-model usability result is claimed.

## Daily-run follow-up, 2026-10-08

The production run exposed three concrete gaps: successful writes repeated entire submitted documents; the portfolio lost finding sources and action criteria during projection; and weekly material mixed the reviewed week with newer changes. Inspection also found that failed measurements could appear as zero branches and cached weekly counts were indexed against the current week instead of the collection week.

The fixes preserve evidence through the portfolio projection, distinguish unknown measurements, provide bounded historical evidence, and replace repeated document bodies with save receipts. The Skill now distinguishes Git remotes from backup evidence and recorded checks from newly executed tests. No broad context compression was added: the larger context supplies information needed to substantiate priorities.

Local token measurements compare baseline `0fb9a8a` with this change using the same SQLite backup in separate scratch instances, the real console executables and `o200k_base` (tiktoken 0.14.0). Text totals include instance framing and every output-page continuation; each returned page is counted once. Payload input, repeated conversation history and Provider wrappers are excluded. These are local estimates, not Provider billing or a model evaluation.

| Surface | Before tokens | After tokens | Change | Calls before → after |
|---|---:|---:|---:|---:|
| Complete operating Skill, four Markdown files | 11,033 | 11,314 | +281 | — |
| Top-level CLI help | 1,552 | 1,552 | 0 | 1 → 1 |
| Portfolio context, eight projects | 13,333 | 19,558 | +6,225 | 7 → 10 |
| Portfolio save receipt | 1,965 | 404 | −1,561 | 1 → 1 |
| Card merge receipt | 4,767 | 257 | −4,510 | 3 → 1 |

The sampled context and two saves total 20,065 → 20,219 tokens (+154) and 11 → 12 calls: substantially more evidence at nearly the same one-time output cost, not an overall speedup. Save receipts alone fall from 6,732 to 661 tokens. Command definitions are unchanged; only top-level help was separately measured. Skill text recurs when loaded into model context, with later exposure depending on host caching and framing. Private sample captures remain in the local temporary directory and are not committed.

Validation for this follow-up: 133 Rust tests pass with desktop features and 133 without; both strict Clippy configurations, formatting and all 25 Node UI checks pass. Added contracts cover receipt save/merge/preview/replay and full JSON/show preservation, failed measurement limits, preserved action evidence, inclusive review dates and cached counts across a week boundary. Real-process measurements use isolated database copies; no owner decisions or production documents were changed by these checks. This is deterministic and process evidence, not a new model usability trial.

Installed both release executables and reopened the previously running window. Binary hashes match the release build; an installed-CLI check against isolated data confirms the richer context. The four Skill files match the repository, installed export and Nabu's existing private package byte for byte. Its scope remains `agent:nabu`, without sharing grants or a new global package.

## Fresh-agent observation, 2026-10-08

At the owner's request, one fresh subagent received the installed Skill and a normal daily-run task, without prior conversation or implementation context. The authorized production run completed as run 4: two due project cards and the portfolio were saved, six current cards retained, and no work remained failed or pending. A portfolio write first returned `version_conflict` with an explicit no-save outcome; the agent reread and reconciled context before succeeding. This is one observed run with recovery, not a capability-selection test or a reliability claim across models.

The retrospective identified an unclear conflict cause, ten pages of portfolio context to reread, legacy effort estimates still visible in agent contexts, and no concise distinction between added, edited and retained suggestions in save receipts. The only visibly changed context field the agent identified during recovery was the owner's last-open timestamp; the actual conflict cause was not established. One tool-output truncation came from the agent batching too many pages, not vWatch pagination. The run added a pattern and a focus item, but no new product/app idea. These findings remain review evidence, not claims of implemented fixes. Tool definitions and the operating Skill were unchanged during this trial, so their before/after definition-token delta is zero; no whole-run token saving was measured.

## Context follow-up from the fresh-agent review

Inspection established that every changed `last_visit` timestamp invalidated portfolio context. The guard now compares only whether the owner has opened vWatch since the previous brief. The first crossing of that boundary still rejects an older draft; later visits on the same side do not. With no previous brief, visits do not affect the token. Cards, facts, owner instructions, feedback, ideas, tasks and warning decisions remain guarded. Opening is not an acknowledgement that the brief was read. This explains a reproducible source of avoidable conflicts without claiming it was the sole cause of the observed trial conflict.

Current card/portfolio contexts and readable latest-document views omit the six former `effort` slots. Known rated-item snapshots in context also omit that obsolete field. The projection changes neither stored content/history nor raw JSON document views; arbitrary extension data and quoted evidence are preserved. Readable portfolio context renders each uniquely matched weekly finding once, moving its full evidence into the existing card row and identifying the weekly subset by 1-based row positions in that project. Ambiguous matches keep the original complete evidence. JSON context keeps both arrays. No additional fetch is required and no general summary replaces evidence.

The portfolio guide permits correcting demonstrated errors in retained fields, including a weekly review that is not due, while preserving its historical period. It no longer equates an app opening with proven reading. The generic conflict error still asks for reconciliation: the opaque token alone cannot establish an exact historical cause, so the message does not invent one. Per-item save diffs and a larger context/delta protocol were not added.

On the same isolated owner-data backup, real CLI output changed from 19,778 to 19,303 `o200k_base` tokens (−475, −2.4%), 1,130 to 1,084 lines and 76,093 to 74,779 Unicode characters. Both versions required ten pages. Every returned page is counted once, including instance framing and continuation text; payload input, repeated conversation exposure and Provider wrappers are excluded. These local estimates use tiktoken 0.14.0, not Provider billing. The four Skill files measured 11,496 to 11,611 tokens (+115) against `1783756`; CLI definitions, schemas and help were unchanged. Skill text adds to model context whenever loaded, subject to host caching and framing; no whole-run savings or new usability trial is claimed.

Focused validation passed 11 concurrency, 19 service and 10 output-contract checks, plus strict no-default-features library Clippy. Real process checks on another scratch copy confirmed preserved tokens for repeated visits before/after the brief, rejection on the first post-brief opening, a successful write after a later repeat visit, and continued rejection after a new owner idea. Six historical card estimates remained in raw JSON and disappeared from current context. Private captures are in `%TEMP%/vwatch-context-review-r17t03p2`; no production document or owner state was changed.


## Purpose-based overview review, 2026-10-08

The owner approved the three independent product reviews, with an explicit exception: keep the project voices. The existing clean baseline was `1783756`. The resulting areas are Today, Projects, Ideas and Handbook; owner decisions and waiting work take precedence over ordinary local Git activity. Failed CI and missing project folders remain actionable exceptions. Report unread markers use the explicit acknowledgement snapshot, separately from fact freshness and report age.

All three original reviewers checked the implementation independently. Their concrete findings were corrected and then rechecked: main disclosures retain their open state; report-age explanations wrap; tasks survive an empty or archived project list; removed-project ideas require restoration before a new handoff; newly saved ideas reveal themselves in the appropriate filter. Search keeps the visible result snapshot and reconciles the selected item's stable ID or canonical content against current data. Insertion or reordering does not redirect a result to a different entry; missing or ambiguous results are reported instead. No reviewer claims live visual verification.

Validation: 143 Rust tests passed with desktop features and 143 without, both Clippy configurations passed with `-D warnings`, formatting passed, and 71 UI behavior tests passed. The operating Skill's executable recipes were also rerun after the documentation changes. These are service, real-process and UI-harness checks, not a new model usability trial.

A separate copy of the owner's database was used for the native-window check. Today, project voices, compact owner controls, handbook tabs and the handoff dialog rendered successfully. Copy-only left zero tasks; assign-and-copy saved exactly one task and the actual clipboard contained its ID and reporting command. A rating moved a proposal out of New and incremented the liked collection. All these mutations stayed in the disposable copy; production preferences, read baseline, project records, feedback, ideas, warnings and tasks were compared with the initial snapshot and remained unchanged.

The complete four-file operating Skill now measures 11,496 to 11,797 `o200k_base` tokens (+301) against `1783756`, using tiktoken 0.14.0. This includes the runtime follow-up above and owner-workflow documentation; it supersedes the runtime slice's +115 when reporting the total Skill change. There is no measured whole-run saving. UI labels and owner workflows changed, while agent command names and document schemas remained stable apart from the additive `ideas` view value and reserved owner-idea identity described above.

Both release executables were installed with `scripts/install.ps1` and match the built binaries byte for byte. All four Skill files match the repository, installed export and Nabu's existing private package. That package retained `agent:nabu` scope and had no independent local edits before replacement. Production owner data remained unchanged after installation.

The final installed-window inspection was stopped by the owner through the Computer Use Escape gesture. No further native input was issued. The earlier native-window observations above cover the pre-release build; the final search reconciliation, removed-project handoff guard and two-column ideas layout passed source/harness review but were not subsequently exercised in the installed window. The last launched window uses the isolated data copy; closing it and launching vWatch normally selects production data again.

## Reflection throughout the daily run, 2026-10-08

The owner clarified that idea discovery belongs after each project investigation and again after all findings have been collected. The earlier fresh run had retained every suggestion and synergy without adding a product idea; its retrospective confirmed that it concentrated on updating existing material. The old main recipe, run continuation and repair prompt allowed skipping synthesis when reports were current, while the card guide framed investigation as only what changes the card.

The revised workflow requires project reflection before each card save and a selection outcome for unchanged cards too. Workers return concise findings and selection reasons to the coordinator. The final synthesis considers all project summaries, including unchanged projects, and combines those with discoveries from the run. Evidence substantiates a need or existing capability; a proposed solution need not already be a TODO. Useful single-project proposals, cross-project combinations and standalone app ideas use the existing document fields. There is no novelty quota, reflection checkbox, new schema field or new owner-facing panel.

`runs start` now continues to portfolio context when no document is due, or to a recorded failed project when resuming that work. Its instruction and the recovery task describe the same two reflection stages. The untracked `due` check and run-completion freshness contracts stay unchanged. A current brief needs no artificial rewrite after synthesis, except after actually repairing a recorded failure: the existing failure contract requires a newer document version. `complete` does not attest that a model performed an idea search. The Skill requires the final reply to distinguish actually added ideas from retained items, or name a concrete considered direction and its exclusion reason. A project idea discovered during synthesis requires its own current context and a new portfolio context after that save.

Independent static review found no remaining workflow skips or contradictory runtime instructions. Before the fresh trial, acceptance criteria were recorded separately from its prompt: project reflection beyond retained suggestions, incorporation of unchanged projects and worker observations, correct new/retained identity, meaningful selection outcomes, and preservation of owner decisions and repository contents. The trial receives an ordinary daily-run request and the installed Skill, without those criteria, the earlier failure or a request to generate a particular idea.

Local token measurement against `a6c2436` uses `o200k_base` and tiktoken 0.14.0. The complete four-file operating Skill changes from 11,797 to 12,957 tokens (+1,160). The first trial used the intermediate 12,879-token version; its retrospective led to 78 further tokens distinguishing uncertain benefit from implementation questions. Run-start instruction and repair-task text, serialized together with identical JSON settings from real CLI calls on separate empty fixtures, change from 229 to 416 tokens (+187). Other CLI definitions and schemas are unchanged. These estimates exclude Provider wrappers and billing; loaded instructions recur in model context subject to host framing and caching. No whole-run token saving is claimed. Private measurements and the predeclared evaluation plans are in `%TEMP%/vwatch-ideation-pg86_yb_`.

The fresh daily trial completed production run 5: four due cards were rewritten, four current cards retained, and portfolio version 12 saved. All eight were current at completion, with no failed work. Three independently new proposal IDs were actually saved; eighteen existing idea IDs remained and two obsolete proposals left the current cards. Separate source review confirmed all three new product gaps and no duplication of prior open ends. One overly broad explanation was subsequently corrected through a guarded card merge: Needle Net's join recommendation applies after a postal code is present. This editorial follow-up is separate from the trial.

Project workers returned selection outcomes and cross-project observations; the coordinator requested a complete handoff when intermediate messages were fragmented. Trading Lab's considered direction already belonged in its reentry, so no duplicate proposal was added. The coordinator included all eight project summaries and investigated two cross-project candidates, but saved neither a new synergy nor a new app. Its retrospective acknowledged a weak rejection reason for one candidate: a missing common identifier was treated as a reason against usefulness, although that was an implementation question. The revised guide now explicitly separates those judgments and asks agents to consider needs beyond current project boundaries. This is a limitation found in the trial, not a claim that the first trial fully resolved synthesis quality.

The trial encountered no CLI errors or lost writes. An external vBot change prompted a fresh facts/context read before saving. The retrospective reported a ten-page, 70,338-character portfolio context, one avoidable extra JSON read to preserve a handbook, one truncated batched repository read recovered by targeted reading, and an ignored legacy effort warning whose inherited origin was unclear. Broad context compression and warning-provenance redesign remain outside this workflow change; they are not claimed fixed.

Production ratings, parked owner ideas, tasks, project intent, warning acknowledgements and explicit read baseline remained unchanged. During the live run, the window preference changed from Today to Ideas and produced its normal undo receipt; no settings mutation appears in the CLI metadata. That live selection was preserved, so blanket preference immutability is not claimed. Autostart and user PATH stayed unchanged. CLI metadata in the private evidence is deduplicated by call ID and includes coordinator verification reads; it is not isolated model cost.

Deterministic validation passed the Rust suites with and without desktop features, both strict Clippy configurations, formatting, the Skill validator and all 71 UI checks. After tightening failed-run continuation, the 12 run tests, 10 real-process tests and executable Skill recipes passed again. The all-current continuation and failed-project resumption are deterministic evidence; no fresh all-current daily-run trial is claimed. Fresh agents inherit the session's model/settings without overrides, and delegated reviews are parts of one trial, not independent reliability samples.

A second fresh agent received a normal request to update the cross-project overview and only the final installed Skill. This was a separate synthesis task, not a retry of the first daily-run trial. It read all ten context pages, considered all project summaries, and saved portfolio version 13 without errors or warnings. One new synergy connects confirmed work with an agreed follow-up about actual use. Its evidence names the benefit assumption and a concrete Morrow workflow for checking it; unresolved timing, channel and correlation details were not treated as disproof of usefulness. An additional integration and a separate app were considered but not proposed, with specific need/duplication reasons. Existing ideas retained their IDs. A final state comparison confirms four new ideas across the two trials: three project suggestions and this synergy. No owner task or follow-up was scheduled by either trial.

The second retrospective found the workflow understandable and distinguished benefit questions from implementation questions. It still found ten pages costly, made one unnecessary `projects list` call, and marked two outdated cards instead of treating them as freshly verified evidence. Both trial attempts and the first trial's synthesis limitation remain part of the record. These are two observed tasks on changing live data, not a controlled before/after comparison, a capability-selection evaluation, or a broad model-reliability claim.

The final executables were installed and match their release builds. All four exported Skill files match the source and Nabu's existing private package; no independent private edits were overwritten and its `agent:nabu` scope was retained. The ordinary production window was reopened. No UI behavior changed in this follow-up, and no new native-window interaction test is claimed.

## Complete agent-interface review, 2026-10-10

The owner requested direct improvements across the CLI, operating Skill and runtime agent text. The review started at `5bdfdf2e864a6b351a865aa9c2cdb7e518776866`. Three reviewers covered command parsing/help, output/recovery and service/schema/runtime behavior; the coordinating review covered the four Skill files, metadata reads and copied task text. A separate cross-review found one missing no-change statement in report-file errors, corrected before final verification. All 29 desktop endpoints remain accounted for: 23 covered and six documented exclusions.

The review exercised 62 leaf help pages and inspected 71 actual readable outputs across their renderer families. Help now describes omitted values, exact token/revision sources and command effects; file-only commands no longer suggest unsupported inline JSON. Multiline task reports use `--file`. Full table values, final-page workflow continuations, early task revisions and resulting remove/restore states reduce follow-up reads. Stored notes and copied task context are distinguished from instructions. Duplicate IDs and nonnullable nulls reject a write rather than silently changing its meaning, and schema-growth detection checks every item.

One initial executable-Skill test returned an unknown-worker result and passed when repeated. Subsequent code inspection identified a real race between checking the saved result and worker liveness. A deterministic test reproduces completion in that interval; the reader now checks the durable result again. The initial fixture was already deleted, so this establishes the race and its fix without asserting that it caused that particular test failure.

The operating Skill retains both reflection stages and owner authority, targets investigation by the due reason, includes staged changes and clarifies capability questions versus project-state reads. The description remains 218 characters. The Skill validator reports no findings: 174 body lines and three reference files totaling 339 lines. Exact text changes are available through `git diff 5bdfdf2 -- skill/vwatch src/cli.rs src/cards.rs src/service.rs ui/app.js`. Review accounting retained 292 of 335 nonblank Skill source lines, 75 of 103 help-text lines and 152 output prose templates; counts include code/table source units and are not linguistic sentence counts. The four Skill files measure 12,957 → 12,820 `o200k_base` tokens (−137, −1.1%); the main body grows from 3,227 to 3,265 while repeated reference prose shrinks. Measurements use tiktoken 0.14.0 and exclude provider framing; no whole-run cost saving is claimed.

Fresh-agent trials used Codex exec with `gpt-6-luna`, normal host configuration, a separate fixture database and executable per run, and the Skill installed by the evaluation harness. Goals and end-state criteria were written before dispatch. All four attempts loaded the Skill; none used production data.

| Goal | Attempt | Result from saved state | Tool calls / planned |
|---|---:|---|---:|
| Rename a watched project | 1 | Pass: stable ID, card, goal/date and task retained | 5 / 3 |
| Record a multiline Unicode report | 1 | Fail: review state and content correct, but line breaks flattened | 6 / 4 |
| Explain unsupported repository cleanup | 1 | Pass: accurate boundary, no mutation | 4 / 1 |
| Record the same multiline report on a fresh fixture | 2 | Pass after recovery: exact quotes, Unicode and newlines saved; no owner confirmation | 4 / 4 |

Extra calls remain visible: rename read the reference and top-level help; the first report listed/showed an already known task and read two help pages; the capability question read project state unnecessarily. The second report still listed the known task and recovered from a PowerShell quoting error; combining file creation and the write made the aggregate call count look ideal despite those extra operations. The main Skill now places the file recipe before the short inline alternative, distinguishes recording a supplied report from executing work, and names the literal here-string syntax that avoids the observed quote error. That final syntax clarification was not followed by another model trial. The failed attempt remains part of the record; these trials do not establish daily synthesis quality, production vBot behavior or general model reliability.

Private goal files, fixture state, textual review diffs and end-state checks are under `%TEMP%/vwatch-interface-review-34b629028ce14ef99a2c86b1869f5ffd`; raw trial transcripts are under `%TEMP%/eval-runs-l02a2rjx` and `%TEMP%/eval-runs-k3asflkt`. They contain test content, not owner documents. This review does not install executables or alter any installed Skill scope. Copied task text was exercised in the UI harness; no native-window interaction is claimed.

Final verification passed: 147 Rust tests with desktop features and 147 without, strict Clippy for both configurations and all targets, formatting, 72 Node UI tests, and both Skill validators. The native executable recipes passed in both full suites after the worker-race correction. The work remains a source change on `main`; release installation and any existing private Skill deployment are separate from this audit.


## Ideas and recommendations, 2026-10-17

The owner requested that feature and product ideas have their own place alongside actionable recommendations. Validation passed 166 Rust tests with desktop features, 166 without them, strict Clippy for all targets and 86 Node UI tests. The real release CLI on a separate copy of the owner database returned 16 ideas and 44 recommendations as disjoint, complete sets, with readable paging and classification requests in both contexts. The native test window displayed both categories and the recommendation-only rating round. No test ratings or assignments were written to the owner database.

The Skill checker reported: `vwatch: description 218 characters, body 179 lines, 3 other files (365 reference lines); no findings`. Nabu's existing private package was updated in `agent:nabu` with no sharing grants; its independent recovery and input-format notes were retained. This verifies application behavior and instruction consistency, not future model ideation quality; a fresh-agent daily-run evaluation remains a separate check. Existing items retain their IDs and initial dates, and missing categories request review on the next run.

## Lean contexts, kept items and idea reviews, 2026-10-20

After completing production run 18, the agent reported its friction: the portfolio context was 142,734 characters of text (20 pages), mostly full card details of projects the previous brief already covered (81k JSON characters) and full retained proposals (37k). Keeping an unchanged list item required copying its whole text; an invented ID for a new brief pattern was accepted silently; receipts did not say which items were new; quiet projects never became due, so their ideas aged; open questions could only be found by reading every card. The owner asked to fix these points and to improve idea quality.

This reverses the 2026-10-08 decision against broad context compression, at the owner's request, without dropping evidence: a card the previous brief already built on (not outdated, `changed_since_brief` empty) appears as a summary with item IDs and titles, and `portfolio context --project <id>` (repeatable) returns full rows under the same context token. Weekly-review evidence appears only while the review is due. Retained proposals in both contexts are one-line reminders; `proposals` has their text. On a copy of the owner database the portfolio text fell from 142,734 to 96,209 characters with four projects still outdated, and the vBot context from 40,533 to 35,949; once a run has saved its changed cards, more projects become summaries.

- An item sent as `{"id": ...}` alone takes its latest saved wording from any version of the same document, so unchanged and retained items cost one line. An unknown ID alone or one of another kind rejects the write; an unknown ID with content is saved as new with a warning.
- Save and dry-run receipts give each item's change: `new`, `kept`, `edited` or `restored`.
- Persistence rows add `unchanged_days`, the days since an item's current wording appeared; the Skill's seven-day rule uses it so a reworded question is not reworded again the next day.
- Due reason `idea_review`: two active projects per day, in rotation over registration order, whose card is at least seven days old and otherwise current. The rotation needs no stored state and is stable within a day, so saving one card never makes another due; a run started before midnight ignores the next day's rotation.
- `questions` lists open owner questions across cards with options and the recommended answer; snoozed questions are counted.
- Card field `usage` (summary, last use, evidence) records how the owner actually uses a project, shown as "Im Alltag" on the project page and part of the brief's basis. The Skill allows reading that project's data and logs outside the repository for usage traces, without copying personal content, and starts ideation from actual use with explicit lenses (before and after, making the app unnecessary, borrowing from other projects or proven products, extreme use, removal, unused capabilities). New app ideas start from the owner's days rather than the existing apps.

Validation: 169 Rust tests in both configurations, strict Clippy for all targets, formatting and 86 Node UI tests passed. The real CLI on a scratch copy of the owner database produced the summaries, per-project detail, keep-by-ID receipts (21 kept, 1 new), the unknown-ID warning and error, and the questions list. These deterministic checks do not measure idea quality; that needs a fresh-agent daily run.

