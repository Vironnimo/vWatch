# Writing Lookout cards

The owner opens Lookout every morning. A card succeeds when, after ten seconds, the owner knows what state the project is in, what is at risk, what waits for them and what the sensible next step is, and when every sentence is specific to this project. A card fails when it is generic, restates what the owner already knows, or says something that is not true.

All text is German, addressed as "du". Dates are `YYYY-MM-DD`. `lookout schema card` prints the exact JSON schema.

## What Lookout already shows

The page renders the measured facts itself: commit counts and the weekly activity curve, last activity, remote or no remote, uncommitted files and lines, unpushed commits, extra worktrees, unmerged branches, CI state (the newest run of every GitHub workflow on the default branch; the worst one decides, and `ci.failing` names the red workflows). Do not copy these numbers into the card for their own sake. Use them to judge the project and mention them where they matter ("12 Tage Arbeit liegen ohne Commit und ohne Remote nur auf dieser Platte").

The facts see only what git sees: work in git-ignored folders (local data, private notes, build output) does not appear in them. Timestamps with a time (`collected_at`, `dirty.oldest_change`, `ci.at`) are UTC; dates without a time (`today`, `last_work`, commit dates) are local.

## Studying a repository

`lookout context <id>` gives you the path, the facts, the previous card and the commits since it. Then read, in roughly this order, whatever exists:

