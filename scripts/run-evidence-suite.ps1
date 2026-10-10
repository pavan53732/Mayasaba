# Run the verification test suite across all configurations and capture exact logs with timestamps and exit codes.
$ErrorActionPreference = 'Stop'

$logsDir = Join-Path $PSScriptRoot '..\docs\implementation\logs'
if (-not (Test-Path $logsDir)) {
    New-Item -ItemType Directory -Force -Path $logsDir | Out-Null
}

function Run-And-Capture {
    param(
        [string]$Name,
        [string]$Command,
        [string]$Arguments,
        [string]$LogFile
    )
    Write-Host "[verification] Running $Name..."
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName = $Command
    $pinfo.Arguments = $Arguments
    $pinfo.RedirectStandardOutput = $true
    $pinfo.RedirectStandardError = $true
    $pinfo.UseShellExecute = $false
    $pinfo.CreateNoWindow = $true

    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $pinfo
    $process.Start() | Out-Null
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    $sw.Stop()

    $fullLog = $stdout
    if ($stderr) {
        $fullLog += "`n[STDERR]`n" + $stderr
    }
    $fullLog += "`n--- Execution Record ---`n"
    $fullLog += "Command: $Command $Arguments`n"
    $fullLog += "ExitCode: $($process.ExitCode)`n"
    $fullLog += "DurationMs: $($sw.ElapsedMilliseconds)`n"
    $fullLog += "TimestampUtc: $([DateTime]::UtcNow.ToString('o'))`n"

    Set-Content -Path $LogFile -Value $fullLog
    Write-Host "[verification] $Name complete: ExitCode $($process.ExitCode), $($sw.ElapsedMilliseconds) ms"
    if ($process.ExitCode -ne 0) {
        throw "$Name failed with exit code $($process.ExitCode)"
    }
}

Run-And-Capture -Name "CTest vs2026-debug" `
    -Command "ctest" `
    -Arguments "--preset vs2026-debug --output-on-failure" `
    -LogFile (Join-Path $logsDir "ctest_vs2026_debug.log")

Run-And-Capture -Name "CTest vs2026-release" `
    -Command "ctest" `
    -Arguments "--preset vs2026-release --output-on-failure" `
    -LogFile (Join-Path $logsDir "ctest_vs2026_release.log")

Run-And-Capture -Name "CTest vs2026-asan" `
    -Command "ctest" `
    -Arguments "--test-dir build/vs2026-asan -C Debug --output-on-failure" `
    -LogFile (Join-Path $logsDir "ctest_vs2026_asan.log")

Write-Host "[verification] All 3 test suites passed and captured to docs/implementation/logs/"
