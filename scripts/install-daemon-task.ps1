# install-daemon-task.ps1
# Registers razer-daemon.exe as a Task Scheduler task that starts at logon,
# runs elevated (highest privileges), and has no visible console window.
#
# Run once from an elevated PowerShell session:
#   .\scripts\install-daemon-task.ps1
#
# To uninstall:
#   Unregister-ScheduledTask -TaskName "RazerDaemon" -Confirm:$false

#Requires -RunAsAdministrator

$TaskName   = "RazerDaemon"
$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot   = Split-Path -Parent $ScriptDir
$DaemonExe  = Join-Path $RepoRoot "target\release\razer-daemon.exe"

if (-not (Test-Path $DaemonExe)) {
    Write-Error "razer-daemon.exe not found at: $DaemonExe`nRun 'cargo build --release' first."
    exit 1
}

$Action    = New-ScheduledTaskAction -Execute $DaemonExe
$Trigger   = New-ScheduledTaskTrigger -AtLogOn
$Principal = New-ScheduledTaskPrincipal `
    -UserId      ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) `
    -LogonType   Interactive `
    -RunLevel    Highest

$Settings  = New-ScheduledTaskSettingsSet `
    -ExecutionTimeLimit (New-TimeSpan -Hours 0) `   # No timeout
    -RestartCount 3 `
    -RestartInterval (New-TimeSpan -Minutes 1) `
    -MultipleInstances IgnoreNew

# Remove existing task if present
Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue

Register-ScheduledTask `
    -TaskName  $TaskName `
    -Action    $Action `
    -Trigger   $Trigger `
    -Principal $Principal `
    -Settings  $Settings `
    -Description "Razer laptop control daemon — runs elevated without console window" `
    | Out-Null

Write-Host "Task '$TaskName' registered. razer-daemon.exe will start at next logon." -ForegroundColor Green
Write-Host "To start immediately: Start-ScheduledTask -TaskName '$TaskName'" -ForegroundColor Cyan
