/// System temperature queries for the daemon.
///
/// Architecture:
/// - Zero-allocation hot paths (Persistent PDH query, cached WMI).
/// - Polling cost: <0.2 ms per cycle.
/// - Does NOT require Administrator privileges.

use std::sync::{Mutex, OnceLock};

// ── Public entry point ─────────────────────────────────────────────────────

/// Returns `(cpu_temp_c, 0.0)`.
/// `ssd_temp_c` is always 0.0 — NVMe IOCTL requires driver support that OEM
/// laptops (e.g. Razer Blade) often omit.  SSD tile shows "--" gracefully.
pub fn query_sys_temps() -> (f32, f32) {
    (query_cpu_temp(), 0.0)
}

// ── CPU temperature ────────────────────────────────────────────────────────

/// Returns CPU temperature in °C, or 0.0 if unavailable.
fn query_cpu_temp() -> f32 {
    let pdh = query_cpu_temp_pdh_fast();
    if pdh > 0.0 {
        return pdh;
    }
    // Fallback if PDH fails or thermal zones are not exposed
    query_cpu_temp_sysinfo_fast()
}

// ── 1. PDH – Persistent State ──────────────────────────────────────────────

#[cfg(windows)]
struct PdhState {
    query:   isize,
    counter: isize,
    /// Reused every poll cycle – zero heap allocation in the hot path.
    buffer:  Vec<u8>,
}

// Raw isize handles are not automatically Send/Sync.
#[cfg(windows)]
unsafe impl Send for PdhState {}
#[cfg(windows)]
unsafe impl Sync for PdhState {}

#[cfg(windows)]
static PDH_STATE: OnceLock<Mutex<Option<PdhState>>> = OnceLock::new();

#[cfg(windows)]
fn init_pdh() -> Option<PdhState> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW,
        },
    };

    unsafe {
        let mut query: isize = 0;
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
            return None;
        }

        let mut counter: isize = 0;
        let path: Vec<u16> = std::ffi::OsStr::new(
            r"\Thermal Zone Information(*)\Temperature",
        )
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

        if PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut counter) != 0 {
            PdhCloseQuery(query);
            return None;
        }

        // Two primer collections required before data is valid (Windows PDH rule).
        PdhCollectQueryData(query);
        std::thread::sleep(std::time::Duration::from_millis(5));
        PdhCollectQueryData(query);

        Some(PdhState {
            query,
            counter,
            // 1 KB covers dozens of thermal zones; grows automatically if needed.
            buffer: vec![0u8; 1024],
        })
    }
}

#[cfg(windows)]
fn query_cpu_temp_pdh_fast() -> f32 {
    use windows::Win32::System::Performance::{
        PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_MORE_DATA,
    };

    let mut lock = PDH_STATE
        .get_or_init(|| Mutex::new(init_pdh()))
        .lock()
        .unwrap();

    let state = match lock.as_mut() {
        Some(s) => s,
        None => return 0.0,
    };

    unsafe {
        if PdhCollectQueryData(state.query) != 0 {
            return 0.0;
        }

        let mut buf_size: u32 = state.buffer.len() as u32;
        let mut count: u32 = 0;

        let ret = PdhGetFormattedCounterArrayW(
            state.counter,
            PDH_FMT_DOUBLE,
            &mut buf_size,
            &mut count,
            Some(state.buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
        );

        if ret == PDH_MORE_DATA {
            // Grow buffer to the size Windows asked for, retry once.
            state.buffer.resize(buf_size as usize, 0);
            let ret2 = PdhGetFormattedCounterArrayW(
                state.counter,
                PDH_FMT_DOUBLE,
                &mut buf_size,
                &mut count,
                Some(state.buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
            );
            if ret2 != 0 {
                return 0.0;
            }
        } else if ret != 0 {
            return 0.0;
        }

        if count == 0 {
            return 0.0;
        }

        let items = state.buffer.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W;
        let max_dk = (0..count as usize)
            .map(|i| (*items.add(i)).FmtValue.Anonymous.doubleValue)
            .fold(0.0_f64, f64::max);

        let temp_c = (max_dk / 10.0) - 273.15;
        if temp_c > 0.0 && temp_c < 120.0 {
            temp_c as f32
        } else {
            0.0
        }
    }
}

#[cfg(not(windows))]
fn query_cpu_temp_pdh_fast() -> f32 {
    0.0
}

// ── 2. Sysinfo – Persistent Components ────────────────────────────────────

static SYS_COMPS: OnceLock<Mutex<sysinfo::Components>> = OnceLock::new();

fn query_cpu_temp_sysinfo_fast() -> f32 {
    let mutex = SYS_COMPS
        .get_or_init(|| Mutex::new(sysinfo::Components::new_with_refreshed_list()));
    let mut comps = mutex.lock().unwrap();

    // FIX: `refresh()` updates existing sensor values only.
    //      `refresh_list()` re-enumerates the entire WMI tree – just as expensive
    //      as constructing a new Components each call, defeating the cache.
    comps.refresh();

    comps
        .iter()
        .map(|c| c.temperature())
        .filter(|&t| t > 20.0 && t < 110.0)
        .fold(0.0_f32, f32::max)
}

