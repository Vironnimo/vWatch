# Writing the vWatch portfolio brief

This guide covers the cross-project brief: today's focus, shared findings, the user's parked ideas and the weekly review. Prioritize what changes a decision this morning.

## Read the full context

Run `vwatch portfolio context` and follow every output page before writing. Keep its context token. The context includes compact cards and facts, tracked tasks, warning acknowledgements, reactions, parked ideas, the previous brief and `owner_last_opened`.

A card the previous brief already built on appears as a summary (`detail: summary`): headline, next step, usage and the titles and IDs of its items. For details such as a suggestion's evidence or a question's options, run `vwatch portfolio context --project <project-id>`, repeating `--project` for several; it returns those projects in full with the same token. `retained_proposals` lists titles; `vwatch proposals` shows their full text.

- If `owner_last_opened` predates the previous brief or is absent, carry forward still-important focus items: the user has not opened vWatch since that brief. Opening alone does not prove it was read. Otherwise retain items only while they remain true and among the highest priorities.
- Start with `changed_since_brief`: changed card fields the brief uses; an empty list means none changed, null means the project had no card then.
- Fill each `new_fields` path; for `synergies[].category`, review the retained proposals and set their categories explicitly while keeping IDs.
- Check every number, date and state against `facts.ok` and `facts.collected_at`; a failed or missing measurement is unknown, not zero. An `outdated` card predates changed facts, settled CI or user instructions; trust current evidence where it disagrees. The context includes card versions, finding sources and suggestion completion criteria. Attribute unchecked claims to their card; inspect the repository only when that evidence cannot establish a claim the brief needs. Git remotes do not establish whether other backups exist.
- Respect task assignments, reported results awaiting review, project status and acknowledged warnings. Do not suggest assigning the same work twice or reversing a deliberate choice.
- `brief_persistence.items` gives each previous item's first appearance and number of briefs. `dropped_focus` lists focus items that left recent briefs without the user marking them done: when their work remains unfinished, name them in `attention_note` so the user can tell set aside from done.
- `owner_response` shows whether the user reacts. With no reaction in 14 days, lead with the single decision that unblocks the most instead of adding questions.

## Synthesize after the project reviews

Do this in every daily run, even when no report is due. Bring together the project-review outcomes and observations collected along the way with all summaries in this context. Start from what the projects help the user accomplish, not just their current tasks or existing suggestions. An unchanged project can provide the missing capability for another project's need.

Look for repeated friction, complementary capabilities, useful transfers of a proven approach, and work that could disappear through simplification. Also consider unmet needs that none of the current projects serves well. Derive concrete possibilities from these connections; a list of shared risks or today's focus is not a substitute for ideas. Check the strongest candidates against actual capabilities, current proposals, user decisions and assigned work. Read more source only where it changes that judgment.

Evaluate product ideas and recommendations separately. Repository hygiene (commit, push, remove worktrees, delete build output, repair CI) is a recommendation, not product ideation; prioritize it in focus when it blocks a goal or a risk is growing. Do not duplicate that focus as a synergy. A distinct coordinated action across projects can be a `synergies` recommendation when it explains the benefit of doing it together.

Choose where each proposal belongs: one project's `suggestions`; a connection across projects in `synergies`; `new_apps` when a distinct need warrants a separate app. Set `category: idea` for new capabilities or simpler user workflows and `category: recommendation` for next steps, upkeep and repairs. New apps are ideas. Inspect both directions independently; existing recommendations do not substitute for product reflection. Either category can stay empty when no candidate earns its place. Explain the benefit and sources in the destination field, considering `cross_project_hints` too.

Evaluate the user's unmet need before choosing a project or implementation. Existing project boundaries are not fixed: consider the wider workflow and gaps between the apps. A missing integration, shared format or identifier is design work, not evidence against an idea's usefulness. Separate uncertain benefit from unresolved implementation; for a promising idea, name the assumption and a concrete way to check it instead of silently discarding it.

For `new_apps`, start from the user's days rather than the existing apps. The cards' `usage`, features and recent work show what the user actually does. Useful lenses: a step the user repeats by hand across several apps; a tool the user keeps rebuilding piecemeal inside different projects; a need outside coding that the projects reveal, such as data the user collects or plans they keep; a data flow that ends between two apps; a well-known product category that would serve the user better as a private, local version. An app idea names the situation it serves and its first useful version.

Compare candidates with retained ideas rather than treating occupied slots as unavailable. Record worthwhile ideas in vWatch, not only in chat. Keep discarded candidates and their reasons for the run reply, not as a checklist in the brief.

