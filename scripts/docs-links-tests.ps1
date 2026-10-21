$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script = Join-Path $PSScriptRoot 'docs-links.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-docs-links-tests-$([guid]::NewGuid().ToString('N'))"

function Write-Doc([string]$Relative, [string]$Text) {
    $path = Join-Path $fixture $Relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
    [IO.File]::WriteAllText($path, $Text.Replace("`r`n", "`n"))
}

function Invoke-Check {
    $output = & pwsh -NoLogo -NoProfile -File $script -Root $fixture 2>&1 | Out-String
    [pscustomobject]@{ Exit = $LASTEXITCODE; Output = $output }
}

function Assert-Broken([string]$Name, [string]$Body, [string]$Expected) {
    Write-Doc 'docs/case.md' $Body
    $result = Invoke-Check
    if ($result.Exit -eq 0 -or -not $result.Output.Contains($Expected)) {
        throw "docs-links accepted $Name (exit $($result.Exit)):`n$($result.Output)"
    }
}

try {
    Write-Doc 'README.md' @'
# Project

See [guides](docs/guides/README.md#agent-quick-path), [contracts](docs/ctsc/),
[smoke](CONTRIBUTING.md#capturereplay-smoke), [local](#project), and [web](https://example.invalid/x).

```text
[ignored](missing-in-fence.md)
```

`[ignored](missing-in-code.md)`

[ref]: docs/guides/README.md
 [__cargo_doc2readme_dependencies_info]: ggGmYW0CYXZlMC43LjNh
'@
    Write-Doc 'CONTRIBUTING.md' "# Contributing`n`n## Capture/replay smoke`n`n## Tests`n`n## Tests`n"
    Write-Doc 'docs/guides/README.md' "# Guides`n`n## Agent quick path`n`n<a id=`"custom`"></a>`n"
    Write-Doc 'docs/ctsc/README.md' "# CTSC`n"
    Write-Doc 'docs/case.md' "# Case`n`n[dup](../CONTRIBUTING.md#tests-1) [custom](guides/README.md#custom)`n"
    Write-Doc 'target/ignored.md' "[skipped](nowhere.md)`n"

    $result = Invoke-Check
    if ($result.Exit -ne 0) {
        throw "docs-links rejected valid links (exit $($result.Exit)):`n$($result.Output)"
    }

    Assert-Broken 'a missing file' "[x](guides/missing.md)`n" "'guides/missing.md' -> missing docs/guides/missing.md"
    Assert-Broken 'a missing heading' "[x](guides/README.md#agent-slow-path)`n" "no heading or anchor '#agent-slow-path'"
    Assert-Broken 'a missing local heading' "# Case`n`n[x](#other)`n" "no heading or anchor '#other'"
    Assert-Broken 'a missing duplicate suffix' "[x](../CONTRIBUTING.md#tests-2)`n" "no heading or anchor '#tests-2'"
    Assert-Broken 'a missing reference target' "[ref]: ../nowhere.md`n" "'../nowhere.md' -> missing nowhere.md"
    Assert-Broken 'a missing directory' "[x](../docs/nothing/)`n" "-> missing docs/nothing"

    Write-Doc 'docs/case.md' "# Case`n"
    Write-Doc '.github/skills/bad.md' "[x](../../docs/gone.md)`n"
    $result = Invoke-Check
    if ($result.Exit -eq 0 -or -not $result.Output.Contains('.github/skills/bad.md:1')) {
        throw "docs-links did not scan .github Markdown:`n$($result.Output)"
    }
    Write-Output 'docs-links regression tests passed'
}
finally {
    Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
