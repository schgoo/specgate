$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$script = Join-Path $PSScriptRoot 'docs-diagnostics.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) "sg-docs-diagnostics-tests-$([guid]::NewGuid().ToString('N'))"

function Write-File([string]$Relative, [string]$Text) {
    $path = Join-Path $fixture $Relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
    [IO.File]::WriteAllText($path, $Text.Replace("`r`n", "`n"))
}

function Write-Guide([string[]]$Rows) {
    $table = "| Message fragment | Cause | Fix |`n| --- | --- | --- |`n" + (($Rows | ForEach-Object { "| $_ | c | f |" }) -join "`n")
    Write-File 'docs/guides/troubleshoot-capture.md' "# T`n`n``not in region``  `n<!-- diagnostics:start -->`n`n$table`n`n<!-- diagnostics:end -->`n"
}

function Invoke-Check {
    $output = & pwsh -NoLogo -NoProfile -File $script -Root $fixture 2>&1 | Out-String
    [pscustomobject]@{ Exit = $LASTEXITCODE; Output = $output }
}

function Assert-Rejected([string]$Name, [string[]]$Rows, [string]$Expected) {
    Write-Guide $Rows
    $result = Invoke-Check
    if ($result.Exit -eq 0 -or -not $result.Output.Contains($Expected)) {
        throw "docs-diagnostics accepted $Name (exit $($result.Exit)):`n$($result.Output)"
    }
}

try {
    Write-File 'rust/crates/a/src/lib.rs' 'fn f() -> String { format!("capture of {x} is not empty; pass --force") }'
    Write-File 'csharp/Weaver/Rules.cs' 'const string M = "is a generic method.";'
    Write-File 'rust/crates/a/src/lib/tests.rs' '"only in unit tests"'
    Write-File 'rust/crates/a/tests/it.rs' '"only in integration tests"'
    Write-File 'rust/target/debug/gen.rs' '"only in build output"'
    Write-File 'csharp/Weaver.Tests/RulesTests.cs' '"only in C# tests"'
    Write-File 'rust/crates/a/src/limitation.rs' "#[cfg(any(test, feature = `"test-util`"))]`nconst A: &str = `"only in cfg test const`";`n"
    Write-File 'rust/crates/a/src/inline.rs' "fn g() {}`n`n#[cfg(test)]`nmod tests {`n    const B: &str = `"only in inline test module`";`n}`n"

    Write-Guide @('`is not empty; pass --force`', '`is a generic method.`')
    $result = Invoke-Check
    if ($result.Exit -ne 0 -or -not $result.Output.Contains('2 fragments')) {
        throw "docs-diagnostics rejected present fragments (exit $($result.Exit)):`n$($result.Output)"
    }

    Assert-Rejected 'a stale fragment' @('`is a generic method.`', '`no such diagnostic`') "'no such diagnostic' appears in no non-test source"
    Assert-Rejected 'a fragment only in unit tests' @('`only in unit tests`') "'only in unit tests'"
    Assert-Rejected 'a fragment only in integration tests' @('`only in integration tests`') "'only in integration tests'"
    Assert-Rejected 'a fragment only in build output' @('`only in build output`') "'only in build output'"
    Assert-Rejected 'a fragment only in C# tests' @('`only in C# tests`') "'only in C# tests'"
    Assert-Rejected 'a fragment only in a cfg(test) constant' @('`only in cfg test const`') "'only in cfg test const'"
    Assert-Rejected 'a fragment only in an inline test module' @('`only in inline test module`') "'only in inline test module'"
    Assert-Rejected 'a row without a fragment' @('plain text') 'first cell is not one backticked fragment'
    Assert-Rejected 'an empty region' @() 'lists no diagnostic fragments'

    Write-File 'docs/guides/troubleshoot-capture.md' "# T`n"
    $result = Invoke-Check
    if ($result.Exit -eq 0 -or -not $result.Output.Contains('has no <!-- diagnostics:start -->')) {
        throw "docs-diagnostics accepted a guide without markers:`n$($result.Output)"
    }
    Write-Output 'docs-diagnostics regression tests passed'
}
finally {
    Remove-Item -LiteralPath $fixture -Recurse -Force -ErrorAction SilentlyContinue
}
