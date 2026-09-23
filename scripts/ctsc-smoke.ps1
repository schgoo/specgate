$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$scratch = Join-Path $root "rust/target/ctsc-smoke-$PID"
$firstCapture = Join-Path $scratch 'capture-a'
$secondCapture = Join-Path $scratch 'capture-b'
$firstReplay = Join-Path $scratch 'candidate-a.otlp.json'
$secondReplay = Join-Path $scratch 'candidate-b.otlp.json'
$binding = Join-Path $root 'test/bindings/rust.yaml'
$manifest = Join-Path $root 'rust/Cargo.toml'
$validator = Join-Path $root 'docs/ctsc/validate.py'

function Invoke-Python {
    param([string[]]$Arguments)

    if (Get-Command python -ErrorAction SilentlyContinue) {
        & python @Arguments
    }
    elseif (Get-Command py -ErrorAction SilentlyContinue) {
        & py -3 @Arguments
    }
    else {
        throw 'Python 3 is required for CTSC validation.'
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Python validation failed: $($Arguments -join ' ')"
    }
}

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
    Invoke-Python @($validator, 'registry', $registry)
    Invoke-Python @($validator, 'trace', $reference)
    Invoke-Python @($validator, 'linked', $reference, $registry)
    Invoke-Python @($validator, 'trace', $firstReplay)
    Invoke-Python @($validator, 'linked', $firstReplay, $registry)

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

    Write-Output 'Deterministic capture/replay smoke passed.'
}
finally {
    Remove-Item -Recurse -Force $scratch -ErrorAction SilentlyContinue
}
