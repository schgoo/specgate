#!/usr/bin/env pwsh
# Extract fixture goldens and either verify or regenerate them.
#
# Invoked by the `extract-check` and `extract-update` justfile recipes. This is
# the single source of truth for which fixtures are extracted and which
# generated files are byte-compared against their committed `expected/` goldens.
#
#   -Mode check   Extract each fixture into a throwaway dir and hash it against
#                 the committed golden; clean up; exit 1 on any mismatch.
#   -Mode update  (Re)generate the committed goldens in place. Review the diff
#                 before committing.
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('check', 'update')]
    [string]$Mode
)

$ErrorActionPreference = 'Stop'

# Run from the `rust` workspace dir so cargo and the `../` fixture paths resolve
# exactly as the committed goldens were generated (keeps output byte-identical).
$rustDir = Join-Path (Join-Path $PSScriptRoot '..') 'rust'
Push-Location $rustDir
try {
    # One entry per extraction (one `-o` spec plus its sibling binding). Paths
    # are relative to the `rust` dir. `files[0]` is the spec written by `-o`.
    $fixtures = @(
        @{ pkg = '../test/rust/crates/specgate-extract-fixture';   component = $null;       cases = $false; files = @('extracted.spec.yaml', 'extracted.binding.yaml') }
        @{ pkg = '../test/rust/crates/specgate-component-fixture'; component = 'comp.app';  cases = $false; files = @('comp.app.spec.yaml', 'comp.app.binding.yaml') }
        @{ pkg = '../test/rust/crates/specgate-component-fixture'; component = 'comp.core'; cases = $false; files = @('comp.core.spec.yaml', 'comp.core.binding.yaml') }
        @{ pkg = '../test/rust/crates/specgate-value-fixture';     component = $null;       cases = $false; files = @('fixture.value.spec.yaml', 'fixture.value.binding.yaml') }
        @{ pkg = '../test/rust/crates/specgate-cases-fixture';     component = $null;       cases = $true;  files = @('fixture.cases.spec.yaml', 'fixture.cases.binding.yaml') }
    )

    # Extract each fixture. `check` writes to a throwaway dir; `update` writes
    # the committed goldens in place.
    foreach ($fx in $fixtures) {
        $outDir = if ($Mode -eq 'check') { "$($fx.pkg)/.extract-check-tmp" } else { "$($fx.pkg)/expected" }
        $cmdArgs = @('extract', $fx.pkg)
        if ($fx.component) { $cmdArgs += @('--component', $fx.component) }
        if ($fx.cases) { $cmdArgs += '--cases' }
        $cmdArgs += @('-o', "$outDir/$($fx.files[0])")
        cargo run -p specgate-cli --quiet -- @cmdArgs
        if ($LASTEXITCODE -ne 0) { throw "extract failed for $($fx.pkg)" }
    }

    if ($Mode -eq 'update') {
        Write-Host 'extract-update: goldens regenerated'
        return
    }

    # Compare each generated file against its committed golden, byte-for-byte.
    $bad = $false
    foreach ($fx in $fixtures) {
        foreach ($f in $fx.files) {
            $exp = "$($fx.pkg)/expected/$f"
            $tmp = "$($fx.pkg)/.extract-check-tmp/$f"
            if ((Get-FileHash $exp).Hash -ne (Get-FileHash $tmp).Hash) {
                Write-Host "extract-check: $f differs from committed golden (run 'just extract-update' to regenerate)"
                $bad = $true
            }
        }
    }

    # Clean up throwaway dirs (pkg is not unique — component-fixture appears twice).
    $fixtures.pkg | Select-Object -Unique | ForEach-Object {
        Remove-Item -Recurse -Force "$_/.extract-check-tmp" -ErrorAction SilentlyContinue
    }

    if ($bad) { exit 1 }
    Write-Host 'extract-check: goldens reproduced byte-for-byte'
}
finally {
    Pop-Location
}
