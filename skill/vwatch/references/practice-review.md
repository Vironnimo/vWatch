# Reviewing the user's working practices

This guide covers an on-demand review of how the user works across the watched projects: where attention goes, what gets finished, how work is handed to agents and which practices could transfer between repositories. The result is a numbered report in your reply; only the findings the user then selects are saved, as agent proposals.

## Authority

Until the user selects findings, the review only reads: vWatch through its CLI, and repositories under the same read-only rules as a daily run. Do not record ratings, own ideas, tasks, goals or status from your findings; those are the user's decisions. Agent chat transcripts and session logs are outside the review unless the user names their location, because they hold personal content beyond the projects.

## Choose the period

Use the period the user names. Otherwise review the last 90 days through today and compare them with the 90 days before. Git activity is reconstructed from up to 365 days of history; vWatch's own records (fact observations, ratings, tasks, runs) start when vWatch began recording, which can be much later. State each source's actual coverage in the report.

## Collect the evidence

Replace `<first-date>` and `<last-date>` with the period's first and last day as YYYY-MM-DD; run the comparison period the same way. Use `--json` only for the project list: its readable form omits folders, goals and status dates.

```powershell
vwatch projects list --json
vwatch statistics --from <first-date> --until <last-date>
vwatch portfolio show
vwatch feedback list
vwatch tasks list
vwatch runs status
```

Then read `vwatch facts show <project-id>` for each active project: it holds hotspots, unpushed commits, uncommitted edit times, worktrees, unmerged branches and the release tag. For each repository folder, read the commit history of the period:

```powershell
git -C "<folder>" log --since=<first-date> --until=<last-date> --date=iso-local --format="%ad %s"
```

For transferable practices, compare the repositories' agent instructions (such as `AGENTS.md`, `CLAUDE.md`), test and CI configuration, Git hooks and scripts; `git -C "<folder>" ls-files` locates them. Read further only where it confirms or refutes a candidate finding. Do not run `statistics --json`; its daily series exceed a megabyte, and the Git log above answers the same questions.

## Look through these lenses

1. Attention and intent: compare commits and active days per project with status, goals and the brief's focus. Which projects received work they were not prioritized for, which goals saw none, and how many projects had commits in the same week?
2. Rhythm: commit hours and weekdays, and many commits on few active days. Do late or burst sessions precede `fix` or `revert` commits on the same files?
3. Rework: hotspots and areas rewritten repeatedly within the period.
4. Finishing: uncommitted work and its oldest edit, `unpushed_since`, extra worktrees, unmerged branches and commits since the last release tag.
5. Handoffs to agents: tasks per status, blocked or reopened tasks, reports waiting in `review`, questions waiting for answers.
6. Taste: reactions by item kind and category. Repeated `down` notes name kinds of proposals to stop making.
7. Transferable practices: instructions, tests, hooks, CI, commit conventions or scripts that one repository has and others lack, where the difference plausibly explains a finding from the other lenses.

Commits and lines measure activity, not time spent or quality; a coverage gap is unknown, not idle. vWatch does not see agent sessions, working hours or token use. Call something a pattern only with at least 3 occurrences or 2 projects. When a lens has fewer records, such as 2 tasks, report the count and that the lens lacks evidence instead of generalizing.

## Report

Write the report in the language of the conversation. Start with the periods and each source's coverage. List at most 8 findings, strongest first. Each finding has a number, a title, the observation with its evidence (projects, files, commits, counts), the proposed change with its observable result, and a `Save as` line: `card <project-id>` for one project, `brief` for several projects, or `not saved` for an observation without an action. Name the category, `idea` for a new capability or simpler workflow, `recommendation` for upkeep or a next step. Before naming a card, check its suggestions in `vwatch card show <project-id>`: a card holds at most 4 of each category, so for a full category name the retained item the finding would replace. Close with one line naming lenses that lacked evidence and one question asking which findings to save.

```text
Period <first-date> to <last-date>, compared with <comparison period>. Git activity covers both periods; vWatch records start <date> (<count> ratings, <count> tasks).

1. <finding title>
   Observation: <what happened, with files, commits and counts>
   Change: <proposed change>; done when <observable result>
   Save as: card <project-id> (recommendation)

2. <finding title>
   Observation: <evidence across the named projects>
   Change: <proposed change>; done when <observable result>
   Save as: brief (idea)

Too little evidence: <lens> (<count> records).
Which findings should I save as proposals?
```

## Save the selected findings

Save only the findings the user names, writing their text in the `report_language` from the context; use English when it is absent.

For a card, read every page of `vwatch context <project-id>` and keep its token. Skip a finding that a `retained_proposals` entry already covers and tell the user its ID. Write a UTF-8 JSON file in your temporary folder whose `suggestions` list holds every current suggestion as `{"id": "<its id>"}`, except one the user agreed to replace, plus the new item without `id`: `category`, `type` (`next`, `feature`, `cleanup` or `park`), `title`, `why`, `evidence` and `done_when`. Word limits: `title` 8, `why` 25, `done_when` 20. Save it:

```powershell
vwatch card put <project-id> --context-token <context-token> --merge --file "<card-file>"
```

For the brief, save card findings first. Then read every page of `vwatch portfolio context`, and write a file whose `synergies` list holds every current synergy as `{"id": "<its id>"}` plus the new item: `category`, `title`, `why`, `projects` (project IDs) and `evidence`, with the same word limits. Save it:

```powershell
vwatch portfolio put --context-token <context-token> --merge --file "<brief-file>"
```

Done when each receipt marks the new items `new` and the retained ones `kept`. Report the saved items with their IDs; the user rates them in the window like other proposals.
