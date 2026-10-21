<#
.SYNOPSIS
Check that documented diagnostic fragments exist in SpecGate's sources.

.DESCRIPTION
Between `<!-- diagnostics:start -->` and `<!-- diagnostics:end -->` in the
troubleshooting guide, the first cell of each table row holds one diagnostic
fragment in backticks. Each fragment must appear verbatim in a non-test Rust
or C# source file under `rust/` or `csharp/`, so a search for the message
finds the code that raises it.

.PARAMETER Root
Repository root. Defaults to the parent of this script's directory.

.PARAMETER Guide
Guide to check, relative to Root.
#>
[CmdletBinding()]
param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot),
    [string]$Guide = 'docs/guides/troubleshoot-capture.md'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path -LiteralPath $Root).Path
$guidePath = Join-Path $Root $Guide
$skipped = @('target', 'bin', 'obj', '.git', 'tests', 'testdata', 'fixtures')

$lines = [IO.File]::ReadAllLines($guidePath)
$start = [Array]::IndexOf($lines, '<!-- diagnostics:start -->')
$end = [Array]::IndexOf($lines, '<!-- diagnostics:end -->')
if ($start -lt 0 -or $end -le $start) {
    Write-Output "docs-diagnostics: ${Guide} has no <!-- diagnostics:start --> ... <!-- diagnostics:end --> region"
    exit 1
}

$fragments = [Collections.Generic.List[object]]::new()
for ($index = $start + 1; $index -lt $end; $index++) {
    $line = $lines[$index]
    if ($line -notmatch '^\|' -or $line -match '^\|\s*(Message fragment|-+)\s*\|') {
        continue
    }
    $cell = $line.Split('|')[1].Trim()
    if ($cell -notmatch '^`([^`]+)`$') {
        Write-Output "docs-diagnostics: ${Guide}:$($index + 1): first cell is not one backticked fragment: $cell"
        exit 1
    }
    $fragments.Add([pscustomobject]@{ Line = $index + 1; Text = $Matches[1] })
}
if ($fragments.Count -eq 0) {
    Write-Output "docs-diagnostics: ${Guide} lists no diagnostic fragments"
    exit 1
}

function Get-Sources([string]$Directory) {
    if (-not (Test-Path -LiteralPath $Directory -PathType Container)) {
        return
    }
    foreach ($entry in Get-ChildItem -LiteralPath $Directory -Force) {
        if ($entry.PSIsContainer) {
            if ($skipped -notcontains $entry.Name -and $entry.Name -notlike '*.Tests') {
                Get-Sources $entry.FullName
            }
        }
        elseif ($entry.Extension -in '.rs', '.cs' -and $entry.Name -ne 'tests.rs' -and
            $entry.Name -notlike '*_tests.rs' -and $entry.Name -notlike '*Tests.cs') {
            $entry.FullName
        }
    }
}

function Remove-TestOnlyRust([string]$Text) {
    # cfg(test) constants (for example replay/limitation.rs) mirror producer
    # wording for tests and must not satisfy the check on their own.
    $Text = [regex]::Replace($Text, '(?m)^[ \t]*#\[cfg\([^\]\r\n]*\btest\b[^\]\r\n]*\)\][ \t]*\r?\n[ \t]*(pub(\([^)]*\))?[ \t]+)?(const|static)\b[^\r\n]*', '')
    # clippy::items_after_test_module keeps inline test modules last.
    $module = [regex]::Match($Text, '(?m)^[ \t]*#\[cfg\(test\)\][ \t]*\r?\n[ \t]*mod[ \t]+\w+[ \t]*\{')
    if ($module.Success) {
        $Text = $Text.Substring(0, $module.Index)
    }
    $Text
}

$sources = @(foreach ($directory in 'rust', 'csharp') { Get-Sources (Join-Path $Root $directory) }) |
    ForEach-Object {
        $text = [IO.File]::ReadAllText($_)
        if ($_.EndsWith('.rs')) { Remove-TestOnlyRust $text } else { $text }
    }

$missing = @($fragments | Where-Object {
        $text = $_.Text
        -not ($sources | Where-Object { $_.Contains($text) } | Select-Object -First 1)
    })
if ($missing.Count -gt 0) {
    $missing | ForEach-Object { Write-Output "missing diagnostic: ${Guide}:$($_.Line): '$($_.Text)' appears in no non-test source under rust/ or csharp/" }
    exit 1
}
Write-Output "docs-diagnostics: $($fragments.Count) fragments in $Guide found in $($sources.Count) sources"
