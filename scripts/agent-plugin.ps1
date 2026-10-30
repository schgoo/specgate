<#
.SYNOPSIS
Build the SpecGate agent plugin from the user guides.

.DESCRIPTION
Reads docs/contributor/agent-plugin.json and writes plugins/<name>: one skill
per task holding its guides, plus a shared copy of every bundled guide and
reference page under docs/. Links to bundled files stay inside the plugin;
links to anything else in this repository become GitHub URLs pinned to the
v<workspace version> tag. Command check tags ([smoke], [it], [doctest],
[manual]) and Source of truth sections are dropped: they describe how this
repository tests its docs, not how to use SpecGate.

The result is validated: every relative link and anchor resolves inside the
plugin, no check tag or Source of truth section remains, and each SKILL.md
stays under 500 lines.

.PARAMETER Root
Repository root. Defaults to the parent of this script's directory.

.PARAMETER Check
Build into a temporary directory and fail unless the result matches the
committed plugin byte for byte.

.EXAMPLE
pwsh scripts/agent-plugin.ps1 -Check
#>
[CmdletBinding()]
param(
    [string]$Root = (Split-Path -Parent $PSScriptRoot),
    [switch]$Check
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path -LiteralPath $Root).Path
$utf8 = [Text.UTF8Encoding]::new($false)
$pluginDir = '${PLUGIN_DIR}'
$tagPattern = '(?<lead>[ \t]*)(?:\[(?:smoke|it|doctest|manual)\](?!\()[ \t]*)+(?:\([^()\n]*\)[ \t]*)?'
$linkPattern = '(?<open>!?\[(?:[^\[\]\n]|\[[^\[\]\n]*\])*\]\()(?<target>[^()\s]+)\)'
$fencePattern = '^\s*(?<fence>```+|~~~+)'
$maxSkillLines = 500

