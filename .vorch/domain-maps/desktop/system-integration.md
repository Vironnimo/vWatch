# Desktop system integration

Use this when changing `src/system.rs`, GUI startup or OS integration. Packaging and installation procedures belong to `development.md`.

## Paths and embedded resources

`app_dir` resolves the executable's folder. Both executables live there with `data/` and `skill/watchtower/`. `WATCHTOWER_HOME` overrides the data folder, not the executable or exported Skill folder. `data_path` resolves without creating directories so help/usage can remain read-only; `data_dir` creates the folder. The database is `watchtower.db` inside that selected data folder.

`SKILL_FILES` embeds the operating Skill and its CLI, card and portfolio guides with `include_str!`. `export_skill` writes only content that differs. GUI startup and the console `status` command export the matching embedded version next to the executable. Changes to embedded wording belong to `agent-interface.md`; update source files rather than treating an exported copy as canonical.

The settings response supplies the exported Skill path and a vBot install command with `--scope own`. Preserve the actual scope when updating an installed operating Skill: an agent-private installation must not become global/shared without the owner's explicit request. An external vBot caller needs the intended agent's scope as documented in `README.md`.

## Window lifecycle

`src/bin/watchtower-ui.rs` selects the Windows GUI subsystem and calls `app::run`. The adapter opens the service, exports the Skill and creates a hidden Tauri WebView using the selected data folder's `webview/` cache. The page's `ready` call shows the rendered window; a timed fallback avoids leaving the application invisible after a page failure. The single-instance plugin focuses the existing window rather than processing console commands there.

## Windows integration

`set_autostart` updates the current user's Watchtower Run entry and targets `Watchtower-UI.exe`. `set_agent_access` changes only the app-folder entry in the user's PATH. Its normalization avoids duplicate entries and its `REG_EXPAND_SZ` write preserves environment placeholders; an environment-change broadcast lets other applications notice the update. These capabilities are owner choices in the settings, excluded from routine agent operations.

`open_external` handles HTTP(S) URLs or filesystem targets. It expands `%VARIABLE%` and normalizes slashes on Windows. Existing files are selected in Explorer; it does not run a file or a handbook command. Keep opening a location separate from executing a suggested task.

## References

- Editing exported Skill instructions or install-command guidance -> `agent-interface/agent-facing-text.md`
- Building/installing the executables and preserving owner settings -> `development.md`
