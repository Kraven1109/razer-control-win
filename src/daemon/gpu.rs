/// GPU monitoring for Windows.
///
/// Strategy:
///
/// 1. Prefer `nvidia-smi` for the discrete NVIDIA GPU. This gives accurate
///    temperature, power, clocks, and dedicated VRAM for the graph.
/// 2. Fall back to PDH / Task Manager counters when `nvidia-smi` is not
///    available. PDH is still useful for lightweight telemetry, but it is not
///    reliable enough on its own for all metrics on Optimus systems.
///
/// The GPU monitor thread calls `query_gpu()` every few seconds and stores the
/// result in `GPU_STATUS_CACHE` which the daemon reads for `GetGpuStatus` IPC.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use lazy_static::lazy_static;
use log::*;

lazy_static! {
    static ref GPU_STATUS_CACHE: Mutex<Option<GpuStatus>> = Mutex::new(None);
    /// Cached path to nvidia-smi.exe — resolved once, never re-spawned for discovery.
    static ref NVIDIA_SMI_PATH: Option<PathBuf> = resolve_nvidia_smi();
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GpuStatus {
    pub name: String,
    pub temp_c: i32,
    pub gpu_util: u8,
    pub mem_util: u8,
    pub power_w: f32,
    pub power_limit_w: f32,
    pub power_max_limit_w: f32,
    pub mem_used_mb: u32,
    pub mem_total_mb: u32,
    pub clock_gpu_mhz: u32,
    pub clock_mem_mhz: u32,
}

pub fn store_gpu_cache(status: &GpuStatus) {
    if let Ok(mut cache) = GPU_STATUS_CACHE.lock() {
        *cache = Some(status.clone());
    }
}

pub fn clear_gpu_cache() {
    if let Ok(mut cache) = GPU_STATUS_CACHE.lock() {
        *cache = None;
    }
}

pub fn get_cached_gpu_status() -> Option<GpuStatus> {
    GPU_STATUS_CACHE.lock().ok().and_then(|g| g.clone())
}

/// Main query entry point.
///
/// Strategy:
/// 1. Always query PDH first — it reads Windows Performance Counters which are
///    passive and do **not** prevent the dGPU from entering D3 sleep.
/// 2. Only call `nvidia-smi` when PDH reports non-trivial GPU activity
///    (utilization > 0 % or dedicated VRAM usage > 500 MB).  When the GPU is
///    actually being used it is already awake, so the subprocess overhead is
///    acceptable and provides accurate power/clock data.
/// 3. If PDH itself fails, fall back to nvidia-smi unconditionally (degraded
///    mode — cannot avoid waking the GPU in this rare path).
pub fn query_gpu() -> Option<GpuStatus> {
    let pdh = query_pdh_gpu();

    // GATEKEEPER — two passive indicators that the dGPU is awake (D0/D3-Hot):
    //
    //  1. Dedicated VRAM > 256 MB: when the dGPU enters D3-Cold the VRAM rail
    //     is powered off and dedicated usage drops to near zero.  The iGPU only
    //     exposes ~128 MB dedicated memory, so > 256 MB unambiguously means the
    //     discrete adapter is holding content.
    //
    //  2. Temperature sensor > 0 °C: in D3-Cold the NVML/PDH thermal sensor
    //     returns 0 because the sensor block is unpowered.  Any positive reading
    //     means the package is at least partially awake.
    //
    // Deliberately NOT using gpu_util here: the wildcard PDH counter
    // `\GPU Engine(*engtype_3D)\Utilization Percentage` captures every 3D engine
    // across ALL adapters, including the iGPU running DWM at 1–5 %.
    // `get_array_max` picks the highest value, so util is NEVER zero on a
    // live desktop — making util a useless gate on Optimus/Advanced-Optimus.
    let gpu_active = pdh.as_ref().map_or(false, |g| {
        g.mem_used_mb > 256 || (g.temp_c > 0 && g.temp_c < 150)
    });

    if gpu_active {
        // dGPU is awake (D0 / D3-Hot) and holding VRAM → safe to call nvidia-smi.
        if let Some(smi) = query_nvidia_smi() {
            return Some(smi);
        }
        // nvidia-smi unavailable — return PDH data as-is (util from iGPU noise
        // is acceptable here because we know the dGPU is actually awake).
        return pdh;
    }

    // dGPU is sleeping (D3-Cold).
    // Sanitize the PDH data: strip iGPU "noise" from the 3D-engine utilisation
    // counter so the GUI does not display phantom dGPU activity.
    pdh.map(|mut g| {
        g.name         = "NVIDIA GPU (Sleeping)".to_string();
        g.gpu_util     = 0;
        g.power_w      = 0.0;
        g.clock_gpu_mhz = 0;
        g.clock_mem_mhz = 0;
        g
    })
}

/// Resolve the path to nvidia-smi.exe once at startup.
fn resolve_nvidia_smi() -> Option<PathBuf> {
    if Command::new("nvidia-smi.exe")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?
        .success()
    {
        return Some(PathBuf::from("nvidia-smi.exe"));
    }

    let classic = PathBuf::from(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe");
    if classic.exists() {
        return Some(classic);
    }

    None
}

fn find_nvidia_smi() -> Option<&'static PathBuf> {
    NVIDIA_SMI_PATH.as_ref()
}

fn parse_nvidia_smi_line(line: &str) -> Option<GpuStatus> {
    fn parse_u8(input: &str) -> u8 {
        input.trim().parse().unwrap_or(0)
    }

    fn parse_u32(input: &str) -> u32 {
        input.trim().parse().unwrap_or(0)
    }

    fn parse_i32(input: &str) -> i32 {
        input.trim().parse().unwrap_or(0)
    }

    fn parse_f32(input: &str) -> f32 {
        input.trim().parse().unwrap_or(0.0)
    }

    let parts: Vec<&str> = line.split(',').map(|part| part.trim()).collect();
    if parts.len() < 11 {
        return None;
    }

    Some(GpuStatus {
        name: parts[0].to_string(),
        temp_c: parse_i32(parts[1]),
        gpu_util: parse_u8(parts[2]),
        mem_util: parse_u8(parts[3]),
        power_w: parse_f32(parts[4]),
        power_limit_w: parse_f32(parts[5]),
        power_max_limit_w: parse_f32(parts[6]),
        mem_used_mb: parse_u32(parts[7]),
        mem_total_mb: parse_u32(parts[8]),
        clock_gpu_mhz: parse_u32(parts[9]),
        clock_mem_mhz: parse_u32(parts[10]),
    })
}

fn query_nvidia_smi() -> Option<GpuStatus> {
    let exe = match find_nvidia_smi() {
        Some(exe) => exe,
        None => {
            debug!("nvidia-smi not found; using PDH fallback");
            return None;
        }
    };

    let output = Command::new(exe)
        .args([
            "--query-gpu=name,temperature.gpu,utilization.gpu,utilization.memory,power.draw,enforced.power.limit,power.max_limit,memory.used,memory.total,clocks.gr,clocks.mem",
            "--format=csv,noheader,nounits",
        ])
        .output();

    let output = match output {
        Ok(output) => output,
        Err(err) => {
            debug!("failed to spawn nvidia-smi: {err}");
            return None;
        }
    };

    if !output.status.success() {
        debug!(
            "nvidia-smi returned non-zero status: {} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = match stdout.lines().find(|line| !line.trim().is_empty()) {
        Some(line) => line,
        None => {
            debug!("nvidia-smi produced no CSV data");
            return None;
        }
    };
    let sample = match parse_nvidia_smi_line(line) {
        Some(sample) => sample,
        None => {
            debug!("failed to parse nvidia-smi CSV line: {line}");
            return None;
        }
    };
    debug!(
        "nvidia-smi GPU: util={}%, mem={}/{}MB, temp={}°C, power={:.1}W/{:.1}W",
        sample.gpu_util,
        sample.mem_used_mb,
        sample.mem_total_mb,
        sample.temp_c,
        sample.power_w,
        sample.power_limit_w,
    );

    Some(sample)
}

// ── PDH fallback — Windows Performance Data Helper ─────────────────────────
//
// Uses the same counters exposed in Task Manager:
//   \GPU Engine(*engtype_3D)\Utilization Percentage
//   \GPU Adapter Memory(*)\Dedicated Usage
//   \GPU Local Adapter Memory(*)\Local Usage
//
// PDH wildcard counters return arrays (one entry per GPU engine / adapter).
// We aggregate conservatively: max across 3D engine instances and max across
// adapter memory instances. This keeps the fallback simple and typically picks
// the discrete adapter on systems where it exposes dedicated memory usage.

#[cfg(windows)]
fn query_pdh_gpu() -> Option<GpuStatus> {
    use windows::{
        core::PCWSTR,
        Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
            PdhGetFormattedCounterArrayW, PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W,
            PDH_FMT_DOUBLE,
        },
    };
    // PDH handles are raw isize in windows 0.58 (not exported as type aliases)
    #[allow(non_camel_case_types)] type PDH_HQUERY = isize;
    #[allow(non_camel_case_types)] type PDH_HCOUNTER = isize;

    fn wide(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // PDH_MORE_DATA reserved for future error handling

    unsafe fn get_array_max(hc: PDH_HCOUNTER) -> Option<f64> {
        unsafe {
            let mut buf_size: u32 = 0;
            let mut count: u32 = 0;
            let ret = PdhGetFormattedCounterArrayW(
                hc,
                PDH_FMT_DOUBLE,
                &mut buf_size,
                &mut count,
                None,
            );
            if buf_size == 0 {
                return None;
            }
            // PDH_MORE_DATA or ERROR_SUCCESS both acceptable at size-query
            let _ = ret;

            let item_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let needed = (buf_size as usize).max(item_size * count as usize);
            let mut buf: Vec<u8> = vec![0u8; needed];
            let items_ptr = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            let ret2 = PdhGetFormattedCounterArrayW(
                hc,
                PDH_FMT_DOUBLE,
                &mut buf_size,
                &mut count,
                Some(items_ptr),
            );
            if ret2 != 0 {
                return None;
            }
            let mut max_val = 0.0_f64;
            for i in 0..count as usize {
                let item = &*items_ptr.add(i);
                let v = item.FmtValue.Anonymous.doubleValue;
                if v > max_val {
                    max_val = v;
                }
            }
            Some(max_val)
        }
    }

    unsafe {
        let mut query: PDH_HQUERY = 0;
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
            return None;
        }

        let mut h_util: PDH_HCOUNTER = 0;
        let mut h_vram_used: PDH_HCOUNTER = 0;
        let mut h_vram_total: PDH_HCOUNTER = 0;
        let mut h_temp: PDH_HCOUNTER = 0;

        let path_util  = wide("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
        // Use max instead of sum for memory counters — the RTX 4090 has the
        // largest dedicated VRAM budget, so max() selects that adapter and
        // ignores the Intel iGPU (which shows a much smaller or zero value).
        let path_used  = wide("\\GPU Adapter Memory(*)\\Dedicated Usage");
        let path_total = wide("\\GPU Adapter Memory(*)\\Dedicated Limit");
        // Wildcard thermal counter — discrete GPU is hotter, so max gives RTX temp.
        let path_temp  = wide("\\GPU Thermal(*)\\Temperature");

        let _ = PdhAddEnglishCounterW(query, PCWSTR(path_util.as_ptr()),  0, &mut h_util);
        let _ = PdhAddEnglishCounterW(query, PCWSTR(path_used.as_ptr()),  0, &mut h_vram_used);
        let _ = PdhAddEnglishCounterW(query, PCWSTR(path_total.as_ptr()), 0, &mut h_vram_total);
        // Thermal counter: ignore failure — not present on all systems/drivers.
        let temp_added =
            PdhAddEnglishCounterW(query, PCWSTR(path_temp.as_ptr()), 0, &mut h_temp) == 0;

        // First collection — needed before rate counters produce values
        let _ = PdhCollectQueryData(query);
        std::thread::sleep(std::time::Duration::from_millis(250));
        // Second collection — counters are now computable
        if PdhCollectQueryData(query) != 0 {
            PdhCloseQuery(query);
            return None;
        }

        let util = get_array_max(h_util).unwrap_or(0.0).min(100.0);
        // Use max to pick the discrete RTX adapter (largest VRAM budget).
        let vram_used_bytes  = get_array_max(h_vram_used).unwrap_or(0.0);
        let vram_total_bytes = get_array_max(h_vram_total).unwrap_or(0.0);

        // Temperature: max across all thermal instances — RTX runs hotter and
        // exposes a real sensor; iGPU either returns 0 or a lower value.
        let temp_c = if temp_added {
            get_array_max(h_temp).unwrap_or(0.0).clamp(0.0, 150.0) as i32
        } else {
            0
        };

        PdhCloseQuery(query);

        let mem_used_mb  = (vram_used_bytes  / 1_048_576.0) as u32;
        let mem_total_mb = (vram_total_bytes / 1_048_576.0) as u32;
        let mem_util = if mem_total_mb > 0 {
            (mem_used_mb * 100 / mem_total_mb) as u8
        } else {
            0
        };

        debug!(
            "PDH GPU: util={:.1}% vram={}/{}MB temp={}°C",
            util, mem_used_mb, mem_total_mb, temp_c
        );

        Some(GpuStatus {
            name: "GPU (Task Manager counters)".to_string(),
            gpu_util: util as u8,
            mem_util,
            mem_used_mb,
            mem_total_mb,
            temp_c,
            ..Default::default()
        })
    }
}

#[cfg(not(windows))]
fn query_pdh_gpu() -> Option<GpuStatus> {
    None
}

#[cfg(test)]
mod tests {
    use super::parse_nvidia_smi_line;

    #[test]
    fn parse_nvidia_smi_csv_line() {
        let sample = parse_nvidia_smi_line(
            "NVIDIA GeForce RTX 4090 Laptop GPU, 73, 43, 1, 106.20, 110.32, 175.00, 197, 16376, 210, 405"
        )
        .expect("nvidia-smi line should parse");

        assert_eq!(sample.name, "NVIDIA GeForce RTX 4090 Laptop GPU");
        assert_eq!(sample.temp_c, 73);
        assert_eq!(sample.gpu_util, 43);
        assert_eq!(sample.mem_util, 1);
        assert_eq!(sample.mem_used_mb, 197);
        assert_eq!(sample.mem_total_mb, 16376);
        assert!((sample.power_w - 106.20).abs() < 0.01);
        assert!((sample.power_limit_w - 110.32).abs() < 0.01);
    }
}
