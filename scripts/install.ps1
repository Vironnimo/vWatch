# Install vWatch for the current user.
#
# From a source checkout (scripts\install.ps1) it builds both executables first.
# From an extracted release package (install.ps1 beside the executables) it
# installs them without Rust or Python.
#
# Result: vWatch.exe (CLI), vWatch-UI.exe (window), and a Start menu entry.
# Data and preferences survive reinstall. An existing autostart entry for the old
# combined executable is migrated to the window; disabled autostart stays disabled.
param(
    [string]$Target = (Join-Path $env:LOCALAPPDATA 'Programs\vWatch')
)
$ErrorActionPreference = 'Stop'
$Target = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Target)
if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'vWatch-UI.exe') -PathType Leaf) {
    $release = $PSScriptRoot
} else {
    $repo = Split-Path -Parent $PSScriptRoot
    $buildTarget = & python (Join-Path $PSScriptRoot 'check.py') target-dir
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($buildTarget)) {
        throw 'Could not resolve the primary Cargo target directory'
    }
    $release = Join-Path $buildTarget 'release'

    # Cargo discovers .cargo/config.toml from its working directory, not from
    # --manifest-path. Keep the checkout's scratch paths even when called elsewhere.
    Push-Location -LiteralPath $repo
    try {
        cargo build --release --bins --target-dir $buildTarget
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    } finally {
        Pop-Location
    }
}

$cliExe = Join-Path $Target 'vWatch.exe'
$guiExe = Join-Path $Target 'vWatch-UI.exe'
$running = @(Get-Process -Name vWatch, vWatch-UI -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $cliExe -or $_.Path -eq $guiExe })
# A legacy vWatch.exe with a window is the old GUI. A process without a
# window may be an agent command: never interrupt it to install an update.
foreach ($process in $running) {
    if (-not $process.HasExited -and $process.MainWindowHandle -eq 0) {
        throw "vWatch process $($process.Id) is still running without a window. Wait for agent commands to finish, then install again."
    }
}
foreach ($process in $running) {
    if ($process.HasExited) { continue }
    if (-not $process.CloseMainWindow() -or -not $process.WaitForExit(10000)) {
        throw "vWatch window $($process.Id) did not close. Close it, then install again."
    }
}

New-Item -ItemType Directory -Force -Path $Target | Out-Null
Copy-Item -Force -LiteralPath (Join-Path $release 'vWatch.exe') -Destination $cliExe
Copy-Item -Force -LiteralPath (Join-Path $release 'vWatch-UI.exe') -Destination $guiExe
# `status` writes the embedded Skill next to the exe, so agents get the new one right away.
& $cliExe status | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'vWatch status failed after installation' }

$runKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Run', $true)
if ($null -ne $runKey) {
    try {
        $entry = $runKey.GetValue('vWatch', $null)
        if ($entry -is [string] -and ($entry -eq $cliExe -or $entry -eq ('"' + $cliExe + '"'))) {
            $runKey.SetValue('vWatch', ('"' + $guiExe + '"'), [Microsoft.Win32.RegistryValueKind]::String)
        }
    } finally {
        $runKey.Dispose()
    }
}

$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'vWatch.lnk'
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $guiExe
$link.WorkingDirectory = $Target
$link.Description = 'Morning overview of your projects'
$link.Save()

Write-Output "Installed CLI: $cliExe"
Write-Output "Installed window: $guiExe"
Write-Output "Start menu: $shortcut"
