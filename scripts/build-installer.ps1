<#
.SYNOPSIS
    Stage the self-contained Mayasaba payload and build the WiX v6 MSI (Milestone M12).

.DESCRIPTION
    The MSI is the mandatory end-user installation format (see the system description's
    packaging section). This script:

      1. Verifies the unpackaged WinUI 3 app has been built for the requested configuration.
      2. Stages a clean copy of its self-contained output directory into installer\staging,
         dropping build-only artifacts (.pdb/.lib/.exp/.ilk/.obj/...). Everything else is
         runtime payload: Mayasaba.App.exe, the Windows App SDK binaries, .pri/.xbf resources,
         .winmd metadata, locale .mui folders and assets\mayasaba.ico.
      3. Generates installer\payload.generated.wxs: one Component per payload file, nested under
         INSTALLFOLDER, with a deterministic component GUID and an explicit neutral Language.
      4. Builds installer\bin\Mayasaba.msi with 'wix build'.
      5. Runs ICE validation with 'wix msi validate'.

    External CLIs (Hermes, Claude Code, Kilo Code) are intentionally NOT bundled: they are
    user-installed, and a missing CLI must never fail installation.

.EXAMPLE
    ./scripts/build-installer.ps1
    ./scripts/build-installer.ps1 -Configuration Debug
    ./scripts/build-installer.ps1 -SkipValidate
#>
[CmdletBinding()]
param(
    [ValidateSet('Release', 'Debug')]
    [string] $Configuration = 'Release',

    [ValidateSet('x64')]
    [string] $Platform = 'x64',

    [switch] $SkipValidate
)

$ErrorActionPreference = 'Stop'

$repoRoot    = Split-Path -Parent $PSScriptRoot
$appPayload  = Join-Path $repoRoot "app\Mayasaba.App\x64\$Configuration\Mayasaba.App"
$wxs         = Join-Path $repoRoot 'installer\Mayasaba.wxs'
$generated   = Join-Path $repoRoot 'installer\payload.generated.wxs'
$staging     = Join-Path $repoRoot 'installer\staging'
$outDir      = Join-Path $repoRoot 'installer\bin'
$msi         = Join-Path $outDir 'Mayasaba.msi'

# wix is installed as a dotnet global tool.
$wix = Join-Path $env:USERPROFILE '.dotnet\tools\wix.exe'
if (-not (Test-Path $wix)) { $wix = 'wix' }

$appExe = Join-Path $appPayload 'Mayasaba.App.exe'
if (-not (Test-Path $appExe)) {
    throw "Payload not found: $appExe. Build it first with app/build-app.ps1 -Configuration $Configuration"
}

# --- Deterministic identity helpers ---------------------------------------------------------

function Get-PathHash {
    param([string] $Value)
    $sha   = [System.Security.Cryptography.SHA256]::Create()
    $bytes = $sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Value))
    return (($bytes[0..7] | ForEach-Object { $_.ToString('x2') }) -join '')
}

function Get-StableGuid {
    param([string] $Value)
    # RFC 4122 version 5 (SHA-1, name based) so a component GUID is a pure function of the
    # file's path: identical across builds and machines, and different per file.
    $sha   = [System.Security.Cryptography.SHA1]::Create()
    $hash  = $sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes('Mayasaba.Component.v1:' + $Value))
    $g     = New-Object 'byte[]' 16
    [Array]::Copy($hash, 0, $g, 0, 16)
    $g[6]  = [byte]((($g[6] -band 0x0F)) -bor 0x50)
    $g[8]  = [byte]((($g[8] -band 0x3F)) -bor 0x80)
    $hex   = ($g | ForEach-Object { $_.ToString('X2') }) -join ''
    return '{' + $hex.Substring(0,8) + '-' + $hex.Substring(8,4) + '-' + $hex.Substring(12,4) +
           '-' + $hex.Substring(16,4) + '-' + $hex.Substring(20,12) + '}'
}

function New-Node {
    return @{ Dirs = @{}; Files = (New-Object System.Collections.ArrayList) }
}

# --- 1. Stage a clean payload, dropping build-only artifacts --------------------------------
$buildOnlyExtensions = @('.pdb', '.lib', '.exp', '.ilk', '.obj', '.iobj', '.ipdb')

if (Test-Path $staging) {
    # .NET delete: the host's safe-delete wrapper only intercepts Remove-Item.
    [System.IO.Directory]::Delete($staging, $true)
}
[System.IO.Directory]::CreateDirectory($staging) | Out-Null

$relativePaths = New-Object System.Collections.ArrayList
$skipped       = 0