$manifest = Get-Content -LiteralPath (Join-Path $Root 'docs/contributor/agent-plugin.json') -Raw | ConvertFrom-Json
$cargo = (Get-Content -LiteralPath (Join-Path $Root 'rust/Cargo.toml') -Raw).Replace("`r`n", "`n")
$workspace = [regex]::Match($cargo, '(?ms)^\[workspace\.package\]\n(.*?)(?=^\[|\z)').Groups[1].Value
$version = [regex]::Match($workspace, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
$repository = [regex]::Match($workspace, '(?m)^repository\s*=\s*"([^"]+)"').Groups[1].Value
$license = [regex]::Match($workspace, '(?m)^license\s*=\s*"([^"]+)"').Groups[1].Value
if (-not $version -or -not $repository -or -not $license) {
    throw 'rust/Cargo.toml [workspace.package] must set version, repository, and license'
}
$bundle = @($manifest.bundle)
$committed = Join-Path $Root "plugins/$($manifest.name)"

function Resolve-RepoPath([string]$From, [string]$Target) {
    $parts = [Collections.Generic.List[string]]::new()
    $directory = $From.Split('/')
    for ($index = 0; $index -lt $directory.Count - 1; $index++) {
        if ($directory[$index]) { $parts.Add($directory[$index]) }
    }
    foreach ($segment in $Target.Split('/')) {
        if ($segment -eq '' -or $segment -eq '.') { continue }
        if ($segment -eq '..') {
            if ($parts.Count -eq 0) { throw "$From links outside the repository: $Target" }
            $parts.RemoveAt($parts.Count - 1)
        }
        else {
            $parts.Add($segment)
        }
    }
    $parts -join '/'
}

function Convert-Target([string]$Source, [string]$Target, [bool]$InSkill) {
    if ($Target -match '^[a-zA-Z][a-zA-Z0-9+.-]*:') { return $Target }
    $hash = $Target.IndexOf('#')
    $path = if ($hash -ge 0) { $Target.Substring(0, $hash) } else { $Target }
    $fragment = if ($hash -ge 0) { $Target.Substring($hash) } else { '' }
    if (-not $path) {
        if ($InSkill) { return "$pluginDir/$Source$fragment" }
        return $Target
    }
    $repoPath = Resolve-RepoPath $Source $path
    $full = Join-Path $Root $repoPath
    if (-not (Test-Path -LiteralPath $full)) { throw "$Source links to missing $Target" }
    if ($bundle -contains $repoPath) {
        if ($InSkill) { return "$pluginDir/$repoPath$fragment" }
        return $Target
    }
    $kind = if (Test-Path -LiteralPath $full -PathType Container) { 'tree' } else { 'blob' }
    "$repository/$kind/v$version/$repoPath$fragment"
}

function Convert-Markdown([string]$Source, [string]$Text, [bool]$InSkill, [int]$Demote, [switch]$DropTitle) {
    $Text = $Text.Replace("`r`n", "`n")
    $Text = [regex]::Replace($Text, 'Verify,\s+and\s+Source\s+of\s+truth\.', 'and Verify.')
    $out = [Collections.Generic.List[string]]::new()
    $fence = $null
    $skipLevel = 0
    $skipToHeading = $false
    foreach ($line in $Text.Split("`n")) {
        $heading = $null
        if ($fence) {
            if ($line.Trim().StartsWith($fence) -and $line.Trim().Trim($fence[0]) -eq '') { $fence = $null }
        }
        elseif ($line -match $fencePattern) {
            $fence = $Matches.fence
        }
        elseif ($line -match '^(?<hashes>#{1,6})\s+(?<title>.*)$') {
            $heading = $Matches
        }

        if ($heading) {
            $level = $heading.hashes.Length
            if ($skipLevel -and $level -le $skipLevel) { $skipLevel = 0 }
            $skipToHeading = $false
            if ($heading.title.Trim() -eq 'Source of truth') { $skipLevel = $level }
        }
        elseif (-not $fence -and $line -match '^Each command is tagged with how it is checked') {
            $skipToHeading = $true
        }
        if ($skipLevel -or $skipToHeading) { continue }

        if ($heading) {
            if ($DropTitle -and $level -eq 1) { continue }
            $line = ('#' * [Math]::Min(6, $level + $Demote)) + ' ' + $heading.title
        }
        elseif (-not $fence -and -not ($line -match $fencePattern)) {
            $current = $line
            $line = [regex]::Replace($current, $tagPattern, {
                    param($match)
                    if ($current.Substring(0, $match.Index).Trim() -eq '') { return $match.Groups['lead'].Value }
                    if ($match.Index + $match.Length -eq $current.Length) { return '' }
                    ' '
                })
            $line = [regex]::Replace($line, $linkPattern, {
                    param($match)
                    $match.Groups['open'].Value + (Convert-Target $Source $match.Groups['target'].Value $InSkill) + ')'
                })
            if ($line.Trim() -eq '') { $line = '' }
        }
        if ($line -eq '' -and ($out.Count -eq 0 -or $out[$out.Count - 1] -eq '') -and -not $fence) { continue }
        $out.Add($line)
    }
    while ($out.Count -and $out[$out.Count - 1] -eq '') { $out.RemoveAt($out.Count - 1) }
    ($out -join "`n") + "`n"
}

function Write-Text([string]$Directory, [string]$Relative, [string]$Text) {
    $path = Join-Path $Directory $Relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $path) | Out-Null
    [IO.File]::WriteAllText($path, $Text, $utf8)
}

function ConvertTo-PluginJson($Value) {
    ($Value | ConvertTo-Json -Depth 5).Replace("`r`n", "`n") + "`n"
}

