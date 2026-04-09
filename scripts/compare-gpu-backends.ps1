param()

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\gpu-source-common.ps1"

$nvidia = Get-NvidiaSmiSample
$pdh = Get-PdhGpuSnapshot

if (-not $pdh.Selected) {
    throw 'No PDH GPU adapter samples were found.'
}

$summary = @(
    [pscustomobject]@{
        Source = 'PDH'
        Adapter = $pdh.Selected.Luid
        UtilPct = $pdh.Selected.Eng3DMaxPct
        MemoryUsedMiB = $pdh.Selected.DedicatedMiB
        MemoryTotalMiB = $null
        TemperatureC = $pdh.Selected.ThermalC
        PowerDrawW = $null
        PowerLimitW = $null
    }
)

if ($nvidia) {
    $summary += [pscustomobject]@{
        Source = 'nvidia-smi'
        Adapter = $nvidia.Name
        UtilPct = $nvidia.UtilizationGpuPct
        MemoryUsedMiB = $nvidia.MemoryUsedMiB
        MemoryTotalMiB = $nvidia.MemoryTotalMiB
        TemperatureC = $nvidia.TemperatureC
        PowerDrawW = $nvidia.PowerDrawW
        PowerLimitW = $nvidia.EnforcedPowerLimitW
    }
}

$summary | Format-Table -AutoSize
''
'PDH adapter table:'
$pdh.AdapterTable | Format-Table -AutoSize

if ($nvidia) {
    ''
    'Delta (PDH selected adapter vs nvidia-smi):'
    [pscustomobject]@{
        MemoryUsedDeltaMiB = [math]::Round(($pdh.Selected.DedicatedMiB - $nvidia.MemoryUsedMiB), 2)
        UtilDeltaPct = [math]::Round(($pdh.Selected.Eng3DMaxPct - $nvidia.UtilizationGpuPct), 2)
        TemperatureDeltaC = if ($pdh.Selected.ThermalC -ne $null) { [math]::Round(($pdh.Selected.ThermalC - $nvidia.TemperatureC), 2) } else { $null }
    } | Format-Table -AutoSize
}