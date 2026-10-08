# Agent interface

Watchtower's console CLI and desktop window call the same service. People and agents read the same ordinary text; programs explicitly request complete JSON. The operating Skill teaches the daily observation run and user-requested owner actions without database access, GUI automation or a mandatory discovery call.

This document records capability coverage, call sequences, recovery contracts and validation scope. Update it with the CLI, desktop handlers and `skill/watchtower/` when those contracts change.

## Entry points and test instances

The installed console entry point is `%LOCALAPPDATA%/Programs/Watchtower/Watchtower.exe`; `Watchtower-UI.exe` is the independent desktop entry point. Production agents use shell execution in vBot or Codex. The operating Skill gives PowerShell and Git Bash forms. The CLI works with the window closed and never attaches a console to a GUI process.

Build the console without desktop dependencies:

```powershell
cargo build --no-default-features --bin Watchtower
```

Use an absolute scratch folder for development calls. This example reads an isolated instance from the repository root:

```powershell
$env:WATCHTOWER_HOME = Join-Path $env:TEMP 'watchtower-agent-interface'
& ./target/debug/Watchtower.exe projects list
```

Git Bash equivalent:

```bash
WATCHTOWER_HOME="$LOCALAPPDATA/Temp/watchtower-agent-interface" ./target/debug/Watchtower.exe projects list
```

The global `--data-dir <folder>` overrides `WATCHTOWER_HOME`. Non-default instances identify themselves in text and JSON. Printed continuations preserve the selected instance through an opaque `--instance` locator; copy them unchanged. The locator is an encoded path, not a secret or permission grant.

Job fault injection requires both a non-default data folder and `WATCHTOWER_TEST_INSTANCE=1`. Tests can set `WATCHTOWER_TEST_JOB_DEADLINE_MS` (20–45000 milliseconds), `WATCHTOWER_TEST_JOB_DELAY_MS`, `WATCHTOWER_TEST_JOB_CRASH=1` or `WATCHTOWER_TEST_JOB_BLOCK=1`. The blocking switch waits for `.release-test-job` in that scratch folder, with a 30-second cap. Production's default instance ignores these switches. Tests in `tests/process.rs` and the job test module create isolated fixtures; do not enable faults for the installed owner's instance.

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
| `set_pref` | `settings update --style` / `--hidden-modules` | Covered; both accepted display preference keys |
| `set_status` | `projects status` | Covered |
| `set_goal` | `projects goal` | Covered |
| `add_idea` | `ideas add` | Covered |
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

Twenty-three of the twenty-nine endpoints have CLI equivalents; six are explicit presentation or OS-setting exclusions. There is no uncovered persisted owner-state operation in this inventory.

The screen/event-handler pass covers behavior outside those endpoints:

| Screen or handler | CLI access | Boundary |
|---|---|---|
| Handover, board, handbook and project drawer | `overview`, `projects show`, `card show`, `facts show` | Stored content is available; layout and opening a drawer are presentation |
| Search and project navigation | `projects list --query`, project/card views | Search accepts ID, name, path or card text; CLI does not manipulate UI focus |
| Decision round and focus completion | Card/brief views plus feedback commands | Ratings/replies represent the owner's request, never an invented agent decision |
| Week review, overnight activity and space overview | `portfolio show`, `overview`, `facts show` | Same stored brief and measured facts; no cleanup operation |
| Copy task / handbook command / settings text | Task, card, project and settings views | Source text is available; clipboard mutation and exact task-prompt layout remain window presentation |
| Export embedded Skill | `status` | Calls the same export as desktop startup; reports the exported folder |
| Skill installation command | `settings show` | Exposes the install command; installation itself belongs to the agent application's installer |

The CLI also exposes agent-only operations: measured refreshes, guarded card/brief writes, schema reads, run tracking, task reports, document history, trash restoration, saved output, jobs, usage and recovery activity.

## Call sequences

Counts below are protocol design targets confirmed by command/contract tests, not measurements of fresh-model behavior. They exclude reading repository evidence and Skill references. A page or unfinished job adds its necessary continuation; it does not repeat the underlying operation.

| Goal | Sequence | Calls without pagination or waiting |
|---|---|---|
| Read the morning overview | `overview`; add `--refresh` when fresh measurement is requested | 1 |
| Rename/reconnect a known project | `projects update <id> ...` | 1 |
| Find and rename a project | `projects list --query <text>` → `projects update <id> ...` | 2; selection is needed only when the ID is unknown |
| Register several selected folders | `projects add <path>...` | 1 |
| Answer a known question | `feedback set <item-id> --value answered --note ...` | 1 |
| Write one current card | `context <id>` → study evidence → `card put <id> --context-token ...` | 2 |
| Daily run with N due cards and a due brief | `runs start` → N context/save pairs → portfolio context/save → `runs finish` | `2N + 4` |
| Daily run with nothing due | `runs start` → `runs finish` | 2 |
| Report assigned work | `tasks report <id> --status done --result ... --evidence ...` | 1; owner review remains separate |
| Confirm a known current task result | `tasks confirm <id> --revision <revision>` | 1 |
| Correct an immediately preceding owner edit | Printed `Undo:` command | 1 where the result provides undo |
| Recover a lost response | `activity` → relevant saved job or current-state read | Depends on whether a completed result was captured; never a blind replay |

