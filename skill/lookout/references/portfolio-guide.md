# Writing the Lookout portfolio brief

The brief is the top of the owner's morning page: one view across all projects. All text is German, addressed as "du". Lookout refuses a text that is too long and names the field. Word limits: `greeting` and `attention_note` 30; every `title` 8; `why`, `pitch`, `because` 25; `done_when` 20. Effort: `focus` uses `15min`, `1h`, `half-day`, `day+`; `synergies` and `new_apps` use `S`, `M`, `L`.

## Before you write

`lookout portfolio context` shows every project side by side (compact facts and card; `lookout card show <id>` has the full card), the previous brief, all of the owner's reactions and `owner_last_opened` (when the owner last opened Lookout). If that is older than the previous brief, the owner has not seen it: carry over its focus items that still matter instead of treating them as known. If it is newer, the owner has seen the brief: keep its items only while they are still true and still among the most important. The brief is where you see what nobody sees while inside one project. Card contents count as sources: cite them as the card's statement ("laut gusto-Karte …") and check in the facts or the repository what the brief depends on. A card marked `outdated` was written before the latest facts; trust the facts where they disagree. `changed_since_brief` lists the card fields that changed since the previous brief (empty: nothing the brief builds on; null: the project had no card then); start from those. Check the numbers, dates and states the brief names (uncommitted lines, unpushed commits, CI) against today's facts; card statements need no new reading of the repository unless the card is `outdated`.

## The fields

`lookout schema portfolio` prints the exact schema; only `greeting` is required. Project references use the ids from the context.

- `greeting` (one sentence): a friendly, slightly playful summary of this morning's state, true to the data.
- `focus` (3; fewer only when fewer things need attention): what deserves the owner's attention today, each with `project`, `title` (an action, ≤ 8 words), `why` (≤ 25 words), `effort` and `done_when` (≤ 20 words: the observable result that shows it is finished; the owner can hand a focus item to a coding agent with one click). Aim for one thing that is urgent (risk of losing work, something due today, a goal whose `goal_due` is near while the work lags), one quick win, and one thing that would feel good; urgency wins over the mix. An unfinished focus item may stay, with the same `id`, as long as it is still among the most important things; drop it when something more urgent needs its place.
- `patterns` (2-4): recurring behaviour or risk across projects, each with `title`, `why`, `projects`, `evidence` and `severity` (`info`, `warn`, `risk`). Example: two projects pin the same outdated framework version, and both fail CI because of it.
- `synergies` (2-4): concrete reuse or combination between named projects, with `effort`. Example: one project plans its own login while another already has a finished, tested one.
- `new_apps` (0-3): new app ideas that follow from the portfolio and the owner's evident needs, each with `title`, `pitch`, `because` and `effort`. Only with evidence; none is better than a random idea.
- `park_or_revive`: an honest verdict (`park`, `revive`, `finish`, `archive`) with `why` for quiet or stuck projects. Respect the owner's status: do not tell them to revive a project they parked.
- `attention_note` (one sentence): where the owner's work goes versus where it is needed.

## Keeping items

Items you keep from the previous brief keep their `id`; check each kept item's `why` and `evidence` against today's facts and cards, and correct or drop what is no longer true. Items rated `down` never come back; `later` items wait 14 days; focus items marked `done` are finished.
