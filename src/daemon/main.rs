/// razer-daemon — Windows background process
///
/// Replaces the Linux daemon (D-Bus/UPower/signal-hook) with:
///   • TCP loopback IPC (127.0.0.1:29494)
///   • GetSystemPowerStatus polling for AC state
///   • ctrlc for Ctrl-C / SIGTERM-equivalent shutdown
///   • nvidia-smi / PDH for GPU monitoring
///
/// Run elevated (Run as Administrator) so HID feature reports succeed.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::thread::{self, JoinHandle};
use std::time;

use lazy_static::lazy_static;
use log::*;

#[path = "../comms.rs"]
mod comms;
mod config;
mod device;
#[path = "../gui/gaming_mode.rs"]
mod gaming_mode;
mod gpu;
mod kbd;
mod power;
mod temps;

use kbd::Effect;

lazy_static! {
    static ref EFFECT_MANAGER: Mutex<kbd::EffectManager> =
        Mutex::new(kbd::EffectManager::new());

    static ref DEV_MANAGER: Mutex<device::DeviceManager> = {
        match device::DeviceManager::read_laptops_file() {
            Ok(m) => Mutex::new(m),
            Err(_) => Mutex::new(device::DeviceManager::new()),
        }
    };

    /// True while the system is sleeping (display off).
    static ref SYSTEM_SLEEPING: AtomicBool = AtomicBool::new(false);
}

// ── Entry point ────────────────────────────────────────────────────────────

fn main() {
    init_logging();

    // Discover the Razer HID device
    if let Ok(mut d) = DEV_MANAGER.lock() {
        d.discover_devices();
        match d.get_device() {
            Some(laptop) => {
                info!("Device found: {}", laptop.get_name());
                // One-time EC command probe — discover all readable EC data so we
                // can identify any thermal/sensor commands the firmware exposes.
                laptop.probe_ec_all();
            }
            None => {
                error!(
                    "No supported Razer device found.\n\
                     • Make sure Razer Synapse services are STOPPED (or uninstall Synapse).\n\
                     • Run this daemon as Administrator."
                );
                std::process::exit(1);
            }
        }
    } else {
        error!("Failed to lock device manager");
        std::process::exit(1);
    }

    // Restore saved settings on startup
    if let Ok(mut d) = DEV_MANAGER.lock() {
        let on_ac = power::is_on_ac();
        d.set_ac_state(on_ac);
        let ac = on_ac as usize;

        if let Some(cfg) = d.get_ac_config(ac) {
            if let Some(laptop) = d.get_device() {
                laptop.set_config(cfg);
            }
        }
        d.restore_standard_effect();

        if let Ok(json) = config::Configuration::read_effects_file() {
            EFFECT_MANAGER.lock().unwrap().load_from_save(json);
        } else {
            info!("No saved effects found — starting with green static");
            EFFECT_MANAGER.lock().unwrap().push_effect(
                kbd::effects::Static::new(vec![0, 255, 0]),
                [true; 90],
            );
        }
    }

    // Start background tasks
    start_keyboard_animator_task();
    start_gpu_monitor_task();
    start_power_monitor_task();
    setup_ctrlc_handler();

    // IPC — accept TCP connections from razer-cli
    match comms::create() {
        Some(listener) => {
            info!("Daemon listening on {}", comms::DAEMON_ADDR);
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => handle_client(s),
                    Err(_) => {}
                }
            }
        }
        None => {
            error!("Could not start TCP listener");
            std::process::exit(1);
        }
    }
}

// ── Logging ────────────────────────────────────────────────────────────────

fn init_logging() {
    let mut builder = env_logger::Builder::from_default_env();
    builder.target(env_logger::Target::Stderr);
    builder.filter_level(log::LevelFilter::Info);
    builder.format_timestamp_millis();
    builder.parse_env("RAZER_LOG");
    builder.init();
}

// ── Ctrl-C / shutdown ──────────────────────────────────────────────────────

fn setup_ctrlc_handler() {
    ctrlc::set_handler(move || {
        info!("Shutdown signal received — saving effects and exiting");
        if let Ok(mut k) = EFFECT_MANAGER.lock() {
            let json = k.save();
            if let Err(e) = config::Configuration::write_effects_save(json) {
                error!("Failed to save effects: {}", e);
            }
        }
        std::process::exit(0);
    })
    .expect("Failed to register Ctrl-C handler");
}

