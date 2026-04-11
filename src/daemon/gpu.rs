/// GPU monitoring for Windows.
///
/// Query priority chain:
///   1. NVML (NVIDIA Management Library) — direct DLL call, ~1–5 ms/poll, no subprocess.
///      Opened and closed on every poll so the driver connection does not persist between
///      polls, allowing the dGPU to enter D3-Cold freely.
///   2. nvidia-smi subprocess — fallback when NVML fails to initialise.
///   3. PDH (Windows Performance Data Helper) — lightweight gatekeeper to avoid waking
///      a sleeping dGPU unnecessarily; also primary data source when above are absent.
///
/// The GPU monitor thread calls `query_gpu()` every few seconds and stores the
/// result in `GPU_STATUS_CACHE` which the daemon reads for `GetGpuStatus` IPC.

use std::path::PathBuf;
use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt as _;
use std::sync::{Mutex, OnceLock};
use serde::{Deserialize, Serialize};
use lazy_static::lazy_static;
use log::*;

/// CREATE_NO_WINDOW — prevents nvidia-smi/find_msvc from flashing a console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
    //
    // map_or(false, …): if PDH failed to initialise we have no passive signal.
    // We fall through to the nvidia-smi path below rather than risk waking the
    // sleeping GPU via NVML.
    let gpu_active = pdh.as_ref().map_or(false, |g| {
        g.mem_used_mb > 256 || (g.temp_c > 0 && g.temp_c < 150)
    });

    if gpu_active {
        // dGPU is awake — try NVML first (sub-millisecond, no subprocess).
        if let Some(nvml) = query_nvml() {
            return Some(nvml);
        }
        // NVML unavailable — fall back to nvidia-smi subprocess.
        if let Some(smi) = query_nvidia_smi() {
            return Some(smi);
        }
        // Both unavailable — return PDH data as-is.
        return pdh;
    }

    // dGPU is sleeping (D3-Cold), OR PDH itself failed.
    // If PDH failed entirely, fall back to nvidia-smi as a last resort.
    // nvidia-smi exits after each query so it does not hold the GPU awake.
    if pdh.is_none() {
        return query_nvidia_smi();
    }

    // dGPU confirmed sleeping via PDH gatekeeper.
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

// ── NVML (NVIDIA Management Library) ─────────────────────────────────────
//
// Opened and closed on every poll so the driver connection is not held open
// between polls.  A persistent NVML connection can prevent the dGPU from
// entering D3-Cold on some Optimus / Advanced-Optimus configurations, which
// would result in idle power draw of 20-30 W that should otherwise be ~0 W.
//
// nvmlInit / nvmlShutdown are fast (< 5 ms each) so the per-poll overhead is
// negligible compared to the 3-second poll interval.

#[cfg(windows)]
fn query_nvml() -> Option<GpuStatus> {
    use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};

    // Init fresh for this poll; will be dropped (→ nvmlShutdown) at function return.
    let nvml = match nvml_wrapper::Nvml::init() {
        Ok(n)  => n,
        Err(e) => { warn!("NVML init failed ({e}); falling back to nvidia-smi"); return None; }
    };

    let device = nvml.device_by_index(0).ok()?;

    let name          = device.name().ok()?;
    let temp_c        = device.temperature(TemperatureSensor::Gpu).ok()? as i32;
    let util          = device.utilization_rates().ok()?;
    let power_mw      = device.power_usage().ok()?;
    let limit_mw      = device.power_management_limit().ok()?;
    let mem           = device.memory_info().ok()?;
    let clk_gpu       = device.clock_info(Clock::Graphics).ok()?;
    let clk_mem       = device.clock_info(Clock::Memory).ok()?;
    let max_limit_mw  = device.power_management_limit_constraints()
        .ok()
        .map(|c| c.max_limit)
        .unwrap_or(limit_mw);

    debug!(
        "NVML GPU: util={}%, mem={}/{}MB, temp={}°C, power={:.1}W/{:.1}W",
        util.gpu,
        mem.used / 1_048_576,
        mem.total / 1_048_576,
        temp_c,
        power_mw as f32 / 1000.0,
        limit_mw as f32 / 1000.0,
    );

    Some(GpuStatus {
        name,
        temp_c,
        gpu_util:         util.gpu as u8,
        mem_util:         util.memory as u8,
        power_w:          power_mw as f32 / 1000.0,
        power_limit_w:    limit_mw as f32 / 1000.0,
        power_max_limit_w: max_limit_mw as f32 / 1000.0,
        mem_used_mb:      (mem.used  / 1_048_576) as u32,
        mem_total_mb:     (mem.total / 1_048_576) as u32,
        clock_gpu_mhz:    clk_gpu,
        clock_mem_mhz:    clk_mem,
    })
}

#[cfg(not(windows))]
fn query_nvml() -> Option<GpuStatus> { None }

