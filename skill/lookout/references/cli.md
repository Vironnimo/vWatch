# Lookout CLI reference

Every command prints one JSON object on stdout; many include `next`, a hint for the usual next command. Success: `{"ok": true, ...}`, exit code 0. Expected failure: `{"ok": false, "error": {"code", "message", "hint"}}`, exit code 1. Wrong arguments print usage on stderr with exit code 2. When `lookout` is not found, call `%LOCALAPPDATA%\Programs\Lookout\Lookout.exe` with the same arguments.

## Daily run

| Command | Returns |
|---|---|
| `lookout due [--max-age-days N] [--no-refresh]` | Refreshes facts, then `projects` (`id`, `name`, `reason`, and for `changed` also `changes`), `up_to_date`, `problems`, `portfolio` (`due`, `reason`) and `next`. `reason` is `no_card`, `changed` (new commits, also in worktrees and unmerged branches, uncommitted work or edited docs), `ci_changed` (CI turned red or green; a card written while CI was unsettled is due when it turns red), `answered` (the owner replied to a question on the card), `status_changed` or `stale` (older than N days, default 14). `changes` lists what differs from the facts the card was written against: `commits`, `uncommitted`, `unpushed`, `docs`, `side_branches` (commits in extra worktrees or unmerged branches), `readable`; it is null for cards written before Lookout recorded this. Portfolio `reason`: `missing`, `new_day`, `cards_changed` (a card is newer than the brief) or `cards_will_change` (cards are due). |
| `lookout context <id>` | `project` (`id`, `name`, `path`, `status`, `status_note`), `facts`, `previous_card` (`version`, `created_at`, `card`) or null, `since_previous_card` (`days`, `commit_count`, `commits` as "YYYY-MM-DD subject", newest first) or null, `feedback`: the owner's ratings from all projects and their replies to this project's questions (see below), `feedback_summary`. |
| `lookout card put <id> --file <path>` | Saves a new card version: `saved`, `version` (this project's card count; its first card is 1), `warnings`, `items` (`id`, `kind`, `title` of every item that carries an id). `--file -` reads stdin; `--json '<object>'` passes it inline. `--dry-run` checks the card and returns `saved: false`, `warnings` and `items` without saving; new items show `id: null` because they get their id when saved. With `--merge`, text in fields you did not send is kept even when it is over a word limit, with a warning; other problems in those fields are marked "(from the previous card)". The card is tied to the facts as last measured (by `due` or `facts refresh`); whatever changes after that makes it due again. |
| `lookout card put <id> --file <path> --merge` | Same, but the document holds only changed top-level fields; all others come from the previous card. A list replaces the previous list completely. |
| `lookout card show <id>` | The current card with `version` and `created_at`. |
| `lookout portfolio context` | `today`, `projects` (compact facts and card per project; the card's `outdated` is true when the facts changed after it was written), `previous_portfolio`, `owner_last_opened` (when the owner last opened Lookout, or null), `feedback` for all items, `feedback_summary`. |
| `lookout portfolio put --file <path> [--merge] [--dry-run]` | Saves the brief; unknown project ids are refused with the list of valid ids. Returns `items` like `card put`. |
| `lookout portfolio show` | The current brief. |
| `lookout schema card` / `lookout schema portfolio` | The JSON schema of a document. |

Items the owner reacts to get an `id` when saved: card `suggestions` and `waiting_on_you`; portfolio `focus`, `patterns`, `synergies`, `new_apps`, `park_or_revive`. Send that `id` again to keep an item's identity and the owner's reaction to it.

## Feedback

The owner reacts to items in the Lookout window; agents only read the reactions.

| Command | Returns |
|---|---|
| `lookout feedback [--project <id>]` | `feedback`: every reaction, newest first. `context` and `portfolio context` already include the relevant ones. |

Each reaction has `id` (the item's id), `project` (or null), `value`, `note` (the owner's words, often empty), `at`, and `item`: the item as it read when the owner reacted, with `kind` (`suggestion`, `question`, `focus`, `pattern`, `synergy`, `new_app`, `verdict`) and its fields. Reactions older than this format have only `item.title`.

Values: `up` (the owner likes it), `later` (not now; hidden for 14 days), `down` (rejected; never suggest again) for suggestions and ideas; `answered` (the answer is in `note`) or `done` (settled or no longer relevant) for questions; `done` for focus items.

`feedback_summary` counts the ratings per `kind` and `type` (a suggestion's type or a verdict) as `up`, `later`, `down`.

## Facts

| Command | Returns |
|---|---|
| `lookout facts show <id>` | `head`, `branch`, `commits` (`total`, `first`, `last`, `last_7d`, `last_30d`, `last_90d`, `weekly`: 26 weekly counts, oldest first), `remotes`, `has_remote`, `upstream`, `ahead`, `behind`, `unpushed_since` (the date of the oldest unpushed commit, or null), `dirty` (`files`, `modified`, `untracked`, `deleted`, `lines_added`, `lines_removed`, `oldest_change` and `newest_change`: the oldest and newest file time among uncommitted files edited after the last commit, or the last commit's time when every such file is older, as with files copied in), `worktrees` (`count`, `prunable`, `items`: `path`, `branch`, `head`, `prunable`, `dirty_files`: uncommitted files in that worktree, null when unreadable), `branches` (`merged_count`, `unmerged`: `name`, `last`, `head`), `ci` for the default branch: the newest run of every workflow counts and the worst decides (`state`: `green`, `red`, `running`, `none`, `unknown`; `workflow`, `title`, `at`, `url` of the deciding run; `failing`: names of red workflows; `workflows`: the newest run of each workflow with `name`, `state`, `conclusion`, `at`, `url`; `branch`; cancelled and timed-out runs count as red), `docs_mtime`, `last_work` (the local date of the newest commit or uncommitted edit). `upstream`, `ahead`, `behind` and `unpushed_since` are null when the branch has no upstream. A folder that is not a git repository has `ok: false` and `error`. |
| `lookout facts refresh [<id> ...] [--no-ci]` | Measures again; `refreshed` lists `id`, `ok`, `error`, `changed`. |

## Projects

| Command | Effect |
|---|---|
| `lookout projects list` | All projects with `status`, `status_note`, `has_card`, `card_updated_at`, `facts_collected_at`. |
| `lookout projects discover <folder>` | Unregistered git repositories: when the folder is itself a repository, that one first and then its neighbours; otherwise those up to two levels below it, newest first. Returns `folder`, `folder_is_repo`, `already_watched` (count) and `candidates` (`path`, `name`, `suggested_id`, `last_commit`, `recommended`: the chosen repository itself, or active within 120 days). Registers nothing. |
| `lookout projects add <path> [--id ID] [--name NAME]` | Registers a folder and measures its facts. The id defaults to the folder name in lowercase. |
| `lookout projects status <id> <active|parked|done|archived> [--note TEXT]` | The owner's intent. Archived projects disappear from runs and the main page. |
| `lookout projects rename <id> <name>` | Changes the display name. |
| `lookout projects remove <id>` | Stops watching and deletes that project's stored cards. Only when the owner asks. |

## Status

| Command | Returns |
|---|---|
| `lookout status` | Also writes the Skill that ships with this exe next to it. Returns `version`, `exe`, `data_dir`, `database`, `skill_dir`, `projects` (count), `with_card` (count) and a `hint` when no project is registered yet. |

The Lookout window itself is opened by the owner; there is no command to start or stop it. It shows new cards and facts within a few seconds of a command writing them.
