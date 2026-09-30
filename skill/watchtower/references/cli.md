# Watchtower CLI reference

Every command prints one JSON object on stdout; many include `next`, a hint for the usual next command. Success: `{"ok": true, ...}`, exit code 0. Expected failure: `{"ok": false, "error": {"code", "message", "hint"}}`, exit code 1. Wrong arguments print usage on stderr with exit code 2. When `watchtower` is not found, call `%LOCALAPPDATA%\Programs\Watchtower\Watchtower.exe` with the same arguments.

## Daily run

| Command | Returns |
|---|---|
| `watchtower due [--max-age-days N] [--no-refresh]` | Refreshes facts, then `projects` (`id`, `name`, `reason`, and for `changed` also `changes`), `up_to_date`, `problems`, `portfolio` (`due`, `reason`) and `next`. `reason` is `no_card`, `changed` (new commits, also in worktrees and unmerged branches, uncommitted work or edited docs), `ci_changed` (CI turned red or green; a card written while CI was unsettled is due when it turns red), `answered` (the owner replied to a question on the card), `status_changed`, `new_fields` (the card lacks fields Watchtower added after it was written) or `stale` (older than N days, default 14). Any entry may carry `new_fields`: the card fields it lacks, as paths such as `handbook.installed` or `suggestions[].done_when`. `changes` lists what differs from the facts the card was written against: `commits`, `uncommitted`, `unpushed`, `docs`, `side_branches` (commits in extra worktrees or unmerged branches), `readable`; it is null for cards written before Watchtower recorded this. Portfolio `reason`: `missing`, `new_day`, `owner_idea` (the owner parked an idea the brief has not assessed yet), `cards_changed` (a card changed in a field the brief builds on) or `cards_will_change` (cards are due for other reasons than `new_fields`). |
| `watchtower context <id>` | `project` (`id`, `name`, `path`, `status`, `status_note`, `goal`, `goal_due`), `facts`, `new_fields` (fields the previous card lacks because Watchtower added them later, as in `due`; null without a card), `previous_card` (`version`, `created_at`, `card`) or null, `since_previous_card` (`days`, `commit_count`, `commits` as "YYYY-MM-DD subject", newest first) or null, `feedback`: the owner's ratings from all projects and their replies to this project's questions (see below), `feedback_summary`. |
| `watchtower card put <id> --file <path>` | Saves a new card version: `saved`, `version` (this project's card count; its first card is 1), `changed_fields` (top-level fields that differ from the previous card), `unfilled_fields` (fields the previous card lacked that this card does not fill either; they are saved empty, with a warning), `warnings`, `items` (`id`, `kind`, `title` of every item that carries an id). `--file -` reads stdin; `--json '<object>'` passes it inline. `--dry-run` checks the card and returns `saved: false`, `warnings` and `items` without saving; new items show `id: null` because they get their id when saved. With `--merge`, text in fields you did not send is kept even when it is over a word limit, with a warning; other problems in those fields are marked "(from the previous card)". The card is tied to the facts as last measured (by `due` or `facts refresh`); whatever changes after that makes it due again. |
| `watchtower card put <id> --file <path> --merge` | Same, but the document holds only changed top-level fields; all others come from the previous card. A list replaces the previous list completely. |
| `watchtower card show <id>` | The current card with `version` and `created_at`. |
| `watchtower portfolio context` | `today`, `review_week` (`week` as `YYYY-Www`, `from`, `to`: the previous calendar week; `due`: true until a brief has a `week_review` for it), `owner_ideas` (`id`, `text`, `added_at`, `checked`: whether the previous brief assessed it), `projects` (status, note, goal, compact facts with `commits_review_week`, and card per project; the card's `outdated` is true when the facts changed after it was written, and `changed_since_brief` lists the fields the brief builds on that changed since the previous brief, or null), `previous_portfolio`, `owner_last_opened` (when the owner last opened Watchtower, or null), `feedback` for all items, `feedback_summary`. |
| `watchtower portfolio put --file <path> [--merge] [--dry-run]` | Saves the brief; unknown project ids are refused with the list of valid ids, `idea_checks` for ideas that are not parked with `unknown_idea`. A brief without `week_review` keeps the previous brief's review when it covers the same week. Returns `items` like `card put`. |
| `watchtower ideas list` / `ideas add "<text>"` / `ideas remove <id>` | The ideas the owner parked for the brief to assess: `ideas` (`id`, `text`, `added_at`), `idea` (`id`, `text`), `removed`. The owner usually parks ideas in the window; add one only when the owner asks you to. |
| `watchtower portfolio show` | The current brief. |
| `watchtower schema card` / `watchtower schema portfolio` | The JSON schema of a document. |

Items the owner reacts to get an `id` when saved: card `suggestions` and `waiting_on_you`; portfolio `focus`, `patterns`, `synergies`, `new_apps`, `park_or_revive`. Send that `id` again to keep an item's identity and the owner's reaction to it.

## Feedback

The owner reacts to items in the Watchtower window; agents only read the reactions.

| Command | Returns |
|---|---|
| `watchtower feedback [--project <id>]` | `feedback`: every reaction, newest first. `context` and `portfolio context` already include the relevant ones. |

Each reaction has `id` (the item's id), `project` (or null), `value`, `note` (the owner's words, often empty), `at`, and `item`: the item as it read when the owner reacted, with `kind` (`suggestion`, `question`, `focus`, `pattern`, `synergy`, `new_app`, `verdict`) and its fields. Reactions older than this format have only `item.title`.

Values: `up` (the owner likes it), `later` (not now; hidden for 14 days), `down` (rejected; never suggest again) for suggestions and ideas; `answered` (the answer is in `note`) or `done` (settled or no longer relevant) for questions; `done` for focus items.

`feedback_summary` counts the ratings per `kind` and `type` (a suggestion's type or a verdict) as `up`, `later`, `down`.

## Facts

| Command | Returns |
|---|---|
| `watchtower facts show <id>` | `head`, `branch`, `commits` (`total`, `first`, `last`, `last_7d`, `last_30d`, `last_90d`, `weekly`: 26 weekly counts, oldest first), `remotes`, `has_remote`, `upstream`, `ahead`, `behind`, `unpushed_since` (the date of the oldest unpushed commit, or null), `dirty` (`files`, `modified`, `untracked`, `deleted`, `lines_added`, `lines_removed`, `oldest_change` and `newest_change`: the oldest and newest file time among uncommitted files edited after the last commit, or the last commit's time when every such file is older, as with files copied in), `worktrees` (`count`, `prunable`, `items`: `path`, `branch`, `head`, `prunable`, `dirty_files`: uncommitted files in that worktree, null when unreadable), `branches` (`merged_count`, `unmerged`: `name`, `last`, `head`), `release`: the newest tag reachable from HEAD (`tag`, `date`, `commits_since`), null without tags; `night`: work since 18:00 local time yesterday (`since`, `commits` on all local branches, `lines_added`, `lines_removed`, `reverts`, `uncommitted_touched`: uncommitted files in the checkout and its worktrees edited in that time); `collisions`: pairs of lines of work that change the same files, which will conflict when merged (`a` and `b` each with `kind`: `main` for uncommitted work in the checkout, `worktree` for an extra worktree's uncommitted and committed changes, `branch` for an unmerged branch without a worktree, and `name`; `count`; `files`: up to 5); `space`: folders a build or package manager recreates (`target`, `node_modules`, `.venv`, `build`, `dist` and similar, at the top level and one level down), measured once a day (`measured_on`, `total_bytes`, `dirs`: `path`, `bytes`, largest first), `ci` for the default branch: the newest run of every workflow counts and the worst decides (`state`: `green`, `red`, `running`, `none`, `unknown`; `workflow`, `title`, `at`, `url` of the deciding run; `failing`: names of red workflows; `workflows`: the newest run of each workflow with `name`, `state`, `conclusion`, `at`, `url`; `branch`; cancelled and timed-out runs count as red), `docs_mtime`, `last_work` (the local date of the newest commit or uncommitted edit). `upstream`, `ahead`, `behind` and `unpushed_since` are null when the branch has no upstream. A folder that is not a git repository has `ok: false` and `error`. |
| `watchtower facts refresh [<id> ...] [--no-ci]` | Measures again; `refreshed` lists `id`, `ok`, `error`, `changed`. |

## Projects

| Command | Effect |
|---|---|
| `watchtower projects list` | All projects with `status`, `status_note`, `has_card`, `card_updated_at`, `facts_collected_at`. |
| `watchtower projects discover <folder>` | Unregistered git repositories: when the folder is itself a repository, that one first and then its neighbours; otherwise those up to two levels below it, newest first. Returns `folder`, `folder_is_repo`, `already_watched` (count) and `candidates` (`path`, `name`, `suggested_id`, `last_commit`, `recommended`: the chosen repository itself, or active within 120 days). Registers nothing. |
| `watchtower projects add <path> [--id ID] [--name NAME]` | Registers a folder and measures its facts. The id defaults to the folder name in lowercase. |
| `watchtower projects status <id> <active|parked|done|archived> [--note TEXT]` | The owner's intent. Archived projects disappear from runs and the main page. |
| `watchtower projects goal <id> <goal> [--due YYYY-MM-DD]` | The owner's goal for the project; an empty goal clears it. Makes the card due like a status change. |
| `watchtower projects rename <id> <name>` | Changes the display name. |
| `watchtower projects remove <id>` | Stops watching and deletes that project's stored cards. Only when the owner asks. |

## Status

| Command | Returns |
|---|---|
| `watchtower status` | Also writes the Skill that ships with this exe next to it. Returns `version`, `exe`, `data_dir`, `database`, `skill_dir`, `projects` (count), `with_card` (count) and a `hint` when no project is registered yet. |

The Watchtower window itself is opened by the owner; there is no command to start or stop it. It shows new cards and facts within a few seconds of a command writing them.
