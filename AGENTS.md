# AGENTS.md


## Load context for the task

At session start, read these core files completely unless their current contents are already in context. Load them once; re-read affected sections when they change:

- `.vorch/PROJECT.md` - project context
- `.vorch/GLOSSARY.md` - project-specific terms

Before working in or discussing a domain, read its root map under `.vorch/domain-maps/` (index in PROJECT.md), plus adjacent maps when ownership or contracts cross domains. Use them to locate owners, contracts, source, and tests. The task's `read:` list is a starting point; add a map only when the task reaches into its domain. Follow the root map's `## References` only for supplementary files whose triggers match the task; do not preload a supplementary folder. Supplementary files never replace the root map or enter the Domain Maps index.

Before changing a domain map or supplementary file, read `.vorch/workflows/domain-map-workflow.md` up to `## Creating or restructuring a map` unless its current contents are already in context; read the rest too when you create, split, merge, rename or restructure a map. Ordinary map reading does not require that workflow.

## Interpret documentation

Distinguish explicit user requirements and engineering contracts from descriptions of current behavior, examples, and proposals. Follow applicable requirements and contracts; a documented implementation or past choice is not automatically a constraint on new work. Existing behavior and design may be revised within the user's requested scope without a documentation exception. When the user asks to rethink or rebuild a system, its documented design, invariants and earlier decisions about it are open for revision; say which ones you would keep and why. A recorded user requirement stays in force until the user agrees to drop it by name.

Source code establishes what is implemented, not what ought to be implemented. Verify behavior claims against source and relevant tests. Correct stale descriptions; when code conflicts with an explicit requirement or contract, address the discrepancy rather than rewriting the requirement to match the code. Keep observations, requirements, and proposed changes identifiable in the docs.

## Communication with the user

### Use established terminology

Use the project's established terms as defined in the glossary and domain maps, such as Card, portfolio brief, Task and Run. Keep these terms untranslated when you talk to the user in another language.

### Present Agent-facing text changes

For runtime Agent-facing text, explain the meaningful changes in the conversation and show the relevant proposed wording verbatim. Include the corresponding current wording when comparison helps the user assess the change.

For larger changes, keep the conversation focused on the passages that materially affect Agent behavior. Make the complete changes available through the source-file diff. Do not create additional files solely to present the review. Show complete wording when the user requests it.

Runtime Agent-facing text includes the operating Skill, CLI help/results/recovery hints, schema descriptions and copied task instructions supplied by vWatch to an Agent or Model. Repository governance and development documentation are outside this review rule.

## Agent-facing work

Before writing or reviewing runtime Agent-facing text, load the matching Skill and follow it: `agent-cli` for the application CLI, `agent-skill-creator` for the operating Skill, `agent-text` for generated task instructions, and `agent-eval` for fresh-model evaluations.

## Architecture & code

**Few, deep modules:** Prefer clear ownership, small interfaces, and hidden implementation. Extend an existing owner when the capability belongs there. Before introducing a new responsibility, public interface, layer, or abstraction, identify the closest existing owner and explain why the new boundary is warranted. Avoid pass-through layers and abstractions that expose their internals or add little beyond their interface.

Judge complexity by responsibilities, coupling, and what callers must understand, not file count alone. Tests, documentation, components, and internal file splits do not need a new-owner justification unless they introduce an architectural boundary. Split files when it improves clarity and cohesion; do not overload an existing file merely to avoid creating one.

**Technical Decisions** - when making technical decisions, do not give much weight to development cost. Instead, prefer quality, simplicity, robustness, scalability, and long-term maintainability.

## Testing

Tests guard behavior against regressions from concurrent and later work. Every test also costs runtime and maintenance for all later tasks, so keep the suite a curated set of behavior contracts, not a record of every change.

**Write tests for contracts, not for changes.**