// ── Background tasks ───────────────────────────────────────────────────────

fn start_keyboard_animator_task() -> JoinHandle<()> {
    thread::spawn(|| loop {
        if !SYSTEM_SLEEPING.load(Ordering::Relaxed) {
            if let (Ok(mut dev), Ok(mut fx)) = (DEV_MANAGER.lock(), EFFECT_MANAGER.lock()) {
                if let Some(laptop) = dev.get_device() {
                    fx.update(laptop);
                }
            }
        }
        thread::sleep(time::Duration::from_millis(kbd::ANIMATION_SLEEP_MS));
    })
}

fn start_gpu_monitor_task() -> JoinHandle<()> {
    thread::spawn(|| loop {
        let on_ac = power::is_on_ac();
        // Poll more aggressively on AC; give the battery a break
        thread::sleep(time::Duration::from_secs(if on_ac { 3 } else { 10 }));

        if SYSTEM_SLEEPING.load(Ordering::Relaxed) {
            continue;
        }
        if let Some(status) = gpu::query_gpu() {
            gpu::store_gpu_cache(&status);
        }
    })
}

/// Polls AC state every 5 seconds and applies the saved config when it changes.
fn start_power_monitor_task() -> JoinHandle<()> {
    thread::spawn(|| {
        let mut last_ac = power::is_on_ac();

        loop {
            thread::sleep(time::Duration::from_secs(5));

            let current_ac = power::is_on_ac();
            if current_ac != last_ac {
                info!(
                    "AC state changed: {} → {}",
                    if last_ac { "on AC" } else { "on battery" },
                    if current_ac { "on AC" } else { "on battery" }
                );
                last_ac = current_ac;

                if let Ok(mut d) = DEV_MANAGER.lock() {
                    d.set_ac_state(current_ac);
                }
                gpu::clear_gpu_cache();
            }
        }
    })
}

// ── IPC request handler ────────────────────────────────────────────────────

fn handle_client(mut stream: TcpStream) {
    let mut buffer = [0u8; 4096];
    if stream.read(&mut buffer).is_err() {
        return;
    }
    if let Some(cmd) = comms::read_from_socket_req(&buffer) {
        if let Some(resp) = process_request(cmd) {
            if let Ok(encoded) = bincode::serialize(&resp) {
                let _ = stream.write_all(&encoded);
            }
        }
    }
}

