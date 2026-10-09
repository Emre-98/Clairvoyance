<#
.SYNOPSIS
  Publishes a new Clairvoyance version: bumps the version everywhere, adds the release notes to
  CHANGELOG.md, commits, tags and pushes. GitHub Actions then builds the installer, signs it and
  creates the GitHub Release with latest.json (installed copies update themselves from it).

.EXAMPLE
  .\scripts\release.ps1 1.0.1 -Notes "Faster library", "Fixed thumbnails for short games"

.EXAMPLE
  .\scripts\release.ps1 1.1.0            # asks for the notes, one line at a time
#>
param(
  [Parameter(Mandatory = $true, Position = 0)][string]$Version,
  [string[]]$Notes,
  [switch]$DryRun
)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

$script:GitExe = (Get-Command git -CommandType Application | Select-Object -First 1).Source
# git writes progress and warnings to stderr; Windows PowerShell would treat that as an error.
function Invoke-Git {
  $old = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  try { $out = & $script:GitExe @args 2>&1 } finally { $ErrorActionPreference = $old }
  $out | ForEach-Object { "$_" }
  if ($LASTEXITCODE -ne 0) { throw "git $args failed ($LASTEXITCODE)" }
}

$Version = $Version.TrimStart("v")
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3" }
$tag = "v$Version"

if (-not $DryRun) {
  if (Invoke-Git status --porcelain) { throw "You have uncommitted changes. Commit or stash them first." }
  $branch = (Invoke-Git rev-parse --abbrev-ref HEAD | Select-Object -First 1).Trim()
  if ($branch -ne "main") { throw "Release from the main branch (you're on $branch)." }
  Invoke-Git fetch --tags --quiet origin | Out-Null
  if (Invoke-Git tag --list $tag) { throw "Tag $tag already exists." }
}

$current = ((Get-Content app/tauri.conf.json -Raw) | ConvertFrom-Json).version
if ([version]$Version -le [version]$current) { throw "New version $Version must be higher than the current $current." }
Write-Host "Releasing $current -> $Version"

function Edit-File($path, [scriptblock]$change) {
  $text = [IO.File]::ReadAllText((Resolve-Path $path))
  $new = & $change $text
  if ($new -eq $text) { throw "Nothing changed in $path" }
  if (-not $DryRun) { [IO.File]::WriteAllText((Resolve-Path $path), $new, (New-Object Text.UTF8Encoding $false)) }
}

# Cargo.toml: [workspace.package] version (the first version line).
Edit-File "Cargo.toml" { param($t) ([regex]'(?m)^version = "[^"]+"').Replace($t, "version = `"$Version`"", 1) }
# Cargo.lock: our own crates.
Edit-File "Cargo.lock" { param($t)
  foreach ($c in "clairvoyance", "cv-core", "cv-capture", "cv-mock-league", "cv-game-league", "cv-game-cs2", "cv-game-deadlock") {
    $t = [regex]::Replace($t, "(name = `"$c`"\r?\nversion = `")[^`"]+`"", "`${1}$Version`"")
  }
  $t }
# Tauri config (the installer and the updater use this version).
Edit-File "app/tauri.conf.json" { param($t) ([regex]'"version": "[^"]+"').Replace($t, "`"version`": `"$Version`"", 1) }
# UI package.
Edit-File "ui/package.json" { param($t) ([regex]'"version": "[^"]+"').Replace($t, "`"version`": `"$Version`"", 1) }
Edit-File "ui/package-lock.json" { param($t) ([regex]'("name": "clairvoyance-ui",\s*"version": ")[^"]+"').Replace($t, "`${1}$Version`"", 2) }

# Release notes.
if (-not $Notes) {
  Write-Host "What's new in $Version? One line per change, empty line to finish:"
  $Notes = @()
  while ($true) { $l = Read-Host " -"; if (-not $l) { break }; $Notes += $l }
}
if (-not $Notes) { $Notes = @("Improvements and fixes.") }
$date = Get-Date -Format "yyyy-MM-dd"
$section = "## [$Version] - $date`n`n" + (($Notes | ForEach-Object { "- $_" }) -join "`n") + "`n`n"
Edit-File "CHANGELOG.md" { param($t) ([regex]'(?m)^## ').Replace($t, "$section## ", 1) }

if ($DryRun) { Write-Host "Dry run: nothing written."; Write-Host $section; exit 0 }

Invoke-Git add Cargo.toml Cargo.lock app/tauri.conf.json ui/package.json ui/package-lock.json CHANGELOG.md
Invoke-Git commit -m "Release $tag"
Invoke-Git tag -a $tag -m "Clairvoyance $tag"
Invoke-Git push origin main
Invoke-Git push origin $tag
Write-Host ""
Write-Host "Pushed $tag. GitHub Actions is building the release (about 10-15 minutes):"
Write-Host "  https://github.com/Emre-98/Clairvoyance/actions"
Write-Host "When it's green, the release is at https://github.com/Emre-98/Clairvoyance/releases/tag/$tag"
Write-Host "and installed copies will offer the update within a few hours (or Settings > Check now)."
