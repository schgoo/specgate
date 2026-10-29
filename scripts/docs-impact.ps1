<#
.SYNOPSIS
Keep product docs tied to the code they describe.

.DESCRIPTION
Two guards share this script.

The impact map, docs/contributor/docs-impact.json, assigns every non-test
product source (rust/crates/*/src, csharp/SpecGate.*, binding-schema.json) to
the pages that describe it. A rule with no pages must give a reason.

A claim anchor pins one statement in a Markdown page to the code that makes it
true. Each dependency is a file, or a file and a function or type name joined
by `::`:

    <!-- docs-claim property-setup-depth: rust/crates/specgate-discovery/src/discovery/setup.rs::fold_operation -->
    Deconstruction is one level deep.
    <!-- /docs-claim property-setup-depth -->

Without -Base, the script validates both guards: map globs match files, listed
pages exist, every product source is covered, every claim marker is closed, and
every claim dependency still exists.

With -Base, it also checks the commits since the merge base with that ref:
- A map rule fails when its code changed but none of its pages did, unless a
  commit carries a `Docs-Impact: none <reason>` trailer.
- A claim fails when a dependency changed but the claim text did not, unless a
  commit carries a `Docs-Reviewed: <claim-id>` trailer. Function and type
  dependencies use `git log -L`, so only changes inside that item count.

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
$claimPattern = [regex]'(?s)<!--\s*docs-claim\s+(?<id>[a-z0-9][a-z0-9-]*)\s*:(?<deps>.*?)-->(?<body>.*?)<!--\s*/docs-claim\s+\k<id>\s*-->'
$markerPattern = [regex]'<!--\s*/?docs-claim\b'
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

# Blank fenced blocks and inline code so documented marker examples are not claims.
function Get-ScannableText([string]$Text) {
    $Text = $Text.Replace("`r`n", "`n")
    $Text = [regex]::Replace($Text, '(?ms)^[ \t]*```.*?^[ \t]*```', { param($m) [regex]::Replace($m.Value, '[^\n]', ' ') })
    [regex]::Replace($Text, '`[^`\n]*`', { param($m) ' ' * $m.Value.Length })
}

function Get-Claims([string]$Path, [string]$Text) {
    $Text = Get-ScannableText $Text
    $found = @(foreach ($match in $claimPattern.Matches($Text)) {
            [pscustomobject]@{
                Id           = $match.Groups['id'].Value
                Path         = $Path
                Line         = $Text.Substring(0, $match.Index).Split("`n").Count
                Dependencies = @($match.Groups['deps'].Value -split '\s+' | Where-Object { $_ })
                Text         = ($match.Groups['deps'].Value + "`n" + $match.Groups['body'].Value).Trim()
            }
        })
    [pscustomobject]@{ Claims = $found; Markers = $markerPattern.Matches($Text).Count }
}

function Split-Dependency([string]$Dependency) {
    $index = $Dependency.IndexOf('::')
    if ($index -lt 0) {
        return $Dependency, ''
    }
    $Dependency.Substring(0, $index), $Dependency.Substring($index + 2)
}

function Test-Symbol([string]$Path, [string]$Symbol) {
    if ($Symbol -notmatch '^[A-Za-z_][A-Za-z0-9_]*$') {
        return $false
    }
    $name = [regex]::Escape($Symbol)
    $text = Get-Content -LiteralPath (Join-Path $Root $Path) -Raw
    if ($Path -like '*.rs') {
        return $text -match "\b(fn|struct|enum|union|trait|type|const|static|mod)\s+$name\b|macro_rules!\s*$name\b"
    }
    if ($Path -like '*.cs') {
        return $text -match "\b(class|struct|record|interface|enum)\s+$name\b|(?m)^\s*(\[[^\]]*\]\s*)*((public|private|protected|internal|static|async|override|virtual|sealed|abstract|partial|unsafe|extern|new)\s+)+[\w<>\[\],.? ]+\s+$name\s*(<[^>]*>)?\s*\("
    }
    $false
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

$pages = @($files | Where-Object { $_ -like '*.md' })
$claims = @(foreach ($page in $pages) {
        $scan = Get-Claims $page (Get-Content -LiteralPath (Join-Path $Root $page) -Raw)
        if ($scan.Markers -ne 2 * $scan.Claims.Count) {
            $problems.Add("$page has an unclosed, mismatched, or nested docs-claim marker")
        }
        foreach ($claim in $scan.Claims) {
            $where = "$($claim.Path):$($claim.Line) claim '$($claim.Id)'"
            if ($claim.Dependencies.Count -eq 0) {
                $problems.Add("$where names no dependency")
            }
            foreach ($dependency in $claim.Dependencies) {
                $file, $symbol = Split-Dependency $dependency
                if (-not (Test-Path -LiteralPath (Join-Path $Root $file) -PathType Leaf)) {
                    $problems.Add("$where depends on missing file '$file'")
                }
                elseif ($symbol -and -not (Test-Symbol $file $symbol)) {
                    $problems.Add("$where depends on '$symbol', which is not declared in '$file'")
                }
            }
            $claim
        }
    })

if ($problems.Count -gt 0) {
    $problems | ForEach-Object { Write-Host "docs-impact: $_" -ForegroundColor Red }
    exit 1
}

if (-not $Base) {
    $claimPages = @($claims | ForEach-Object Path | Sort-Object -Unique).Count
    Write-Output "docs-impact: map covers $($productSources.Count) product sources with $($rules.Count) rules; $($claims.Count) claims anchored in $claimPages pages"
    exit 0
}

$mergeBase = Invoke-Git @('merge-base', $Base, 'HEAD') | Select-Object -First 1
$range = "$mergeBase..HEAD"
$hasCommits = [int](Invoke-Git @('rev-list', '--count', $range) | Select-Object -First 1) -gt 0
$changed = @(Invoke-Git @('diff', '--name-only', $mergeBase, 'HEAD'))
$bypass = @(Invoke-Git @('log', '--format=%(trailers:key=Docs-Impact,valueonly)', $range))
$reasoned = @($bypass | Where-Object { $_.Trim() -match '^none\b\W*\w' })
$reviewed = @(Invoke-Git @('log', '--format=%(trailers:key=Docs-Reviewed,valueonly)', $range) | ForEach-Object { ($_.Trim() -split '\s+')[0] })
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

function Get-CommittedClaims([string]$Revision, [string]$Path) {
    $text = & git -C $Root show "${Revision}:$Path" 2>$null
    if ($LASTEXITCODE -ne 0) {
        return @()
    }
    (Get-Claims $Path ($text -join "`n")).Claims
}

$dependencyChanged = @{}
function Test-DependencyChanged([string]$Dependency) {
    if (-not $dependencyChanged.ContainsKey($Dependency)) {
        $file, $symbol = Split-Dependency $Dependency
        # -L takes a basic regex; a declared name is always followed by '(', '<', '{', or a space.
        $commits = @(if (-not $hasCommits) {
            }
            elseif ($symbol) {
                Invoke-Git @('log', '--no-patch', '--format=%H', "-L:[^A-Za-z0-9_]$symbol[^A-Za-z0-9_]:$file", $range)
            }
            else {
                Invoke-Git @('diff', '--name-only', $mergeBase, 'HEAD', '--', $file)
            })
        $dependencyChanged[$Dependency] = $commits.Count -gt 0
    }
    $dependencyChanged[$Dependency]
}

$staleClaims = @(foreach ($page in $claims | ForEach-Object Path | Sort-Object -Unique) {
        $before = @(Get-CommittedClaims $mergeBase $page)
        foreach ($claim in Get-CommittedClaims 'HEAD' $page) {
            $edited = -not ($before | Where-Object { $_.Id -eq $claim.Id -and $_.Text -eq $claim.Text } | Select-Object -First 1)
            if ($edited -or $reviewed -contains $claim.Id) {
                continue
            }
            $stale = @($claim.Dependencies | Where-Object { Test-DependencyChanged $_ })
            if ($stale.Count -gt 0) {
                [pscustomobject]@{ Claim = $claim; Changed = $stale }
            }
        }
    })
if ($staleClaims.Count -gt 0) {
    $failed = $true
    foreach ($entry in $staleClaims) {
        Write-Host "docs-impact: claim '$($entry.Claim.Id)' at $($entry.Claim.Path):$($entry.Claim.Line) depends on changed code" -ForegroundColor Red
        $entry.Changed | ForEach-Object { Write-Host "  changed: $_" }
    }
    Write-Host "docs-impact: edit each listed claim, or add a 'Docs-Reviewed: <claim-id>' commit trailer after confirming the claim is still true." -ForegroundColor Red
}

if ($failed) {
    exit 1
}
Write-Output "docs-impact: $($changed.Count) changed files since $Base keep the map and $($claims.Count) claims in step"
