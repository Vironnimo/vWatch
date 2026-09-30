# Lookout CLI reference

Every command prints one JSON object on stdout. Success: `{"ok": true, ...}`, exit code 0. Expected failure: `{"ok": false, "error": {"code", "message", "hint"}}`, exit code 1. Wrong arguments print usage on stderr with exit code 2. When `lookout` is not found, call `%LOCALAPPDATA%\Programs\Lookout\Lookout.exe` with the same arguments.

## Daily run

| Command | Returns |
|---|---|
| `lookout due [--max-age-days N] [--no-refresh]` | Refreshes facts, then `projects` (`id`, `name`, `reason`), `up_to_date`, `problems`, `portfolio` (`due`, `reason`) and `next`. `reason` is `no_card`, `changed`, `ci_changed`, `status_changed` or `stale` (older than N days, default 14). |
| `lookout context <id>` | `project` (`id`, `name`, `path`, `status`, `status_note`), `facts`, `previous_card` (`version`, `created_at`, `card`) or null, `since_previous_card` (`days`, `commit_count`, `commits` as "YYYY-MM-DD subject", newest first) or null, `feedback` (`id`, `title`, `value`, `at`). |
| `lookout card put <id> --file <path>` | Saves a new card version: `saved`, `version` (this project's card count; its first card is 1), `warnings`, `rated_items` (`id`, `title`). `--file -` reads stdin; `--json '<object>'` passes it inline. |
| `lookout card put <id> --file <path> --merge` | Same, but the document holds only changed top-level fields; all others come from the previous card. A list replaces the previous list completely. |
| `lookout card show <id>` | The current card with `version` and `created_at`. |
| `lookout portfolio context` | `projects` (compact facts and card per project), `previous_portfolio`, `feedback` for all items. |
| `lookout portfolio put --file <path> [--merge]` | Saves the brief; unknown project ids are refused with the list of valid ids. |
| `lookout portfolio show` | The current brief. |
| `lookout schema card` / `lookout schema portfolio` | The JSON schema of a document. |

Rated items (card `suggestions`; portfolio `patterns`, `synergies`, `new_apps`, `park_or_revive`) get an `id` when saved. Send that `id` again to keep an item's identity and its rating.

| Command | Returns |
|---|---|
| `lookout feedback [--project <id>]` | `feedback`: the owner's ratings, newest first (`id`, `project`, `title`, `value`, `at`). `context` and `portfolio context` already include the relevant ones. |

Feedback values: `up` (the owner likes it), `later` (not now; hidden for 14 days), `down` (rejected; never suggest again).

## Facts

| Command | Returns |
|---|---|
| `lookout facts show <id>` | `head`, `branch`, `commits` (`total`, `first`, `last`, `last_7d`, `last_30d`, `last_90d`, `weekly`: 26 weekly counts, oldest first), `remotes`, `has_remote`, `upstream`, `ahead`, `behind`, `dirty` (`files`, `modified`, `untracked`, `deleted`, `lines_added`, `lines_removed`, `oldest_change` and `newest_change`: file times of the uncommitted files, never earlier than the last commit), `worktrees`, `branches` (`merged_count`, `unmerged`), `ci` for the default branch: the newest run of every workflow counts and the worst decides (`state`: `green`, `red`, `running`, `none`, `unknown`; `workflow`, `title`, `at`, `url` of the deciding run; `failing`: names of red workflows; `branch`), `docs_mtime`, `last_work`. A folder that is not a git repository has `ok: false` and `error`. |
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
| `lookout status` | `version`, `exe`, `data_dir`, `database`, `projects` (count), `with_card` (count) and a `hint` when no project is registered yet. |

The Lookout window itself is opened by the owner; there is no command to start or stop it. It shows new cards and facts within a few seconds of a command writing them.
