# Build the release package for GitHub Releases.
#
# Result: .tmp\release\vWatch-<version>-windows-x64.zip with both executables,
# install.ps1 and LICENSE. Extracted, `install.ps1` installs it without Rust or
# Python. The version comes from Cargo.toml.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$version = (Select-String -LiteralPath (Join-Path $repo 'Cargo.toml') -Pattern '^version = "([^"]+)"' |
    Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { throw 'Could not read the version from Cargo.toml' }
$buildTarget = & python (Join-Path $PSScriptRoot 'check.py') target-dir
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($buildTarget)) {
    throw 'Could not resolve the primary Cargo target directory'
}

# Cargo discovers .cargo/config.toml from its working directory.
Push-Location -LiteralPath $repo
try {
    cargo build --release --bins --locked --target-dir $buildTarget
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally {
    Pop-Location
}

$output = Join-Path $repo '.tmp\release'
$name = "vWatch-$version"
$stage = Join-Path $output $name
$archive = Join-Path $output "$name-windows-x64.zip"
foreach ($stale in @($stage, $archive)) {
    if (Test-Path -LiteralPath $stale) { Remove-Item -LiteralPath $stale -Recurse -Force }
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null
foreach ($binary in @('vWatch.exe', 'vWatch-UI.exe')) {
    Copy-Item -LiteralPath (Join-Path $buildTarget "release\$binary") -Destination $stage
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'install.ps1') -Destination $stage
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination $stage
Compress-Archive -Path $stage -DestinationPath $archive
Remove-Item -LiteralPath $stage -Recurse -Force

$hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Output "Package: $archive"
Write-Output "SHA256: $hash"
