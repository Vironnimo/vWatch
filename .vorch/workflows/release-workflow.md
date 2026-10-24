# Installation Workflow

Use when the owner requests installation of finished work. There is no remote release pipeline.

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
