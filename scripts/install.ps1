# Build Watchtower and install it for the current user.
#
# Result: Watchtower.exe (CLI), Watchtower-UI.exe (window), and a Start menu entry.
# Data and preferences survive reinstall. An existing autostart entry for the old
# combined executable is migrated to the window; disabled autostart stays disabled.
param(
    [string]$Target = (Join-Path $env:LOCALAPPDATA 'Programs\Watchtower')
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$Target = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Target)

cargo build --release --bins --manifest-path (Join-Path $repo 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$cliExe = Join-Path $Target 'Watchtower.exe'
$guiExe = Join-Path $Target 'Watchtower-UI.exe'
$running = @(Get-Process -Name Watchtower, Watchtower-UI -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $cliExe -or $_.Path -eq $guiExe })
# A legacy Watchtower.exe with a window is the old GUI. A process without a
# window may be an agent command: never interrupt it to install an update.
foreach ($process in $running) {
    if (-not $process.HasExited -and $process.MainWindowHandle -eq 0) {
        throw "Watchtower process $($process.Id) is still running without a window. Wait for agent commands to finish, then install again."
    }
}
foreach ($process in $running) {
    if ($process.HasExited) { continue }
    if (-not $process.CloseMainWindow() -or -not $process.WaitForExit(10000)) {
        throw "Watchtower window $($process.Id) did not close. Close it, then install again."
    }
}

New-Item -ItemType Directory -Force -Path $Target | Out-Null
Copy-Item -Force -LiteralPath (Join-Path $repo 'target\release\Watchtower.exe') -Destination $cliExe
Copy-Item -Force -LiteralPath (Join-Path $repo 'target\release\Watchtower-UI.exe') -Destination $guiExe
# `status` writes the embedded Skill next to the exe, so agents get the new one right away.
& $cliExe status | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Watchtower status failed after installation' }

$runKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Run', $true)
if ($null -ne $runKey) {
    try {
        $entry = $runKey.GetValue('Watchtower', $null)
        if ($entry -is [string] -and ($entry -eq $cliExe -or $entry -eq ('"' + $cliExe + '"'))) {
            $runKey.SetValue('Watchtower', ('"' + $guiExe + '"'), [Microsoft.Win32.RegistryValueKind]::String)
        }
    } finally {
        $runKey.Dispose()
    }
}

$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Watchtower.lnk'
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $guiExe
$link.WorkingDirectory = $Target
$link.Description = 'Morning overview of your projects'
$link.Save()

Write-Output "Installed CLI: $cliExe"
Write-Output "Installed window: $guiExe"
Write-Output "Start menu: $shortcut"
