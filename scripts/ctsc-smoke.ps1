$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$scratch = Join-Path $root "rust/target/ctsc-smoke-$PID"
$firstCapture = Join-Path $scratch 'capture-a'
$secondCapture = Join-Path $scratch 'capture-b'
$firstReplay = Join-Path $scratch 'candidate-a.otlp.json'
$secondReplay = Join-Path $scratch 'candidate-b.otlp.json'
$changedReplay = Join-Path $scratch 'candidate-changed.otlp.json'
$binding = Join-Path $root 'test/bindings/rust.yaml'
$manifest = Join-Path $root 'rust/Cargo.toml'

function Invoke-SpecGate {
    param([string[]]$Arguments)

    & cargo run --manifest-path $manifest -p specgate-cli --quiet -- @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "specgate failed: $($Arguments -join ' ')"
    }
}

try {
    New-Item -ItemType Directory -Force $scratch | Out-Null

    Invoke-SpecGate @('capture', $binding, '--component', 'fixture.stateless_add', '--out', $firstCapture)
    Invoke-SpecGate @('capture', $binding, '--component', 'fixture.stateless_add', '--out', $secondCapture)

    foreach ($name in @('manifest.json', 'registry.ctsc.json', 'reference.otlp.json')) {
        $first = [System.IO.File]::ReadAllBytes((Join-Path $firstCapture $name))
        $second = [System.IO.File]::ReadAllBytes((Join-Path $secondCapture $name))
        if ([Convert]::ToHexString($first) -ne [Convert]::ToHexString($second)) {
            throw "$name is not byte-deterministic across captures."
        }
    }

    Invoke-SpecGate @('replay', $firstCapture, $binding, '--out', $firstReplay)
    Invoke-SpecGate @('replay', $firstCapture, $binding, '--out', $secondReplay)
    $firstReplayBytes = [System.IO.File]::ReadAllBytes($firstReplay)
    $secondReplayBytes = [System.IO.File]::ReadAllBytes($secondReplay)
    if ([Convert]::ToHexString($firstReplayBytes) -ne [Convert]::ToHexString($secondReplayBytes)) {
        throw 'Candidate replay is not byte-deterministic.'
    }

    $registry = Join-Path $firstCapture 'registry.ctsc.json'
    $reference = Join-Path $firstCapture 'reference.otlp.json'
    Invoke-SpecGate @('validate', 'registry', $registry)
    Invoke-SpecGate @('validate', 'trace', $reference)
    Invoke-SpecGate @('validate', 'linked', $reference, $registry)
    Invoke-SpecGate @('validate', 'bundle', $firstCapture)
    Invoke-SpecGate @('validate', 'trace', $firstReplay)
    Invoke-SpecGate @('validate', 'linked', $firstReplay, $registry)
    Invoke-SpecGate @('compare', $reference, $firstReplay, '--registry', $registry)

    $referenceJson = Get-Content $reference -Raw | ConvertFrom-Json
    $candidateJson = Get-Content $firstReplay -Raw | ConvertFrom-Json
    $referenceTrace = $referenceJson.resourceSpans[0].scopeSpans[0].spans[0].traceId
    $candidateTrace = $candidateJson.resourceSpans[0].scopeSpans[0].spans[0].traceId
    if ($referenceTrace -eq $candidateTrace) {
        throw 'Reference and candidate trace IDs must be independent.'
    }
    if ((Get-Content $firstReplay -Raw) -notmatch '"intValue":"5"') {
        throw 'Candidate replay did not emit add(2,3) -> 5.'
    }

    $changed = Get-Content $firstReplay -Raw | ConvertFrom-Json
    $operation = $changed.resourceSpans[0].scopeSpans[0].spans |
        Where-Object name -eq 'conformance.operation' |
        Select-Object -First 1
    $result = $operation.events |
        Where-Object name -eq 'conformance.result' |
        Select-Object -First 1
    $value = $result.attributes |
        Where-Object key -eq 'conformance.result.value' |
        Select-Object -First 1
    $value.value.intValue = '6'
    $changed | ConvertTo-Json -Depth 100 -Compress | Set-Content -NoNewline $changedReplay

    $mismatch = & cargo run --manifest-path $manifest -p specgate-cli --quiet -- compare $reference $changedReplay --registry $registry
    if ($LASTEXITCODE -ne 1) {
        throw "Expected changed result comparison to exit 1, got $LASTEXITCODE."
    }
    if (($mismatch -join "`n") -notmatch 'mismatch scenario\[') {
        throw 'Changed result comparison did not report a semantic mismatch path.'
    }

    Write-Output 'Deterministic native capture/replay/validate/compare smoke passed.'
}
finally {
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
