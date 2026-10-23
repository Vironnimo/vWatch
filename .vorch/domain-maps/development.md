# Development

This domain owns building, verifying, committing and installing Watchtower without disturbing owner data or concurrent work.

## Overview

`Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`, `build.rs`, `tauri.conf.json` and `capabilities/` define the build. `scripts/` and `.githooks/` own development tooling; `tests/common/mod.rs` and `tests/ui-helpers.cjs` own shared application fixtures. Behavior tests belong to their application domains.

Cargo uses its committed lock file. `desktop` enables Tauri and the GUI executable; `custom-protocol` embeds the static frontend. The console builds without default features. No npm installation, frontend bundler or local HTTP server is needed. Runtime versions and common commands live in PROJECT -> Development and Testing.

## Commit hook

Enable hooks with repository-local `git config --local core.hooksPath .githooks`. `.claude/settings.json` also enables them at session start and supplies the confirmed author identity. Other clients use repository-local Git identity; no script changes global Git configuration.

`pre-commit` and `pre-merge-commit` check identity and call `scripts/commit_check.py`; `commit-msg` strips AI attribution. Its report starts `Commit check`. The checker exports the index into a unique scratch snapshot, so partially staged content is checked independently of the working tree. It checks Python/JavaScript syntax, Rust formatting, structured Clippy diagnostics and domain maps. It does not format, modify or stage source. Every compiler error in the staged snapshot blocks: even an unchanged caller can be broken by the staged change. Unstaged working-tree content is absent from that snapshot and is neither inspected nor rewritten. A tool failure without diagnostics also blocks.

Runtime pins are checked before tools execute. Checks never install packages; Cargo uses `--locked --offline`. Missing tools/cache are setup failures, not permission to bypass checks. Full static checks cover both desktop and core-only configurations. Historical authorship findings are reported without rewriting history.

## Worktrees and build state

`scripts/worktree.py` owns Vorch task checkouts and `codex/` branches. Markers establish ownership; setup checks tools without linking checkout directories. Merges serialize, prepare and check a merge in a private landing checkout, then fast-forward primary `main`. Uncommitted primary changes survive unless they overlap, in which case landing fails.

Cleanup validates owned paths and refuses directory links. A successful merge can retain its task checkout and branch when ignored data remain: inspect them before explicitly discarding anything. Failed landing state is retained for recovery. Read the worktree workflow before lifecycle operations.

Builds use the primary checkout's single `target/` through `--target-dir`; Cargo serializes cache access. Build artifacts can contain another checkout's last build, so invoke Cargo for the intended checkout before launching. Logs, scripts, index snapshots and live-instance data use checkout-local `.tmp/`; test fixture homes/repositories use the external root in the testing guide.

Codex-managed worktrees use the app's attachment/archive lifecycle. Do not add a Vorch marker just to remove one through a different manager.

## Local checks and installation

There is no remote or CI. `scripts/push.py --no-push` verifies an immutable commit in a private checkout; it never fetches or publishes. `scripts/check.py all` checks the working tree. The local verification workflow describes failure handling and exact checked revisions.

`scripts/install.ps1` resolves the shared primary target through `check.py target-dir`, passes it explicitly to Cargo and copies both release binaries from that same directory, including from worktrees or with inherited Cargo target settings. It preserves installed data/preferences and refuses to interrupt an active console. It closes the GUI for replacement, exports the Skill and updates the Start menu. Runtime OS integration belongs to desktop; release and live-test procedures are workflows.

## Constraints & Gotchas

- Scratch application data do not isolate registry, user PATH, Start menu or installed Skill scopes. Leave owner choices intact.
- Node VM tests cannot establish native layout, focus or WebView behavior.
- Timeouts/duration reports are harness evidence, not fresh-agent evaluation or power-loss durability evidence.
- `scripts/domain_maps.py` reports budgets/routing; static checks fail on blocking findings. Budget warnings require placement review.

## References

- Writing/changing tests or interpreting a timeout/survey -> `development/testing.md`
- Creating, merging or removing task checkouts -> `.vorch/workflows/worktree-workflow.md`
- Checking an immutable commit or responding to a push request -> `.vorch/workflows/push-workflow.md`
- Installing binaries or updating installed Skills -> `.vorch/workflows/release-workflow.md`
- Verifying the native window or console -> `.vorch/workflows/live-testing-workflow.md`
- Changing maps -> `.vorch/workflows/domain-map-workflow.md`
- Auditing map placement/routing -> `.vorch/workflows/domain-map-audit-workflow.md`
- Auditing suite cost and contracts -> `.vorch/workflows/test-audit-workflow.md`
- Auditing module boundaries and large owners -> `.vorch/workflows/deep-module-audit-workflow.md`
