# Local Verification Workflow

Watchtower is local-only, with no remote or CI. The Vorch push entry point verifies immutable commits; it cannot publish. Do not add a remote to make it run.

## Check the intended state

During development, `python scripts/check.py all` checks the working tree, including uncommitted files. For completed main, run:

```powershell
python scripts/push.py --no-push
```

Add `--ref <commit-ish>` for another branch/commit. The command resolves that revision once, creates its own checkout, runs complete checks and reports the exact commit/logs. It uses the primary build cache and isolated scratch, excludes uncommitted files, and never fetches or pushes. Without `--no-push`, it refuses before work starts.

Checks are defined in `scripts/check.py`: pinned tools, syntax, domain routing, Rust formatting, Clippy in both configurations, authorship, default/core Rust suites, Node UI checks and development-tool contracts. The runner records per-case durations. Prepare pinned tools and Cargo dependencies first; checks never install them.

## Resolve failures

Read the failed step log and reproduce the smallest relevant contract. Investigate even failures from another change; a passing retry does not explain a race. Fix source unless the intended contract changed or the test is demonstrably wrong. Commit bounded fixes through the hook after relevant checks pass.

Repeat affected checks while fixing; repeat the complete immutable check when the checked commit changes. Interrupted runs are incomplete: retain logs and inspect surviving checkouts before cleanup. Never sweep another run's directory.

## Report

Name the exact checked commit, failures/skips, test count and practical limits. There is no push or CI success to report. Remote hosting requires a separate owner decision before adding a remote or publishing history.
