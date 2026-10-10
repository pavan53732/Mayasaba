<#
.SYNOPSIS
    Verifies the rendered native Mayasaba Chat shell through Windows UI Automation.

.DESCRIPTION
    Starts the unpackaged Release executable, locates its native top-level window by process ID,
    and verifies the accessible Chat controls required before a project is bound. It never opens a
    folder, submits a message, or changes user data. The launched process is closed before exit.

    This is a shell/runtime oracle, not the sustained multi-agent streaming oracle required by
    AGENTS.md section 14. It intentionally does not certify that broader gate.
#>
[CmdletBinding()]
param(
    [string] $ExecutablePath = (Join-Path $PSScriptRoot '..\app\Mayasaba.App\x64\Release\Mayasaba.App\Mayasaba.App.exe'),
    [int] $TimeoutSeconds = 15
)

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$exe = [IO.Path]::GetFullPath($ExecutablePath)
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "Mayasaba executable was not found: $exe"
}

$process = Start-Process -FilePath $exe -PassThru
try {
    $window = $null
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $processCondition = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty, $process.Id)
    while ($timer.Elapsed.TotalSeconds -lt $TimeoutSeconds -and -not $window) {
        $window = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
            [System.Windows.Automation.TreeScope]::Children, $processCondition)
        Start-Sleep -Milliseconds 200
    }
    if (-not $window) {
        throw "No native UI Automation window was exposed for Mayasaba PID $($process.Id)."
    }

    function Find-NamedElement([string] $name) {
        $condition = New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::NameProperty, $name)
        return $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
    }

    $composer = Find-NamedElement 'Message Mayasaba'
    $openFolder = Find-NamedElement 'Open or switch the project folder'
    $send = Find-NamedElement 'Send message'
    if (-not $composer -or -not $openFolder -or -not $send) {
        throw 'The rendered window did not expose all required Chat automation controls.'
    }
    if ($send.Current.IsEnabled) {
        throw 'Send is enabled before an authorized project root is bound.'
    }

    Write-Output "UIA PASS: PID=$($process.Id); Window=$($window.Current.Name); Composer=True; OpenFolder=True; SendDisabled=True"
}
finally {
    if (-not $process.HasExited) {
        $null = $process.CloseMainWindow()
        if (-not $process.WaitForExit(5000)) {
            Stop-Process -Id $process.Id -Force
            $process.WaitForExit()
        }
    }
}
