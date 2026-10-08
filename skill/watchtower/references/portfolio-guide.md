# Writing the Watchtower portfolio brief

This guide covers the cross-project brief: today's focus, shared findings, the user's parked ideas and the weekly review. Write in German, address the user as "du", and prioritize what changes a decision this morning.

## Read the full context

Run `watchtower portfolio context` and follow every output page before writing. Keep its context token. The context includes compact cards and facts, tracked tasks, warning acknowledgements, reactions, parked ideas, the previous brief and `owner_last_opened`.

- If `owner_last_opened` predates the previous brief, the user has not seen that brief. Carry forward still-important focus items. Otherwise retain them only while they remain true and among the highest priorities.
- Start with `changed_since_brief`: changed card fields the brief uses; an empty list means none changed, null means the project had no card then.
- Check every number, date and state against `facts.ok` and `facts.collected_at`; a failed or missing measurement is unknown, not zero. An `outdated` card predates changed facts, settled CI or user instructions; trust current evidence where it disagrees. The context includes card versions, finding sources and suggestion completion criteria. Attribute unchecked claims to their card; inspect the repository only when that evidence cannot establish a claim the brief needs. Git remotes do not establish whether other backups exist.
- Respect task assignments, reported results awaiting review, project status and acknowledged warnings. Do not suggest assigning the same work twice or reversing a deliberate choice.
- Actively consider new app ideas and cross-project opportunities in every daily brief, even when suggestions already exist. Look for concrete unmet needs or useful combinations of existing capabilities. Retained ideas do not reserve all available places; compare them with new candidates. Add ideas when the evidence supports them, without a daily novelty quota.

## Field meanings

Only `greeting` is required by the schema. Fill the other fields when findings warrant them. Empty lists are better than filler. `watchtower schema portfolio` gives the exact accepted shape and limits when needed. Project references use the stable IDs from the context.

Every text has at most 600 characters. Limits below are words; a word is a space-separated piece containing a letter or digit. `evidence` has no word limit. Retained items keep their IDs in `focus`, `patterns`, `synergies`, `new_apps` and `park_or_revive`; omit `id` for a new item.

| Field | Content |
|---|---|
| `greeting` | Friendly, slightly playful summary of the actual state; at most 30 words. |
| `focus` | Aim for 3 priorities, fewer when appropriate. Each has `id` when retained, `project`, action `title` (8 words), `why` (25 words) and `done_when` (20 words). Prioritize urgent risk, the user's goals and useful progress. Urgency wins over variety; do not rank by guessed duration. |
| `patterns` | Aim for 2–4 shared behaviors or risks with evidence. Each has `id` when retained, `title` (8 words), `why` (25 words), `projects`, `evidence` and `severity` (`info`, `warn`, `risk`). |
| `synergies` | Aim for 2–4 concrete opportunities to reuse or combine work. Each has `id` when retained, `title` (8 words), `why` (25 words), `projects` and `evidence`. Name the relevant projects and existing work. |
| `new_apps` | At most 3 evidence-based app ideas. Each has `id` when retained, `title` (8 words), `pitch` (25 words) and `because` (25 words). Omit speculative filler. |
| `park_or_revive` | For quiet or stuck projects, each has `id` when retained, `project`, `verdict` (`park`, `revive`, `finish`, `archive`) and `why` (25 words). Respect the user's status and reason. |
| `attention_note` | Where observed activity goes compared with the user's priorities; at most 30 words. |
| `idea_checks` | One assessment for each `owner_ideas` entry, including previously checked ideas. Each has `idea` (that numeric idea ID), `verdict` (`build`, `fold_in`, `later`, `drop`), `fits` (project IDs; empty for a standalone app) and `why` (25 words). Keep a prior verdict unless evidence changed. |
| `week_review` | When `review_week.due` is true, supply `week`, `done`, `stalled` and `time_note` as below. Otherwise omit the field; the review for the same week is kept. |

`done_when` states an observable completion result so the user can hand the item to another agent. Do not replace it with "improve" or "review" without saying what completion means.

For `idea_checks`, `build` means worth a standalone app, `fold_in` means existing projects fit better, `later` means sound but not timely, and `drop` means not worth pursuing. Cite a concrete need, overlap or goal conflict in `why`. The user parked ideas to receive an honest assessment, not automatic approval.

For `week_review`, copy `review_week.week` exactly (`YYYY-Www`). Use each project's `review_week_evidence`: its changes and milestones fall within that week's inclusive dates. `done` holds up to 6 strings of 10 words drawn from those entries. `stalled` holds up to 6 strings of 10 words about work demonstrably unfinished during that week; today's open ends do not prove past state. `time_note` has at most 30 words comparing `commits_review_week` with the priorities; null means the stored measurement cannot establish that week's final count. Keep lists empty when no dated evidence supports an entry. A retained review describes its historical week, not today's remaining work.

## Retain intent, not stale wording

Check the `why` and `evidence` of retained items against current facts and cards. Keep an unfinished focus item's ID while it remains a priority; replace lower-priority work when a stronger reason appears. Remove completed or disproved items. Items rated `down` never return, including rewordings. Items rated `later` wait until `snoozed_until`, or 14 days after their reaction when no date is present. Focus items marked `done` are finished unless evidence shows remaining work.

## Save

Write the brief as UTF-8 JSON in your own temporary folder. Replace `<context-token>` with the token from the complete context and `<brief-file>` with the absolute Windows path to the payload:

```powershell
watchtower portfolio put --context-token <context-token> --file <brief-file>
```

Read the save result and warnings. With `--merge`, a provided list or object replaces that whole field. On a schema error, fix the named fields. On `version_conflict`, reread the context and reconcile changed facts or instructions before using its new token. The output confirms the saved version; no verification read is needed.