## Field meanings

Only `greeting` is required by the schema. Fill other fields when findings warrant them; use empty lists for no findings and null only for `week_review`. `vwatch schema portfolio` gives exact shapes and limits when needed. Project references use the stable IDs from context.

Every text has at most 600 characters. Limits below are words; a word is a space-separated piece containing a letter or digit. `evidence` has no word limit. Retained items keep their IDs in `focus`, `patterns`, `synergies`, `new_apps` and `park_or_revive`; omit `id` for a new item. To keep an item unchanged, send only `{"id": "<its id>"}`; the saved wording is reused, also for a retained proposal. Never invent an ID: an unknown one is saved as a new item with a warning.

| Field | Content |
|---|---|
| `greeting` | Friendly, slightly playful summary of the actual state; at most 30 words. |
| `focus` | Aim for 3 priorities, fewer when appropriate. Each has `id` when retained, `project`, action `title` (8 words), `why` (25 words) and `done_when` (20 words). Prioritize urgent risk, the user's goals and useful progress. Urgency wins over variety; do not rank by guessed duration. |
| `patterns` | Aim for 2–4 shared behaviors or risks with evidence. Each has `id` when retained, `title` (8 words), `why` (25 words), `projects`, `evidence` and `severity` (`info`, `warn`, `risk`). |
| `synergies` | Concrete opportunities to reuse or combine work. Each has `id` when retained, `category` (`idea` or `recommendation`), `title` (8 words), `why` (25 words), `projects` and `evidence`. Name the relevant projects and existing work. Keep the same ID when reclassifying. Entries without a category are displayed as recommendations until reviewed. |
| `new_apps` | At most 3 evidence-based app ideas. Each has `id` when retained, `title` (8 words), `pitch` (25 words) and `because` (25 words). Omit speculative filler. |
| `park_or_revive` | For quiet or stuck projects, each has `id` when retained, `project`, `verdict` (`park`, `revive`, `finish`, `archive`) and `why` (25 words). Respect the user's status and reason. |
| `attention_note` | Where observed activity goes compared with the user's priorities; at most 30 words. |
| `idea_checks` | One assessment for each `owner_ideas` entry, including previously checked ideas; each idea ID appears at most once. Each has `idea` (that numeric idea ID), `verdict` (`build`, `fold_in`, `later`, `drop`), `fits` (project IDs; empty for a standalone app) and `why` (25 words). Keep a prior verdict unless evidence changed. |
| `week_review` | When `review_week.due` is true, supply `week`, `done`, `stalled` and `time_note` as below. Otherwise omit it to keep that week's review, except to correct an evidenced error. |

`done_when` states an observable completion result so the user can hand the item to another agent. Do not replace it with "improve" or "review" without saying what completion means.

For `idea_checks`, `build` means worth a standalone app, `fold_in` means existing projects fit better, `later` means sound but not timely, and `drop` means not worth pursuing. Cite a concrete need, overlap or goal conflict in `why`. The user parked ideas to receive an honest assessment, not automatic approval.

For `week_review`, copy the valid ISO calendar week from `review_week.week` exactly (`YYYY-Www`). Use each project's `review_week_evidence`, present while the review is due: its changes and milestones fall within that week's inclusive dates. `done` holds up to 6 strings of 10 words drawn from those entries. `stalled` holds up to 6 strings of 10 words about work demonstrably unfinished during that week; today's open ends do not prove past state. `time_note` has at most 30 words comparing `commits_review_week` with the priorities; null means the stored measurement cannot establish that week's final count. Keep lists empty when no dated evidence supports an entry. A retained review describes its historical week, not today's remaining work.

## Retain intent, not stale wording

Check retained `why` and `evidence` against current facts and cards. Correct demonstrated errors even when a field is not due, including misdated weekly claims; preserve the review's historical period. Keep an unfinished focus item's ID while it remains a priority; replace lower-priority work when a stronger reason appears. Apply the Skill's feedback and task rules, and remove completed or disproved items.

## Save

Write the brief as UTF-8 JSON in your own temporary folder. Replace `<context-token>` with the token from the complete context and `<brief-file>` with the absolute Windows path to the payload:

```powershell
vwatch portfolio put --context-token <context-token> --file "<brief-file>"
```

Read the receipt's save state, warnings and each item's change (`new`, `kept`, `edited`, `restored`). With `--merge`, a provided list or object replaces that whole field. On a schema error, fix the named fields.