- Test behavior at the owning module's public interface: contracts, persisted formats, protocols, CLI outcomes and recovery, measured facts, and owner-visible state. Test an internal helper directly only when it carries its own contract that is impractical to reach through that interface.
- A bug fix extends the existing test that owns the behavior, with a new parameter case or assertion. Add a new test only when no existing test covers that behavior.
- One test per distinct behavior; parameterize only over genuinely different cases.
- Developer scripts and probes get at most a smoke test, unless they protect data or installations (converters, installers, update and worktree tooling).
- Keep tests fast and deterministic: controlled clocks and in-memory fakes instead of avoidable waits and network. Real Git repositories and console processes remain necessary at those public boundaries. A test that needs more than about a second must justify it. Load and scale variants carry the `stress` marker, which the normal suite skips (PROJECT.md -> Testing).
- Durability and process recovery are real contracts here. Keep real SQLite synchronization and job-result syncs until a test-only seam can preserve the durability cases; do not weaken production writes to speed up tests. See the testing guide for isolation and opt-in stress cases.
- When a change makes tests obsolete or redundant, delete or merge them in the same change.
- Documentation-only edits need no tests; review content, references, and the diff.

Before writing or changing a test, read `.vorch/domain-maps/development/testing.md`: what to assert, how to wait, and what the shared fixtures isolate.

**Commits and merges get static checks only.** Run the tests covering your change yourself and commit once they pass. Before completing a larger change or an installation, run the complete local check; this repository has no remote or CI gate. Investigate failures and report any unresolved ones.

Do not run existing suites merely to analyze code. Read source and contract tests with a question in mind; use a targeted probe for an uncertain behavior claim. A requested survey or test audit runs the suite to measure it.

## vWatch development boundaries

- Observe the owner's repositories; do not add Git-writing commands, cleanup, backup or repository-editing features to vWatch. Development commits in this repository are separate from application behavior.
- Keep the owner's data and OS choices intact. Use a task-specific scratch instance for development; do not leave test ratings, replies or ticks in the installed app. Never toggle the owner's autostart or PATH setting.
- Verify changes to the window in the running window. VM tests do not establish layout or WebView behavior; follow the live-testing workflow.
- UI text and agent-written report content are German, addressing the owner as `du`. Code, comments, repository documentation, the operating Skill and commit messages are English.
- Changes to application commands, output, recovery, owner capabilities or document schemas update the operating Skill and `docs/agent-interface.md` in the same commit. The agent-interface map routes to those contracts.
- Preserve the actual scope when updating an installed operating Skill. An agent-private installation stays private unless the owner requests another scope.
- Keep build/log/script scratch in the current checkout's `.tmp/` and source-run data in a task-specific subfolder. The test runner puts fixture homes and repositories outside checkouts and the user profile, as the testing guide describes. Use the primary checkout's single `target/` through the development tools; never link checkout folders or create a second build tree.

## Dependencies

If a task requires a new dependency, **check first** that no existing dependency already covers the need. Then install it, add it to `Cargo.toml` (application dependencies) or an explicit development manifest when the existing standard-library tools cannot do the job, and commit the lock file changes. Do not install packages speculatively - only what the current task requires.

## You maintain the docs & domain maps

Maintain affected documentation as part of the task with small, factual updates:

- `.vorch/PROJECT.md` - architecture, conventions, dev/test setup, domain-maps index, strategic context. Every agent reads it, so a fact whose use is confined to one domain goes into that domain's map instead
- `.vorch/domain-maps/<domain>.md` - a domain's ownership, contracts, or documented behavior changes, including affected supplementary files, or a new domain emerges (a new domain also gets added to the domain-maps index in PROJECT.md)
- `.vorch/GLOSSARY.md` - shared vocabulary under the Terminology rules below
- `.vorch/FLAGGED.md` - git-ignored, never commit it; keep only confirmed, concrete, still-open concerns, with the evidence and affected behavior. During implementation, fix confirmed bugs with a clear, bounded, low-risk solution even outside the original task; defer them to this file only with a concrete reason. Investigate a suspicion before adding it; when reviewing an existing suspicion, check it before removing it. Do not add pending evaluations, missing live checks, speculative future failures, or completion notes. Append at the bottom, or fold into a related entry when one fits; delete an entry once its concern is resolved or verified fixed instead of annotating it.

