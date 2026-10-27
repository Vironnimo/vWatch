# Measurement details

Use this when changing collection internals or expensive measurement reuse. Public consumer meaning and storage ownership start in `repository-facts.md`.

## Processes and working trees

`run` in private `src/facts/process.rs` requests the C locale for Git/gh diagnostics and uses English OS error-kind descriptions with numeric OS codes. It uses separate stdout/stderr readers. Its deadline covers the child and both output pipes because a descendant can keep a pipe open after its parent exits. On Windows, `spawn_contained` creates the child suspended, assigns a private kill-on-close Job Object, then resumes its initial thread. A process-local PSS thread snapshot finds that thread without enumerating every thread on the machine; its descriptors require PSS cleanup APIs, with the current process as snapshot owner. Every return closes the job, including after a successful parent exits, so descendants cannot survive by detaching or retaining output pipes. Never resume before assignment or replace this with a PID-tree kill after parent exit: orphaned descendants are no longer reachable that way. Setup failure terminates the still-suspended child; helper console windows remain hidden. The existing `command_timeout_includes_descendant_output_pipes` test checks both deadline and descendant termination.

`dirty` and extra-checkout measurement in private `working_tree.rs` share the NUL-delimited porcelain decoder, including individual untracked files. Rename/copy records consume the following original-path token; status bytes and destination-path order are preserved. Dirty fingerprints hash the original status output, staged index entries (including conflict stages), and bounded file content plus size/high-resolution mtime, so unchanged file/line counts alone do not mean unchanged work. A failed index read makes the collection unsuccessful. Symlink fingerprints use the link target; source counts exclude symlinks.

Dirty file counts describe successful status enumeration independently of text-line coverage. `dirty.line_counts.ok` certifies both nullable line totals; any failed/invalid tracked numstat output, unread or nonregular untracked file (including a symlink) or exhausted untracked size/file limit makes both totals unknown. Binary files are excluded from text lines. Reads remain bounded even when a file grows after its metadata check. The real-repository contract probes oversized text, an exclusively locked file on Windows and a Git diff failure.

Linked-checkout collection excludes the checkout being measured rather than assuming the first worktree is always the current one. Other checkout heads and dirty fingerprints affect the report basis. Worktree measurement uses bounded parallelism because starting Git dominates its cost. See `tests/facts.rs` for linked-checkout and dirty-work coverage.

CI examines the newest returned run per workflow, ignores Dependabot's dynamic update jobs and selects the worst remaining state. It reads a bounded run list, not an unlimited workflow history. Local-only or skipped measurements must not manufacture a green CI result.

## Source counts

`Files` in `src/facts/statistics.rs` lists tracked and non-ignored untracked paths once per collection. Counts describe the current working tree, including local edits; extra worktrees are not added to that project's source size. Missing tracked files are absent from the current tree rather than unread source.

`scan_file` classifies paths before Tokei parsing. Common dependency/build directories, lockfiles, minified/generated files and generated headers are excluded heuristically. Documentation and configuration are counted separately from source-code language shares. Unsupported/binary files and excluded files are different from unread files.

The source scan is bounded by elapsed time, total bytes, file count and per-file size. Reads enforce the per-file limit even if a file grows after its metadata check. Exhausted budgets or unread files make `code.ok` false and mark the result partial, even though partial totals exist. Consumers must not present those totals as a complete measurement. Method identifiers are part of reuse and historical comparison, not decorative labels.

A complete code scan can be reused only when the previous overall facts are readable, HEAD and dirty fingerprint match, and the counting method matches. Changes to exclusion/counting rules require reconsidering the method identifier and reuse tests.

## Reconstructed activity

Daily activity follows HEAD ancestry and commit dates. Merge commits contribute to commit counts; merge diffs and binary line counts are excluded. Hotspots have a separate recent window and rank source files that still exist and pass the current scan's classification. They are not a list of every historically changed path.

`carried_days` proves that the prior series can be extended or trimmed before reusing it. Method, date window, branch, reachable ancestry and earlier commit counts matter. Rewritten/grafted/deepened history, clock movement or date-window boundaries can require a full scan even when reuse looks plausible. Hotspot reuse additionally requires the same tree and day.

The `measurement_reuse_*` tests in `tests/facts.rs` compare incremental results with full collection across five distinct contracts: unchanged inputs, elapsed date windows, forward HEAD movement, rewritten history and grafted history. Marked previous values prove when reuse occurred rather than merely accepting equivalent fresh results. Keep those equivalences and proofs when optimizing. Persisted observation coverage and comparison baselines belong to `src/service/statistics.rs`, not this reconstruction cache.

## Space

`space` looks for known rebuildable directories at repository root and one level below; directory traversal does not follow symlinks/junctions. The service retains its dated result between daily measurements. This is an observation API only; any decision to clean or keep a directory happens outside vWatch.

Discovery, entry/type/metadata and nested enumeration errors propagate into an incomplete result. A directory's bytes and the overall total are exact only when their respective `ok` flag is true; failures produce null, not zero. Complete sibling directories retain their sizes. Only complete same-day results are reused; a partial or uncertified result is retried on refresh. `measurement_quality` adapts older persisted facts and historical read models without completeness evidence to unknown totals; stored history is not rewritten. Exact public payload meanings are in `docs/agent-interface.md` -> Output and error contracts.

## References

- Changing historical coverage or size comparison eligibility -> `application.md`
- Changing observation storage or repeated-payload retention -> `persistence.md`
- Changing scratch/test setup or running process checks -> `development.md`
