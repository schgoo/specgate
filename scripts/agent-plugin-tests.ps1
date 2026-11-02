$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script = Join-Path $PSScriptRoot 'agent-plugin.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-agent-plugin-tests-$([guid]::NewGuid().ToString('N'))"
$manifest = @'
{
  "name": "demo",
  "description": "Demo plugin.",
  "keywords": ["demo"],
  "bundle": ["docs/guides/a.md", "docs/guides/b.md"],
  "skills": [
    {
      "name": "demo-skill",
      "title": "Demo skill",
      "description": "Does the demo.",
      "guides": ["docs/guides/a.md"]
    }
  ]
}
'@
$guide = @'
# Guide A

Run this. [smoke] (from `fixture`)

Mid [manual] sentence and [it]
[doctest]

See [B](b.md#b-title), [here](#steps), and [lib](../../src/lib.rs).

## Steps

```text
keep [manual] and [raw](../../missing.md)
```

## Source of truth

- [lib](../../src/lib.rs)

## After
'@

function Write-File([string]$Relative, [string]$Text) {
    $path = Join-Path $fixture $Relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
    [IO.File]::WriteAllText($path, $Text.Replace("`r`n", "`n"))
}

function Read-Output([string]$Relative) {
    [IO.File]::ReadAllText((Join-Path $fixture "plugins/demo/$Relative"))
}

function Invoke-Generator([switch]$Check) {
    $arguments = @('-NoLogo', '-NoProfile', '-File', $script, '-Root', $fixture)
    if ($Check) { $arguments += '-Check' }
    $output = & pwsh @arguments 2>&1 | Out-String
    [pscustomobject]@{ Exit = $LASTEXITCODE; Output = $output }
}

function Assert-Pass([string]$Name, [switch]$Check) {
    $result = Invoke-Generator -Check:$Check
    if ($result.Exit -ne 0) { throw "agent-plugin rejected $Name (exit $($result.Exit)):`n$($result.Output)" }
}

function Assert-Fail([string]$Name, [string]$Expected, [switch]$Check) {
    $result = Invoke-Generator -Check:$Check
    if ($result.Exit -eq 0 -or -not $result.Output.Contains($Expected)) {
        throw "agent-plugin accepted $Name (exit $($result.Exit)):`n$($result.Output)"
    }
}

function Assert-Contains([string]$Name, [string]$Text, [string]$Expected) {
    if (-not $Text.Contains($Expected)) { throw "$Name is missing '$Expected':`n$Text" }
}

function Assert-Missing([string]$Name, [string]$Text, [string]$Unexpected) {
    if ($Text.Contains($Unexpected)) { throw "$Name still contains '$Unexpected':`n$Text" }
}

try {
    Write-File 'rust/Cargo.toml' "[workspace.package]`nversion = `"1.2.3`"`nrepository = `"https://example.test/repo`"`nlicense = `"MIT`"`n"
    Write-File 'docs/contributor/agent-plugin.json' $manifest
    Write-File 'docs/guides/a.md' $guide
    Write-File 'docs/guides/b.md' "# B title`n"
    Write-File 'src/lib.rs' "pub fn f() {}`n"

    Assert-Pass 'a valid fixture'
    $copy = Read-Output 'docs/guides/a.md'
    $skill = Read-Output 'skills/demo-skill/SKILL.md'

    Assert-Contains 'line-end tag with parenthetical' $copy "Run this.`n"
    Assert-Contains 'mid-line tag' $copy "Mid sentence and`n"
    Assert-Missing 'tag-only line' $copy '[doctest]'
    Assert-Contains 'fenced tag' $copy 'keep [manual] and [raw](../../missing.md)'
    Assert-Missing 'source of truth section' $copy 'Source of truth'
    Assert-Contains 'heading after source of truth' $copy '## After'
    Assert-Contains 'bundled link in copy' $copy '[B](b.md#b-title)'
    Assert-Contains 'pinned link' $copy '[lib](https://example.test/repo/blob/v1.2.3/src/lib.rs)'

    Assert-Contains 'skill name' $skill 'name: demo-skill'
    Assert-Contains 'plugin root' $skill '`${CLAUDE_PLUGIN_ROOT}`'
    Assert-Missing 'skill title' $skill '# Guide A'
    Assert-Contains 'bundled link in skill' $skill '[B](${PLUGIN_DIR}/docs/guides/b.md#b-title)'
    Assert-Contains 'in-page link in skill' $skill '[here](${PLUGIN_DIR}/docs/guides/a.md#steps)'

    Assert-Pass 'an unchanged plugin' -Check
    Add-Content -LiteralPath (Join-Path $fixture 'plugins/demo/README.md') 'stale'
    Assert-Fail 'a stale file' 'stale README.md' -Check
    Write-File 'plugins/demo/extra.md' "extra`n"
    Assert-Fail 'an unexpected file' 'unexpected extra.md' -Check
    Assert-Pass 'regeneration'
    Assert-Pass 'a regenerated plugin' -Check

    Write-File 'docs/guides/b.md' "# B renamed`n"
    Assert-Fail 'a broken anchor' 'b-title'
    Write-File 'docs/guides/b.md' "# B title`n"

    Write-File 'docs/guides/a.md' "# Guide A`n`nSee [gone](gone.md).`n"
    Assert-Fail 'a missing link target' 'links to missing gone.md'

    Write-Output 'agent-plugin tests: passed'
}
finally {
    if (Test-Path -LiteralPath $fixture) { Remove-Item -LiteralPath $fixture -Recurse -Force }
}
