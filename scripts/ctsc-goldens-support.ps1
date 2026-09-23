Set-StrictMode -Version Latest

function Get-GoldenHarnessExecutable {
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [object[]]$CargoMessages
    )

    $executables = @(
        @(
            foreach ($line in $CargoMessages) {
                try {
                    $message = [string]$line | ConvertFrom-Json -ErrorAction Stop
                }
                catch {
                    continue
                }
                if ($message.reason -ne 'compiler-artifact' -or
                    -not $message.profile.test -or
                    $message.target.name -ne 'specgate_cli' -or
                    -not ($message.target.kind -contains 'lib') -or
                    [string]::IsNullOrWhiteSpace([string]$message.executable)) {
                    continue
                }
                [string]$message.executable
            }
        ) | Sort-Object -Unique
    )

    if ($executables.Count -ne 1) {
        throw "Expected exactly one specgate-cli library test harness executable, found $($executables.Count)."
    }
    return $executables[0]
}

function Assert-SingleGoldenTestSelection {
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [object[]]$Lines,
        [Parameter(Mandatory)]
        [string]$TestName
    )

    $selected = @(
        foreach ($line in $Lines) {
            $text = ([string]$line).Trim()
            if ($text.EndsWith(': test', [StringComparison]::Ordinal)) {
                $text.Substring(0, $text.Length - ': test'.Length)
            }
        }
    )
    if ($selected.Count -ne 1 -or $selected[0] -cne $TestName) {
        $found = if ($selected.Count -eq 0) { 'none' } else { $selected -join ', ' }
        throw "Expected exactly one golden harness test '$TestName', found $($selected.Count): $found."
    }
}

function Assert-SingleGoldenTestExecution {
    param(
        [Parameter(Mandatory)]
        [AllowEmptyCollection()]
        [object[]]$Lines
    )

    $running = @($Lines | Where-Object { ([string]$_).Trim() -ceq 'running 1 test' })
    $summaries = @(
        $Lines | Where-Object {
            ([string]$_).Trim() -cmatch '^test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; [0-9]+ filtered out; finished in .+$'
        }
    )
    if ($running.Count -ne 1 -or $summaries.Count -ne 1) {
        throw "Golden harness output did not prove exactly one successful test execution (running markers: $($running.Count), summaries: $($summaries.Count))."
    }
}