/// Resolve the path to nvidia-smi.exe once at startup.
fn resolve_nvidia_smi() -> Option<PathBuf> {
    if Command::new("nvidia-smi.exe")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
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
        .creation_flags(CREATE_NO_WINDOW)
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

// ── PDH fallback — Persistent Windows Performance Data Helper ──────────────
//
// The PDH query is opened ONCE at daemon startup and kept alive.
// Subsequent polls call PdhCollectQueryData only (no sleep, no open/close).
// The 250 ms primer sleep happens exactly once so rate counters have a
// baseline sample before the first read.

#[cfg(windows)]
struct GpuPdhState {
    query:       isize,    // PDH_HQUERY
    h_util:      isize,    // PDH_HCOUNTER — GPU Engine 3D utilisation
    h_vram_used: isize,    // PDH_HCOUNTER — Dedicated Memory Usage
    h_vram_total:isize,    // PDH_HCOUNTER — Dedicated Memory Limit
    h_temp:      isize,    // PDH_HCOUNTER — GPU Thermal (optional)
    temp_added:  bool,
    buffer:      Vec<u8>,  // scratch buffer for PdhGetFormattedCounterArrayW
}

// SAFETY: GpuPdhState is only ever accessed through Mutex<Option<GpuPdhState>>.
#[cfg(windows)] unsafe impl Send for GpuPdhState {}
#[cfg(windows)] unsafe impl Sync for GpuPdhState {}

#[cfg(windows)]
static GPU_PDH_STATE: OnceLock<Mutex<Option<GpuPdhState>>> = OnceLock::new();

#[cfg(windows)]
fn init_gpu_pdh() -> Option<GpuPdhState> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW,
        },
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    unsafe {
        let mut query: isize = 0;
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
            return None;
        }

        let mut h_util      = 0_isize;
        let mut h_vram_used = 0_isize;
        let mut h_vram_total= 0_isize;
        let mut h_temp      = 0_isize;

        let path_util  = wide("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
        let path_used  = wide("\\GPU Adapter Memory(*)\\Dedicated Usage");
        let path_total = wide("\\GPU Adapter Memory(*)\\Dedicated Limit");
        let path_temp  = wide("\\GPU Thermal(*)\\Temperature");

        if PdhAddEnglishCounterW(query, PCWSTR(path_util.as_ptr()),  0, &mut h_util) != 0 ||
           PdhAddEnglishCounterW(query, PCWSTR(path_used.as_ptr()),  0, &mut h_vram_used) != 0 ||
           PdhAddEnglishCounterW(query, PCWSTR(path_total.as_ptr()), 0, &mut h_vram_total) != 0
        {
            PdhCloseQuery(query);
            return None;
        }

        // Thermal is optional — not present on all driver versions
        let temp_added = PdhAddEnglishCounterW(query, PCWSTR(path_temp.as_ptr()), 0, &mut h_temp) == 0;

        // ONE-TIME 250 ms primer: rate counters need two samples to produce a value.
        // This happens at daemon startup while the rest of the daemon initialises.
        PdhCollectQueryData(query);
        std::thread::sleep(std::time::Duration::from_millis(250));
        PdhCollectQueryData(query);

        info!("PDH GPU counters initialised (persistent)");

        Some(GpuPdhState {
            query,
            h_util,
            h_vram_used,
            h_vram_total,
            h_temp,
            temp_added,
            buffer: vec![0u8; 4096],
        })
    }
}

#[cfg(windows)]
fn query_pdh_gpu() -> Option<GpuStatus> {
    use windows::Win32::System::Performance::{
        PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_MORE_DATA,
    };

    let cell  = GPU_PDH_STATE.get_or_init(|| Mutex::new(init_gpu_pdh()));
    let mut lock = cell.lock().unwrap();
    let state = lock.as_mut()?;

    unsafe {
        // Instant collect — uses the inter-poll interval as the sampling window
        if PdhCollectQueryData(state.query) != 0 {
            return None;
        }

        // Helper: get max value across all counter instances for a given handle
        macro_rules! get_max {
            ($hc:expr) => {{
                let mut buf_size = state.buffer.len() as u32;
                let mut count    = 0_u32;
                let mut ret = PdhGetFormattedCounterArrayW(
                    $hc, PDH_FMT_DOUBLE, &mut buf_size, &mut count,
                    Some(state.buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
                );
                if ret == PDH_MORE_DATA {
                    state.buffer.resize(buf_size as usize, 0);
                    ret = PdhGetFormattedCounterArrayW(
                        $hc, PDH_FMT_DOUBLE, &mut buf_size, &mut count,
                        Some(state.buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
                    );
                }
                if ret == 0 && count > 0 {
                    let items = state.buffer.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W;
                    (0..count as usize)
                        .map(|i| (*items.add(i)).FmtValue.Anonymous.doubleValue)
                        .fold(0.0_f64, f64::max)
                } else {
                    0.0_f64
                }
            }};
        }

        let util           = get_max!(state.h_util).min(100.0);
        let vram_used_bytes = get_max!(state.h_vram_used);
        let vram_total_bytes= get_max!(state.h_vram_total);
        let temp_c = if state.temp_added {
            get_max!(state.h_temp).clamp(0.0, 150.0) as i32
        } else {
            0
        };

        let mem_used_mb  = (vram_used_bytes  / 1_048_576.0) as u32;
        let mem_total_mb = (vram_total_bytes / 1_048_576.0) as u32;
        let mem_util = if mem_total_mb > 0 { (mem_used_mb * 100 / mem_total_mb) as u8 } else { 0 };

        debug!("PDH GPU: util={:.1}% vram={}/{}MB temp={}°C", util, mem_used_mb, mem_total_mb, temp_c);

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
