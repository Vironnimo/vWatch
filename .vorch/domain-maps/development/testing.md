# Testing Watchtower

Read this when writing or changing a test or investigating a test-run failure. The development root map owns build, installation and Git tooling; this file owns suite selection, isolation and timing.

## Run the relevant contracts

Use the Python version in `.python-version`; the runner uses the exact Rust toolchain in `rust-toolchain.toml` and requires the Node version in `.node-version`. All are existing local tools. No package installation or additional Python/Node dependencies are involved.

```powershell
python scripts/test.py
python scripts/test.py --suite rust --no-default-features
python scripts/test.py --suite rust --filter owner_undo
python scripts/test.py --suite ui --filter "assignment"
python scripts/test.py --stress
```

The default suite is `all` (Rust and UI). `--suite rust` or `--suite ui` narrows it. `--filter` is a case-sensitive substring of a Rust `target::test` name or a Node test name; it does not select UI files. `--no-default-features` removes desktop features only from the Rust build. `--stress` includes stress cases in the selected suite; normal and push runs omit them. A selection that executes no tests fails.

`scripts/check.py test` runs the full default suite, the Rust suite without desktop features, and the Python development-tooling contracts. `scripts/check.py static` performs static checks without application tests. Follow AGENTS.md for when those checks belong in the workflow.

Rust tests build once with locked, offline dependencies in the primary checkout's shared `target/`. The runner discovers Cargo's test executables and libtest names, then executes each exact case in its own process. It uses at most four workers, one libtest thread per process, and a hard 30-second deadline per ordinary case. The one named aggregate `skill_commands::every_cli_recipe_in_the_operating_skill_runs_with_real_arguments` has a 120-second budget: it executes every documented CLI recipe, repeated fixture setup and the PowerShell forms sequentially inside one libtest case. This is a multi-command contract budget; individual test deadlines stay at 30 seconds. Tests cannot share process globals. Cargo build/discovery retains the developer's tool caches; test processes receive the isolated environment below. There are currently no Rust documentation tests; the runner covers libtest unit and integration tests. If executable documentation examples are added, extend the runner to include them.

UI files run in separate Node processes with `--test-concurrency=1`, a 30-second native per-test timeout and a 120-second process deadline. At most four Rust cases/UI files run at once. Files matching `tests/ui*.cjs` are included except the shared `ui-helpers.cjs`. The UI fixtures use a fresh VM and state for each `app()` call; HOME isolation is per UI file, not per test. They do not start a browser or the installed application.

Each run keeps logs and `report.json` in the current checkout's `.tmp/test-runs/<unique-run>/`. The report lists every selected case's outcome and duration. The console shows the 20 slowest cases. Rust durations include process startup/teardown; Node durations come from TAP's `duration_ms` and describe the test body. Do not compare those measurements as identical quantities. Build/discovery output is retained separately from test results. Failed test logs are linked in the console.

## Fixture isolation and cleanup

Test data lives outside the repository and the owner's profile: `<primary-checkout-parent>/.vorch-test-data/<primary-checkout-name>/<unique-run>/`. For the usual checkout this is `C:/Development/projects/.vorch-test-data/watchtower/`. Set the process-local `VORCH_TEST_ROOT` to another directory when that location is unsuitable; the runner rejects roots inside either checkout or the current HOME/USERPROFILE. Reports, compiler scratch and source-run scratch remain in `.tmp/`; only application-test fixture data uses this external root.

Every Rust case, UI file and Rust discovery process receives its own fixture directory. `isolated_environment` in `scripts/test.py` sets:

- Empty HOME and USERPROFILE, isolated APPDATA and LOCALAPPDATA, and no XDG_CONFIG_HOME.
- TEMP, TMP and TMPDIR under its fixture directory, plus a safe WATCHTOWER_HOME and the explicit WATCHTOWER_TEST_INSTANCE opt-in.
- An empty GIT_CONFIG_GLOBAL, disabled system Git config and prompts, and GIT_CEILING_DIRECTORIES at the temporary-directory boundary. All inherited GIT_* variables, including author/committer identity and index/config overrides, are removed first.
- No inherited WATCHTOWER_* fault switches, NODE_OPTIONS, NODE_PATH or EMAIL.

`tests/common/mod.rs::make_repo` configures test-only Git identity in each newly created repository and disables commit signing. Its `git_at` helper supplies explicit dates. Tests must not rely on the owner's Git config. Existing shared helpers use `tempfile` under the runner's per-case TEMP and explicitly pass database/repository paths.

