<#
.SYNOPSIS
    Build the Mayasaba WinUI 3 / C++/WinRT unpackaged desktop app (Mayasaba.App).

.DESCRIPTION
    Restores NuGet packages from nuget.org and builds with MSBuild (toolset v145 on
    Visual Studio 2026). Mirrors build-app.bat but with -Clean support.

.EXAMPLE
    ./build-app.ps1
    ./build-app.ps1 -Configuration Debug
    ./build-app.ps1 -Clean
#>
[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string] $Configuration = 'Release',

    [ValidateSet('x64')]
    [string] $Platform = 'x64',

    [switch] $Clean
)

$ErrorActionPreference = 'Stop'

$projectDir = $PSScriptRoot
$project    = Join-Path $projectDir 'Mayasaba.App\Mayasaba.App.vcxproj'

$msbuildCandidates = @(
    (Join-Path $env:ProgramFiles 'Microsoft Visual Studio\18\Enterprise\MSBuild\Current\Bin\amd64\MSBuild.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\amd64\MSBuild.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe')
)
$msbuild = $msbuildCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $msbuild) {
    throw 'MSBuild.exe not found. Install Visual Studio 2022/2026 with Desktop development with C++.'
}

if ($Clean) {
    foreach ($dir in @('Mayasaba.App\x64', 'Mayasaba.App\Generated Files', 'Mayasaba.App\obj')) {
        $path = Join-Path $projectDir $dir
        if (Test-Path $path) {
            Write-Host "[build-app] removing $path"
            Remove-Item -Recurse -Force $path
        }
    }
}

# -clp:Summary is explicit: MSBuild 18.x omits the "Build succeeded / N Warning(s) /
# N Error(s)" block at -v:m unless the console logger's Summary is turned on.
$msbuildArgs = @(
    $project,
    '-restore',
    "-p:Configuration=$Configuration",
    "-p:Platform=$Platform",
    '-m',
    '-v:m',
    '-clp:Summary'
)

Write-Host "[build-app] MSBuild : $msbuild"
Write-Host "[build-app] Project : $project"
Write-Host "[build-app] Config  : $Configuration|$Platform"
Write-Host "[build-app] Command : `"$msbuild`" $($msbuildArgs -join ' ')"
Write-Host ''

& $msbuild @msbuildArgs
exit $LASTEXITCODE
