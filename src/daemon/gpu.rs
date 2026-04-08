/// GPU monitoring for Windows.
///
/// Strategy (lightweight, no background services):
///
/// 1. **nvidia-smi.exe** (primary) — ships with every NVIDIA driver package on
///    Windows.  Same CSV query as the Linux build.  Gives full data: util,
///    temp, power, VRAM, clocks.
///
/// 2. **PDH performance counters** (fallback) — the same API that Task Manager
///    uses.  Zero-overhead kernel counters via PdhOpenQuery/PdhCollectQueryData.
///    Gives GPU utilisation + VRAM; no temperature or power data.
///
/// The GPU monitor thread calls `query_gpu()` every few seconds and stores the
/// result in `GPU_STATUS_CACHE` which the daemon reads for GetGpuStatus IPC.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};
use lazy_static::lazy_static;
use log::*;

lazy_static! {
    static ref GPU_STATUS_CACHE: Mutex<Option<GpuStatus>> = Mutex::new(None);
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

/// Main query entry point — tries nvidia-smi first, falls back to PDH.
pub fn query_gpu() -> Option<GpuStatus> {
    if let Some(s) = query_nvidia_smi() {
        return Some(s);
    }
    query_pdh_gpu()
}

// ── nvidia-smi path detection ──────────────────────────────────────────────

fn find_nvidia_smi() -> Option<PathBuf> {
    // 1. Try plain name — covers System32 and anything in PATH
    if Command::new("nvidia-smi.exe")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return Some(PathBuf::from("nvidia-smi.exe"));
    }

    // 2. Classic NVSMI directory (older driver layouts)
    let classic = PathBuf::from(
        r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe",
    );
    if classic.exists() {
        return Some(classic);
    }

    None
}

// ── nvidia-smi query — identical CSV format to the Linux build ──────────────

fn query_nvidia_smi() -> Option<GpuStatus> {
    let exe = find_nvidia_smi()?;

    let output = Command::new(exe)
        .args([
            "--query-gpu=name,temperature.gpu,utilization.gpu,utilization.memory,\
             power.draw,enforced.power.limit,power.max_limit,\
             memory.used,memory.total,clocks.gr,clocks.mem",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_nvidia_smi_line(stdout.trim())
}

fn parse_nvidia_smi_line(line: &str) -> Option<GpuStatus> {
    let parts: Vec<&str> = line.splitn(11, ", ").collect();
    if parts.len() < 11 {
        return None;
    }
    Some(GpuStatus {
        name: parts[0].to_string(),
        temp_c: parts[1].trim().parse().unwrap_or(0),
        gpu_util: parts[2].trim().parse().unwrap_or(0),
        mem_util: parts[3].trim().parse().unwrap_or(0),
        power_w: parts[4].trim().parse().unwrap_or(0.0),
        power_limit_w: parts[5].trim().parse().unwrap_or(0.0),
        power_max_limit_w: parts[6].trim().parse().unwrap_or(0.0),
        mem_used_mb: parts[7].trim().parse().unwrap_or(0),
        mem_total_mb: parts[8].trim().parse().unwrap_or(0),
        clock_gpu_mhz: parts[9].trim().parse().unwrap_or(0),
        clock_mem_mhz: parts[10].trim().parse().unwrap_or(0),
    })
}

// ── PDH fallback — Windows Performance Data Helper ─────────────────────────
//
// Uses the same counters exposed in Task Manager:
//   \GPU Engine(*engtype_3D)\Utilization Percentage
//   \GPU Adapter Memory(*)\Dedicated Usage
//   \GPU Adapter Memory(*)\Dedicated Limit
//
// PDH wildcard counters return arrays (one entry per GPU engine / adapter).
// We aggregate: max across 3D engine instances for utilisation, sum across
// adapters for VRAM (usually just one discrete adapter).

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

    unsafe fn get_array_sum(hc: PDH_HCOUNTER) -> Option<f64> {
        unsafe {
            let mut buf_size: u32 = 0;
            let mut count: u32 = 0;
            let _ = PdhGetFormattedCounterArrayW(
                hc,
                PDH_FMT_DOUBLE,
                &mut buf_size,
                &mut count,
                None,
            );
            if buf_size == 0 {
                return None;
            }
            let item_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let needed = (buf_size as usize).max(item_size * count as usize);
            let mut buf: Vec<u8> = vec![0u8; needed];
            let items_ptr = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            let ret = PdhGetFormattedCounterArrayW(
                hc,
                PDH_FMT_DOUBLE,
                &mut buf_size,
                &mut count,
                Some(items_ptr),
            );
            if ret != 0 {
                return None;
            }
            let mut sum = 0.0_f64;
            for i in 0..count as usize {
                sum += (*items_ptr.add(i)).FmtValue.Anonymous.doubleValue;
            }
            Some(sum)
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

        let path_util = wide("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
        let path_used = wide("\\GPU Adapter Memory(*)\\Dedicated Usage");
        let path_total = wide("\\GPU Adapter Memory(*)\\Dedicated Limit");

        let _ = PdhAddEnglishCounterW(query, PCWSTR(path_util.as_ptr()), 0, &mut h_util);
        let _ = PdhAddEnglishCounterW(query, PCWSTR(path_used.as_ptr()), 0, &mut h_vram_used);
        let _ = PdhAddEnglishCounterW(query, PCWSTR(path_total.as_ptr()), 0, &mut h_vram_total);

        // First collection — needed before rate counters produce values
        let _ = PdhCollectQueryData(query);
        std::thread::sleep(std::time::Duration::from_millis(250));
        // Second collection — counters are now computable
        if PdhCollectQueryData(query) != 0 {
            PdhCloseQuery(query);
            return None;
        }

        let util = get_array_max(h_util).unwrap_or(0.0).min(100.0);
        let vram_used_bytes = get_array_sum(h_vram_used).unwrap_or(0.0);
        let vram_total_bytes = get_array_sum(h_vram_total).unwrap_or(0.0);

        PdhCloseQuery(query);

        let mem_used_mb = (vram_used_bytes / 1_048_576.0) as u32;
        let mem_total_mb = (vram_total_bytes / 1_048_576.0) as u32;
        let mem_util = if mem_total_mb > 0 {
            (mem_used_mb * 100 / mem_total_mb) as u8
        } else {
            0
        };

        debug!(
            "PDH GPU: util={:.1}% vram={}/{}MB",
            util, mem_used_mb, mem_total_mb
        );

        Some(GpuStatus {
            name: "GPU (PDH)".to_string(),
            gpu_util: util as u8,
            mem_util,
            mem_used_mb,
            mem_total_mb,
            ..Default::default()
        })
    }
}

#[cfg(not(windows))]
fn query_pdh_gpu() -> Option<GpuStatus> {
    None
}