On Windows a waiting launcher is assigned to a kill-on-close Job Object before starting the test. Descendants, including detached CLI job workers, stay in that job. A timeout terminates the whole tree; a case that exits while descendants are still running fails and cleans them up. This catches process leaks, not every possible resource leak. Tests must still close database connections and remove their own runtime resources. On other platforms process groups provide teardown; the Windows-specific process contracts still require Windows.

Ctrl+C cancels queued cases and signals active workers to terminate their process trees. The run exits with code 130, saves a failed report with explicit `cancelled` outcomes, and retains its fixtures. Cancellation never reports unfinished work as passed.

Successful runs remove only their verified, uniquely created external fixture directory. Failed runs retain it for diagnosis and name it in `report.json`. Logs remain in `.tmp/` for both outcomes. Never remove an entire configured test root to clean up one run. The runner does not alter the owner's installed database, preferences, autostart, PATH or Git identity.

## Test ownership and assertions

Extend the existing owner/behavior test before adding a file. `tests/common/mod.rs` owns reusable Rust setup; `tests/ui-helpers.cjs` owns the UI VM, fake invoke bridge, clipboard and controlled promises. Rust integration files cover distinct service, store, facts, CLI, process, recovery and history contracts; unit tests near the source cover their local owners. UI files group overview, project, idea, handoff and statistics behavior. When a large file becomes hard to navigate, split by behavior within its owner instead of creating one file per fix.

Assert exact strings for stable CLI continuation tokens, protocol/envelope fields, persisted formats, accessibility labels and forbidden internal output. For editable help, advice and errors prefer structured codes, required fields, types, state transitions and meaningful visible outcomes. A broad HTML snapshot or a full prose copy makes editing expensive without protecting more behavior.

Service tests use `common::service` and its injectable `Clock`, starting at a fixed UTC instant, with CI lookups disabled. `Clock::advance` changes time without waiting. Git fixtures use fixed commit dates. UI `app()` supplies fake timers, a VM-local clock when needed and `deferred()` promises for explicit completion order. Patch those local seams; never replace process-wide timers or sleep functions.

Real subprocesses in `tests/process.rs`, `tests/jobs.rs` and the executable contract tests are deliberate: stdout/stderr, exit status, shell waiting, deadlines, detached work and recovery cannot be proven through service calls alone. Existing short polling waits coordinate those processes. Application sleeps/retries currently do not all have injectable seams; do not describe them as mocked. Add a local seam or explicit synchronization before adding a test that depends on elapsed time. Do not stabilize failures by raising deadlines. A test above about a second needs a behavioral reason; investigate any test at five seconds or more using the duration report, and rewrite or remove avoidable waiting.

## Stress and durable I/O

Rust stress tests use both a function name starting with `stress_` and `#[ignore = "stress"]`. The runner verifies the name prefix against libtest's ignored-test list. An ignored test without that prefix, or a stress-prefixed test without `ignore`, fails discovery rather than silently disappearing. The ignored-reason string is documentation; libtest's stable listing does not expose it, so the runner cannot validate the spelling of that string. `--stress` passes `--include-ignored` only to explicitly discovered stress cases.

UI stress tests use a top-level test name beginning with `stress: `. Node's skip pattern excludes those names by default; `--stress` removes it. Use `node:test`'s documented options, not an invented marker option. Neither suite currently contains stress cases, and there is no general custom marker registry.

All database tests currently use real SQLite synchronization. `src/store.rs::open` enables WAL and does not expose a sync-off seam. The runner deliberately preserves that production behavior; no environment variable can disable database durability. This is an explicit exception to Vorch's generic sync-skipping setup. Existing recovery, reopen and concurrency tests establish transactional/restart behavior, not simulated power-loss guarantees. There are no specially marked durable tests today. If sync costs justify an optimization, add a narrowly owned, test-only seam and corresponding real-sync contract tests before introducing a durable marker. Do not add a production-wide `synchronous=OFF` switch for test speed.

## Windows traps

- The runner launches Cargo-built executables directly so Cargo cannot overwrite per-case TEMP through `.cargo/config.toml`. Running raw `cargo test` bypasses the isolation, timeout and duration-report guarantees above.
- CLI process tests must exercise the console executable, never the GUI binary. Shell and job tests need their actual process boundaries; a mocked command is not equivalent coverage.
- WAL files and executable handles can stay locked while a child process is alive. Close and wait for the owner before deleting a fixture; a forced timeout is a failed run, not permission to ignore a lock.
- Temporary folders without a `.git` must not discover the checkout's parent repository. Keep the Git ceiling and explicit fixture paths when extending test launchers.
- UI VM tests validate behavior with a small fake DOM. They do not establish native window layout, focus behavior in WebView, or Windows integration. Verify relevant UI changes in a scratch running window without changing the owner's settings or feedback.