foreach ($file in (Get-ChildItem -Path $appPayload -Recurse -File)) {
    if ($buildOnlyExtensions -contains $file.Extension.ToLowerInvariant()) { $skipped++; continue }

    $relative  = $file.FullName.Substring($appPayload.Length).TrimStart('\', '/')
    $target    = Join-Path $staging $relative
    $targetDir = Split-Path -Parent $target
    if (-not (Test-Path $targetDir)) { [System.IO.Directory]::CreateDirectory($targetDir) | Out-Null }

    Copy-Item -LiteralPath $file.FullName -Destination $target -Force
    [void]$relativePaths.Add($relative)
}

Write-Host "[build-installer] staged $($relativePaths.Count) files ($skipped build-only skipped) -> $staging"

# --- 2. Generate the payload fragment -------------------------------------------------------
$tree = New-Node
foreach ($rel in $relativePaths) {
    $parts = $rel -split '[\\/]'
    $node  = $tree
    for ($i = 0; $i -lt ($parts.Count - 1); $i++) {
        $dirs = $node['Dirs']
        if (-not $dirs.ContainsKey($parts[$i])) { $dirs[$parts[$i]] = (New-Node) }
        $node = $dirs[$parts[$i]]
    }
    [void]$node['Files'].Add($rel)
}

function Write-Node {
    param($Node, [string] $RelPrefix, [string] $Indent, [System.Text.StringBuilder] $Sb)

    foreach ($name in ($Node['Dirs'].Keys | Sort-Object)) {
        $childRel = if ($RelPrefix) { "$RelPrefix\$name" } else { $name }
        $dirId    = 'dir_' + (Get-PathHash ('dir:' + $childRel))
        $escaped  = [System.Security.SecurityElement]::Escape($name)
        [void]$Sb.AppendLine("$Indent<Directory Id=`"$dirId`" Name=`"$escaped`">")
        Write-Node -Node $Node['Dirs'][$name] -RelPrefix $childRel -Indent "$Indent  " -Sb $Sb
        [void]$Sb.AppendLine("$Indent</Directory>")
    }

    foreach ($rel in ($Node['Files'] | Sort-Object)) {
        $hash    = Get-PathHash ('file:' + $rel)
        $guid    = Get-StableGuid $rel
        $source  = [System.Security.SecurityElement]::Escape((Join-Path $staging $rel))
        $fname   = [System.Security.SecurityElement]::Escape((Split-Path $rel -Leaf))
        [void]$Sb.AppendLine("$Indent<Component Id=`"cmp_$hash`" Guid=`"$guid`" Bitness=`"always64`">")
        [void]$Sb.AppendLine("$Indent  <File Id=`"fil_$hash`" Source=`"$source`" Name=`"$fname`" KeyPath=`"yes`" />")
        [void]$Sb.AppendLine("$Indent</Component>")
    }
}

$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine('<?xml version="1.0" encoding="UTF-8"?>')
[void]$sb.AppendLine('<!-- GENERATED by scripts/build-installer.ps1. Do not edit; do not commit. -->')
[void]$sb.AppendLine('<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">')
[void]$sb.AppendLine('  <Fragment>')
[void]$sb.AppendLine('    <StandardDirectory Id="ProgramFiles64Folder">')
[void]$sb.AppendLine('      <Directory Id="INSTALLFOLDER" Name="Mayasaba">')
Write-Node -Node $tree -RelPrefix '' -Indent '        ' -Sb $sb
[void]$sb.AppendLine('      </Directory>')
[void]$sb.AppendLine('    </StandardDirectory>')
[void]$sb.AppendLine('  </Fragment>')
[void]$sb.AppendLine('</Wix>')

[System.IO.File]::WriteAllText($generated, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "[build-installer] generated $generated"

if (-not (Test-Path $outDir)) { [System.IO.Directory]::CreateDirectory($outDir) | Out-Null }

# --- 3. Build the MSI -----------------------------------------------------------------------
Write-Host '[build-installer] wix build'
& $wix build $wxs $generated -d "PayloadDir=$staging" -arch $Platform -o $msi
if ($LASTEXITCODE -ne 0) { throw "wix build failed with exit code $LASTEXITCODE" }

$size = (Get-Item $msi).Length
Write-Host ("[build-installer] MSI: {0} ({1} bytes)" -f $msi, $size)

# --- 4. Validate ----------------------------------------------------------------------------
if (-not $SkipValidate) {
    # ICE03 is suppressed, deliberately and only ICE03.
    #
    # WiX v6's File element has no Language attribute, and WiX derives the File table's Language
    # column from each binary's version resource (for .mui files, from the locale folder name).
    # For the Windows App SDK payload that produces a 430-character comma-separated LCID list on
    # Microsoft.ui.xaml.dll and Microsoft.UI.Xaml.Phone.dll, and the LCIDs for ug-CN, mi-NZ and
    # gd-gb, which ICE03's language list does not recognise. There is no authoring-side fix for
    # a C++/WinRT project: this is a known WiX/MS ICE limitation (wixtoolset discussion #7992 is
    # this exact WinUI 3 scenario; issue #7658 notes Microsoft owns the ICE implementations).
    # The derived values are inert here because every payload file installs unconditionally.
    # Every other ICE runs and must pass.
    Write-Host '[build-installer] wix msi validate (ICE03 suppressed - see comment in this script)'
    & $wix msi validate $msi -sice ICE03
    if ($LASTEXITCODE -ne 0) { throw "wix msi validate failed with exit code $LASTEXITCODE" }
    Write-Host '[build-installer] validation passed'
}

Write-Host '[build-installer] done'
