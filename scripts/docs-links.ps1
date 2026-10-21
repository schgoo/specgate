<#
.SYNOPSIS
Check relative links in the repository's Markdown documentation.

.DESCRIPTION
Every relative link and reference definition outside code must name an
existing file or directory. A `#fragment` on a Markdown target, or on the
current file, must match a heading slug (GitHub rules: lowercase; drop
characters other than letters, digits, spaces, `-`, and `_`; spaces become
`-`; repeats get `-1`, `-2`, ...) or an explicit `<a id>`/`<a name>`.
Fragments on other files are not checked. Absolute URLs (`https:`, `mailto:`,
...) are not fetched.

.PARAMETER Root
Repository root to scan. Defaults to the parent of this script's directory.
#>
[CmdletBinding()]
param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path -LiteralPath $Root).Path
$skipped = @('target', 'bin', 'obj', '.git', 'node_modules', 'artifacts', '.specgate')

function Get-MarkdownFiles([string]$Directory) {
    if (-not (Test-Path -LiteralPath $Directory -PathType Container)) {
        return
    }
    foreach ($entry in Get-ChildItem -LiteralPath $Directory -Force) {
        if ($entry.PSIsContainer) {
            if ($skipped -notcontains $entry.Name) {
                Get-MarkdownFiles $entry.FullName
            }
        }
        elseif ($entry.Extension -eq '.md') {
            $entry.FullName
        }
    }
}

function Get-Files {
    foreach ($name in @('README.md', 'CONTRIBUTING.md', 'AGENTS.md', 'SUPPORT.md', 'CHANGELOG.md')) {
        $path = Join-Path $Root $name
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            $path
        }
    }
    foreach ($directory in @('docs', '.github', 'test/behavior', 'csharp', 'rust/crates')) {
        Get-MarkdownFiles (Join-Path $Root $directory)
    }
}

# Lines outside fenced code, with inline code spans blanked.
function Get-ProseLines([string]$Path) {
    $fence = $null
    $number = 0
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        $number++
        $trimmed = $line.TrimStart()
        if ($null -eq $fence) {
            if ($trimmed -match '^(```+|~~~+)') {
                $fence = $Matches[1]
                continue
            }
        }
        else {
            if ($trimmed.StartsWith($fence)) {
                $fence = $null
            }
            continue
        }
        [pscustomobject]@{ Number = $number; Text = ($line -replace '(`+)(?:(?!\1).)+?\1', '``') }
    }
}

function ConvertTo-Slug([string]$Heading) {
    $text = $Heading -replace '!?\[([^\]]*)\]\([^)]*\)', '$1'
    $text = $text -replace '<[^>]+>', ''
    $text = $text.ToLowerInvariant() -replace '[^\p{L}\p{N}\p{Mn}\p{Pc} -]', ''
    $text.Replace(' ', '-')
}

$anchorCache = @{}
function Get-Anchors([string]$Path) {
    if ($anchorCache.ContainsKey($Path)) {
        return $anchorCache[$Path]
    }
    $anchors = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $counts = @{}
    $fence = $null
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        $trimmed = $line.TrimStart()
        if ($null -eq $fence) {
            if ($trimmed -match '^(```+|~~~+)') {
                $fence = $Matches[1]
                continue
            }
        }
        else {
            if ($trimmed.StartsWith($fence)) {
                $fence = $null
            }
            continue
        }
        if ($line -match '^ {0,3}#{1,6}\s+(.*?)\s*#*\s*$') {
            $slug = ConvertTo-Slug $Matches[1]
            if ($counts.ContainsKey($slug)) {
                $counts[$slug]++
                $null = $anchors.Add("$slug-$($counts[$slug])")
            }
            else {
                $counts[$slug] = 0
                $null = $anchors.Add($slug)
            }
        }
        foreach ($match in [regex]::Matches($line, '<a\s+(?:id|name)\s*=\s*"([^"]+)"')) {
            $null = $anchors.Add($match.Groups[1].Value)
        }
    }
    $anchorCache[$Path] = $anchors
    $anchors
}

function Get-Links([string]$Text) {
    foreach ($match in [regex]::Matches($Text, '\]\(\s*(<[^>]*>|[^)\s]+)(?:\s+"[^"]*")?\s*\)')) {
        $match.Groups[1].Value.Trim('<', '>')
    }
    # cargo-doc2readme stores generator metadata in a reference definition.
    if ($Text -match '^\s{0,3}\[([^\]]+)\]:\s*(<[^>]*>|\S+)' -and
        $Matches[1] -cne '__cargo_doc2readme_dependencies_info') {
        $Matches[2].Trim('<', '>')
    }
}

$problems = [Collections.Generic.List[string]]::new()
$fileCount = 0
$linkCount = 0
foreach ($file in Get-Files) {
    $fileCount++
    $relative = [IO.Path]::GetRelativePath($Root, $file).Replace('\', '/')
    foreach ($line in Get-ProseLines $file) {
        foreach ($link in Get-Links $line.Text) {
            if ($link -match '^[A-Za-z][A-Za-z0-9+.-]*:') {
                continue
            }
            $linkCount++
            $pathPart, $fragment = $link.Split('#', 2)
            $pathPart = [Uri]::UnescapeDataString($pathPart)
            if ($pathPart -eq '') {
                $target = $file
            }
            elseif ($pathPart.StartsWith('/')) {
                $target = Join-Path $Root $pathPart.TrimStart('/')
            }
            else {
                $target = Join-Path (Split-Path -Parent $file) $pathPart
            }
            $target = [IO.Path]::GetFullPath($target)
            if (-not (Test-Path -LiteralPath $target)) {
                $problems.Add("${relative}:$($line.Number): '$link' -> missing $([IO.Path]::GetRelativePath($Root, $target).Replace('\', '/'))")
                continue
            }
            if ($fragment -and $target.EndsWith('.md') -and (Test-Path -LiteralPath $target -PathType Leaf)) {
                $anchor = [Uri]::UnescapeDataString($fragment)
                if ($anchor -cnotin @(Get-Anchors $target)) {
                    $problems.Add("${relative}:$($line.Number): '$link' -> no heading or anchor '#$anchor'")
                }
            }
        }
    }
}

if ($problems.Count -gt 0) {
    $problems | ForEach-Object { Write-Output "broken link: $_" }
    Write-Output "docs-links: $($problems.Count) broken of $linkCount relative links in $fileCount files"
    exit 1
}
Write-Output "docs-links: $linkCount relative links in $fileCount files are valid"
