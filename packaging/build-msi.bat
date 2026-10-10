rem ===========================================================================
rem  build-msi.bat - Compile the Mayasaba WiX source into an MSI package.
rem
rem  Usage:  packaging\build-msi.bat [Debug|Release]   (default: Release)
rem
rem  Prerequisites: WiX 6 toolchain (wix.exe) on PATH.
rem  The app must already be built:  run app\build-app.bat Release first.
rem ===========================================================================
setlocal EnableExtensions

set "SCRIPT_DIR=%~dp0"
set "ROOT=%SCRIPT_DIR%.."
set "CONFIG=%~1"
if "%CONFIG%"=="" set "CONFIG=Release"

rem Locate the unpackaged app output tree from build-app.bat
set "APPBIN=%ROOT%\app\Mayasaba.App\x64\%CONFIG%\Mayasaba.App"

if not exist "%APPBIN%\Mayasaba.App.exe" (
  echo ERROR: Mayasaba.App.exe not found at %APPBIN%. Run app\build-app.bat %CONFIG% first. 1>&2
  exit /b 1
)

rem Locate WiX 6 (wix.exe build subcommand)
set "WIX=wix"
where %WIX% >nul 2>&1
if errorlevel 1 (
  echo ERROR: wix.exe not found on PATH. Install WiX 6. 1>&2
  exit /b 1
)

set "OUTDIR=%ROOT%\packaging\release"
if not exist "%OUTDIR%" mkdir "%OUTDIR%"

set "MSI=%OUTDIR%\Mayasaba.msi"

echo.
echo [build-msi] Config : %CONFIG%
echo [build-msi] AppBin : %APPBIN%
echo [build-msi] Output : %MSI%
echo.

rem AppBinDir is passed without a trailing backslash; the .wxs Source paths
rem include the separator (e.g. $(var.AppBinDir)\Chat\ChatPage.xaml).
rem -sice ICE03 is applied during validation (wix msi validate), not build.
rem ICE03 false-positive on Windows language pack locale dirs (gd-gb, mi-nz, ug-CN)
"%WIX%" build "%SCRIPT_DIR%Mayasaba.wxs" -d AppBinDir="%APPBIN%" -b "%APPBIN%" -out "%MSI%"
if errorlevel 1 (
  echo [build-msi] wix build FAILED (exit %errorlevel%) 1>&2
  exit /b %errorlevel%
)

echo.
echo [build-msi] MSI built: %MSI%
dir "%MSI%"
exit /b 0