There is no mandatory status/preflight call, dry run before a save, or verification read after a successful mutation. A save returns its result and warnings. First cards use a complete document without `--merge`; incremental updates use `--merge` and preserve whole nested fields and list item IDs.

Common noun/verb aliases and case-insensitive enumerated values tolerate clear spelling variations. Unknown or ambiguous targets fail with candidate/recovery information instead of mutating a guessed project. The parser owns accepted aliases; `--help` is the command index rather than a second hand-maintained list in the Skill.

## Output and error contracts

Plain output is the default regardless of redirection. Its first line states the outcome; lists state their scope, order and total. `Note:` explains limitations or partial effects, `Next:` is an executable continuation, `Error:` identifies failure, and `Undo:` names a supported reversal. Stored content is escaped and indented so text inside an item cannot forge a top-level marker.

Card and brief writes return concise readable receipts: saved version or replay/preview state, context token, warnings, card field changes/omissions, item IDs and guarded Undo. They do not echo the submitted document. Explicit JSON and document `show` retain the full content; large sets of item receipts still use normal paging.

Portfolio context carries each card's version and actionable finding evidence (details, sources, question options, suggestion effort and completion criteria). Measurement validity, errors and collection time remain visible; missing branch counts remain null. Per-project `review_week_evidence` separates strictly week-bounded changes/milestones from current card summaries. The review's commit count is selected relative to the stored measurement's calendar week and remains null when that sample cannot establish the completed week's count. These are additive JSON fields except for correcting previously fabricated zero or misdated counts.

Ordinary responses, including oversized errors, fit within 10,000 Unicode characters including instance framing. The pager limits each saved page to 8,500 characters, reserves room for the instance note and bounds the page body at 7,500 characters. A complete immutable snapshot is stored locally; `output <token> --start <offset>` reads it by zero-based Unicode character offset. Snapshots never rerun writes or remeasure facts. Failure to persist output reports that the original operation has already run and directs the caller to current-state inspection.

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

Stderr was empty in every sample. The capture is `%TEMP%/watchtower-rename-cli-work/agent-interface-output-samples.json`; these measurements characterize the sampled fixtures, while the contract tests exercise larger and adversarial content.

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
| Repeated write/report, trash restoration, stale undo | Avoid duplicate effects, preserve confirmed task results and history, reject intervening affected-field changes | `tests/effects_recovery.rs`, `tests/owner_undo.rs`, `tests/cli_parity.rs`, `tests/ui.cjs` |
| Metadata privacy and incomplete activity | Omit values/content from usage; distinguish unfinished or unknown from failed/unchanged | `tests/usage.rs`, `tests/recovery_contract.rs` |

## Effects, replay and recovery

- Registration changes retain stable project IDs. Renames and path updates preserve cards, reactions, tasks and history and do not move repository files. Invalid combined updates change neither field. A saved path edit can outlive a failed facts refresh; output names that partial effect and the refresh recovery.
- Removed projects and ideas are hidden, not purged. Trash and restore preserve original IDs and records. Card/brief history restoration appends a version with its original evidence basis; restoring wording does not claim a fresh analysis.
- Card/brief writes require the context that was studied. A changed document, fact or owner instruction rejects a stale payload. An identical request with its previous token replays a durable saved result only while the post-write context still matches; a different payload or intervening change fails. Agents reread and reconcile, not merely replace a token.
- Owner edits to names/paths, status/notes, goals/dates, feedback, warning acknowledgements and display preferences return guarded undo receipts. Undo restores exactly the affected fields, rejects intervening changes to them and preserves unrelated changes. Folder undo discards facts from the other folder and returns a refresh continuation. Ordinary project/idea trash inverses are not blanket revision guards; preserve later owner changes before applying them.
- Repeated registration of the same path retains the existing record when supplied identity details agree. Identical active ideas and tasks are deduplicated. Run start coalesces an unfinished run; an identical failure report retains its original failure basis. No-op changes do not emit a destructive inverse.
- Task reports record evidence and request review. Only an explicit owner decision confirms completion, reopens or cancels a task; those transitions require the current revision. Daily observation does not execute assignments or fabricate ratings and answers.
- Reads do not acknowledge the changes digest. `changes read` refers to the exact snapshot token. Visits are explicit and separate. Deliberate warning acknowledgements preserve measured facts and expire on material worsening.

