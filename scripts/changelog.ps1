<#
.SYNOPSIS
Generate CHANGELOG entries from Conventional Commit subjects.

.DESCRIPTION
Reads commit subjects since the most recent `vX.Y.Z` tag, groups them by
Conventional Commit type, and emits one bullet per commit. A bullet is a commit
subject, so entries stay short by construction.

Rationale that does not fit in a subject belongs in the commit body, an ADR
under docs/decisions/, or docs/roadmap.md -- not in the changelog.

Prints to stdout by default. Use -Apply to rewrite CHANGELOG.md in place.

.PARAMETER Version
Version for the new section. Omit to write an Unreleased section.

.PARAMETER Since
Override the starting ref. Defaults to the highest `vX.Y.Z` tag.

.PARAMETER Apply
Rewrite CHANGELOG.md instead of printing.

.EXAMPLE
just changelog

.EXAMPLE
pwsh scripts/changelog.ps1 -Version 0.6.0 -Apply
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$Since,
    [switch]$Apply
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$changelogFile = Join-Path $root 'CHANGELOG.md'
$prBaseUrl = 'https://github.com/schgoo/specgate/pull'

# Maps a Conventional Commit type to a shared group key.
$typeGroups = @{
    'chore' = 'task'
    'doc'   = 'docs'
}

# Maps a group key to its changelog heading.
$headings = @{
    'breaking'      = 'Breaking'
    'feat'          = 'Added'
    'fix'           = 'Fixed'
    'perf'          = 'Performance'
    'refactor'      = 'Changed'
    'docs'          = 'Documentation'
    'test'          = 'Testing'
    'build'         = 'Build'
    'ci'            = 'Continuous Integration'
    'task'          = 'Tasks'
    'style'         = 'Styling'
    'miscellaneous' = 'Miscellaneous'
}

$order = @('breaking', 'feat', 'fix', 'perf', 'refactor', 'test', 'docs', 'build', 'ci', 'task', 'style', 'miscellaneous')

# Nothing is dropped. Oxidizer filters `test` commits, but SpecGate is
# CTSC-first: a test commit usually changes golden artifacts or fixture
# coverage, which is product-visible.
$ignoredTypes = @()

$conventional = [regex]'^(?<type>[a-z]+)(?:\((?<scope>[^)]+)\))?(?<breaking>!)?:\s*(?<description>.+)$'
$prReference = [regex]'\s*\(#(?<number>\d+)\)$'

function Get-LatestVersionTag {
    $tags = @(git tag --list 'v*' 2>$null)
    $versioned = @($tags | Where-Object { $_ -match '^v\d+\.\d+\.\d+$' })
    if ($versioned.Count -eq 0) { return $null }
    return @($versioned | Sort-Object { [version]($_.Substring(1)) })[-1]
}

$startRef = if ($Since) { $Since } else { Get-LatestVersionTag }
if (-not $startRef) {
    Write-Warning 'No vX.Y.Z tag found. Reading the whole history.'
}
$range = if ($startRef) { "$startRef..HEAD" } else { 'HEAD' }

$subjects = @(git log $range --no-merges --pretty=format:'%s')
if ($subjects.Count -eq 0) {
    Write-Warning "No commits since '$startRef'. Nothing to write."
    return
}

$grouped = [ordered]@{}
foreach ($subject in $subjects) {
    $match = $conventional.Match($subject)
    if ($match.Success) {
        $type = $match.Groups['type'].Value
        $description = $match.Groups['description'].Value
        $isBreaking = $match.Groups['breaking'].Value -eq '!'
    } else {
        $type = 'miscellaneous'
        $description = $subject
        $isBreaking = $false
    }

    if ($ignoredTypes -contains $type) { continue }

    $pr = $prReference.Match($description)
    if ($pr.Success) {
        $number = $pr.Groups['number'].Value
        $description = $prReference.Replace($description, " ([#$number]($prBaseUrl/$number))")
    }

    $key = if ($isBreaking) {
        'breaking'
    } elseif ($typeGroups.ContainsKey($type)) {
        $typeGroups[$type]
    } else {
        $type
    }

    if (-not $grouped.Contains($key)) { $grouped[$key] = [System.Collections.ArrayList]::new() }
    [void]$grouped[$key].Add("- $description")
}

if ($grouped.Count -eq 0) {
    Write-Warning 'Every commit was filtered out. Nothing to write.'
    return
}

$known = @($order | Where-Object { $grouped.Contains($_) })
$unknown = @($grouped.Keys | Where-Object { $order -notcontains $_ } | Sort-Object)

$heading = if ($Version) {
    "## [$Version] - {0}" -f (Get-Date).ToString('yyyy-MM-dd')
} else {
    '## [Unreleased]'
}

$lines = @($heading, '')
foreach ($key in @($known + $unknown)) {
    $name = if ($headings.ContainsKey($key)) { $headings[$key] } else { $key }
    $lines += @("### $name", '') + @($grouped[$key]) + @('')
}

if (-not $Apply) {
    $lines | ForEach-Object { Write-Output $_ }
    Write-Host ''
    Write-Host "Preview only. Re-run with -Apply to rewrite $changelogFile." -ForegroundColor Yellow
    return
}

$existing = if (Test-Path $changelogFile) { Get-Content $changelogFile -Raw } else { "# Changelog`n`n" }

# Fold any hand-curated `## [Unreleased]` body into the new section and drop the
# orphaned heading. Curated prose leads, generated bullets follow. This is the
# escape hatch for a change whose subject line cannot carry it.
if ($Version) {
    $unreleasedPattern = '(?ims)^##[ \t]+(?:\[Unreleased[^\]]*\]|Unreleased)[^\r\n]*\r?\n(?<body>.*?)(?=^##[ \t]|\z)'
    $unreleased = [regex]::Match($existing, $unreleasedPattern)
    if ($unreleased.Success) {
        $curated = @($unreleased.Groups['body'].Value -split "`r?`n")
        while ($curated.Count -gt 0 -and [string]::IsNullOrWhiteSpace($curated[-1])) {
            $curated = if ($curated.Count -eq 1) { @() } else { @($curated[0..($curated.Count - 2)]) }
        }
        while ($curated.Count -gt 0 -and [string]::IsNullOrWhiteSpace($curated[0])) {
            $curated = if ($curated.Count -eq 1) { @() } else { @($curated[1..($curated.Count - 1)]) }
        }
        if ($curated.Count -gt 0) {
            $lines = @($lines[0], '') + $curated + @('') + @($lines[2..($lines.Count - 1)])
        }
        $existing = $existing.Remove($unreleased.Index, $unreleased.Length)
    }
}

# Insert ahead of the first version section, not immediately after the title,
# so any preamble between the two stays above the releases.
if ($existing -notmatch '(?m)^# Changelog\s*$') {
    throw "$changelogFile has no '# Changelog' heading to insert under."
}
$firstSection = [regex]::Match($existing, '(?m)^##[ \t]')
$insertAt = if ($firstSection.Success) {
    $firstSection.Index
} else {
    $existing.Length
}
$updated = $existing.Substring(0, $insertAt) + (($lines -join "`n") + "`n") + $existing.Substring($insertAt)
[System.IO.File]::WriteAllText($changelogFile, $updated, (New-Object System.Text.UTF8Encoding $false))
Write-Host "Changelog updated at '$changelogFile'."