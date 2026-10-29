$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script = Join-Path $PSScriptRoot 'docs-impact.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-docs-impact-tests-$([guid]::NewGuid().ToString('N'))"
$map = @'
{
  "rules": [
    { "id": "core", "code": ["rust/crates/core/src/**"], "docs": ["docs/core.md"] },
    { "id": "tool", "code": ["rust/crates/tool/src/**"], "docs": [], "reason": "internal" }
  ]
}
'@
$core = "pub fn other() -> u32 {`n    2`n}`n"

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
    Write-File 'docs/contributor/docs-impact.json' $map
    Write-File 'rust/crates/core/src/lib.rs' $core
    Write-File 'rust/crates/core/tests/ignored.rs' "fn uncovered() {}`n"
    Write-File 'rust/crates/tool/src/lib.rs' "pub struct Tool;`n"
    Write-File 'docs/core.md' "# Core`n"

    Assert-Pass 'a valid map' '' 'map covers 2 product sources with 2 rules'

    Assert-StaticFail 'an uncovered product source' 'rust/crates/extra/src/lib.rs' "pub fn x() {}`n" "product source 'rust/crates/extra/src/lib.rs' is covered by no rule"
    Assert-StaticFail 'a dead code glob' 'docs/contributor/docs-impact.json' $map.Replace('rust/crates/tool/src/**', 'rust/crates/gone/src/**') "code glob 'rust/crates/gone/src/**' matches no file"
    Assert-StaticFail 'a missing doc page' 'docs/contributor/docs-impact.json' $map.Replace('docs/core.md', 'docs/gone.md') "names missing doc 'docs/gone.md'"
    Assert-StaticFail 'a rule without docs or reason' 'docs/contributor/docs-impact.json' $map.Replace(', "reason": "internal"', '') "rule 'tool' has no docs and no reason"
    Assert-StaticFail 'a duplicate rule id' 'docs/contributor/docs-impact.json' $map.Replace('"id": "tool"', '"id": "core"') "rule 'core' is declared twice"

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
    Assert-Pass 'a reasoned trailer' 'base' 'skipped 1 map rule(s)'

    Write-File 'docs/core.md' "# Core`n`nUpdated.`n"
    Save-Commit 'update docs'
    Assert-Pass 'a doc update' 'base'

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
