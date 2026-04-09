/// Shared data types for the GUI.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;
use service::{SupportedDevice, DEVICE_FILE};

// ── Tab ───────────────────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Tab {
    Ac,
    Battery,
    Keyboard,
    System,
}

// ── Power profile slot ───────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Pwr {
    pub mode: u8,
    pub cpu: u8,
    pub gpu: u8,
    pub fan: i32,
    pub bright: u8,
    pub logo: u8,
    /// Temperature-target mode (°C). 0 = disabled.  When positive the GUI
    /// computes and sends the fan RPM each frame to stay at or below this
    /// temperature. Mutually exclusive with manual-fan mode (fan > 0).
    pub temp_target: i32,
}

impl Default for Pwr {
    fn default() -> Self {
        Self { mode: 0, cpu: 0, gpu: 0, fan: 0, bright: 50, logo: 1, temp_target: 0 }
    }
}

// ── GPU metrics ───────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Clone)]
pub struct GpuInfo {
    pub name: String,
    pub temp: i32,
    pub util: u8,
    pub mem_util: u8,
    pub power_w: f32,
    pub power_limit_w: f32,
    pub power_max_limit_w: f32,
    pub mem_used_mb: u32,
    pub mem_total_mb: u32,
    pub clk_gpu_mhz: u32,
    pub clk_mem_mhz: u32,
    pub stale: bool,
}

// ── Chart sample ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub temp_c: f64,
    pub gpu_pct: f64,
    pub vram_pct: f64,
    pub power_w: f64,
}

// ── Notification banner ───────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub enum BannerTone {
    Success,
    Warn,
    Error,
}

pub struct Banner {
    pub tone: BannerTone,
    pub text: String,
    pub until: Instant,
}

#[derive(Clone, Copy)]
struct PendingPowerProfile {
    mode: u8,
    cpu: u8,
    gpu: u8,
    until: Instant,
}

#[derive(Clone, Copy)]
pub struct FanController {
    prev_temp: f32,
    integral: f32,
    smoothed_temp: f32,
    smoothed_util: f32,
    thermal_energy: f32,
    last_update: Instant,
}

impl Default for FanController {
    fn default() -> Self {
        Self {
            prev_temp: 0.0,
            integral: 0.0,
            smoothed_temp: 0.0,
            smoothed_util: 0.0,
            thermal_energy: 0.0,
            last_update: Instant::now(),
        }
    }
}

fn supported_devices() -> &'static Vec<SupportedDevice> {
    static DEVICES: OnceLock<Vec<SupportedDevice>> = OnceLock::new();
    DEVICES.get_or_init(|| {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEVICE_FILE);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str::<Vec<SupportedDevice>>(&json).ok())
            .unwrap_or_default()
    })
}

fn lookup_fan_range(devname: &str) -> Option<(i32, i32)> {
    let device = supported_devices().iter().find(|device| {
        device.name.eq_ignore_ascii_case(devname)
            || devname.contains(&device.name)
            || device.name.contains(devname)
    })?;
    if device.fan.len() < 2 {
        return None;
    }
    Some((device.fan[0] as i32, device.fan[1] as i32))
}

