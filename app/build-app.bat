@echo off
rem ============================================================================
rem  Mayasaba.App — build the WinUI 3 / C++/WinRT unpackaged desktop app.
rem
rem  Restores NuGet packages from nuget.org and builds with MSBuild.
rem  Usage:  build-app.bat [Debug|Release]      (default: Release)
rem
rem  Notes:
rem    * Visual Studio 2026 Enterprise (MSBuild 18.x, toolset v145) is the primary
rem      toolchain; Visual Studio 2022 paths are used as a fallback.
rem    * Output: Mayasaba.App\x64\<Configuration>\Mayasaba.App\Mayasaba.App.exe
rem ============================================================================
setlocal EnableExtensions

set "SCRIPT_DIR=%~dp0"
set "PROJECT=%SCRIPT_DIR%Mayasaba.App\Mayasaba.App.vcxproj"

set "CONFIG=%~1"
if "%CONFIG%"=="" set "CONFIG=Release"
set "PLATFORM=x64"

set "MSBUILD=%ProgramFiles%\Microsoft Visual Studio\18\Enterprise\MSBuild\Current\Bin\amd64\MSBuild.exe"
if not exist "%MSBUILD%" set "MSBUILD=%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\amd64\MSBuild.exe"
if not exist "%MSBUILD%" set "MSBUILD=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe"

if not exist "%MSBUILD%" (
  echo ERROR: MSBuild.exe not found. Install Visual Studio 2022/2026 with Desktop development with C++. 1>&2
  exit /b 1
)

echo.
echo [build-app] MSBuild : %MSBUILD%
echo [build-app] Project : %PROJECT%
echo [build-app] Config  : %CONFIG%^|%PLATFORM%
echo [build-app] Command : "%MSBUILD%" "%PROJECT%" -restore -p:Configuration=%CONFIG% -p:Platform=%PLATFORM% -m -v:m -clp:Summary
echo.

rem -clp:Summary is explicit: MSBuild 18.x omits the "Build succeeded / N Warning(s) /
rem N Error(s)" block at -v:m unless the console logger's Summary is turned on.
"%MSBUILD%" "%PROJECT%" -restore -p:Configuration=%CONFIG% -p:Platform=%PLATFORM% -m -v:m -clp:Summary
exit /b %ERRORLEVEL%