function Build-Plugin([string]$Out) {
    foreach ($relative in $bundle) {
        $source = Join-Path $Root $relative
        if ($relative.EndsWith('.md')) {
            Write-Text $Out $relative (Convert-Markdown $relative (Get-Content -LiteralPath $source -Raw) $false 0)
        }
        else {
            Write-Text $Out $relative ((Get-Content -LiteralPath $source -Raw).Replace("`r`n", "`n"))
        }
    }

    foreach ($skill in $manifest.skills) {
        $guides = @($skill.guides)
        $text = [Text.StringBuilder]::new()
        [void]$text.Append("---`nname: $($skill.name)`ndescription: $(ConvertTo-Json $skill.description)`n---`n`n")
        [void]$text.Append("# $($skill.title)`n`n")
        [void]$text.Append("Part of the SpecGate agent plugin $version. It covers using SpecGate in the user's project, not developing SpecGate itself.`n`n")
        [void]$text.Append("## Path resolution`n`n")
        [void]$text.Append("``$pluginDir`` is the plugin root, the directory that holds ``.claude-plugin/plugin.json``.`n`n")
        [void]$text.Append("- **Claude Code:** ``$pluginDir`` is ```${CLAUDE_PLUGIN_ROOT}``.`n")
        [void]$text.Append("- **Other agents:** this file is ``skills/$($skill.name)/SKILL.md``, so the plugin root is two directories up.`n`n")
        [void]$text.Append("``$pluginDir/docs/`` holds every SpecGate guide and reference page. Links to ``$repository/…/v$version/`` show SpecGate's own sources and examples at this release. Read them as examples; do not adopt SpecGate's contributor workflow (``just`` recipes, the golden matrix) in the user's project.`n")
        foreach ($guide in $guides) {
            if (-not ($bundle -contains $guide)) { throw "skill $($skill.name) uses $guide, which is not bundled" }
            $body = Get-Content -LiteralPath (Join-Path $Root $guide) -Raw
            if ($guides.Count -eq 1) {
                $converted = Convert-Markdown $guide $body $true 0 -DropTitle
            }
            else {
                $converted = Convert-Markdown $guide $body $true 1
            }
            [void]$text.Append("`n" + $converted)
        }
        Write-Text $Out "skills/$($skill.name)/SKILL.md" $text.ToString()
    }

    $author = [ordered]@{ name = 'SpecGate'; url = $repository }
    $claude = [ordered]@{
        name        = $manifest.name
        version     = $version
        description = $manifest.description
        author      = $author
        homepage    = $repository
        repository  = $repository
        license     = $license
        keywords    = @($manifest.keywords)
    }
    Write-Text $Out '.claude-plugin/plugin.json' (ConvertTo-PluginJson $claude)
    $copilot = [ordered]@{}
    foreach ($key in $claude.Keys) { $copilot[$key] = $claude[$key] }
    $copilot.skills = 'skills/'
    Write-Text $Out 'plugin.json' (ConvertTo-PluginJson $copilot)

    $readme = [Text.StringBuilder]::new()
    [void]$readme.Append("# SpecGate agent plugin`n`n")
    [void]$readme.Append("$($manifest.description)`n`n")
    [void]$readme.Append("Generated from the [SpecGate]($repository) v$version user guides by ``scripts/agent-plugin.ps1``. Do not edit these files; change the guides or ``docs/contributor/agent-plugin.json`` in SpecGate and run ``just agent-plugin``.`n`n")
    [void]$readme.Append("## Skills`n`n| Skill | Use |`n| --- | --- |`n")
    foreach ($skill in $manifest.skills) {
        [void]$readme.Append("| [``$($skill.name)``](skills/$($skill.name)/SKILL.md) | $($skill.description) |`n")
    }
    [void]$readme.Append("`n## Contents`n`n")
    [void]$readme.Append("- ``skills/``: one skill per task, each holding its guides.`n")
    [void]$readme.Append("- ``docs/``: the guides and reference pages the skills link to.`n")
    [void]$readme.Append("- ``binding-schema.json``: the JSON Schema for ``specgate.yaml``.`n")
    Write-Text $Out 'README.md' $readme.ToString()
}

function Get-Slugs([string]$Path) {
    $slugs = [Collections.Generic.HashSet[string]]::new()
    $counts = @{}
    $fence = $null
    foreach ($line in [IO.File]::ReadAllLines($Path)) {
        if ($fence) {
            if ($line.Trim().StartsWith($fence)) { $fence = $null }
            continue
        }
        if ($line -match $fencePattern) { $fence = $Matches.fence; continue }
        if ($line -match '^#{1,6}\s+(.*)$') {
            $slug = ($Matches[1].Trim().ToLowerInvariant() -replace '[^\p{L}\p{N}\s_-]', '') -replace '\s', '-'
            if ($counts.ContainsKey($slug)) {
                $counts[$slug]++
                [void]$slugs.Add("$slug-$($counts[$slug])")
            }
            else {
                $counts[$slug] = 0
                [void]$slugs.Add($slug)
            }
        }
    }
    $slugs
}

