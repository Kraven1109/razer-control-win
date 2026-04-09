param(
    [string]$CliPath = ".\\target\\debug\\razer-cli.exe",
    [ValidateSet("ac", "bat")]
    [string]$PowerSource = "ac",
    [int]$DelaySeconds = 2
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $CliPath)) {
    throw "CLI not found: $CliPath"
}

$nvidiaSmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
if (-not $nvidiaSmi) {
    throw "nvidia-smi is not available in PATH"
}

$profiles = @(
    @{ Mode = 0; Name = "Balanced" },
    @{ Mode = 1; Name = "Gaming" },
    @{ Mode = 2; Name = "Creator" },
    @{ Mode = 3; Name = "Silent" }
)

$results = foreach ($profile in $profiles) {
    & $CliPath write power $PowerSource $profile.Mode 0 0 | Out-Host
    Start-Sleep -Seconds $DelaySeconds

    $readBack = (& $CliPath read power $PowerSource) -join " "
    $gpu = (& nvidia-smi --query-gpu=name,power.draw,enforced.power.limit,power.max_limit,temperature.gpu --format=csv,noheader,nounits) -join " "

    [pscustomobject]@{
        Mode = $profile.Mode
        Profile = $profile.Name
        ReadBack = $readBack
        NvidiaSmi = $gpu
    }
}

$results | Format-Table -AutoSize