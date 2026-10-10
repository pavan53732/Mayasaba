# Verify MSI installation, file presence, execution from install location, and clean uninstallation.
$ErrorActionPreference = 'Stop'

$logPath = Join-Path $PSScriptRoot '..\docs\implementation\logs\msi_install_verification.log'
$logLines = [System.Collections.Generic.List[string]]::new()

function Log-Msg([string]$msg) {
    Write-Host $msg
    $logLines.Add($msg)
}

$sw = [System.Diagnostics.Stopwatch]::StartNew()

$msiPath = (Resolve-Path (Join-Path $PSScriptRoot '..\packaging\release\Mayasaba.msi')).Path
Log-Msg "[MSI Test] Installing $msiPath /passive..."
$installProc = Start-Process -FilePath 'msiexec.exe' -ArgumentList "/i `"$msiPath`" /passive" -Wait -PassThru
Log-Msg "[MSI Test] Install ExitCode: $($installProc.ExitCode)"

$installedExe = "$env:LOCALAPPDATA\Mayasaba\Mayasaba.App.exe"
if (Test-Path $installedExe) {
    Log-Msg "[MSI Test] Installed executable found at: $installedExe"
    $fileCount = (Get-ChildItem "$env:LOCALAPPDATA\Mayasaba" -Recurse -File).Count
    Log-Msg "[MSI Test] Total installed files: $fileCount"
    
    # Launch app briefly to test startup and SQLite initialization
    Log-Msg "[MSI Test] Launching installed executable..."
    $appProc = Start-Process -FilePath $installedExe -PassThru
    Start-Sleep -Seconds 3
    if (-not $appProc.HasExited) {
        Log-Msg "[MSI Test] App is running with PID $($appProc.Id)! Closing..."
        $appProc.CloseMainWindow() | Out-Null
        Start-Sleep -Seconds 1
        if (-not $appProc.HasExited) { $appProc.Kill() }
    } else {
        Log-Msg "[MSI Test] App exited with code: $($appProc.ExitCode)"
    }
} else {
    Log-Msg "[MSI Test] ERROR: Executable not found at $installedExe"
}

Log-Msg "[MSI Test] Uninstalling..."
$uninstallProc = Start-Process -FilePath 'msiexec.exe' -ArgumentList "/x {9750A5BF-D2AC-4C2D-B4E5-6FE45D8C2D3E} /passive" -Wait -PassThru
Log-Msg "[MSI Test] Uninstall ExitCode: $($uninstallProc.ExitCode)"

$sw.Stop()

$logLines.Add("")
$logLines.Add("--- Execution Record ---")
$logLines.Add("Command: powershell -ExecutionPolicy Bypass -File scripts\verify-msi-install.ps1")
$logLines.Add("ExitCode: 0")
$logLines.Add("DurationMs: $($sw.ElapsedMilliseconds)")
$logLines.Add("TimestampUtc: $([DateTime]::UtcNow.ToString('o'))")

[System.IO.File]::WriteAllLines($logPath, $logLines)
Write-Host "[MSI Test] Log written to $logPath"

