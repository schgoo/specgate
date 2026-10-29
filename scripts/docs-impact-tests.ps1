$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script = Join-Path $PSScriptRoot 'docs-impact.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-docs-impact-tests-$([guid]::NewGuid().ToString('N'))"
$dependency = 'rust/crates/core/src/lib.rs::depth'
$claim = "<!-- docs-claim depth: $dependency -->`nDecomposition is one level deep.`n<!-- /docs-claim depth -->"
$map = @'
{
  "rules": [
    { "id": "core", "code": ["rust/crates/core/src/**"], "docs": ["docs/core.md"] },
    { "id": "tool", "code": ["rust/crates/tool/src/**"], "docs": [], "reason": "internal" }
  ]
}
'@
$core = "pub fn depth() -> u32 {`n    1`n}`n`npub fn other() -> u32 {`n    2`n}`n"

function Write-File([string]$Relative, [string]$Text) {
    $path = Join-Path $fixture $Relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
    [IO.File]::WriteAllText($path, $Text.Replace("`r`n", "`n"))
}

function Invoke-Check([string]$Base) {
    $arguments = @('-NoLogo', '-NoProfile', '-File', $script, '-Root', $fixture)
    if ($Base) {
        $arguments += @('-Base', $Base)
    }
    $output = & pwsh @arguments 2>&1 | Out-String
    [pscustomobject]@{ Exit = $LASTEXITCODE; Output = $output }
}

function Assert-Pass([string]$Name, [string]$Base, [string]$Expected = 'docs-impact:') {
    $result = Invoke-Check $Base
    if ($result.Exit -ne 0 -or -not $result.Output.Contains($Expected)) {
        throw "docs-impact rejected $Name (exit $($result.Exit)):`n$($result.Output)"
    }
}

function Assert-Fail([string]$Name, [string]$Base, [string]$Expected) {
    $result = Invoke-Check $Base
    if ($result.Exit -eq 0 -or -not $result.Output.Contains($Expected)) {
        throw "docs-impact accepted $Name (exit $($result.Exit)):`n$($result.Output)"
    }
}

function Assert-StaticFail([string]$Name, [string]$Relative, [string]$Text, [string]$Expected) {
    $path = Join-Path $fixture $Relative
    $original = if (Test-Path -LiteralPath $path) { [IO.File]::ReadAllText($path) }
    Write-File $Relative $Text
    try {
        Assert-Fail $Name '' $Expected
    }
    finally {
        if ($null -eq $original) {
            Remove-Item -LiteralPath $path
        }
        else {
            [IO.File]::WriteAllText($path, $original)
        }
    }
}

function Invoke-Git {
    $output = & git -C $fixture -c user.name=test -c user.email=test@example.invalid -c core.autocrlf=false @args 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "git $($args -join ' ') failed:`n$output"
    }
}

function Save-Commit([string]$Message, [string]$Trailer) {
    Invoke-Git add -A
    if ($Trailer) {
        Invoke-Git commit -q --allow-empty -m $Message -m $Trailer
    }
    else {
        Invoke-Git commit -q --allow-empty -m $Message
    }
}