fn process_request(cmd: comms::DaemonCommand) -> Option<comms::DaemonResponse> {
    if let Ok(mut d) = DEV_MANAGER.lock() {
        return match cmd {
            comms::DaemonCommand::SetPowerMode { ac, pwr, cpu, gpu } => {
                let ok = d.set_power_mode(ac, pwr, cpu, gpu);
                let confirmed = d.get_power_mode(ac);
                if confirmed == pwr {
                    info!("Power mode set OK (pwr={} cpu={} gpu={} ac={})", pwr, cpu, gpu, ac);
                } else {
                    warn!("Power mode mismatch: sent {} EC reports {}", pwr, confirmed);
                }
                gpu::clear_gpu_cache();
                Some(comms::DaemonResponse::SetPowerMode { result: ok })
            }
            comms::DaemonCommand::SetFanSpeed { ac, rpm } => {
                Some(comms::DaemonResponse::SetFanSpeed { result: d.set_fan_rpm(ac, rpm) })
            }
            comms::DaemonCommand::SetLogoLedState { ac, logo_state } => {
                Some(comms::DaemonResponse::SetLogoLedState {
                    result: d.set_logo_led_state(ac, logo_state),
                })
            }
            comms::DaemonCommand::SetBrightness { ac, val } => {
                Some(comms::DaemonResponse::SetBrightness { result: d.set_brightness(ac, val) })
            }
            comms::DaemonCommand::SetIdle { ac, val } => {
                Some(comms::DaemonResponse::SetIdle { result: d.change_idle(ac, val) })
            }
            comms::DaemonCommand::SetSync { sync } => {
                Some(comms::DaemonResponse::SetSync { result: d.set_sync(sync) })
            }
            comms::DaemonCommand::GetBrightness { ac } => {
                Some(comms::DaemonResponse::GetBrightness { result: d.get_brightness(ac) })
            }
            comms::DaemonCommand::GetLogoLedState { ac } => {
                Some(comms::DaemonResponse::GetLogoLedState {
                    logo_state: d.get_logo_led_state(ac),
                })
            }
            comms::DaemonCommand::GetKeyboardRGB { layer } => {
                let map = EFFECT_MANAGER.lock().unwrap().get_map(layer);
                Some(comms::DaemonResponse::GetKeyboardRGB { layer, rgbdata: map })
            }
            comms::DaemonCommand::GetSync() => {
                Some(comms::DaemonResponse::GetSync { sync: d.get_sync() })
            }
            comms::DaemonCommand::GetFanSpeed { ac } => {
                Some(comms::DaemonResponse::GetFanSpeed { rpm: d.get_fan_rpm(ac) })
            }
            comms::DaemonCommand::GetPwrLevel { ac } => {
                Some(comms::DaemonResponse::GetPwrLevel { pwr: d.get_power_mode(ac) })
            }
            comms::DaemonCommand::GetCPUBoost { ac } => {
                Some(comms::DaemonResponse::GetCPUBoost { cpu: d.get_cpu_boost(ac) })
            }
            comms::DaemonCommand::GetGPUBoost { ac } => {
                Some(comms::DaemonResponse::GetGPUBoost { gpu: d.get_gpu_boost(ac) })
            }
            comms::DaemonCommand::SetEffect { name, params } => {
                let mut res = false;
                if let Ok(mut k) = EFFECT_MANAGER.lock() {
                    let effect: Option<Box<dyn Effect>> = match name.as_str() {
                        "static" => Some(kbd::effects::Static::new(params)),
                        "static_gradient" => Some(kbd::effects::StaticGradient::new(params)),
                        "wave_gradient" => Some(kbd::effects::WaveGradient::new(params)),
                        "breathing_single" => Some(kbd::effects::BreathSingle::new(params)),
                        "breathing_dual" => Some(kbd::effects::BreathDual::new(params)),
                        "spectrum_cycle" => Some(kbd::effects::SpectrumCycle::new(params)),
                        "rainbow_wave" => Some(kbd::effects::RainbowWave::new(params)),
                        "starlight" => Some(kbd::effects::Starlight::new(params)),
                        "ripple" => Some(kbd::effects::Ripple::new(params)),
                        "wheel" => Some(kbd::effects::Wheel::new(params)),
                        _ => None,
                    };
                    if let Some(e) = effect {
                        if let Some(laptop) = d.get_device() {
                            k.pop_effect(laptop);
                            k.push_effect(e, [true; 90]);
                            res = true;
                        }
                    }
                }
                // Persist immediately so the effect survives a crash / force-kill.
                if res {
                    if let Ok(mut k) = EFFECT_MANAGER.lock() {
                        let json = k.save();
                        if let Err(e) = config::Configuration::write_effects_save(json) {
                            error!("Failed to save effects: {}", e);
                        }
                    }
                }
                Some(comms::DaemonResponse::SetEffect { result: res })
            }
            comms::DaemonCommand::SetStandardEffect { name, params } => {
                let mut res = false;
                if let Some(laptop) = d.get_device() {
                    if let Ok(mut k) = EFFECT_MANAGER.lock() {
                        k.pop_effect(laptop);
                        let effect_id = match name.as_str() {
                            "off" => Some(device::RazerLaptop::OFF),
                            "wave" => Some(device::RazerLaptop::WAVE),
                            "reactive" => Some(device::RazerLaptop::REACTIVE),
                            "breathing" => Some(device::RazerLaptop::BREATHING),
                            "spectrum" => Some(device::RazerLaptop::SPECTRUM),
                            "static" => Some(device::RazerLaptop::STATIC),
                            "starlight" => Some(device::RazerLaptop::STARLIGHT),
                            _ => None,
                        };
                        if let Some(id) = effect_id {
                            res = d.set_standard_effect(id, params);
                        }
                    }
                }
                Some(comms::DaemonResponse::SetStandardEffect { result: res })
            }
            comms::DaemonCommand::SetBatteryHealthOptimizer { is_on, threshold } => {
                Some(comms::DaemonResponse::SetBatteryHealthOptimizer {
                    result: d.set_bho_handler(is_on, threshold),
                })
            }
            comms::DaemonCommand::GetSysTemps => {
                // All Windows user-mode temperature APIs on Razer Blade 16 (2023) read
                // ACPI _TMP which the firmware pins to ~45 °C.  Real die temperature
                // requires a ring-0 MSR read — currently not available without a kernel
                // driver.  The ACPI value is the best we can do in user-space.
                let (cpu_temp_c, _) = temps::query_sys_temps();
                Some(comms::DaemonResponse::GetSysTemps { cpu_temp_c })
            }
            comms::DaemonCommand::GetBatteryHealthOptimizer() => {
                let (is_on, threshold) = d
                    .get_bho_handler()
                    .unwrap_or((false, 80));
                Some(comms::DaemonResponse::GetBatteryHealthOptimizer { is_on, threshold })
            }
            comms::DaemonCommand::GetDeviceName => {
                let name = d
                    .get_device()
                    .map(|l| l.get_name().to_string())
                    .unwrap_or_else(|| "Unknown".to_string());
                Some(comms::DaemonResponse::GetDeviceName { name })
            }
            comms::DaemonCommand::GetGpuStatus => {
                match gpu::get_cached_gpu_status() {
                    Some(s) => Some(comms::DaemonResponse::GetGpuStatus {
                        name: s.name,
                        temp_c: s.temp_c,
                        gpu_util: s.gpu_util,
                        mem_util: s.mem_util,
                        stale: false,
                        power_w: s.power_w,
                        power_limit_w: s.power_limit_w,
                        power_max_limit_w: s.power_max_limit_w,
                        mem_used_mb: s.mem_used_mb,
                        mem_total_mb: s.mem_total_mb,
                        clock_gpu_mhz: s.clock_gpu_mhz,
                        clock_mem_mhz: s.clock_mem_mhz,
                    }),
                    None => None,
                }
            }
            comms::DaemonCommand::GetPowerLimits { ac } => {
                // RAPL via sysfs is not available on Windows.
                // Return stored config values (may be 0 if never set).
                let (pl1, pl2) = d.get_rapl_limits(ac);
                Some(comms::DaemonResponse::GetPowerLimits {
                    pl1_watts: pl1,
                    pl2_watts: pl2,
                    pl1_max_watts: 0,
                })
            }
            comms::DaemonCommand::SetPowerLimits { ac, pl1_watts, pl2_watts } => {
                // Persist to config for round-trip compatibility; not applied
                // to the hardware on Windows (no kernel RAPL access).
                let ok = d.set_rapl_limits(ac, pl1_watts, pl2_watts);
                if ok {
                    warn!(
                        "RAPL limits stored but NOT applied (unsupported on Windows): \
                         PL1={}W PL2={}W",
                        pl1_watts, pl2_watts
                    );
                }
                Some(comms::DaemonResponse::SetPowerLimits { result: ok })
            }
            comms::DaemonCommand::GetCurrentEffect => {
                let (name, args) = EFFECT_MANAGER
                    .lock()
                    .ok()
                    .and_then(|mut k| k.get_current_effect_info())
                    .unwrap_or_else(|| (String::new(), vec![]));
                Some(comms::DaemonResponse::GetCurrentEffect { name, args })
            }
            comms::DaemonCommand::SetFnSwap { swap } => {
                let ok = d.set_fn_swap_handler(swap);
                if ok {
                    info!("Fn key swap verified at state {}", swap);
                } else {
                    warn!(
                        "Fn key swap write did not persist; Blade 16 likely uses Synapse's storage plus bladeNative path instead of a standalone hardware toggle"
                    );
                }
                Some(comms::DaemonResponse::SetFnSwap { result: ok })
            }
            comms::DaemonCommand::GetFnSwap() => {
                let swap = d.get_fn_swap_handler().unwrap_or(false);
                Some(comms::DaemonResponse::GetFnSwap { swap })
            }
            comms::DaemonCommand::SetGamingMode {
                win_key,
                alt_tab,
                alt_f4,
            } => {
                gaming_mode::set_blocks(win_key, alt_tab, alt_f4);
                info!(
                    "Gaming mode blocks updated: win_key={} alt_tab={} alt_f4={}",
                    win_key, alt_tab, alt_f4
                );
                Some(comms::DaemonResponse::SetGamingMode { result: true })
            }
        };
    }
    None
}
