/// Background poll thread and IPC send helper.

use crate::app::{PollData, SysMetrics, SysStatic};
use crate::app::{GpuInfo, Pwr};
use crate::comms;
use std::sync::mpsc;
use std::time::Duration;

// Persistent sysinfo System — kept alive across polls so CPU usage delta works.
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
lazy_static::lazy_static! {
    static ref SYS: std::sync::Mutex<System> = {
        let s = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::new().with_cpu_usage())
                .with_memory(MemoryRefreshKind::everything()),
        );
        std::sync::Mutex::new(s)
    };
}

// ── Windows registry helper ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn reg_read_sz(subkey: &str, value: &str) -> String {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE,
        RRF_RT_REG_SZ,
    };
    let key = HSTRING::from(subkey);
    let val = HSTRING::from(value);
    let mut buf = [0u16; 256];
    let mut len = (buf.len() * 2) as u32;
    unsafe {
        let ret = RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &key,
            &val,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        );
        if ret.is_ok() {
            let chars = (len / 2).saturating_sub(1) as usize;
            String::from_utf16_lossy(&buf[..chars.min(buf.len())])
        } else {
            String::new()
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn reg_read_sz(_subkey: &str, _value: &str) -> String { String::new() }

fn collect_sys() -> (SysMetrics, SysStatic) {
    let mut sys = SYS.lock().unwrap_or_else(|e| e.into_inner());
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu_pct      = sys.global_cpu_usage();
    let ram_used_mb  = sys.used_memory() / 1024 / 1024;
    let ram_total_mb = sys.total_memory() / 1024 / 1024;

    let cpu_temp_c = query_cpu_temp_pdh();
    let ssd_temp_c = query_ssd_temp();

    let metrics = SysMetrics { cpu_pct, ram_used_mb, ram_total_mb, cpu_temp_c, ssd_temp_c };

    const BIOS_KEY: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
    let laptop_model = reg_read_sz(BIOS_KEY, "SystemProductName");
    let bios_version = reg_read_sz(BIOS_KEY, "BIOSVersion");
    let bios_date    = reg_read_sz(BIOS_KEY, "BIOSReleaseDate");

    let cpu_name  = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
    let host_name = System::host_name().unwrap_or_default();
    let os_name   = System::long_os_version().unwrap_or_default();
    let uptime_secs = System::uptime();

    let statics = SysStatic {
        cpu_name,
        host_name,
        os_name,
        laptop_model,
        bios_version,
        bios_date,
        uptime_secs,
    };
    (metrics, statics)
}

/// Fire a single IPC command at the daemon and return its response.
pub fn send(cmd: comms::DaemonCommand) -> Option<comms::DaemonResponse> {
    comms::try_bind().ok().and_then(|sock| comms::send_to_daemon(cmd, sock))
}

// ── Hardware temperature queries ─────────────────────────────────────────────

/// CPU package temperature via PDH `\Thermal Zone Information(*)\Temperature`.
///
/// PDH reports decikelvin (1/10 K).  We take the maximum across all thermal
/// zone instances (the CPU package is the hottest component) and convert to °C.
/// Returns 0.0 when the counter is not present on this system.
#[cfg(windows)]
fn query_cpu_temp_pdh() -> f32 {
    use windows::{
        core::PCWSTR,
        Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData,
            PdhGetFormattedCounterArrayW, PdhOpenQueryW,
            PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
        },
    };
    #[allow(non_camel_case_types)] type PDH_HQUERY   = isize;
    #[allow(non_camel_case_types)] type PDH_HCOUNTER = isize;
    fn wide(s: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }
    unsafe {
        let mut query: PDH_HQUERY = 0;
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 { return 0.0; }

        let mut h_temp: PDH_HCOUNTER = 0;
        let path = wide("\\Thermal Zone Information(*)\\Temperature");
        if PdhAddEnglishCounterW(query, PCWSTR(path.as_ptr()), 0, &mut h_temp) != 0 {
            PdhCloseQuery(query);
            return 0.0;
        }
        // Temperature is a snapshot counter — one collect is sufficient.
        PdhCollectQueryData(query);

        let mut buf_size: u32 = 0;
        let mut count:    u32 = 0;
        PdhGetFormattedCounterArrayW(h_temp, PDH_FMT_DOUBLE, &mut buf_size, &mut count, None);
        if buf_size == 0 || count == 0 {
            PdhCloseQuery(query);
            return 0.0;
        }

        let item_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let needed    = (buf_size as usize).max(item_size * count as usize);
        let mut buf   = vec![0u8; needed];
        let items     = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let ret       = PdhGetFormattedCounterArrayW(h_temp, PDH_FMT_DOUBLE, &mut buf_size, &mut count, Some(items));
        PdhCloseQuery(query);
        if ret != 0 { return 0.0; }

        // The hottest zone is the CPU package; iGPU and fans are cooler.
        let max_dk = (0..count as usize)
            .map(|i| (*items.add(i)).FmtValue.Anonymous.doubleValue)
            .fold(0.0_f64, f64::max);

        // Decikelvin → Celsius  (e.g. 3295 → 329.5 K → 56.35 °C)
        let temp_c = (max_dk / 10.0) - 273.15;
        if temp_c > 0.0 && temp_c < 120.0 { temp_c as f32 } else { 0.0 }
    }
}
#[cfg(not(windows))]
fn query_cpu_temp_pdh() -> f32 { 0.0 }

