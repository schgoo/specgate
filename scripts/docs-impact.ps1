<#
.SYNOPSIS
Keep product docs tied to the code they describe.

.DESCRIPTION
The impact map, docs/contributor/docs-impact.json, assigns every non-test
product source (rust/crates/*/src, csharp/SpecGate.*, binding-schema.json) to
the pages that describe it. A rule with no pages must give a reason.

Without -Base, the script validates the map: code globs match files, listed
pages exist, and every product source is covered.

With -Base, it also checks the commits since the merge base with that ref. A
rule fails when its code changed but none of its pages did, unless a commit
carries a `Docs-Impact: none <reason>` trailer.

.PARAMETER Root
Repository root. Defaults to the parent of this script's directory.

.PARAMETER Base
Git ref to diff against, for example origin/main.

.EXAMPLE
pwsh scripts/docs-impact.ps1 -Base origin/main
#>
[CmdletBinding()]
param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot),
    [string]$Base
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path -LiteralPath $Root).Path
$mapPath = Join-Path $Root 'docs/contributor/docs-impact.json'
$skipped = @('target', 'bin', 'obj', '.git', 'node_modules', 'artifacts', '.specgate')
$problems = [Collections.Generic.List[string]]::new()

function ConvertTo-Regex([string]$Glob) {
    $pattern = [regex]::Escape($Glob).Replace('\*\*/', '(?:.*/)?').Replace('\*\*', '.*').Replace('\*', '[^/]*').Replace('\?', '[^/]')
    "^$pattern$"
}

function Test-ProductSource([string]$Path) {
    if ($Path -match '(^|/)tests?(/|\.rs$)' -or $Path -match '(^|/)(bin|obj)/') {
        return $false
    }
    ($Path -match '^rust/crates/[^/]+/src/.+\.rs$') -or ($Path -match '^csharp/SpecGate\.[^/]+/.+\.(cs|targets)$') -or ($Path -eq 'binding-schema.json')
}

function Get-RepositoryFiles([string]$Directory) {
    foreach ($entry in Get-ChildItem -LiteralPath $Directory -Force) {
        if ($entry.PSIsContainer) {
            if ($skipped -notcontains $entry.Name) {
                Get-RepositoryFiles $entry.FullName
            }
        }
        else {
            $entry.FullName.Substring($Root.Length + 1).Replace('\', '/')
        }
    }
}

function Invoke-Git([string[]]$Arguments) {
    $output = & git -C $Root @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "docs-impact: git $($Arguments -join ' ') failed"
    }
    @($output | Where-Object { $_ })
}

if (-not (Test-Path -LiteralPath $mapPath -PathType Leaf)) {
    throw "docs-impact: missing $mapPath"
}
$rules = @((Get-Content -LiteralPath $mapPath -Raw | ConvertFrom-Json).rules)
$files = @(Get-RepositoryFiles $Root)
$ids = @{}

foreach ($rule in $rules) {
    if ($ids.ContainsKey($rule.id)) {
        $problems.Add("rule '$($rule.id)' is declared twice")
    }
    $ids[$rule.id] = $true
    $rule | Add-Member -NotePropertyName patterns -NotePropertyValue @($rule.code | ForEach-Object { ConvertTo-Regex $_ })
    foreach ($glob in $rule.code) {
        $regex = ConvertTo-Regex $glob
        if (-not ($files | Where-Object { $_ -match $regex } | Select-Object -First 1)) {
            $problems.Add("rule '$($rule.id)' code glob '$glob' matches no file")
        }
    }
    foreach ($doc in $rule.docs) {
        if (-not (Test-Path -LiteralPath (Join-Path $Root $doc) -PathType Leaf)) {
            $problems.Add("rule '$($rule.id)' names missing doc '$doc'")
        }
    }
    if (@($rule.docs).Count -eq 0 -and -not ($rule.PSObject.Properties['reason'] -and $rule.reason)) {
        $problems.Add("rule '$($rule.id)' has no docs and no reason")
    }
}

function Test-RuleMatch($Rule, [string]$Path) {
    [bool]($Rule.patterns | Where-Object { $Path -match $_ } | Select-Object -First 1)
}

$productSources = @($files | Where-Object { Test-ProductSource $_ })
foreach ($file in $productSources) {
    if (-not ($rules | Where-Object { Test-RuleMatch $_ $file } | Select-Object -First 1)) {
        $problems.Add("product source '$file' is covered by no rule")
    }
}

if ($problems.Count -gt 0) {
    $problems | ForEach-Object { Write-Host "docs-impact: $_" -ForegroundColor Red }
    exit 1
}

if (-not $Base) {
    Write-Output "docs-impact: map covers $($productSources.Count) product sources with $($rules.Count) rules"
    exit 0
}

$mergeBase = Invoke-Git @('merge-base', $Base, 'HEAD') | Select-Object -First 1
$range = "$mergeBase..HEAD"
$changed = @(Invoke-Git @('diff', '--name-only', $mergeBase, 'HEAD'))
$bypass = @(Invoke-Git @('log', '--format=%(trailers:key=Docs-Impact,valueonly)', $range))
$reasoned = @($bypass | Where-Object { $_.Trim() -match '^none\b\W*\w' })
$failed = $false

$missing = @(foreach ($rule in $rules | Where-Object { @($_.docs).Count -gt 0 }) {
        $touched = @($changed | Where-Object { (Test-ProductSource $_) -and (Test-RuleMatch $rule $_) })
        if ($touched.Count -gt 0 -and -not ($changed | Where-Object { $rule.docs -contains $_ } | Select-Object -First 1)) {
            [pscustomobject]@{ Rule = $rule; Touched = $touched }
        }
    })
if ($missing.Count -gt 0 -and $reasoned.Count -gt 0) {
    Write-Output "docs-impact: skipped $($missing.Count) map rule(s) by trailer 'Docs-Impact: $($reasoned[0].Trim())'"
}
elseif ($missing.Count -gt 0) {
    $failed = $true
    foreach ($entry in $missing) {
        Write-Host "docs-impact: rule '$($entry.Rule.id)' code changed without a doc update" -ForegroundColor Red
        $entry.Touched | ForEach-Object { Write-Host "  changed: $_" }
        $entry.Rule.docs | ForEach-Object { Write-Host "  review:  $_" }
    }
    if ($bypass.Count -gt 0) {
        Write-Host "docs-impact: 'Docs-Impact' trailers need the form 'none <reason>'" -ForegroundColor Red
    }
    Write-Host "docs-impact: update a listed page, or add a 'Docs-Impact: none <reason>' commit trailer after checking every listed page." -ForegroundColor Red
}

if ($failed) {
    exit 1
}
Write-Output "docs-impact: $($changed.Count) changed files since $Base keep the map in step"
