# Build Watchtower and install it for the current user.
#
# Result: <Target>\Watchtower.exe plus a Start menu entry "Watchtower". Data (<Target>\data) and
# settings stay untouched on reinstall; the app writes <Target>\skill\watchtower on start.
param(
    [string]$Target = (Join-Path $env:LOCALAPPDATA 'Programs\Watchtower')
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot

cargo build --release --manifest-path (Join-Path $repo 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$exe = Join-Path $Target 'Watchtower.exe'
Get-Process -Name Watchtower -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $exe } |
    ForEach-Object { $_.CloseMainWindow() | Out-Null; if (-not $_.WaitForExit(5000)) { $_.Kill() } }

New-Item -ItemType Directory -Force -Path $Target | Out-Null
Copy-Item -Force (Join-Path $repo 'target\release\Watchtower.exe') $exe
# `status` writes the embedded Skill next to the exe, so agents get the new one right away.
& $exe status | Out-Null

$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Watchtower.lnk'
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $exe
$link.WorkingDirectory = $Target
$link.Description = 'Morning overview of your projects'
$link.Save()

Write-Output "Installed $exe"
Write-Output "Start menu: $shortcut"
