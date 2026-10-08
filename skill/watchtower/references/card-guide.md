# Writing Watchtower cards

This guide covers studying a due project and writing every field of its card. The user needs to understand the state, risk and next step within ten seconds. Write in German, address the user as "du", and keep each finding specific to evidence you read.

## Study only what changes the card

1. Read the complete `watchtower context <project-id>` result: project path and intent, facts, previous card, changes, feedback, tasks and warning acknowledgements. Keep the context token for the save. For `changed` or `stale`, start with the previous card and investigate what changed.
2. Read the project's history and documents that explain its purpose, decisions and unfinished work: README, project instructions, roadmap, TODO, plans and review reports where present. Use `git log --format="%cs %s"`, then `git show --stat <commit>` for unclear changes. For large histories, group related commits instead of reading each one.
3. Inspect uncommitted work with `git diff --stat` and `git ls-files --others --exclude-standard`; read relevant changed files and accompanying notes. Inspect extra worktrees when their facts show work. Git-ignored project notes can establish intent; private personal content stays out of the card.
4. Read the source layout enough to establish what the app does today. For red CI, inspect `facts.ci.workflows` and, when needed, `gh run view <run-id>` using the ID at the end of the run URL. Read failed logs only when the cause remains unclear. Distinguish a weeks-old failure from recent news.
5. For a changed or missing handbook, read the project's documented start/test/release commands, data locations, installation and shortcuts. Inspect the installed version without starting the app or changing its files. Never execute the handbook's commands during this observation run.
6. Check task results against evidence before describing progress. Assigned work is unfinished; `review` means reported but unconfirmed; `done` is confirmed. Respect deliberate warning acknowledgements and recorded decisions. Report material worsening instead of repeating an unwanted proposal.

Measured facts already appear in the window. Use them to interpret a finding; do not fill card text with numbers already visible there. Useful signals include `collisions` (overlapping changes), `release.commits_since` (work after the reachable release tag) and `night.reverts` (reverted commits since yesterday evening). Facts cover git-visible work, not ignored data. A failed facts measurement does not mean a clean repository. A configured remote does not prove a backup exists; no remote does not rule out other backups. Distinguish committed, tested, installed and actually used: each needs its own evidence. Attribute recorded test results to their date rather than implying you ran them. Timestamps containing a time are UTC; dates such as `last_work` are local.

## Place each finding once

- `open_ends`: unfinished work, including uncommitted/unpushed work, unfinished branches, plans and broken CI.
- `health_notes`: other concrete risk or deterioration.
- `waiting_on_you`: a decision only the user can settle.
- `suggestions`: a distinct proposal grounded in a finding and the project's goals.

The obvious repair of an open end is not an additional suggestion. The `reentry` step does not reappear as a suggestion. `headline` and `reentry` can refer to the leading finding without duplicating it in other sections. Remove finished, disproved or rejected items; do not replace completed work with an unsupported generic testing task. Keep IDs for retained questions and suggestions; omit IDs for new ones.

## Fields and limits

The required top-level fields are `one_liner`, `phase`, `headline` and `reentry`. Fill other fields when evidence exists; empty lists and nulls are better than invented findings. `watchtower schema card` describes the accepted shape and limits when a detail is unclear.

A word is a space-separated piece containing a letter or digit. A standalone dash does not count. Every text has at most 600 characters. The limits below are words unless stated otherwise. Paths, commands, `source` and `evidence` have no word limit. One-sentence fields allow 30 words. Suggested list lengths describe reading priorities, not a reason to fabricate entries.

| Field | Content |
|---|---|
| `one_liner` | Purpose in plain words; at most 15 words. |
| `stack` | Main technologies; at most 25 words. |
| `phase` | `active`: work within 7 days; `cooling`: 1–4 weeks; `dormant`: over 4 weeks; `maintenance`: scope finished, upkeep continues; `done`: stated scope complete. Use `facts.last_work`, which includes uncommitted edits. |
| `phase_evidence` | One sentence supporting the phase. |
| `headline` | Most consequential truth, at most 10 words; e.g. "Release fertig, aber noch nicht veröffentlicht." |
| `personality` | One playful but strictly true sentence in the project's voice. |
| `recent_changes` | Aim for 3–6 themes, newest first. Each has `date` (`YYYY-MM-DD`), `title` (8 words) and `detail` (30 words). Describe the user-visible effect. Date uncommitted work by file times and identify it as uncommitted. |
| `milestones` | Up to 8 major lifetime steps, oldest first. Each has `date` (`YYYY-MM-DD`) and `title` (10 words). Include releases. |
| `features` | Aim for 5–10 capabilities that exist today; each at most 8 words. |
| `open_ends` | Each has `kind`, `title` (10 words), `detail` (30 words), `source` and `age_days`. Kinds: `worktree`, `branch`, `uncommitted`, `unpushed`, `ci`, `flagged`, `todo`, `plan`, `other`. Sources identify a file, git ref, or fact such as `facts: has_remote`. Use the oldest dated evidence for age, or null if unknown. |
| `waiting_on_you` | Consequential questions in priority order; aim for 0–3. Each has `id` when retained, `title` as a question (12 words), `source` and `options`. Each of at most 3 options has `answer` (8 words) and `effect` (20 words). Use options only for known alternatives; for names, places or unknown values, use an empty list. Remove answered or dismissed questions. |
| `vision` | `statement`: documented goal, one sentence or null. `progress_note`: progress against that goal, one sentence or null. `progress_pct`: integer 0–100 only when derivable from stated work, otherwise null. |
| `reentry` | One useful next step: `next_step` (20 words), `effort` (`15min`, `1h`, `half-day`, `day+`), `why` (30 words), `done_when` (20 words). Completion must be observable. |
| `health_notes` | Concrete risks or deterioration, each at most 30 words. Empty when none were found. |
| `decisions` | Recorded deliberate choices with their source, each at most 20 words. Example: "Kein Remote: bewusst nur lokal (AGENTS.md)." These are the user's decisions, not your conclusions. |
| `suggestions` | Up to 4 distinct proposals; fields and selection below. |
| `cross_project_hints` | Possible reuse or overlap with other projects, each at most 30 words. The brief uses these findings. |
| `handbook` | Project commands, installation, structure and orientation; fields below. |