/// Primary NVMe / SSD temperature via the Windows Storage Device Temperature IOCTL.
///
/// Probes PhysicalDrive0..3.  The `StorageDeviceTemperatureProperty` IOCTL is
/// supported on all NVMe drives under Windows 10 v1607+ and returns temperature
/// already in Celsius (no unit conversion needed).
/// Returns 0.0 when the drive does not support this property or access fails.
#[cfg(windows)]
fn query_ssd_temp() -> f32 {
    use std::mem;
    use windows::{
        core::HSTRING,
        Win32::Foundation::CloseHandle,
        Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        },
        Win32::System::IO::DeviceIoControl,
    };

    // IOCTL_STORAGE_QUERY_PROPERTY = CTL_CODE(IOCTL_STORAGE_BASE=0x2D, 0x500, METHOD_BUFFERED, FILE_ANY_ACCESS)
    const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
    const PROP_TEMPERATURE: u32 = 8; // StorageDeviceTemperatureProperty
    const QUERY_STANDARD:   u32 = 0; // PropertyStandardQuery
    const GENERIC_READ:     u32 = 0x8000_0000;

    // Manual struct definitions (windows-rs 0.58 may not export these)
    #[repr(C)] struct PropQuery  { prop_id: u32, q_type: u32, extra: [u8; 1] }
    #[repr(C)] struct TempDesc   { _ver: u32, _sz: u32, _crit: i16, _warn: i16, count: u16, _r0: [u8;2], _r1: [i32;2] }
    #[repr(C)] struct TempInfo   { _idx: u16, temp_c: i16, _oh: i16, _cool: i16, _r: u32 }

    let q = PropQuery { prop_id: PROP_TEMPERATURE, q_type: QUERY_STANDARD, extra: [0] };

    for i in 0..4u32 {
        let path = HSTRING::from(format!(r"\\.\PhysicalDrive{}", i));
        let Ok(handle) = (unsafe {
            CreateFileW(
                &path,
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }) else { continue; };
        let mut buf   = vec![0u8; 4096];
        let mut bytes: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                Some(&q as *const _ as *const _),
                mem::size_of::<PropQuery>() as u32,
                Some(buf.as_mut_ptr() as *mut _),
                buf.len() as u32,
                Some(&mut bytes),
                None,
            )
        };
        unsafe { let _ = CloseHandle(handle); }

        if ok.is_ok() && (bytes as usize) >= mem::size_of::<TempDesc>() {
            let desc    = unsafe { &*(buf.as_ptr() as *const TempDesc) };
            let offset  = mem::size_of::<TempDesc>();
            if desc.count > 0 && (bytes as usize) >= offset + mem::size_of::<TempInfo>() {
                let info = unsafe { &*(buf.as_ptr().add(offset) as *const TempInfo) };
                let t = info.temp_c;
                if t > 0 && t < 80 { return t as f32; }
            }
        }
    }
    0.0
}
#[cfg(not(windows))]
fn query_ssd_temp() -> f32 { 0.0 }
#[cfg(windows)]
fn is_on_ac() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    unsafe {
        let mut status = SYSTEM_POWER_STATUS::default();
        let _ = GetSystemPowerStatus(&mut status);
        status.ACLineStatus == 1
    }
}
#[cfg(not(windows))]
fn is_on_ac() -> bool { true }

/// Battery charge percentage (0–100). Returns 255 if unknown.
#[cfg(windows)]
fn battery_percent() -> u8 {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    unsafe {
        let mut status = SYSTEM_POWER_STATUS::default();
        let _ = GetSystemPowerStatus(&mut status);
        status.BatteryLifePercent // 0-100, or 255 if unknown
    }
}
#[cfg(not(windows))]
fn battery_percent() -> u8 { 255 }

