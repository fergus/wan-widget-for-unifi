<#
.SYNOPSIS
  Prepares a release: bumps the app version, dates the changelog and runs every check.

.DESCRIPTION
  Run it on a release branch (not main) with a clean working tree and an "## Unreleased"
  section in CHANGELOG.md. It changes three files and leaves them uncommitted for you to
  review: CHANGELOG.md, src-tauri/Cargo.toml and src-tauri/Cargo.lock. See
  docs/RELEASING.md for the whole process.

.EXAMPLE
  .\scripts\prepare-release.ps1 0.3.0

.EXAMPLE
  .\scripts\prepare-release.ps1 0.3.0 -SkipChecks
#>
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,
    # Edit the files only; do not run fmt, clippy and the tests.
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

function Step($text) { Write-Host "`n== $text" -ForegroundColor Cyan }

# Runs a command and stops the script if it fails.
function Run([string]$dir, [scriptblock]$cmd) {
    Push-Location $dir
    try {
        & $cmd
        if ($LASTEXITCODE -ne 0) { throw "Command failed (exit $LASTEXITCODE): $cmd" }
    }
    finally { Pop-Location }
}

# UTF-8 without a BOM, and the file's own line endings left alone.
function ReadText($path) { [IO.File]::ReadAllText((Join-Path $PWD $path)) }
function WriteText($path, $text) {
    [IO.File]::WriteAllText((Join-Path $PWD $path), $text, (New-Object Text.UTF8Encoding($false)))
}

Step 'Checking the starting point'
$branch = (git branch --show-current).Trim()
if ($branch -in 'main', 'master') {
    throw "On $branch. Releases go through a pull request: create a branch such as release-$Version first."
}
if (git status --porcelain) {
    throw 'The working tree has uncommitted changes. Commit or stash them first.'
}

$changelog = ReadText 'CHANGELOG.md'
if ($changelog -match "(?m)^## $([regex]::Escape($Version)) - ") {
    throw "CHANGELOG.md already has an entry for $Version."
}
if ($changelog -notmatch '(?m)^## Unreleased[ \t]*\r?\n') {
    throw 'CHANGELOG.md has no "## Unreleased" section. Describe the changes there first.'
}
$unreleased = [regex]::Match($changelog, '(?ms)^## Unreleased[ \t]*\r?\n(.*?)(?=^## |\z)').Groups[1].Value
if (-not $unreleased.Trim()) {
    throw 'The "## Unreleased" section of CHANGELOG.md is empty.'
}

$cargo = ReadText 'src-tauri/Cargo.toml'
$current = [regex]::Match($cargo, '(?m)^version = "(.+)"').Groups[1].Value
if ([version]$Version -le [version]$current) {
    throw "$Version is not newer than the current version $current."
}

Step "Bumping $current -> $Version"
$date = Get-Date -Format 'yyyy-MM-dd'
# `(?=\r?$)`: with CRLF line endings `$` matches before the `\n`, after the `\r`.
$dated = [regex]::Replace($changelog, '(?m)^## Unreleased[ \t]*(?=\r?$)', "## $Version - $date", 1)
if ($dated -eq $changelog) {
    throw 'Could not rename the "## Unreleased" heading in CHANGELOG.md.'
}
WriteText 'CHANGELOG.md' $dated
WriteText 'src-tauri/Cargo.toml' ([regex]::Replace($cargo, '(?m)^version = ".+"', "version = `"$Version`"", 1))

# A normal (unlocked) build rewrites the app's entry in Cargo.lock.
Run 'src-tauri' { cargo check }
$lock = ReadText 'src-tauri/Cargo.lock'
if ($lock -notmatch "(?ms)name = `"unifi-widget`"\s+version = `"$([regex]::Escape($Version))`"") {
    throw 'Cargo.lock was not updated to the new version.'
}

if ($SkipChecks) {
    Write-Host "`nSkipping checks." -ForegroundColor Yellow
}
else {
    Step 'widget-core: fmt, clippy, tests'
    Run '.' { cargo fmt --check }
    Run '.' { cargo clippy --all-targets -- -D warnings }
    Run '.' { cargo test }

    Step 'Windows app: fmt, clippy, tests (locked, as in CI)'
    Run 'src-tauri' { cargo fmt --check }
    Run 'src-tauri' { cargo clippy --all-targets --locked -- -D warnings }
    Run 'src-tauri' { cargo test --locked }

    Step 'UI tests'
    Run 'ui' { npm test }
}

Write-Host "`nReady for $Version. Review the changes:" -ForegroundColor Green
git diff --stat
Write-Host @"

Next:
  1. Check README.md and the screenshots match the UI (docs/RELEASING.md, "Docs").
  2. git add -A; git commit -m "Prepare $Version"
  3. Push the branch, open a pull request and wait for CI.
  4. Do the manual checks, merge, then tag v$Version from main.
"@