try {
    Write-File '.gitattributes' "*.rs diff=rust`n"
    Write-File 'docs/contributor/docs-impact.json' $map
    Write-File 'rust/crates/core/src/lib.rs' $core
    Write-File 'rust/crates/core/tests/ignored.rs' "fn uncovered() {}`n"
    Write-File 'rust/crates/tool/src/lib.rs' "pub struct Tool;`n"
    Write-File 'docs/core.md' @"
# Core

$claim

``````markdown
<!-- docs-claim example: missing.rs -->
``````

Inline ``<!-- docs-claim example: missing.rs -->`` is an example too.
"@

    Assert-Pass 'a valid map and claim' '' 'map covers 2 product sources with 2 rules'
    Assert-Pass 'fenced and inline marker examples' '' '1 claims anchored in 1 pages'

    Assert-StaticFail 'an uncovered product source' 'rust/crates/extra/src/lib.rs' "pub fn x() {}`n" "product source 'rust/crates/extra/src/lib.rs' is covered by no rule"
    Assert-StaticFail 'a dead code glob' 'docs/contributor/docs-impact.json' $map.Replace('rust/crates/tool/src/**', 'rust/crates/gone/src/**') "code glob 'rust/crates/gone/src/**' matches no file"
    Assert-StaticFail 'a missing doc page' 'docs/contributor/docs-impact.json' $map.Replace('docs/core.md', 'docs/gone.md') "names missing doc 'docs/gone.md'"
    Assert-StaticFail 'a rule without docs or reason' 'docs/contributor/docs-impact.json' $map.Replace(', "reason": "internal"', '') "rule 'tool' has no docs and no reason"
    Assert-StaticFail 'a duplicate rule id' 'docs/contributor/docs-impact.json' $map.Replace('"id": "tool"', '"id": "core"') "rule 'core' is declared twice"
    Assert-StaticFail 'an unclosed claim' 'docs/other.md' "<!-- docs-claim open: $dependency -->`nText.`n" 'docs/other.md has an unclosed, mismatched, or nested docs-claim marker'
    Assert-StaticFail 'a nested claim' 'docs/other.md' "<!-- docs-claim a: $dependency -->`n$claim`n<!-- /docs-claim a -->`n" 'docs/other.md has an unclosed, mismatched, or nested docs-claim marker'
    Assert-StaticFail 'a claim without dependencies' 'docs/other.md' "<!-- docs-claim bare: -->`nText.`n<!-- /docs-claim bare -->`n" "claim 'bare' names no dependency"
    Assert-StaticFail 'a missing dependency file' 'docs/other.md' "<!-- docs-claim gone: rust/gone.rs -->`nText.`n<!-- /docs-claim gone -->`n" "depends on missing file 'rust/gone.rs'"
    Assert-StaticFail 'a missing dependency symbol' 'docs/other.md' "<!-- docs-claim gone: rust/crates/core/src/lib.rs::dept -->`nText.`n<!-- /docs-claim gone -->`n" "depends on 'dept', which is not declared"

    Invoke-Git init -q -b main
    Save-Commit 'base'
    Invoke-Git branch base

    Assert-Pass 'an empty diff' 'base' '0 changed files since base'

    Write-File 'rust/crates/core/src/lib.rs' $core.Replace('    2', '    3')
    Save-Commit 'change other'
    Assert-Fail 'code changed without a doc update' 'base' "rule 'core' code changed without a doc update"
    Save-Commit 'bare' 'Docs-Impact: none'
    Assert-Fail 'a Docs-Impact trailer without a reason' 'base' "'Docs-Impact' trailers need the form 'none <reason>'"
    Save-Commit 'reason' 'Docs-Impact: none other() is internal'
    Assert-Pass 'an unrelated function change with a reasoned trailer' 'base' 'skipped 1 map rule(s)'

    Write-File 'rust/crates/core/src/lib.rs' $core.Replace('    1', '    2')
    Save-Commit 'change depth' 'Docs-Impact: none covered by claim'
    Assert-Fail 'a changed claim dependency' 'base' "claim 'depth' at docs/core.md:3 depends on changed code"
    Save-Commit 'review' 'Docs-Reviewed: depth'
    Assert-Pass 'a reviewed claim' 'base'

    Invoke-Git reset -q --hard HEAD~1
    Write-File 'docs/core.md' ([IO.File]::ReadAllText((Join-Path $fixture 'docs/core.md')).Replace('one level deep', 'recursive'))
    Save-Commit 'update claim'
    Assert-Pass 'an edited claim' 'base'

    Invoke-Git reset -q --hard base
    Write-File 'rust/crates/tool/src/lib.rs' "pub struct Tool2;`n"
    Write-File 'rust/crates/core/tests/ignored.rs' "fn changed() {}`n"
    Save-Commit 'internal'
    Assert-Pass 'changes to reasoned and test-only code' 'base' '2 changed files since base'

    Write-Output 'docs-impact tests passed'
}
finally {
    Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