/// Read one power slot's settings (ac=1 for AC power, ac=0 for battery).
pub fn poll_pwr_slot(ac: usize) -> Pwr {
    let mut p = Pwr::default();
    if let Some(comms::DaemonResponse::GetPwrLevel { pwr }) =
        send(comms::DaemonCommand::GetPwrLevel { ac })
    {
        p.mode = pwr;
    }
    if let Some(comms::DaemonResponse::GetCPUBoost { cpu }) =
        send(comms::DaemonCommand::GetCPUBoost { ac })
    {
        p.cpu = cpu;
    }
    if let Some(comms::DaemonResponse::GetGPUBoost { gpu }) =
        send(comms::DaemonCommand::GetGPUBoost { ac })
    {
        p.gpu = gpu;
    }
    if let Some(comms::DaemonResponse::GetFanSpeed { rpm }) =
        send(comms::DaemonCommand::GetFanSpeed { ac })
    {
        p.fan = rpm;
    }
    if let Some(comms::DaemonResponse::GetBrightness { result }) =
        send(comms::DaemonCommand::GetBrightness { ac })
    {
        p.bright = result;
    }
    if let Some(comms::DaemonResponse::GetLogoLedState { logo_state }) =
        send(comms::DaemonCommand::GetLogoLedState { ac })
    {
        p.logo = logo_state;
    }
    p
}

/// Full poll cycle — runs entirely in the background thread, no UI access.
pub fn do_poll() -> PollData {
    let mut data = PollData {
        ok: false,
        devname: String::new(),
        ac: Pwr::default(),
        bat: Pwr::default(),
        sync: false,
        effect_name: String::new(),
        effect_args: Vec::new(),
        bho: false,
        bho_thr: 80,
        fn_swap: false,
        on_ac: is_on_ac(),
        battery_pct: battery_percent(),
        gpu: None,
        sys: SysMetrics::default(),
        sys_static: SysStatic::default(),
    };

    // Collect CPU/RAM/system-info regardless of daemon availability.
    let (sys, sys_static) = collect_sys();
    data.sys = sys;
    data.sys_static = sys_static;

    // Quick alive-check first; bail early if the daemon is unreachable.
    let Some(comms::DaemonResponse::GetDeviceName { name }) =
        send(comms::DaemonCommand::GetDeviceName)
    else {
        return data;
    };
    data.ok = true;
    data.devname = name;

    data.ac = poll_pwr_slot(1);
    data.bat = poll_pwr_slot(0);

    if let Some(comms::DaemonResponse::GetSync { sync }) =
        send(comms::DaemonCommand::GetSync())
    {
        data.sync = sync;
    }
    if let Some(comms::DaemonResponse::GetBatteryHealthOptimizer { is_on, threshold }) =
        send(comms::DaemonCommand::GetBatteryHealthOptimizer())
    {
        data.bho = is_on;
        data.bho_thr = threshold;
    }
    if let Some(comms::DaemonResponse::GetFnSwap { swap }) =
        send(comms::DaemonCommand::GetFnSwap())
    {
        data.fn_swap = swap;
    }
    if let Some(comms::DaemonResponse::GetCurrentEffect { name, args }) =
        send(comms::DaemonCommand::GetCurrentEffect)
    {
        data.effect_name = name;
        data.effect_args = args;
    }
    if let Some(comms::DaemonResponse::GetGpuStatus {
        name,
        temp_c,
        gpu_util,
        mem_util,
        stale,
        power_w,
        power_limit_w,
        power_max_limit_w,
        mem_used_mb,
        mem_total_mb,
        clock_gpu_mhz,
        clock_mem_mhz,
    }) = send(comms::DaemonCommand::GetGpuStatus)
    {
        data.gpu = Some(GpuInfo {
            name,
            temp: temp_c,
            util: gpu_util,
            mem_util,
            power_w,
            power_limit_w,
            power_max_limit_w,
            mem_used_mb,
            mem_total_mb,
            clk_gpu_mhz: clock_gpu_mhz,
            clk_mem_mhz: clock_mem_mhz,
            stale,
        });
    }

    data
}

/// Spawn the background poll thread.
/// Returns (receiver for poll results, sender to wake the thread early).
pub fn start_poll_thread(
    ctx: eframe::egui::Context,
) -> (mpsc::Receiver<PollData>, mpsc::SyncSender<()>) {
    let (data_tx, data_rx) = mpsc::channel::<PollData>();
    // capacity=1: at most one queued "poll now" wake signal
    let (wake_tx, wake_rx) = mpsc::sync_channel::<()>(1);
    std::thread::spawn(move || loop {
        let result = do_poll();
        if data_tx.send(result).is_err() {
            break; // UI thread dropped the receiver (app is closing)
        }
        ctx.request_repaint();
        // Wait up to 3 s, or until woken by a write operation.
        let _ = wake_rx.recv_timeout(Duration::from_secs(3));
    });
    (data_rx, wake_tx)
}
