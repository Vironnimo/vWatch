# Writing vWatch cards

This guide covers investigating a project and writing every card field so the user understands its state, risk and next step within ten seconds.

## Study the project, then reflect before saving

1. Read the complete project context: intent, facts, previous card, changes, feedback, tasks and warning acknowledgements. Use the due reason to identify what needs reassessment; retain supported knowledge from the previous card.
2. Establish the project's purpose and current work from its README, project instructions and relevant history or plans. For a known project, focus on changes and unresolved claims rather than rereading every document. Use `git log --format="%cs %s"` and `git show --stat <commit>` where they answer those questions; group large histories into themes.
3. Inspect both unstaged and staged work with `git diff --stat` and `git diff --cached --stat`, plus untracked files with `git ls-files --others --exclude-standard`. Read relevant changes and notes; inspect extra worktrees when their facts show work. Git-ignored project notes can establish intent; private personal content stays out of the card.
4. Read the source layout enough to establish what the app does today. For red CI, inspect `facts.ci.workflows` and, when needed, `gh run view <run-id>` using the ID at the end of the run URL. Read failed logs only when the cause remains unclear. Distinguish a weeks-old failure from recent news.
5. For a changed or missing handbook, read the project's documented start/test/release commands, data locations, installation and shortcuts. Inspect the installed version without starting the app or changing its files. Never execute the handbook's commands during this observation run.
6. Check task-result evidence before describing progress; a reported result awaiting the user's confirmation is not confirmed completion. Interpret risks against recorded decisions and warning acknowledgements.
7. Establish actual use: who uses the project, how often and when last. Look for traces outside the code: log and data files, saved sessions or exports, the installed copy's file times, scheduled tasks (`Get-ScheduledTask`), running processes (`Get-Process`) and the project's own usage records. Record dates, counts and kinds of activity, never personal content. Installed is not used; no trace is a finding too. Save the result in `usage`.
8. Pause on this project before moving to the next: what could make its actual use more useful, clearer or simpler? Evaluate opportunities from what you just learned, following "Suggestions" below. Save useful proposals and cross-project hints; carry a concise selection outcome into the final synthesis even when the card stays unchanged.

Measured facts already appear in the window. Use them to interpret a finding; do not fill card text with numbers already visible there. Useful signals include `collisions` (overlapping changes), `release.commits_since` (work after the reachable release tag) and `night.reverts` (reverted commits since yesterday evening). Facts cover git-visible work, not ignored data. A failed facts measurement does not mean a clean repository. A configured remote does not prove a backup exists; no remote does not rule out other backups. Distinguish committed, tested, installed and actually used: each needs its own evidence. Attribute recorded test results to their date rather than implying you ran them. Timestamps containing a time are UTC; dates such as `last_work` are local.

## Place each finding once

- `open_ends`: unfinished work, including uncommitted/unpushed work, unfinished branches, plans and broken CI.
- `health_notes`: other concrete risk or deterioration.
- `waiting_on_you`: a decision only the user can settle.
- `suggestions`: distinct recommendations and product ideas, separated by `category`.

Repository hygiene (commit, push, remove worktrees or branches, delete build output, repair CI) belongs in `open_ends`. Its obvious repair and the `reentry` step need no duplicate suggestion. A distinct action that needs its own assessment can be a recommendation; it is not a product idea. `headline` and `reentry` can refer to the leading finding. Remove finished, disproved or rejected items; do not replace completed work with an unsupported generic testing task. Keep IDs for retained questions and suggestions, including reclassification; omit IDs for new ones. To keep an item unchanged, send only `{"id": "<its id>"}` in its list; the saved wording is reused. This also restores a retained proposal from context. An ID that no saved version of this card has is saved as a new item with a warning.

## Fields and limits

The required top-level fields are `one_liner`, `phase`, `headline` and `reentry`. Fill other fields when evidence exists; use empty lists for no findings and null only for nullable fields. Omitted optional fields receive defaults on a first save; in a merge they retain the previous value. `vwatch schema card` describes the accepted shape and limits when a detail is unclear.

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
| `usage` | How the user actually uses the project today. `summary`: one sentence on who uses it, how and how often, or null when no trace exists. `last_used`: `YYYY-MM-DD` of the newest usage trace, or null. `evidence`: the traces you read (paths, log names, processes), or what you checked without finding one. |
| `open_ends` | Each has `kind`, `title` (10 words), `detail` (30 words), `source` and `age_days`. Kinds: `worktree`, `branch`, `uncommitted`, `unpushed`, `ci`, `flagged`, `todo`, `plan`, `other`. Sources identify a file, git ref, or fact such as `facts: has_remote`. Use the oldest dated evidence for age, or null if unknown. |
| `waiting_on_you` | At most 3 questions, in priority order, that block the next step or the user's goal. Collect later-horizon decisions from a document as one open end citing it. Each question has `id` when retained, `title` as a question (12 words), `source`, `options` and `recommended`. Each of at most 3 options has `answer` (8 words) and `effect` (20 words). Use options only for known alternatives; for names, places or unknown values, use an empty list. `recommended` is the 1-based number of the option you advise, or null. When the repository records a user decision that the project's status lacks, such as a pause, ask whether to set that status. Remove answered or dismissed questions. |
| `vision` | `statement`: documented goal, one sentence or null. `progress_note`: progress against that goal, one sentence or null. `progress_pct`: integer 0–100 only when derivable from stated work, otherwise null. |
| `reentry` | One useful next step: `next_step` (20 words), `why` (30 words), `done_when` (20 words). Completion must be observable. |
| `health_notes` | Concrete risks or deterioration, each at most 30 words. Empty when none were found. |
| `decisions` | Recorded deliberate choices with their source, each at most 20 words. Example: "Kein Remote: bewusst nur lokal (AGENTS.md)." These are the user's decisions, not your conclusions. |
| `suggestions` | At most 8 distinct proposals; select up to 4 ideas and up to 4 recommendations independently. Empty categories are valid. Fields and selection below. |
| `cross_project_hints` | Possible reuse or overlap with other projects, each at most 30 words. The brief uses these findings. |
| `handbook` | Project commands, installation, structure and orientation; fields below. |

