$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'ctsc-goldens-support.ps1')

function Assert-Throws {
    param(
        [Parameter(Mandatory)]
        [scriptblock]$Action,
        [Parameter(Mandatory)]
        [string]$MessageFragment
    )

    try {
        & $Action
    }
    catch {
        if ($_.Exception.Message -notlike "*$MessageFragment*") {
            throw "Expected failure containing '$MessageFragment', got: $($_.Exception.Message)"
        }
        return
    }
    throw "Expected action to fail with '$MessageFragment'."
}

$testName = 'goldens::ctsc_golden_matrix'
$harnessMessage = @{
    reason = 'compiler-artifact'
    profile = @{ test = $true }
    target = @{ name = 'specgate_cli'; kind = @('lib') }
    executable = 'specgate-cli-tests'
} | ConvertTo-Json -Compress

if ((Get-GoldenHarnessExecutable -CargoMessages @($harnessMessage)) -cne 'specgate-cli-tests') {
    throw 'The expected golden harness executable was not selected.'
}
Assert-Throws {
    Get-GoldenHarnessExecutable -CargoMessages @()
} 'found 0'
Assert-Throws {
    Get-GoldenHarnessExecutable -CargoMessages @(
        $harnessMessage,
        ($harnessMessage | ConvertFrom-Json | ForEach-Object {
                $_.executable = 'duplicate-specgate-cli-tests'
                $_
            } | ConvertTo-Json -Compress)
    )
} 'found 2'

Assert-SingleGoldenTestSelection -Lines @("$testName`: test") -TestName $testName
Assert-Throws {
    Assert-SingleGoldenTestSelection -Lines @('goldens::renamed_test: test') -TestName $testName
} 'found 1'
Assert-Throws {
    Assert-SingleGoldenTestSelection -Lines @() -TestName $testName
} 'found 0'

Assert-SingleGoldenTestExecution -Lines @(
    'running 1 test',
    'test goldens::ctsc_golden_matrix ... ok',
    'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 99 filtered out; finished in 0.01s'
)
Assert-Throws {
    Assert-SingleGoldenTestExecution -Lines @(
        'running 0 tests',
        'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 100 filtered out; finished in 0.00s'
    )
} 'did not prove exactly one'

Write-Host 'CTSC golden wrapper regressions passed.'