fn quantize_rpm(rpm: f32, min_rpm: f32, max_rpm: f32) -> f32 {
    const FAN_STEPS: [f32; 9] = [
        1500.0,
        1800.0,
        2100.0,
        2500.0,
        3000.0,
        3500.0,
        4000.0,
        4500.0,
        5000.0,
    ];

    let min_rpm = min_rpm.min(max_rpm);
    let max_rpm = max_rpm.max(min_rpm);
    let mut steps: Vec<f32> = FAN_STEPS
        .iter()
        .copied()
        .filter(|step| *step >= min_rpm && *step <= max_rpm)
        .collect();

    if steps.is_empty() {
        steps.push(min_rpm);
        if (max_rpm - min_rpm).abs() > f32::EPSILON {
            steps.push(max_rpm);
        }
    } else {
        if !steps.iter().any(|step| (*step - min_rpm).abs() < 1.0) {
            steps.push(min_rpm);
        }
        if !steps.iter().any(|step| (*step - max_rpm).abs() < 1.0) {
            steps.push(max_rpm);
        }
        steps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    }

    steps
        .iter()
        .min_by(|a, b| {
            (rpm.clamp(min_rpm, max_rpm) - **a)
                .abs()
                .partial_cmp(&(rpm.clamp(min_rpm, max_rpm) - **b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .unwrap_or(rpm.clamp(min_rpm, max_rpm))
}

// ── System metrics (per-poll) ─────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct SysMetrics {
    pub cpu_pct:     f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
}

// ── System static info (collected once) ───────────────────────────────────────

#[derive(Clone, Default)]
pub struct SysStatic {
    pub cpu_name:     String,
    pub host_name:    String,
    pub os_name:      String,
    pub laptop_model: String,
    pub bios_version: String,
    pub bios_date:    String,
    pub uptime_secs:  u64,
}

// ── Poll result (background thread → UI thread) ───────────────────────────────

/// All data returned by one complete background poll cycle.
pub struct PollData {
    pub ok: bool,
    pub devname: String,
    pub ac: Pwr,
    pub bat: Pwr,
    pub sync: bool,
    pub effect_name: String,
    pub effect_args: Vec<u8>,
    pub bho: bool,
    pub bho_thr: u8,
    pub fn_swap: bool,
    pub on_ac: bool,
    pub battery_pct: u8,
    pub gpu: Option<GpuInfo>,
    pub sys: SysMetrics,
    pub sys_static: SysStatic,
}

// ── App ───────────────────────────────────────────────────────────────────────

use crate::constants::*;
use crate::poll::start_poll_thread;
use std::sync::mpsc;
use std::time::Duration;

pub struct App {
    pub tab: Tab,
    // Daemon state — updated by background poll.
    pub ok: bool,
    pub devname: String,
    pub ac: Pwr,
    pub bat: Pwr,
    pub sync: bool,
    pub bho: bool,
    pub bho_thr: u8,
    pub gpu: Option<GpuInfo>,
    pub history: VecDeque<Sample>,
    pub sys: SysMetrics,
    pub sys_static: SysStatic,
    pub fan_min_rpm: i32,
    pub fan_max_rpm: i32,
    // Effect state — protected by effect_dirty.
    pub eidx: usize,
    pub c1: [f32; 3],
    pub c2: [f32; 3],
    pub spd: u8,
    pub dir: u8,
    pub den: u8,
    pub dur: u8,
    /// True = user has unsaved local changes; background poll will not overwrite.
    pub effect_dirty: bool,
    /// True = chart is floating in a separate always-on-top viewport.
    pub chart_detached: bool,
    pub ac_controller: FanController,
    pub bat_controller: FanController,
    pending_ac_profile: Option<PendingPowerProfile>,
    pending_bat_profile: Option<PendingPowerProfile>,
    // UI-only state.
    pub banner: Option<Banner>,
    // Gaming mode (GUI-only, keyboard hooks).
    pub gaming_win_key: bool,
    pub gaming_alt_tab: bool,
    pub gaming_alt_f4: bool,
    // Fn key swap (daemon EC HID).
    pub fn_swap: bool,
    // Display refresh-rate (GUI-only).
    pub bat_low_refresh: bool,
    pub display_rates: Vec<u32>,
    pub display_rate_high: u32,
    pub display_rate_low: u32,
    last_ac_state: Option<bool>,
    // Low-battery lighting auto-off (GUI-only).
    pub low_bat_lighting: bool,
    pub low_bat_pct: u8,
    low_bat_dimmed: bool,
    // Background poll channel.
    pub poll_rx: mpsc::Receiver<PollData>,
    pub wake_tx: mpsc::SyncSender<()>,
    // System tray.
    #[allow(dead_code)]
    tray_icon: Option<tray_icon::TrayIcon>,
    pub tray_rx: Option<std::sync::mpsc::Receiver<crate::tray::TrayAction>>,
}

impl App {
    pub fn new(ctx: &eframe::egui::Context) -> Self {
        crate::widgets::setup(ctx);
        let (poll_rx, wake_tx) = start_poll_thread(ctx.clone());
        let rates = crate::display::available_refresh_rates();
        let rate_high = rates.last().copied().unwrap_or(60);
        let rate_low  = rates.first().copied().unwrap_or(60);

        // Load persisted GUI settings.
        let gui_cfg = crate::gui_config::GuiConfig::load();

        // Restore gaming mode keyboard hooks through the daemon so the hook
        // runs in the elevated long-lived process.
        if gui_cfg.gaming_win_key || gui_cfg.gaming_alt_tab || gui_cfg.gaming_alt_f4 {
            let _ = crate::poll::send(crate::comms::DaemonCommand::SetGamingMode {
                win_key: gui_cfg.gaming_win_key,
                alt_tab: gui_cfg.gaming_alt_tab,
                alt_f4: gui_cfg.gaming_alt_f4,
            });
        }

        // Create system tray icon.
        let (tray_icon, tray_rx) = match std::panic::catch_unwind(crate::tray::create_tray) {
            Ok((icon, rx)) => (Some(icon), Some(rx)),
            Err(_) => (None, None),
        };

        Self {
            tab: Tab::Ac,
            ok: false,
            devname: "Unknown".into(),
            ac: Pwr::default(),
            bat: Pwr::default(),
            sync: false,
            bho: false,
            bho_thr: 80,
            gpu: None,
            history: VecDeque::with_capacity(HISTORY_LEN + 1),
            sys: SysMetrics::default(),
            sys_static: SysStatic::default(),
            fan_min_rpm: 1500,
            fan_max_rpm: 5000,
            eidx: 0,
            c1: [0.27, 1.0, 0.63],
            c2: [0.33, 0.73, 1.0],
            spd: 5,
            dir: 0,
            den: 10,
            dur: 10,
            effect_dirty: false,
            chart_detached: false,
            ac_controller: FanController::default(),
            bat_controller: FanController::default(),
            pending_ac_profile: None,
            pending_bat_profile: None,
            banner: None,
            gaming_win_key: gui_cfg.gaming_win_key,
            gaming_alt_tab: gui_cfg.gaming_alt_tab,
            gaming_alt_f4: gui_cfg.gaming_alt_f4,
            fn_swap: false,
            bat_low_refresh: gui_cfg.bat_low_refresh,
            display_rates: rates,
            display_rate_high: rate_high,
            display_rate_low: rate_low,
            last_ac_state: None,
            low_bat_lighting: gui_cfg.low_bat_lighting,
            low_bat_pct: gui_cfg.low_bat_pct,
            low_bat_dimmed: false,
            poll_rx,
            wake_tx,
            tray_icon,
            tray_rx,
        }
    }

    /// Ask the background thread to run the next poll immediately.
    pub fn wake_poll(&self) {
        let _ = self.wake_tx.try_send(());
    }

    /// Persist GUI-only settings to disk.
    pub fn save_gui_config(&self) {
        let cfg = crate::gui_config::GuiConfig {
            gaming_win_key: self.gaming_win_key,
            gaming_alt_tab: self.gaming_alt_tab,
            gaming_alt_f4: self.gaming_alt_f4,
            bat_low_refresh: self.bat_low_refresh,
            low_bat_lighting: self.low_bat_lighting,
            low_bat_pct: self.low_bat_pct,
        };
        let _ = cfg.save();
    }

    pub fn set_banner(&mut self, tone: BannerTone, text: impl Into<String>) {
        self.banner = Some(Banner {
            tone,
            text: text.into(),
            until: Instant::now() + Duration::from_secs(5),
        });
    }

    pub fn apply_temp_control(&mut self, is_ac: bool) -> Option<i32> {
        let target = if is_ac { self.ac.temp_target } else { self.bat.temp_target };
        if target <= 0 {
            let controller = if is_ac {
                &mut self.ac_controller
            } else {
                &mut self.bat_controller
            };

            controller.integral = 0.0;
            controller.thermal_energy = 0.0;
            controller.prev_temp = 0.0;
            controller.smoothed_temp = 0.0;
            controller.smoothed_util = 0.0;
            controller.last_update = Instant::now();

            return None;
        }

        let gpu = match &self.gpu {
            Some(g) => g,
            None => return None,
        };

        let controller = if is_ac {
            &mut self.ac_controller
        } else {
            &mut self.bat_controller
        };

        let slot = if is_ac { &mut self.ac } else { &mut self.bat };

        let now = Instant::now();
        let mut dt = now.duration_since(controller.last_update).as_secs_f32();

        if dt < 0.25 {
            return None;
        }

        controller.last_update = now;
        dt = dt.clamp(0.25, 1.0);

        let raw_temp = (gpu.temp as f32).clamp(0.0, 120.0);
        let raw_util = (gpu.util as f32).clamp(0.0, 100.0);
        let cur_fan = slot.fan.max(0) as f32;
        let min_rpm = self.fan_min_rpm.max(0) as f32;
        let max_rpm = self.fan_max_rpm.max(self.fan_min_rpm.max(0)) as f32;

        const TEMP_ALPHA: f32 = 0.35;
        const UTIL_ALPHA: f32 = 0.18;

        const KP: f32 = 170.0;
        const KI: f32 = 10.0;
        const KD: f32 = 40.0;

        const STEP_UP: f32 = 1000.0;
        const STEP_DOWN: f32 = 250.0;

        const ENERGY_INPUT_GAIN: f32 = 0.02;
        const ENERGY_DECAY: f32 = 0.92;

        const TEMP_PREDICT_GAIN: f32 = 1.6;
        const ENERGY_PREDICT_GAIN: f32 = 15.0;

        if controller.smoothed_temp == 0.0 {
            controller.smoothed_temp = raw_temp;
            controller.smoothed_util = raw_util;
            controller.prev_temp = raw_temp;
        }

        controller.smoothed_temp =
            TEMP_ALPHA * raw_temp + (1.0 - TEMP_ALPHA) * controller.smoothed_temp;

        controller.smoothed_util =
            UTIL_ALPHA * raw_util + (1.0 - UTIL_ALPHA) * controller.smoothed_util;

        let temp = controller.smoothed_temp;
        let util = controller.smoothed_util;

        controller.thermal_energy += util * ENERGY_INPUT_GAIN * dt;
        controller.thermal_energy *= ENERGY_DECAY;

        let velocity = (temp - controller.prev_temp) / dt;

        let predicted_temp = (
            temp
                + velocity * TEMP_PREDICT_GAIN
                + controller.thermal_energy * ENERGY_PREDICT_GAIN
        )
            .clamp(temp - 5.0, temp + 10.0);

        controller.prev_temp = temp;

        let error = predicted_temp - target as f32;

        if raw_util < 5.0 && temp < (target as f32 - 8.0) {
            let idle_rpm = quantize_rpm(min_rpm, min_rpm, max_rpm) as i32;

            if idle_rpm != slot.fan {
                slot.fan = idle_rpm;
                return Some(idle_rpm);
            }

            return None;
        }

        if error.abs() < 0.8 && util < 35.0 {
            return None;
        }

        if error.abs() < 8.0 {
            controller.integral += error * dt;
            controller.integral = controller.integral.clamp(-160.0, 160.0);
        } else {
            controller.integral *= 0.9;
        }

        let pid = KP * error + KI * controller.integral + KD * velocity;

        let mut desired = min_rpm + pid;
        desired = desired.clamp(min_rpm, max_rpm);

        let step_up = STEP_UP * dt;
        let step_down = STEP_DOWN * dt;

        let next = if desired > cur_fan {
            (cur_fan + step_up).min(desired)
        } else {
            (cur_fan - step_down).max(desired)
        };

        let final_rpm = quantize_rpm(next, min_rpm, max_rpm);
        let next_rpm = final_rpm as i32;

        if next_rpm != slot.fan {
            slot.fan = next_rpm;
            return Some(next_rpm);
        }

        None
    }

    pub fn trim_banner(&mut self) {
        if self.banner.as_ref().is_some_and(|b| Instant::now() > b.until) {
            self.banner = None;
        }
    }

    pub fn push_sample(&mut self, sample: Sample) {
        self.history.push_back(sample);
        while self.history.len() > HISTORY_LEN {
            self.history.pop_front();
        }
    }

    pub fn remember_power_profile_write(&mut self, is_ac: bool) {
        let profile = if is_ac { self.ac } else { self.bat };
        let pending = PendingPowerProfile {
            mode: profile.mode,
            cpu: profile.cpu,
            gpu: profile.gpu,
            until: Instant::now() + Duration::from_secs(4),
        };
        if is_ac {
            self.pending_ac_profile = Some(pending);
        } else {
            self.pending_bat_profile = Some(pending);
        }
    }

    fn reconcile_power_profile(
        current: Pwr,
        mut incoming: Pwr,
        pending: &mut Option<PendingPowerProfile>,
    ) -> Pwr {
        // `temp_target` is a GUI-only mode today, so polling must never clear it.
        incoming.temp_target = current.temp_target;

        let Some(expected) = *pending else {
            return incoming;
        };

        if Instant::now() > expected.until {
            *pending = None;
            return incoming;
        }

        if incoming.mode == expected.mode
            && incoming.cpu == expected.cpu
            && incoming.gpu == expected.gpu
        {
            *pending = None;
            return incoming;
        }

        incoming.mode = current.mode;
        incoming.cpu = current.cpu;
        incoming.gpu = current.gpu;
        incoming
    }

    /// Merge a PollData result into App state.
    pub fn apply_poll(&mut self, data: PollData) {
        self.ok = data.ok;

        // ── GUI-only features (work even without daemon) ────────────────
        // Auto-switch refresh rate on AC ↔ battery transitions.
        if self.bat_low_refresh {
            let prev_ac = self.last_ac_state;
            self.last_ac_state = Some(data.on_ac);
            if let Some(was_ac) = prev_ac {
                if was_ac != data.on_ac {
                    let target_hz = if data.on_ac {
                        self.display_rate_high
                    } else {
                        self.display_rate_low
                    };
                    crate::display::set_refresh_rate(target_hz);
                }
            }
        } else {
            self.last_ac_state = Some(data.on_ac);
        }

        // Low-battery lighting auto-off: dim KB + logo when below threshold.
        if self.low_bat_lighting && !data.on_ac && data.battery_pct <= self.low_bat_pct && data.battery_pct != 255 {
            if !self.low_bat_dimmed {
                self.low_bat_dimmed = true;
                let ac_slot = 0usize; // battery slot
                crate::poll::send(crate::comms::DaemonCommand::SetBrightness { ac: ac_slot, val: 0 });
                crate::poll::send(crate::comms::DaemonCommand::SetLogoLedState { ac: ac_slot, logo_state: 0 });
            }
        } else if self.low_bat_dimmed && (data.on_ac || data.battery_pct > self.low_bat_pct) {
            self.low_bat_dimmed = false;
        }

        // Update system metrics regardless of daemon state.
        self.sys = data.sys;
        self.sys_static = data.sys_static;

        if !data.ok {
            self.gpu = None;
            return;
        }
        self.devname = data.devname;
        if let Some((fan_min_rpm, fan_max_rpm)) = lookup_fan_range(&self.devname) {
            self.fan_min_rpm = fan_min_rpm;
            self.fan_max_rpm = fan_max_rpm;
        }
        self.ac = Self::reconcile_power_profile(self.ac, data.ac, &mut self.pending_ac_profile);
        self.bat = Self::reconcile_power_profile(self.bat, data.bat, &mut self.pending_bat_profile);
        self.sync = data.sync;
        self.bho = data.bho;
        self.bho_thr = data.bho_thr;
        self.fn_swap = data.fn_swap;

        // Only sync effect from daemon when user has no pending local edits.
        if !self.effect_dirty && !data.effect_name.is_empty() {
            self.load_effect(&data.effect_name, &data.effect_args);
        }

        // Update GPU info and append a chart sample.
        self.gpu = data.gpu.clone();

        // Keep the chart timeline continuous even if one poll misses GPU data.
        let (temp_c, gpu_pct, vram_pct, power_w) = if let Some(ref gpu) = data.gpu {
            let v_pct = if gpu.mem_total_mb > 0 {
                gpu.mem_used_mb as f64 * 100.0 / gpu.mem_total_mb as f64
            } else {
                gpu.mem_util as f64
            };
            (gpu.temp as f64, gpu.util as f64, v_pct, gpu.power_w as f64)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };

        self.push_sample(Sample {
            temp_c,
            gpu_pct,
            vram_pct,
            power_w,
        });
    }

    pub fn load_effect(&mut self, name: &str, args: &[u8]) {
        self.eidx = effect_index_from_name(name).min(EFFECT_COUNT - 1);
        let f = |i: usize| args.get(i).copied().unwrap_or(0) as f32 / 255.0;
        let u = |i: usize| args.get(i).copied().unwrap_or(0);
        match self.eidx {
            0 => self.c1 = [f(0), f(1), f(2)],
            1 | 2 => {
                self.c1 = [f(0), f(1), f(2)];
                self.c2 = [f(3), f(4), f(5)];
            }
            3 => {
                self.c1 = [f(0), f(1), f(2)];
                self.dur = u(3).max(1);
            }
            4 => {
                self.c1 = [f(0), f(1), f(2)];
                self.c2 = [f(3), f(4), f(5)];
                self.dur = u(6).max(1);
            }
            5 => self.spd = u(0).max(1),
            6 => {
                self.spd = u(0).max(1);
                self.dir = u(1);
            }
            7 => {
                self.c1 = [f(0), f(1), f(2)];
                self.den = u(3).max(1);
            }
            8 => {
                self.c1 = [f(0), f(1), f(2)];
                self.spd = u(3).max(1);
            }
            9 => {
                self.spd = u(0).max(1);
                self.dir = u(1);
            }
            _ => {}
        }
    }

    pub fn effect_args(&self) -> Vec<u8> {
        let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
        let c1 = [to_u8(self.c1[0]), to_u8(self.c1[1]), to_u8(self.c1[2])];
        let c2 = [to_u8(self.c2[0]), to_u8(self.c2[1]), to_u8(self.c2[2])];
        match self.eidx {
            0 => vec![c1[0], c1[1], c1[2]],
            1 | 2 => vec![c1[0], c1[1], c1[2], c2[0], c2[1], c2[2]],
            3 => vec![c1[0], c1[1], c1[2], self.dur],
            4 => vec![c1[0], c1[1], c1[2], c2[0], c2[1], c2[2], self.dur],
            5 => vec![self.spd],
            6 => vec![self.spd, self.dir],
            7 => vec![c1[0], c1[1], c1[2], self.den],
            8 => vec![c1[0], c1[1], c1[2], self.spd],
            9 => vec![self.spd, self.dir],
            _ => vec![],
        }
    }

    pub fn apply_effect(&mut self) {
        use crate::poll::send;
        use crate::comms;
        let name = effect_key(self.eidx).to_string();
        let params = self.effect_args();
        match send(comms::DaemonCommand::SetEffect { name, params }) {
            Some(comms::DaemonResponse::SetEffect { result: true }) => {
                self.effect_dirty = false;
                self.set_banner(BannerTone::Success, "Lighting effect applied.");
                self.wake_poll();
            }
            Some(comms::DaemonResponse::SetEffect { result: false }) => {
                self.set_banner(
                    BannerTone::Error,
                    "Daemon rejected the effect command. Check device support.",
                );
            }
            Some(_) => {
                self.set_banner(BannerTone::Error, "Unexpected response from daemon.");
            }
            None => {
                self.set_banner(BannerTone::Error, "Cannot reach razer-daemon.");
            }
        }
    }
}