For ages, use `dirty.oldest_change` for uncommitted work, `unpushed_since` for unpushed commits, the first failing run for CI, or the oldest documented evidence. Do not invent a date from today's observation.

## Suggestions

Each suggestion has `id` when retained, `type` (`next`, `feature`, `cleanup`, `park`), `title` (8 words), `why` (25 words), `evidence`, `effort` (`S`, `M`, `L`) and `done_when` (20 words). Name files, functions or commits in evidence; line numbers drift. Tie features to the project's vision or the user's notes. A parking proposal is useful when the scope is complete or remaining work depends on people, money or decisions.

The user can turn a suggestion or `reentry` into an assigned task. Write `done_when` so another agent can tell whether it succeeded: "Monatsraten stimmen mit dem Kalender überein; dokumentierter Planungstest besteht." Include an exact test command only when the repository actually defines it. Assignments keep their original text through card rewrites.

Read feedback before choosing suggestions. Never reintroduce a rejected idea through rewording. Preserve a still-relevant item's ID, respect snooze dates and remove completed proposals. The user's recorded choices take priority over generic advice.

## Handbook

Update this field when commands, structure, installation or documents change. With `--merge`, an omitted `handbook` carries over. When changing part of it, send the whole object with the unchanged fields included.

| Field | Content |
|---|---|
| `run` | At most 8 entries, each with `what` (8 words), `how` (command or one URL) and `folder`. Prioritize starting, testing and releasing. |
| `installed` | Object with `version` (4 words), `path` and `open` (8 words; empty if unknown), or null when no installed copy exists. Take the version from the installed app's manifest/version file or the matching build source. Use "unbekannt" when an installed copy exists but its version cannot be established. `open` describes a shortcut or menu action in words. |
| `architecture` | Main parts and how they interact, at most 60 words. |
| `places` | At most 10 important folders/files/patterns; each has `path` and `what` (12 words). Include data and configuration. |
| `read_first` | At most 5 documents; each has `path` and `why` (12 words). Include relevant private project notes without copying personal content. |
| `needs` | At most 8 strings of 15 words: runtimes, services, hardware and credential names/configuration locations. Never credential values. |
| `terms` | At most 8 project-specific terms; each has `term` (4 words) and `meaning` (20 words). Omit standard programming vocabulary. |

`run.how` contains only what the user can copy and run or open. Use commands taken from the project's files, with the documented shell syntax and exact flags. Keep explanations, shortcuts and working-directory changes out of `how`: put the working directory in `folder`, relative to the repository (`.` for its root), or empty when none is needed. A URL opened after starting a service is a separate entry. Use `<placeholders>` for variable values such as a free port. Prefer one entry with variants when they share a command.

Paths inside the repository are relative to its root. Outside it, use `%LOCALAPPDATA%`, `%APPDATA%` or `~` where applicable so stored cards contain no user name. Use forward slashes in paths. Preserve the command syntax of the shell the project documents.

## Save only the intended change

For `changed`, `ci_changed`, `answered` or `stale`, update the affected fields and keep still-valid content. For `new_fields`, fill every listed path; a path inside a list item or the handbook requires the whole containing top-level field. Context lists these under `new_fields`. A save lists omissions under `unfilled_fields`; complete them before declaring the card current.

Use a UTF-8 file for the card payload. For a card that already exists, replace `<project-id>`, `<context-token>` and `<card-file>` with the project ID, token from its current context and your changed-field payload path:

```powershell
watchtower card put <project-id> --context-token <context-token> --file <card-file> --merge
```

For a project's first card, omit `--merge` and provide the complete card. The output confirms whether it saved, the version, changed fields, item IDs and warnings. A list in a merge replaces the whole list. A context conflict saves nothing: read the changed context and reconcile before submitting again. A schema error saves nothing: fix the named fields and retry. Never record credentials or unsupported conclusions to satisfy a missing field.