function Test-Plugin([string]$Out) {
    $problems = [Collections.Generic.List[string]]::new()
    $outFull = (Resolve-Path -LiteralPath $Out).Path
    foreach ($file in Get-ChildItem -LiteralPath $Out -Recurse -File -Filter '*.md') {
        $relative = [IO.Path]::GetRelativePath($outFull, $file.FullName).Replace('\', '/')
        $lines = [IO.File]::ReadAllLines($file.FullName)
        $fence = $null
        for ($index = 0; $index -lt $lines.Count; $index++) {
            $line = $lines[$index]
            $where = "${relative}:$($index + 1)"
            if ($fence) {
                if ($line.Trim().StartsWith($fence)) { $fence = $null }
                continue
            }
            if ($line -match $fencePattern) { $fence = $Matches.fence; continue }
            if ($line -match '\[(smoke|it|doctest|manual)\](?!\()') { $problems.Add("${where}: check tag remains") }
            if ($line -match '^#{1,6}\s+Source of truth\s*$') { $problems.Add("${where}: Source of truth section remains") }
            foreach ($match in [regex]::Matches($line, $linkPattern)) {
                $target = $match.Groups['target'].Value
                if ($target -match '^[a-zA-Z][a-zA-Z0-9+.-]*:') { continue }
                $hash = $target.IndexOf('#')
                $path = if ($hash -ge 0) { $target.Substring(0, $hash) } else { $target }
                $fragment = if ($hash -ge 0) { $target.Substring($hash + 1) } else { '' }
                if ($path.StartsWith("$pluginDir/")) {
                    $resolved = Join-Path $outFull $path.Substring($pluginDir.Length + 1)
                }
                elseif ($path) {
                    $resolved = [IO.Path]::GetFullPath((Join-Path $file.DirectoryName $path))
                }
                else {
                    $resolved = $file.FullName
                }
                $inside = [IO.Path]::GetFullPath($resolved).StartsWith($outFull + [IO.Path]::DirectorySeparatorChar)
                if (-not $inside -or -not (Test-Path -LiteralPath $resolved)) {
                    $problems.Add("${where}: $target does not resolve inside the plugin")
                }
                elseif ($fragment -and $resolved.EndsWith('.md') -and -not (Get-Slugs $resolved).Contains($fragment)) {
                    $problems.Add("${where}: $target names a missing anchor")
                }
            }
        }
    }
    foreach ($skill in $manifest.skills) {
        $path = Join-Path $Out "skills/$($skill.name)/SKILL.md"
        $count = [IO.File]::ReadAllLines($path).Count
        if ($count -ge $maxSkillLines) { $problems.Add("skills/$($skill.name)/SKILL.md: $count lines, limit $maxSkillLines") }
        if ($skill.name -notmatch '^[a-z0-9]+(-[a-z0-9]+)*$') { $problems.Add("skill name $($skill.name) is not kebab-case") }
    }
    if ($problems.Count) {
        $problems | ForEach-Object { Write-Output "agent-plugin: $_" }
        throw "agent-plugin: $($problems.Count) problems in the generated plugin"
    }
}

function Get-Files([string]$Directory) {
    if (-not (Test-Path -LiteralPath $Directory)) { return @() }
    $full = (Resolve-Path -LiteralPath $Directory).Path
    @(Get-ChildItem -LiteralPath $full -Recurse -File -Force |
            ForEach-Object { [IO.Path]::GetRelativePath($full, $_.FullName).Replace('\', '/') } |
            Sort-Object)
}

$scratch = Join-Path ([IO.Path]::GetTempPath()) "sg-agent-plugin-$([guid]::NewGuid().ToString('N'))"
try {
    Build-Plugin $scratch
    Test-Plugin $scratch
    $files = Get-Files $scratch
    if ($Check) {
        $existing = Get-Files $committed
        $differences = [Collections.Generic.List[string]]::new()
        foreach ($file in $files) {
            if (-not ($existing -contains $file)) { $differences.Add("missing $file"); continue }
            $expected = [IO.File]::ReadAllBytes((Join-Path $scratch $file))
            $actual = [IO.File]::ReadAllBytes((Join-Path $committed $file))
            if (-not [Linq.Enumerable]::SequenceEqual($expected, $actual)) { $differences.Add("stale $file") }
        }
        foreach ($file in $existing) {
            if (-not ($files -contains $file)) { $differences.Add("unexpected $file") }
        }
        if ($differences.Count) {
            $differences | ForEach-Object { Write-Output "agent-plugin: $_" }
            throw "agent-plugin: plugins/$($manifest.name) is out of date; run just agent-plugin"
        }
        Write-Output "agent-plugin: plugins/$($manifest.name) matches ($($files.Count) files, $(@($manifest.skills).Count) skills)"
    }
    else {
        if (Test-Path -LiteralPath $committed) { Remove-Item -LiteralPath $committed -Recurse -Force }
        New-Item -ItemType Directory -Force $committed | Out-Null
        Get-ChildItem -LiteralPath $scratch -Force | Copy-Item -Destination $committed -Recurse -Force
        Write-Output "agent-plugin: wrote plugins/$($manifest.name) ($($files.Count) files, $(@($manifest.skills).Count) skills)"
    }
}
finally {
    if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
}