## Privacy and metadata

All state, snapshots, worker files and usage metadata stay in the selected local data folder. There is no telemetry. Repository observation remains read-only; `gh` reads CI state over the network.

`cli-usage.jsonl` records parser-owned command/flag names, target counts, UTC timestamps, process/parent IDs, elapsed milliseconds, exit status and output character counts. It never records argument values, project IDs, paths from arguments, document text or output content. Logging is best-effort and does not turn a successful write into a reported failure. Usage reports inspect the last 20,000 log events; the append-only file is not an automatic deletion policy. `usage --since` filters inclusive UTC dates.

`activity` returns the most recent 20 recorded calls. A recorded exit code describes that command, not the completeness of a daily agent run. Missing completion metadata does not prove that a mutation had no effect. Parent process IDs are grouping hints, not verified model sessions; repeated command shapes can involve different values or targets.

Job request/result files and output snapshots are different from usage metadata: they retain arguments, instance/working-directory paths and result text needed for recovery. Treat those files like the private database. Do not attach unredacted snapshots, worker requests or owner documents to an external report. No model-session corpus is collected by Watchtower.

## Runtime text and its reasons

| Surface | Constraint and reason |
|---|---|
| Skill description | Select operating Watchtower, not coding its repository; avoids taking over unrelated tasks |
| Skill calling/output sections | Default text, exact shell fallback and continuations; avoids forced JSON, late-output polling and repeated mutations |
| Skill authority rules | Daily observation versus explicit owner changes; prevents invented decisions and unintended repository edits |
| Skill card/brief recipes | Context reconciliation, first-card behavior and whole-field merge semantics; prevents stale overwrites and missing list entries |
| Field guides | Every schema field, evidence and word limits; avoids invented payloads and generic filler |
| Feedback/task rules | Stable item IDs, rejection/snooze semantics and reported versus confirmed completion; prevents repeated unwanted suggestions and false completion |
| Errors and partial-effect notices | What failed, what remains saved and the available recovery; prevents blind retries and database recreation |
| Output data framing | Stored text cannot forge `Next:`/`Undo:` instructions; prevents records from acquiring command authority |
| Generated task prompt | Reports results through the CLI and leaves completion confirmation to the owner |

The complete source Skill measured 12,887 → 11,033 `o200k_base` tokens, a reduction of 1,854 tokens (14.4%). The main body changed from 3,407 to 2,806 tokens while adding recovery and owner-operation guidance. `AGENTS.md` changed from 2,668 to 2,908 tokens: the 240-token increase records owner authority, recovery ownership and the on-demand interface reference. README changed from 1,289 to 1,480 tokens to explain continuations, explicit JSON and restoration. Keep shared rules in their owning surface rather than repeating them in every prompt.

Static trigger review used five requests that require this Skill: a Watchtower daily run, a card update, a watched-project rename/reconnection, rating a displayed suggestion and reporting a Watchtower assignment. Five neighboring requests do not require it: implementing Watchtower Rust code, fixing another repository's CI, unsaved brainstorming, renaming a Codex project and generic Windows PATH configuration. These are static scope checks, not model trigger measurements.

The exact before/after Skill text is preserved in the repository, with review baseline `479455e27c59a77c54ddbfcc29c86f324cbe8c7c`. The after text is the source in `skill/watchtower/`; do not duplicate the four complete documents in this report. Review their changes from the repository root:

```powershell
git diff 479455e -- skill/watchtower/SKILL.md skill/watchtower/references/cli.md skill/watchtower/references/card-guide.md skill/watchtower/references/portfolio-guide.md
```

## Validation and release evidence

Contract suites cover service behavior, readable/JSON output, real native process streams, PowerShell/CMD/Git Bash behavior, Unicode input, closed stdin, output continuation, deadlines, recovery, schema errors, history preservation and guarded undo. `tests/skill_commands.rs` extracts command lines from every Skill Markdown code block and runs them with isolated real fixture values so new examples enter the checks automatically.

Run from the repository root:

```powershell
cargo test --no-default-features
cargo test
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
node --test tests/ui.cjs
cargo fmt --check
```

The Skill review verified 63 actual command-help calls, and its extracted PowerShell recipes passed against isolated data. The Bash fallback is normalized to the same executable in that recipe test; the process suite separately checks actual Git Bash argument and Unicode behavior. Independent static review corrected the first-card merge recipe, per-kind feedback values, snooze default, response-field distinctions, undo scope and CI-state coverage. The provided Skill checker returned: `watchtower: description 218 characters, body 159 lines, 3 other files (308 reference lines); no findings`.

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
