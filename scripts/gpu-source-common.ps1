Set-StrictMode -Version Latest

function Convert-BytesToMiB {
    param([double]$Bytes)

    if ($Bytes -lt 0) {
        return 0.0
    }

    return [math]::Round($Bytes / 1MB, 2)
}

function Resolve-LuidBase {
    param([string]$InstanceName)

    if (-not $InstanceName) {
        return $null
    }

    $lower = $InstanceName.ToLowerInvariant()
    if ($lower -match '(luid_0x[0-9a-f]+_0x[0-9a-f]+_phys_\d+)') {
        return $Matches[1]
    }

    return $lower -replace '_part_\d+$', ''
}

function Get-NvidiaSmiSample {
    param([string]$NvidiaSmiPath)

    if (-not $NvidiaSmiPath) {
        $cmd = Get-Command nvidia-smi -ErrorAction Stop
        $NvidiaSmiPath = $cmd.Source
    }

    $line = & $NvidiaSmiPath --query-gpu=name,pci.bus_id,memory.total,memory.used,utilization.gpu,utilization.memory,temperature.gpu,power.draw,enforced.power.limit,power.max_limit --format=csv,noheader,nounits 2>$null | Select-Object -First 1
    if (-not $line) {
        return $null
    }

    $parts = $line -split ',' | ForEach-Object { $_.Trim() }
    if ($parts.Count -lt 10) {
        return $null
    }

    [pscustomobject]@{
        Name = $parts[0]
        PciBusId = $parts[1]
        MemoryTotalMiB = [int]($parts[2])
        MemoryUsedMiB = [int]($parts[3])
        UtilizationGpuPct = [double]($parts[4])
        UtilizationMemoryPct = [double]($parts[5])
        TemperatureC = [double]($parts[6])
        PowerDrawW = [double]($parts[7])
        EnforcedPowerLimitW = [double]($parts[8])
        PowerMaxLimitW = [double]($parts[9])
    }
}

function Get-PdhGpuSnapshot {
    $adapterRows = @{}

    $memoryCounters = @(
        '\GPU Adapter Memory(*)\Dedicated Usage',
        '\GPU Adapter Memory(*)\Shared Usage',
        '\GPU Adapter Memory(*)\Total Committed',
        '\GPU Local Adapter Memory(*)\Local Usage'
    )

    $memorySamples = Get-Counter $memoryCounters | Select-Object -ExpandProperty CounterSamples
    foreach ($sample in $memorySamples) {
        $luid = Resolve-LuidBase $sample.InstanceName
        if (-not $luid) {
            continue
        }

        if (-not $adapterRows.ContainsKey($luid)) {
            $adapterRows[$luid] = [ordered]@{
                Luid = $luid
                DedicatedBytes = 0.0
                SharedBytes = 0.0
                TotalCommittedBytes = 0.0
                LocalBytes = 0.0
                Eng3DMaxPct = 0.0
                Eng3DSumPct = 0.0
                ThermalC = [double]::NaN
            }
        }

        $path = $sample.Path.ToLowerInvariant()
        switch -Regex ($path) {
            'dedicated usage$' { $adapterRows[$luid].DedicatedBytes = [double]$sample.CookedValue; break }
            'shared usage$' { $adapterRows[$luid].SharedBytes = [double]$sample.CookedValue; break }
            'total committed$' { $adapterRows[$luid].TotalCommittedBytes = [double]$sample.CookedValue; break }
            'local usage$' { $adapterRows[$luid].LocalBytes = [double]$sample.CookedValue; break }
        }
    }

    $engineSamples = Get-Counter '\GPU Engine(*engtype_3D)\Utilization Percentage' | Select-Object -ExpandProperty CounterSamples
    foreach ($sample in $engineSamples) {
        $luid = Resolve-LuidBase $sample.InstanceName
        if (-not $luid) {
            continue
        }

        if (-not $adapterRows.ContainsKey($luid)) {
            $adapterRows[$luid] = [ordered]@{
                Luid = $luid
                DedicatedBytes = 0.0
                SharedBytes = 0.0
                TotalCommittedBytes = 0.0
                LocalBytes = 0.0
                Eng3DMaxPct = 0.0
                Eng3DSumPct = 0.0
                ThermalC = [double]::NaN
            }
        }

        $value = [double]$sample.CookedValue
        $adapterRows[$luid].Eng3DMaxPct = [math]::Max($adapterRows[$luid].Eng3DMaxPct, $value)
        $adapterRows[$luid].Eng3DSumPct += $value
    }

    try {
        $thermalSamples = Get-Counter '\GPU Thermal(*)\Temperature' -ErrorAction Stop | Select-Object -ExpandProperty CounterSamples
        foreach ($sample in $thermalSamples) {
            $luid = Resolve-LuidBase $sample.InstanceName
            if (-not $luid -or -not $adapterRows.ContainsKey($luid)) {
                continue
            }

            $adapterRows[$luid].ThermalC = [math]::Max($adapterRows[$luid].ThermalC, [double]$sample.CookedValue)
        }
    } catch {
    }

    $table = @(
        $adapterRows.Values |
            ForEach-Object {
                [pscustomobject]@{
                    Luid = $_.Luid
                    DedicatedMiB = Convert-BytesToMiB $_.DedicatedBytes
                    SharedMiB = Convert-BytesToMiB $_.SharedBytes
                    TotalCommittedMiB = Convert-BytesToMiB $_.TotalCommittedBytes
                    LocalMiB = Convert-BytesToMiB $_.LocalBytes
                    Eng3DMaxPct = [math]::Round($_.Eng3DMaxPct, 2)
                    Eng3DSumPct = [math]::Round($_.Eng3DSumPct, 2)
                    ThermalC = if ([double]::IsNaN($_.ThermalC)) { $null } else { [math]::Round($_.ThermalC, 2) }
                }
            } |
            Sort-Object @{ Expression = 'DedicatedMiB'; Descending = $true }, @{ Expression = 'Eng3DMaxPct'; Descending = $true }, @{ Expression = 'LocalMiB'; Descending = $true }
    )

    $selected = $table | Where-Object { $_.DedicatedMiB -gt 0 } | Select-Object -First 1
    if (-not $selected) {
        $selected = $table | Select-Object -First 1
    }

    [pscustomobject]@{
        Selected = $selected
        AdapterTable = $table
    }
}