For `age_days` of uncommitted work, unpushed commits or red CI, use `days` of the matching `risk_streaks` entry; with `at_least` true it is a lower bound. Otherwise use `unpushed_since`, the first failing run for CI, or the oldest documented evidence. Do not invent a date from today's observation.

## Suggestions

Each suggestion has `id` when retained, `category` (`idea` or `recommendation`), `type` (`next`, `feature`, `cleanup`, `park`), `title` (8 words), `why` (25 words), `evidence` and `done_when` (20 words). Set `category` by the benefit, independently of `type`: `idea` adds a useful capability or simplifies the user's workflow; `recommendation` advances existing work, fixes a concrete problem, maintains or secures it. An export preview is an idea; publishing an already finished release is a recommendation. Name files, functions or commits in evidence; line numbers drift. Tie ideas to the project's purpose or the user's notes.

Review both categories even when recommendations already occupy the card. For an idea, establish who benefits, in what situation and what becomes possible or easier; a generic improvement title is insufficient. For a recommendation, establish why action is warranted now. Reclassify retained entries with their existing IDs. Until reviewed, entries without a category use `idea` for `type: feature` and `recommendation` otherwise. The context's `new_fields` requests explicit classification of older entries.

The user can turn a suggestion or `reentry` into an assigned task. Write `done_when` so another agent can tell whether it succeeded: "Monatsraten stimmen mit dem Kalender überein; dokumentierter Planungstest besteht." Include an exact test command only when the repository actually defines it. Assignments keep their original text through card rewrites.

Develop possibilities throughout the investigation and evaluate them before saving, rather than merely checking whether yesterday's suggestions remain true. Start from actual use (`usage`), not from the code alone: the best ideas change what the user can do in a moment they really have. Useful lenses:

- Before and after: what does the user do right before opening the app and right after? Absorb that step.
- Unnecessary: what would spare the user opening it at all, such as a result that arrives on its own, a default or a notification?
- Borrowed: which capability of another watched project, or which proven pattern of a well-known product in the same domain, would serve here? Name it.
- Extreme: ten times the data, daily instead of weekly, from the phone, with no attention to spare. What breaks, what becomes possible?
- Removed: which feature or step costs more attention than it returns?
- Unused: which existing capability does the usage evidence show nobody reaches, and why?

Prefer an idea that changes what the user can do over one that polishes what exists. New work can enable new uses; an unchanged repository can still reveal a previously missed opportunity. These are lenses for judgment, not categories to fill.

For a promising direction, establish the observed need, the proposed change and its concrete benefit. Evidence must support the need or available capability; the solution need not already appear in a TODO or plan. Inspect just enough relevant code or documentation to check that it is not already implemented. Distinguish your proposal from existing behavior and name any uncertainty. Generic testing, pushing commits, merging worktrees or fixing an already listed open end is not product ideation.

Compare candidates with existing suggestions, owner feedback and assigned work. Unfinished suggestions do not block new ideas. Save the strongest distinct proposals in `suggestions`, with an observable `done_when`; preserve useful retained items and their IDs within the limit. Put reusable capabilities or needs worth comparing with other projects in `cross_project_hints`, with a concrete source. No novelty quota: when nothing earns a new proposal, carry forward the specific direction considered and why it was unnecessary, unsuitable or already covered. Do not fill the card with rejected candidates or a reflection checklist; the selection outcome belongs in the worker handoff or run reply.

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

For `changed`, `ci_changed`, `answered` or `stale`, update the affected fields and keep still-valid content. For `idea_review`, nothing measured changed: establish actual use, update `usage`, review `suggestions` with the lenses above and keep other fields with `--merge`. When no idea earns a place, save `usage` alone and carry the strongest considered direction to your report. For `new_fields`, fill every listed path; a path inside a list item or the handbook requires the whole containing top-level field. Context lists these under `new_fields`. A save lists omissions under `unfilled_fields`; complete them before declaring the card current.

Use a UTF-8 file for the card payload. For a card that already exists, replace `<project-id>`, `<context-token>` and `<card-file>` with the project ID, token from its current context and your changed-field payload path:

```powershell
vwatch card put <project-id> --context-token <context-token> --file "<card-file>" --merge
```

For a first card, omit `--merge`. Read the receipt's save state, changed fields, each item's change (`new`, `kept`, `edited`, `restored`), omissions and warnings. A schema error saves nothing: fix the named fields. Resolve missing fields with supported content or their empty defaults, not invented conclusions.