**Write all project documents in English.** Plans, design documents, decision records, domain maps, PROJECT.md, GLOSSARY.md, FLAGGED.md - every project artifact is written in English, regardless of the language you and the user speak in chat. User-facing chat follows the user's language; the documents do not.

## Terminology

`.vorch/GLOSSARY.md` holds the terms every agent needs in every task: a term belongs there when several domains use it and an agent would otherwise misread it, especially as a concept of its own environment or of another system, such as Project, Task, Run and Job. A term only one domain uses belongs in that domain map's `## Terms` section, even when the user mentions it; when other domains start using it, move it to the glossary and leave a pointer. Standard vocabulary gets no entry.

An entry, in the glossary or in a map's Terms, says what the term means, not how it works: at most two sentences of definition plus a "Not ..." distinction where a misreading is realistic, and the map or section that owns the concept. Tables, columns, functions, syntax, limits, states and procedures stay out of entries; they belong in the owning map's Data Model, Interfaces or a supplementary file, which the entry names. Give each term one canonical home; use references elsewhere.

Maintain terminology as part of relevant work when a missing, stale, or ambiguous definition could cause mistakes. Make clear, evidence-backed updates directly; no separate Skill or interview is required. Ask the user when competing meanings would materially affect the work, rather than silently establishing a new meaning.

## Git

- Work directly on `main` - no feature branches by default. When you finish a task, commit it (the user may also ask you to commit mid-way); you don't need to wait to be asked.
- **Small, quick, low-risk changes go directly on `main`; use a worktree for larger or uncertain tasks, or when concurrent work could interfere.** Create worktrees with `python scripts/worktree.py create <task-name>` (see PROJECT.md -> Development), then work and commit inside them. Once everything is committed, run `python scripts/worktree.py merge <task-name>` to merge into `main` and remove the worktree; the merge commit passes the commit check. For conflicts, follow `.vorch/workflows/worktree-workflow.md`. No user confirmation is needed before merging.
- **Commits carry only the user's authorship.** Never commit as an AI tool (such as `Claude <noreply@anthropic.com>`) and never add `Co-Authored-By`, `Claude-Session`, or "Generated with" lines to commit messages or PRs. The hooks enforce this (`scripts/commit_authorship.py`): a tool identity blocks the commit, attribution lines are stripped from messages, and a push carrying either is refused. If a commit is blocked, commit under the user's identity with repository-local config only, never `--global`.
- **Line endings are handled for you.** `.gitattributes` stores every text file with LF; Git's `CRLF will be replaced by LF` warning is harmless. Never convert files by hand to silence it.
- Conventional format: `<type>(<scope>): <what>` - lowercase, <=72 chars, no trailing period. Types: `feat` `fix` `docs` `refactor` `perf` `test` `chore`. Breaking change -> `!`.
- One logical unit per commit; never batch unrelated changes; commit once the tests covering your change pass.
- **The commit hook checks every commit.** It checks the staged snapshot with Rust formatting and Clippy, JavaScript/Python syntax, and domain-map validation without rewriting or staging working-tree files (PROJECT.md -> Testing). Review the staged diff. When it blocks, fix the reported problems, stage, and commit again; never bypass it with `--no-verify`. It excludes other sessions' unstaged content entirely; all errors in the staged snapshot block. Leave other sessions' files alone. If a commit of source files prints no `Commit check` report, the hook is not enabled: enable it (PROJECT.md -> Development).
- **Local repository:** do not add a remote or push. `python scripts/push.py --no-push` is the immutable-commit verification path; read `.vorch/workflows/push-workflow.md` before using it. `python scripts/check.py all` checks the current checkout.
