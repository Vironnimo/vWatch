---
name: lookout
description: Operate Lookout, the owner's morning overview of all their software projects. Use for the daily Lookout run (find which projects need a new card, study those repositories, write project cards and the cross-project brief), for adding, removing or re-statusing watched projects, and for answering questions from the stored cards, facts and ratings.
---

# Lookout

Lookout is a local app the owner opens every morning to see where each of their software projects stands, what is at risk, what waits for their decision and what to do next. Most of these projects are built by coding agents, so the owner often does not know their state. Lookout is only worth opening if what it says is specific, true and new.

Lookout holds two kinds of data:

- **Facts** that Lookout measures itself from git: commits and weekly activity, whether a remote (backup) exists, uncommitted files and lines, unpushed commits, extra worktrees, unmerged branches, GitHub CI state. You read them; you never compute or store them.
- **Interpretation** that agents write: one **card** per project and one **portfolio brief** across all projects. This is your job.

## Calling Lookout

Run `lookout <command>`. If `lookout` is not found, call the program by its full path: `%LOCALAPPDATA%\Programs\Lookout\Lookout.exe` (bash: `"$LOCALAPPDATA/Programs/Lookout/Lookout.exe"`). If that file does not exist either, stop and tell the owner that Lookout is not installed or lives elsewhere; turning on "Für Agents aufrufbar" in Lookout's settings makes `lookout` available by name.

Every command prints one JSON object. `"ok": true` means success. `"ok": false` comes with `error.code`, `error.message` and usually `error.hint`: follow the hint and try again. Never read or write Lookout's database or files directly. `lookout status` shows the version, the data folder and how many projects have a card.

The full command list with JSON shapes is in `references/cli.md`. Read only the section you need.

## What do you need to do?

- **The daily run** (a scheduled task, or the owner asks you to update Lookout): follow "Daily run" below.
- **Add projects:** the owner usually adds projects in the Lookout window. When the owner asks you to, `lookout projects discover <folder>` lists git repositories there that are not watched yet and `lookout projects add <path>` registers one. Add only the projects the owner named or confirmed.
- **The owner changes a project's intent** ("park needle-net", "gusto is done"): `lookout projects status <id> <active|parked|done|archived> --note "<reason>"`.
- **Questions about projects:** `lookout card show <id>`, `lookout facts show <id>`, `lookout portfolio show`, `lookout feedback`. Answer from these; say when a card is old or missing.

## Daily run

1. Run `lookout due`. It refreshes the facts and returns:
   - `projects`: each project that needs a new card, with a `reason`: `no_card`, `changed` (new commits, also in worktrees and unmerged branches, uncommitted work or edited docs since the last card), `ci_changed` (CI turned red or green since the last card), `answered` (the owner answered one of the card's questions), `status_changed` (the owner changed its intent) or `stale` (the card is old).
   - `problems`: projects whose folder is gone. Mention them in your report; do not write cards for them.
   - `portfolio.due`: whether the cross-project brief needs rewriting.
   If nothing is due, report that in one sentence and stop.
2. **Before your first card of the run, read `references/card-guide.md` completely.** It says what to read in a repository and how to write every field. The quality of the whole app depends on it.
3. For each due project:
   1. `lookout context <id>`: the project, its current facts, the previous card, the commits since that card, and the owner's reactions: ratings from all projects and replies to this card's questions.
   2. Study the repository read-only, as the card guide describes. For `changed` or `stale`, start from the previous card and the new commits instead of re-reading everything. For `ci_changed`, find out what the CI change means (`facts.ci`, the failing run) and update only the fields it affects with `--merge`. For `answered`, work the owner's replies into the card with `--merge` (see the rules).
   3. Write the card as JSON to a temporary file and run `lookout card put <id> --file <path>`. When only some fields changed, send just those with `--merge`. A list you send replaces the whole list, so include the items you keep, with their `id`.
   4. Read the result. On `invalid_card`, fix the named fields and send again (`--dry-run` checks a card without saving it). Take `warnings` about ignored fields seriously: that content was dropped.
   If you can delegate to sub-agents, you may give each due project to one. Each needs this Skill, the card guide and the project id, and returns only whether its card was saved.
4. When all cards are written and `portfolio.due` was true (or any card changed), run `lookout portfolio context`, then write the brief as `references/card-guide.md` → "The portfolio brief" describes, and save it with `lookout portfolio put --file <path>`.
5. Report in a few lines: which cards you updated, anything urgent (risk of losing work, something time-bound today) and the brief's three focus items. The owner reads the details in Lookout, so do not repeat whole cards.

## Rules

- **Repositories are read-only.** Read files and use git and `gh` only for reading: git `log`, `show`, `diff`, `status`, `ls-files`, `branch`, `tag`, `rev-list`, `blame`, `worktree list`; `gh` `run`, `pr`, `issue` and `release` with `list` or `view`, and `repo view`. Never edit, commit, stage, fetch, pull, checkout, stash, install, build, or start an app or test suite in a watched repository, and never run git commands that rewrite its `.git` folder either (`gc`, `prune`, `worktree prune`, `fsck --lost-found`, `maintenance`), even when you notice something that is easy to fix. Put it in the card instead.
- **Outside the repository, read only what belongs to the project**, and only read: its installed version, its own data or configuration folders (for example under the user's home), the shortcuts and autostart entries that start it (the Start menu, the `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` key), its registered scheduled tasks and coding-agent automations. Never change anything there. Never copy credentials, tokens or personal data into a card; for the owner's own content in such places (notes, messages, recordings), say only what kind of thing exists, never quote it.
- **Only what you saw.** Every statement must rest on something you read: a file, a commit, a fact. If you are unsure, leave it out. Generic advice ("add tests", "improve the docs", "add dark mode") without a concrete finding is worse than nothing, because it teaches the owner to skip Lookout.
- **Learn from the owner's reactions.** `context` and `portfolio context` list them under `feedback`: the item as it read (`item`, with its `kind`), the owner's `value`, an optional `note` in their words, and the date `at`. `feedback_summary` counts the ratings per kind and type of item.
  - `down`: never suggest that idea again, not even in new words. `later`: do not bring it back within 14 days of its `at` date. `up`: the owner likes the direction; keep the item (same `id`) until it is done, or suggest its natural next step.
  - Ratings from other projects count too: they show which kinds of ideas the owner values. When they have rejected a kind of suggestion several times, propose that kind only on an unusually strong finding. A `note` gives the reason; follow it wherever it applies.
  - A question marked `answered` (the answer is in `note`) or `done`: never ask it again. Carry the answer into the card where it belongs (`reentry`, `suggestions`, `vision`, `open_ends`) and drop the question from `waiting_on_you`.
  - A focus item marked `done`: the owner did it. Do not put it into focus again unless the facts show it is unfinished.
- **Respect documented decisions.** When a project's own docs record a deliberate choice ("no remote on purpose", "no tests for the prototype"), put it into the card's `decisions`, mention it as that choice where it matters, not as a risk or warning, and do not suggest undoing it. The portfolio brief follows the cards' `decisions` the same way.
- **Respect the owner's status.** `parked`: describe the state calmly; do not push to revive it. `done`: focus on anything that could rot (backups, failing CI, security). `archived` projects never appear in a run.
- **Write for the owner:** all text in German, addressed as "du", short and concrete. The card guide sets the lengths.
