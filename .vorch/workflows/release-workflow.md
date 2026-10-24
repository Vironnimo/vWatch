# Installation and Release Workflow

Use when the owner requests installation of finished work or a GitHub release. There is no CI release pipeline; releases are built and uploaded from this machine.

1. Read the development and desktop maps. Review the exact source and pending changes in the installation checkout. Run relevant tests and the complete local check for a larger change; do not install known failures.
2. Check for agents using the installed console. The installer independently refuses replacement while a CLI is running; do not kill that process to bypass the guard. The normal window will be asked to close.
3. Run from the intended checkout:

   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts/install.ps1
   ```

   Both release binaries go to `%LOCALAPPDATA%/Programs/vWatch`. The script preserves data/preferences and updates the Start menu link. Existing enabled autostart can be retargeted to the GUI; it is not newly enabled. Leave autostart and PATH choices intact.
4. Start the installed `vWatch-UI.exe` again. Verify changed behavior with the live-testing workflow, using separate data for mutations.
5. `status` exports four embedded Skill files beside the executable. For an agent application's separate installed copy, locate and preserve its actual scope. Private installations stay private unless the owner requests otherwise. Preserve unrelated private adaptations.
6. Report binaries/Skill copies updated, verification and any partial failure. A failed later step does not roll back earlier saved effects.

After a build/replacement failure, inspect the error and existing files before retrying. Do not erase data, reset preferences or introduce repository cleanup.

## Publishing a GitHub release

Only on the owner's request, after the version is committed and pushed (`push-workflow.md`):

1. Raise `version` in `Cargo.toml` (and `tauri.conf.json`) when the release needs a new number, and commit it.
2. Run `powershell -ExecutionPolicy Bypass -File scripts/package.ps1` from the primary checkout on the pushed commit. It builds with the locked dependencies and writes `.tmp/release/vWatch-<version>-windows-x64.zip` with both executables, `install.ps1` and `LICENSE`, and prints its SHA256.
3. Check the package in a scratch folder under `.tmp/`: extract it and run its `vWatch.exe status --data-dir <scratch>`. Do not run its `install.ps1` against a scratch target: the installer always rewrites the owner's Start menu entry. The installer's package mode is covered by the development tests.
4. Write release notes for users: what is new, what changed, what to back up or do when upgrading. Commit lists are not release notes. While vWatch is below 1.0, mark releases as pre-release.
5. `gh release create v<version> <zip> --prerelease --title "vWatch <version>" --notes-file <notes>`, with the SHA256 in the notes. The executables are unsigned; the README tells users about the SmartScreen warning.
6. When visible UI or capabilities changed, refresh the README screenshots and claims before the release (`scripts/readme-assets/`).
