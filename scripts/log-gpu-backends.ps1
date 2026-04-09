param(
    [int]$Count = 30,
    [double]$IntervalSeconds = 1,
    [string]$OutputPath = $(Join-Path $env:TEMP ("gpu-compare-" + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.csv'))
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\gpu-source-common.ps1"

$rows = for ($index = 0; $index -lt $Count; $index++) {
    $nvidia = Get-NvidiaSmiSample
    $pdh = Get-PdhGpuSnapshot
    $selected = $pdh.Selected

    [pscustomobject]@{
        Timestamp = (Get-Date).ToString('o')
        PdhLuid = $selected.Luid
        PdhEng3DMaxPct = $selected.Eng3DMaxPct
        PdhEng3DSumPct = $selected.Eng3DSumPct
        PdhDedicatedMiB = $selected.DedicatedMiB
        PdhSharedMiB = $selected.SharedMiB
        PdhTotalCommittedMiB = $selected.TotalCommittedMiB
        PdhLocalMiB = $selected.LocalMiB
        PdhThermalC = $selected.ThermalC
        NvidiaName = if ($nvidia) { $nvidia.Name } else { $null }
        NvidiaBusId = if ($nvidia) { $nvidia.PciBusId } else { $null }
        NvidiaUtilPct = if ($nvidia) { $nvidia.UtilizationGpuPct } else { $null }
        NvidiaMemUtilPct = if ($nvidia) { $nvidia.UtilizationMemoryPct } else { $null }
        NvidiaMemoryUsedMiB = if ($nvidia) { $nvidia.MemoryUsedMiB } else { $null }
        NvidiaMemoryTotalMiB = if ($nvidia) { $nvidia.MemoryTotalMiB } else { $null }
        NvidiaTemperatureC = if ($nvidia) { $nvidia.TemperatureC } else { $null }
        NvidiaPowerDrawW = if ($nvidia) { $nvidia.PowerDrawW } else { $null }
        NvidiaPowerLimitW = if ($nvidia) { $nvidia.EnforcedPowerLimitW } else { $null }
        UtilDeltaPct = if ($nvidia) { [math]::Round(($selected.Eng3DMaxPct - $nvidia.UtilizationGpuPct), 2) } else { $null }
        MemoryDeltaMiB = if ($nvidia) { [math]::Round(($selected.DedicatedMiB - $nvidia.MemoryUsedMiB), 2) } else { $null }
    }

    if ($index -lt ($Count - 1)) {
        Start-Sleep -Milliseconds ([int]($IntervalSeconds * 1000))
    }
}

$rows | Export-Csv -NoTypeInformation -Path $OutputPath
Write-Output "Wrote GPU comparison samples to $OutputPath"
$rows | Select-Object -First ([Math]::Min($Count, 5)) | Format-Table -AutoSize