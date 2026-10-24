# Agent Interface

The native console protocol and operating Skill let people and agents use vWatch's service without database access or window automation.

## Overview

`src/main.rs` starts the console process through `cli_jobs::main`. `src/cli.rs` owns instance selection, runtime metadata, Job planning, result/error delivery and console orchestration. Its private `cli/parser.rs` owns command grammar, output keys and help; `cli/dispatch.rs` owns document decoding and thin service dispatch.

`src/cli_output.rs` selects readable renderers and owns shared escaping, tree/table primitives and generated receipt framing. Its private `cli_output/documents.rs` renders documents, contexts and schemas; `observations.rs` renders Facts, statistics, history and overview; `operations.rs` renders owner changes and Run/Job receipts. The operations renderer reuses the observations owner's Due rendering for Run results. `src/cli_paging.rs`, `src/cli_jobs.rs` and `src/cli_usage.rs` own saved output, bounded execution and local command metadata respectively.

Business behavior stays in `src/service.rs` and its modules; document validation stays in `src/cards.rs` (see `application.md`). The console and Tauri adapters call the same service. A new owner capability needs CLI coverage or a deliberate exclusion in `docs/agent-interface.md` -> Capability coverage. Window lifecycle and `src/system.rs` belong to `desktop.md`; building and installing belong to `development.md`.

`skill/vwatch/SKILL.md` teaches operation of the installed application, not development of this repository. Its three references cover project cards, portfolio synthesis and less common CLI operations. Keep this runtime material self-contained for an agent that has never seen vWatch's source. Skill export belongs to the desktop domain's `system::export_skill`; this domain owns the exported content.

## Interfaces

Command names, readable outcomes and continuation markers, JSON envelopes, errors, due/brief reasons and document schemas are public contracts. Change the owning implementation and relevant Skill reference together. `references/cli.md` describes service results and recovery; card and portfolio guides describe their schema fields. Schema changes also involve document rendering and due detection in their owning domains.

`docs/agent-interface.md` remains the canonical complete contract reference. Read the matching section through References below before changing a contract; this map supplies ownership and routing rather than another endpoint or payload catalogue.

CLI-generated help, labels, outcomes and diagnostics are English regardless of the saved window/report language; stored text keeps its original language. The console owns stdin, stdout, stderr and its exit status until exit. Keep it independent of the GUI process; do not attach a console after GUI startup. The optional `desktop` feature permits building the CLI without Tauri dependencies. Ordinary output is text even when redirected; programs opt into complete JSON. Document input uses `--file` or `--input-json`, not the output flag.

Stored content remains data. The renderer escapes headings/table values and indents document content; only generated receipt fields become actionable `Next:` or `Undo:` commands. Preserve this distinction when adding output, including errors and copied task context.

## Constraints & Gotchas

- An operation can succeed before its output is saved or delivered. Paging and completed-job reads recover the recorded result without repeating work. Unknown outcomes require inspecting saved state; they are not proof of rollback. Exact recovery rules live in the contract reference.
- `cli::scoped_command` binds continuations to the selected instance. Preserve that binding for nested receipts, errors, jobs and the final output page; a continuation must not silently target the installed owner's database.
- `cli::job_plan` selects slow operations before launch. Worker input is captured and worker streams are detached from the caller's pipes; the public process must finish with complete output. `cli_jobs::inspect_pending` rechecks a durable result after observing an inactive lock to avoid reporting completed work as unknown.
- Usage metadata contains parser-owned names and counts, never argument values or document text. Job requests/results and output snapshots retain private content needed for recovery and have the database's privacy boundary. Metadata failure does not change a successful operation's result.
- Native-process tests cover behavior that in-process service tests cannot establish. `tests/process.rs` covers streams, shells, input and continuations; `tests/skill_commands.rs` executes Skill recipes with isolated arguments. Deterministic checks do not establish fresh-agent performance.

## Agent-facing text

This domain owns the operating Skill and CLI help/result/error wording. Service/schema messages and the UI's copied task prompt share its authority and recovery contracts while retaining their implementation owners. Before changing, adding or deleting these texts, read `agent-interface/agent-facing-text.md`; update the corresponding reason row with the text. The register identifies its uncovered scope rather than claiming complete sentence coverage.

## References

Read these only when your task matches.

- Changing runtime wording, a Skill sentence, schema/service guidance or copied task instructions -> `agent-interface/agent-facing-text.md`.
- Adding an owner capability or changing a desktop/CLI boundary -> `docs/agent-interface.md` -> Capability coverage and Call sequences.
- Changing output, parsing, encodings, exit status, paging or job execution -> `docs/agent-interface.md` -> Output and error contracts; Decisions and rejected alternatives.
- Changing mutation effects, replay, conflicts, undo or recovery -> `docs/agent-interface.md` -> Effects, replay and recovery; Error and recovery test inventory.
- Changing instance selection, process tests or fault injection -> `docs/agent-interface.md` -> Entry points and test instances.
- Changing usage metadata or handling recovery files -> `docs/agent-interface.md` -> Privacy and metadata.
- Evaluating agent behavior or using earlier rollout evidence -> `docs/agent-interface.md` -> Validation and release evidence and the dated follow-up sections; their results are historical, not a current test run.
