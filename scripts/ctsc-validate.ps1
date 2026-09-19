$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
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
}

function Assert-Valid {
    param([string]$Kind, [string[]]$Paths)

    $arguments = @($validator, $Kind) + $Paths
    Invoke-Python -Arguments $arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Expected valid CTSC $Kind corpus entry: $($Paths -join ', ')"
    }
}

function Assert-Invalid {
    param([string]$Kind, [string[]]$Paths)

    $arguments = @($validator, $Kind) + $Paths
    Invoke-Python -Arguments $arguments 2>$null
    if ($LASTEXITCODE -eq 0) {
        throw "Expected invalid CTSC $Kind corpus entry: $($Paths -join ', ')"
    }
}

$corpus = Join-Path $root 'docs/ctsc/corpus'

Get-ChildItem (Join-Path $corpus 'registry/valid') -Recurse -Filter '*.json' |
    ForEach-Object { Assert-Valid registry @($_.FullName) }
Get-ChildItem (Join-Path $corpus 'registry/invalid') -Recurse -Filter '*.json' |
    ForEach-Object { Assert-Invalid registry @($_.FullName) }

Get-ChildItem (Join-Path $corpus 'trace/valid') -Recurse -File |
    Where-Object { $_.Extension -in @('.json', '.jsonl') } |
    ForEach-Object { Assert-Valid trace @($_.FullName) }
Get-ChildItem (Join-Path $corpus 'trace/invalid') -Recurse -Filter '*.json' |
    ForEach-Object { Assert-Invalid trace @($_.FullName) }

Get-ChildItem (Join-Path $corpus 'linked/valid') -Recurse -Filter 'trace*.json' |
    ForEach-Object {
        $registry = Join-Path $_.DirectoryName 'registry.json'
        Assert-Valid linked @($_.FullName, $registry)
    }
Get-ChildItem (Join-Path $corpus 'linked/invalid') -Recurse -Filter 'trace*.json' |
    ForEach-Object {
        $registry = Get-ChildItem $_.DirectoryName -Filter '*.json' |
            Where-Object { $_.Name -notlike 'trace*' } |
            Select-Object -First 1
        if ($null -eq $registry) {
            throw "No registry found beside $($_.FullName)"
        }
        Assert-Invalid linked @($_.FullName, $registry.FullName)
    }

Write-Output 'CTSC corpus validation passed.'
