# Push Workflow

`origin` is the public GitHub repository `Vironnimo/vWatch`; there is no CI. `scripts/push.py` checks an immutable commit completely and pushes only that commit. Push only when the owner asks: a push publishes history that cannot be taken back.

## Check the intended state

During development, `python scripts/check.py all` checks the working tree, including uncommitted files. For completed main, run:

```powershell
python scripts/push.py --no-push
```

Add `--ref <commit-ish>` for another branch/commit; `--ref` always checks only. The command resolves that revision once, creates its own checkout, runs complete checks and reports the exact commit/logs. It uses the primary build cache and isolated scratch, excludes uncommitted files, and never fetches or pulls.

Checks are defined in `scripts/check.py`: pinned tools, syntax, domain routing, Rust formatting, Clippy in both configurations, authorship, default/core Rust suites, Node UI checks and development-tool contracts. The runner records per-case durations. Prepare pinned tools and Cargo dependencies first; checks never install them.

## Push

Land all task work on `main` first, then run `python scripts/push.py` from the primary checkout. After every check passes, it pushes the checked commit to `origin`'s `main`. Git refuses anything but a fast-forward, and `pre-push` refuses commits with AI attribution; never bypass either with `--force` or `--no-verify`. A missing `origin` stops the command before the checks start.

If `origin` has commits that `main` lacks, stop and ask the owner; do not rewrite or force-push published history.

## Resolve failures

Read the failed step log and reproduce the smallest relevant contract. Investigate even failures from another change; a passing retry does not explain a race. Fix source unless the intended contract changed or the test is demonstrably wrong. Commit bounded fixes through the hook after relevant checks pass.

Repeat affected checks while fixing; repeat the complete immutable check when the checked commit changes. Interrupted runs are incomplete: retain logs and inspect surviving checkouts before cleanup. Never sweep another run's directory.

## Report

Name the exact checked commit, failures/skips, test count and practical limits, and whether it was pushed.
