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

    let metrics = SysMetrics { cpu_pct, ram_used_mb, ram_total_mb };

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

// ── Power-status helpers ──────────────────────────────────────────────────────

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
        p.fan = rpm; // configured manual target (0 = auto)
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
    // Live tachometer is AC-independent; assign to whichever slot is active.
    if let Some(comms::DaemonResponse::GetFanTachometer { rpm }) =
        send(comms::DaemonCommand::GetFanTachometer)
    {
        data.ac.fan_live = rpm;
        data.bat.fan_live = rpm;
    }

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