1. The previous card, if any. For `changed` or `stale`, most of it probably still holds; check what the new commits and edits change.
2. The commit history: `git log --format="%cs %s"`. For large histories, group by prefix or scope and look at merge commits rather than reading every line. Use `git show --stat <commit>` only where a theme is unclear.
3. The project's own documents: README, AGENTS.md or CLAUDE.md, VISION, ROADMAP, TODO, IDEAS, CHANGELOG, `.vorch/PROJECT.md`, `.vorch/FLAGGED.md`, plans under `.vorch/plans/` or `docs/`, review and audit reports. Private notes (for example git-ignored TODO files) often say what the owner actually wants.
4. Uncommitted work: `git diff --stat` for changed files and `git ls-files --others --exclude-standard` for new ones (`git status --short` folds new folders into one line, so its count differs from Lookout's). For a project whose real work is uncommitted, read the changed files' names and the docs written alongside them. For a red CI, `facts.ci.workflows` lists the newest run of every workflow with its `state`, `conclusion` and date; cancelled and timed-out runs count as red. `gh run view <run-id>` names the failing job and step (the id ends the run's `url`); add `--log-failed` only when you need the log. Check the date: a red run weeks old, with nothing pushed since, is a stale warning rather than news, so say so.
5. The source layout, enough to know what the app really does today. Do not read every file.

Stay read-only (see the Skill's rules). Never include credentials, tokens or keys you come across.

## The card fields

`one_liner`, `phase`, `headline` and `reentry` are required; fill the others whenever you found something for them. Keep to the lengths; the page has room for exactly that. Lookout checks every word limit below and refuses a card with a text that is too long, naming the field and its word count. A word is any space-separated piece of text that contains a letter or digit; a lone dash or symbol does not count.

**Each finding has one home.** Unfinished work (uncommitted or unpushed changes, a branch, a half-done plan) goes into `open_ends`; a risk or rot that is not unfinished work into `health_notes`; a decision into `waiting_on_you`; a proposal for new work into `suggestions`. The obvious fix of a finding (commit it, push it, merge it) is not a separate suggestion, and the step named in `reentry` never reappears as one. `headline` and `reentry` may point to the most important finding in their own words; do not repeat it in further fields.

- `one_liner` (≤ 15 words): what the project is, in plain words. "Wartungslog fürs Fahrrad, das an Kettenwechsel und Reifendruck erinnert."
- `stack`: the main technologies, e.g. "Python/FastAPI + Svelte, SQLite".
- `phase`, judged by `facts.last_work` (the newest commit or uncommitted edit): `active` (worked on in the last 7 days), `cooling` (last work 1-4 weeks ago), `dormant` (no work for more than 4 weeks), `maintenance` (finished, only upkeep), `done` (its stated scope is complete). `phase_evidence`: one sentence with the facts behind the choice.
- `headline` (≤ 10 words): the single most important truth about the project right now, like a newspaper headline. "Release 1.2 fertig, aber nie veröffentlicht." Not "Projekt macht Fortschritte."
- `personality` (one sentence): the project speaking about itself, a little playful, strictly true. "Ich kann Statistiken, Export und Erinnerungen, benutzt werde ich nur für den Reifendruck."
- `recent_changes` (3-6, newest first): recent **themes**, not single commits. `title` (≤ 8 words) names the effect a user would notice; `detail` is one sentence. For uncommitted work, date it by the file times and say it is not committed.
- `milestones` (up to 8, oldest first; fewer for a young project): the big steps of the project's whole life, releases included.
- `features` (5-10, ≤ 8 words each): what the app can actually do today, not what is planned.
- `open_ends`: concrete unfinished things you found, each with `kind` (`worktree`, `branch`, `uncommitted`, `unpushed`, `ci`, `flagged`, `todo`, `plan`, `other`), `title` (≤ 10 words), `detail` (one sentence), `source` (file path or git ref; for a finding from Lookout's facts, `facts: <field>`, e.g. `facts: has_remote`) and `age_days`: days since the oldest evidence, such as `dirty.oldest_change` for uncommitted work, the oldest unpushed commit, the first red run, or `commits.first` for a missing remote; null when nothing dates it. Prefer what matters: forgotten finished work, risks, promises in docs that were never kept. Leave out trivia.
- `waiting_on_you`: decisions or answers only the owner can give (a provider to choose, a key to obtain, a direction to pick), each a question (≤ 12 words) with its `source`. Not tasks an agent could do. Usually 0-3 per project, the most consequential first; more only when each one blocks progress. Ask so that a short answer settles it; the owner can answer or dismiss each question in Lookout. Keep a still-open question's `id` when you send it again, and remove questions the owner answered or marked `done`.
- `vision`: `statement` is the project's own stated goal in one sentence; `progress_note` says in one sentence how far along it is against that goal; `progress_pct` only when the docs make it derivable (for example completed phases), otherwise null.
- `reentry`: if the owner opened this project today, the **one** most sensible concrete step (`next_step`, ≤ 20 words), its `effort` (`15min`, `1h`, `half-day`, `day+`) and `why` (one sentence).
- `health_notes`: concrete risks or rot you found, one sentence each: failing checks and why, docs that contradict the code, dependencies that will break, a bug you can point to. May be empty.
- `suggestions` (2-4): see below.
- `cross_project_hints`: things that look reusable in, or overlap with, other kinds of projects; one sentence each. The portfolio brief builds on them.

## Suggestions

A suggestion is worth showing when the owner reads it and thinks "right, I had not seen that". Each has `type` (`next`, `feature`, `cleanup`, `park`), `title` (≤ 8 words), `why` (one sentence, ≤ 25 words), `evidence` (what you saw, with file paths or commits) and `effort` (`S`, `M`, `L`).

- Ground every suggestion in a finding. "Erinnerung reparieren: der Zeitplan rechnet Monate als 30 Tage (src/schedule.ts:42)" is good. "Fehlerbehandlung verbessern" is not.
- Features should follow the project's own vision or the owner's notes, not your taste.
- `park` is welcome when honest: the scope is complete, or everything left needs people, money or decisions rather than code.
- Check the owner's ratings in `context` first (see the Skill's rules). To keep a suggestion from the previous card, send it again with its `id`; a suggestion without `id` counts as new.
- Replace suggestions that are done or no longer true.

## The portfolio brief

`lookout portfolio context` shows every project side by side (compact facts and card), the previous brief, all of the owner's reactions and `owner_last_opened` (when the owner last opened Lookout). If that is older than the previous brief, the owner has not seen it: carry over its focus items that still matter instead of treating them as known. The brief is where you see what nobody sees while inside one project. `lookout schema portfolio` prints the exact schema; only `greeting` is required. Project references use the ids from the context.

- `greeting` (one sentence): a friendly, slightly playful summary of this morning's state, true to the data.
- `focus` (3; fewer only when fewer things need attention): what deserves the owner's attention today, each with `project`, `title` (an action, ≤ 8 words), `why` (≤ 25 words) and `effort`. Mix one thing that is urgent (risk of losing work, something due today), one quick win, and one thing that would feel good.
- `patterns` (2-4): recurring behaviour or risk across projects, each with `title`, `why`, `projects`, `evidence` and `severity` (`info`, `warn`, `risk`). Example: two projects pin the same outdated framework version, and both fail CI because of it.
- `synergies` (2-4): concrete reuse or combination between named projects, with `effort`. Example: one project plans its own login while another already has a finished, tested one.
- `new_apps` (0-3): new app ideas that follow from the portfolio and the owner's evident needs, each with `title`, `pitch`, `because` and `effort`. Only with evidence; none is better than a random idea.
- `park_or_revive`: an honest verdict (`park`, `revive`, `finish`, `archive`) with `why` for quiet or stuck projects. Respect the owner's status: do not tell them to revive a project they parked.
- `attention_note` (one sentence): where the owner's work goes versus where it is needed.

Items you keep from the previous brief keep their `id`. Items rated `down` never come back; `later` items wait 14 days; focus items marked `done` are finished.
