# Live Testing Workflow

Use after UI changes or for console/window contracts deterministic tests cannot establish.

## Prepare an isolated instance

Read PROJECT -> Development for scratch and build setup. Select a task-specific `WATCHTOWER_HOME` below checkout-local `.tmp/` before mutation probes. Use generated fixtures or a consistent SQLite backup for realistic data. Copying only a live WAL database file can omit transactions; close source connections or use SQLite's backup API.

Shared build output can contain another checkout's last build: build from the intended checkout before launching. Scratch data do not isolate registry, PATH, Start menu or installed Skill changes; leave those owner choices alone.

## Window changes

1. Run affected Node UI tests. They check state/interaction, not native layout.
2. Start the isolated GUI and verify the changed owner journey in the actual window. Single-instance handling can focus an existing window; verify the instance before test decisions.
3. Check relevant empty/unknown states and recovery. For asynchronous changes, preserve drafts, focus and the displayed read snapshot while data reloads.
4. Confirm no test ratings, replies or ticks reached the owner's instance. If owner data was used for setup, compare the relevant read-only state afterward. Do not change saved display preferences merely to reach a screen.
5. Close the test window and reopen the owner's normal window if testing/installation closed it. Keep screenshots/logs in task scratch.

If native-window control is unavailable, state that limit. A mock DOM or browser rendering is not native verification.

## Console changes

Use the real console executable against isolated data. Check affected success/recovery cases through stdout, stderr and exit status; include PowerShell/Git Bash and Unicode when relevant. Follow exact continuations rather than repeating a possibly completed mutation.

Faults require the opt-in in `docs/agent-interface.md` -> Entry points and test instances; the installed default instance ignores them. Process checks do not imply fresh-model evaluation.
