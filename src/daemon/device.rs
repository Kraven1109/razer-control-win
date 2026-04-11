/// Razer HID device layer — Windows port.
///
/// The Razer EC protocol (RazerPacket, feature-report commands) is identical
/// to the Linux build.  The only platform differences are:
///   • hidapi uses the `windows-native` backend (SetupAPI + HidD_*)
///   • AC-state is read via GetSystemPowerStatus instead of D-Bus UPower
///   • The laptops.json device list is embedded at compile time

use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use std::{io, thread, time::Duration};
use hidapi::HidApi;
use lazy_static::lazy_static;
use log::*;

use crate::config;
use service::SupportedDevice;

const RAZER_VENDOR_ID: u16 = 0x1532;
const REPORT_SIZE: usize = 91;

// Embed the device database at compile time — no install step needed.
static LAPTOPS_JSON: &str = include_str!("../../data/devices/laptops.json");

// Tweak 1: Cache HidApi so it doesn't incur the heavy SetupAPI enumeration cost on re-discovery
lazy_static! {
    static ref HID_API: core::result::Result<HidApi, hidapi::HidError> = HidApi::new();
}

// ── Wire protocol ──────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
pub struct RazerPacket {
    report: u8,
    status: u8,
    id: u8,
    remaining_packets: u16,
    protocol_type: u8,
    data_size: u8,
    pub command_class: u8,
    pub command_id: u8,
    #[serde(with = "BigArray")]
    pub args: [u8; 80],
    crc: u8,
    reserved: u8,
}

impl RazerPacket {
    const RAZER_CMD_NEW: u8 = 0x00;
    const RAZER_CMD_SUCCESSFUL: u8 = 0x02;
    const RAZER_CMD_NOT_SUPPORTED: u8 = 0x05;

    fn new(command_class: u8, command_id: u8, data_size: u8) -> RazerPacket {
        RazerPacket {
            report: 0x00,
            status: RazerPacket::RAZER_CMD_NEW,
            id: 0xFF, // transaction_id for Blade laptops
            remaining_packets: 0x0000,
            protocol_type: 0x00,
            data_size,
            command_class,
            command_id,
            args: [0x00; 80],
            crc: 0x00,
            reserved: 0x00,
        }
    }

    /// Calculates the checksum and returns the strictly serialized payload.
    fn calc_crc_and_serialize(&mut self) -> Vec<u8> {
        let mut res: u8 = 0x00;
        let buf: Vec<u8> = bincode::serialize(self).unwrap();
        for i in 2..88 {
            res ^= buf[i];
        }
        self.crc = res;
        
        // Critical Bug Fix: Re-serialize to embed the newly calculated CRC byte 
        // into the final payload before dispatching.
        bincode::serialize(self).unwrap()
    }
}

// ── Device manager ─────────────────────────────────────────────────────────

pub struct DeviceManager {
    pub device: Option<RazerLaptop>,
    supported_devices: Vec<SupportedDevice>,
    pub config: Option<config::Configuration>,
    fan_overrides: [i32; 2],
}

impl DeviceManager {
    pub fn new() -> DeviceManager {
        DeviceManager {
            device: None,
            supported_devices: vec![],
            config: None,
            fan_overrides: [0, 0],
        }
    }

    pub fn read_laptops_file() -> io::Result<DeviceManager> {
        let mut res = DeviceManager::new();
        res.supported_devices = serde_json::from_str(LAPTOPS_JSON)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        info!("Supported devices loaded: {}", res.supported_devices.len());
        match config::Configuration::read_from_config() {
            Ok(mut c) => {
                if c.reset_fan_profiles_to_auto() {
                    let _ = c.write_to_file();
                }
                res.config = Some(c);
            }
            Err(_) => res.config = Some(config::Configuration::new()),
        }
        Ok(res)
    }

    pub fn get_device(&mut self) -> Option<&mut RazerLaptop> {
        self.device.as_mut()
    }

    fn get_config(&mut self) -> Option<&mut config::Configuration> {
        self.config.as_mut()
    }

    pub fn get_ac_config(&mut self, ac: usize) -> Option<config::PowerConfig> {
        self.get_config().map(|c| c.power[ac])
    }

    pub fn find_supported_device(&mut self, vid: u16, pid: u16) -> Option<&SupportedDevice> {
        self.supported_devices.iter().find(|device| {
            let svid = u16::from_str_radix(&device.vid, 16).unwrap_or(0);
            let spid = u16::from_str_radix(&device.pid, 16).unwrap_or(0);
            svid == vid && spid == pid
        })
    }

    pub fn discover_devices(&mut self) {
        match &*HID_API {
            Ok(api) => {
                // Filter by Razer vendor ID; collect all interfaces regardless of
                // usage page because different Razer models place the proprietary
                // EC control interface on different vendor usage pages.
                // Sorting descending ensures interface 2 (Razer control) is tried
                // before interface 1 and 0 (keyboard/consumer).
                let mut devices: Vec<_> = api
                    .device_list()
                    .filter(|d| d.vendor_id() == RAZER_VENDOR_ID)
                    .collect();

                if log::log_enabled!(log::Level::Debug) {
                    for d in &devices {
                        debug!(
                            "Razer HID candidate: PID=0x{:04X} iface={} \
                             usage_page=0x{:04X} usage=0x{:04X} path={}",
                            d.product_id(),
                            d.interface_number(),
                            d.usage_page(),
                            d.usage(),
                            d.path().to_string_lossy()
                        );
                    }
                }

                // Sort descending so interface 2/1 comes before 0.
                devices.sort_by_key(|d| -(d.interface_number() as i32));

                for device in devices {
                    let result = self
                        .find_supported_device(device.vendor_id(), device.product_id())
                        .cloned(); // Clone to drop the immutable borrow on self
                    
                    if let Some(supported) = result {
                        match api.open_path(device.path()) {
                            Ok(dev) => {
                                info!(
                                    "Opened HID device: {} (interface {})",
                                    supported.name,
                                    device.interface_number()
                                );
                                self.device = Some(RazerLaptop::new(
                                    supported.name,
                                    supported.features,
                                    supported.fan,
                                    dev,
                                ));
                                return;
                            }
                            Err(e) => {
                                debug!(
                                    "Could not open interface {}: {}",
                                    device.interface_number(), e
                                );
                            }
                        }
                    }
                }
                error!(
                    "No supported Razer HID interface could be opened.\n\
                     • Make sure Razer Synapse services are STOPPED.\n\
                     • Run this daemon as Administrator."
                );
            }
            Err(e) => error!("HidApi init error: {}", e),
        }
    }

    // ── Config helpers (mirrors Linux build) ──────────────────────────────

    pub fn set_sync(&mut self, sync: bool) -> bool {
        let mut ac: usize = 0;
        if let Some(laptop) = self.get_device() {
            ac = laptop.ac_state as usize;
        }
        let other = (ac + 1) & 0x01;
        if let Some(config) = self.get_config() {
            config.sync = sync;
            config.power[other].brightness = config.power[ac].brightness;
            config.power[other].logo_state = config.power[ac].logo_state;
            config.power[other].idle = config.power[ac].idle;
            let _ = config.write_to_file();
        }
        true
    }

    pub fn get_sync(&mut self) -> bool {
        self.get_config().map(|c| c.sync).unwrap_or(false)
    }

    pub fn change_idle(&mut self, ac: usize, timeout: u32) -> bool {
        if let Some(config) = self.get_config() {
            if config.power[ac].idle != timeout {
                config.power[ac].idle = timeout;
                if config.sync {
                    config.power[(ac + 1) & 1].idle = timeout;
                }
                let _ = config.write_to_file();
            }
        }
        true
    }

    pub fn set_power_mode(&mut self, ac: usize, pwr: u8, cpu: u8, gpu: u8) -> bool {
        if pwr > 4 || cpu > 3 || gpu > 2 {
            return false;
        }
        if let Some(config) = self.get_config() {
            config.power[ac].power_mode = pwr;
            config.power[ac].cpu_boost = cpu;
            config.power[ac].gpu_boost = gpu;
            let _ = config.write_to_file();
        }
        if let Some(laptop) = self.get_device() {
            if laptop.ac_state as usize == ac {
                return laptop.set_power_mode(pwr, cpu, gpu);
            }
        }
        true
    }

    pub fn set_standard_effect(&mut self, effect_id: u8, params: Vec<u8>) -> bool {
        if let Some(config) = self.get_config() {
            config.standard_effect = effect_id;
            config.standard_effect_params = params.clone();
            let _ = config.write_to_file();
        }
        if let Some(laptop) = self.get_device() {
            laptop.set_standard_effect(effect_id, params);
        }
        true
    }

    pub fn restore_standard_effect(&mut self) {
        let (effect, params) = self
            .get_config()
            .map(|c| (c.standard_effect, c.standard_effect_params.clone()))
            .unwrap_or((0, vec![]));
        if let Some(laptop) = self.get_device() {
            laptop.set_standard_effect(effect, params);
        }
    }

    pub fn set_fan_rpm(&mut self, ac: usize, rpm: i32) -> bool {
        let rpm = rpm.max(0);
        self.fan_overrides[ac.min(1)] = rpm;
        if let Some(laptop) = self.get_device() {
            if laptop.ac_state as usize == ac {
                return laptop.set_fan_rpm(rpm as u16);
            }
        }
        true
    }

    pub fn set_logo_led_state(&mut self, ac: usize, logo_state: u8) -> bool {
        if let Some(config) = self.get_config() {
            config.power[ac].logo_state = logo_state;
            if config.sync {
                config.power[(ac + 1) & 1].logo_state = logo_state;
            }
            let _ = config.write_to_file();
        }
        if let Some(laptop) = self.get_device() {
            if laptop.ac_state as usize == ac {
                return laptop.set_logo_led_state(logo_state);
            }
        }
        true
    }

    pub fn get_logo_led_state(&mut self, ac: usize) -> u8 {
        self.get_ac_config(ac).map(|c| c.logo_state).unwrap_or(0)
    }

    pub fn set_brightness(&mut self, ac: usize, brightness: u8) -> bool {
        let val = brightness as u16 * 255 / 100;
        if let Some(config) = self.get_config() {
            config.power[ac].brightness = val as u8;
            if config.sync {
                config.power[(ac + 1) & 1].brightness = val as u8;
            }
            let _ = config.write_to_file();
        }
        if let Some(laptop) = self.get_device() {
            if laptop.ac_state as usize == ac {
                return laptop.set_brightness(val as u8);
            }
        }
        true
    }

    pub fn get_brightness(&mut self, ac: usize) -> u8 {
        if let Some(laptop) = self.get_device() {
            if laptop.ac_state as usize == ac {
                let val = laptop.get_brightness() as u32;
                // Convert 0-255 → 0-100, rounding correctly.
                return ((val * 100 + 127) / 255) as u8;
            }
        }
        self.get_ac_config(ac)
            .map(|c| ((c.brightness as u32 * 100 + 127) / 255) as u8)
            .unwrap_or(0)
    }

    /// Returns the user-configured manual target (0 = auto mode).
    /// Used by GetFanSpeed IPC — the power tab needs this for mode detection.
    pub fn get_fan_rpm(&mut self, ac: usize) -> i32 {
        self.fan_overrides[ac.min(1)]
    }

    /// Returns the live measured fan RPM from the EC tachometer.
    /// Falls back to configured target if EC read fails.
    pub fn get_fan_tachometer(&mut self) -> i32 {
        if let Some(laptop) = self.get_device() {
            return laptop.get_fan_tachometer() as i32;
        }
        0
    }

    pub fn get_power_mode(&mut self, ac: usize) -> u8 {
        self.get_ac_config(ac).map(|c| c.power_mode).unwrap_or(0)
    }

    pub fn get_cpu_boost(&mut self, ac: usize) -> u8 {
        self.get_ac_config(ac).map(|c| c.cpu_boost).unwrap_or(0)
    }

    pub fn get_gpu_boost(&mut self, ac: usize) -> u8 {
        self.get_ac_config(ac).map(|c| c.gpu_boost).unwrap_or(0)
    }

    pub fn set_ac_state(&mut self, online: bool) {
        let ac = online as usize;
        let override_rpm = self.fan_overrides[ac.min(1)];
        if let Some(laptop) = self.get_device() {
            laptop.set_ac_state(online);
        }
        if let Some(config) = self.get_ac_config(ac) {
            if let Some(laptop) = self.get_device() {
                laptop.set_config(config);
                if override_rpm > 0 {
                    laptop.set_fan_rpm(override_rpm as u16);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn set_ac_state_get(&mut self) {
        let online = crate::power::is_on_ac();
        self.set_ac_state(online);
    }

    #[allow(dead_code)]
    pub fn light_off(&mut self) {
        if let Some(laptop) = self.get_device() {
            laptop.set_brightness(0);
            laptop.set_logo_led_state(0);
        }
    }

    #[allow(dead_code)]
    pub fn restore_light(&mut self) {
        let mut brightness = 0;
        let mut logo_state = 0;
        let mut ac: usize = 0;
        if let Some(laptop) = self.get_device() {
            ac = laptop.get_ac_state();
        }
        if let Some(config) = self.get_ac_config(ac) {
            brightness = config.brightness;
            logo_state = config.logo_state;
        }
        if let Some(laptop) = self.get_device() {
            laptop.set_brightness(brightness);
            laptop.set_logo_led_state(logo_state);
        }
    }

    pub fn set_bho_handler(&mut self, is_on: bool, threshold: u8) -> bool {
        self.get_device()
            .map_or(false, |l| l.set_bho(is_on, threshold))
    }

    pub fn get_bho_handler(&mut self) -> Option<(bool, u8)> {
        self.get_device()
            .and_then(|l| l.get_bho().map(byte_to_bho))
    }

    pub fn get_fn_swap_handler(&mut self) -> Option<bool> {
        self.get_device().and_then(|l| l.get_fn_swap())
    }

    pub fn set_fn_swap_handler(&mut self, swap: bool) -> bool {
        let Some(laptop) = self.get_device() else {
            return false;
        };

        if !laptop.set_fn_swap(swap) {
            return false;
        }

        for _ in 0..3 {
            thread::sleep(Duration::from_millis(40));
            if let Some(current) = laptop.get_fn_swap() {
                if current == swap {
                    return true;
                }
            }
        }
        false
    }

    pub fn get_rapl_limits(&mut self, ac: usize) -> (u32, u32) {
        self.get_ac_config(ac)
            .map(|c| (c.rapl_pl1_watts, c.rapl_pl2_watts))
            .unwrap_or((0, 0))
    }

    pub fn set_rapl_limits(&mut self, ac: usize, pl1_watts: u32, pl2_watts: u32) -> bool {
        if let Some(config) = self.get_config() {
            config.power[ac].rapl_pl1_watts = pl1_watts;
            config.power[ac].rapl_pl2_watts = pl2_watts;
            return config.write_to_file().is_ok();
        }
        false
    }
}

// ── RazerLaptop — HID commands ─────────────────────────────────────────────

pub struct RazerLaptop {
    name: String,
    features: Vec<String>,
    fan: Vec<u16>,
    device: hidapi::HidDevice,
    power: u8,
    fan_rpm: u8,
    pub ac_state: u8,
    screensaver: bool,
    /// EC command used to read live fan RPM (detected once on first poll).
    /// 0x88 = tachometer (actual RPM), 0x81 = set-point, None = unsupported.
    fan_read_cmd: Option<u8>,
}

impl RazerLaptop {
    // LED storage
    const NOSTORE: u8 = 0x00;
    const VARSTORE: u8 = 0x01;
    // LED IDs
    const LOGO_LED: u8 = 0x04;
    // Hardware effect IDs
    pub const OFF: u8 = 0x00;
    pub const WAVE: u8 = 0x01;
    pub const REACTIVE: u8 = 0x02;
    pub const BREATHING: u8 = 0x03;
    pub const SPECTRUM: u8 = 0x04;
    pub const CUSTOMFRAME: u8 = 0x05;
    pub const STATIC: u8 = 0x06;
    pub const STARLIGHT: u8 = 0x19;

    pub fn new(
        name: String,
        features: Vec<String>,
        fan: Vec<u16>,
        device: hidapi::HidDevice,
    ) -> RazerLaptop {
        RazerLaptop {
            name,
            features,
            fan,
            device,
            power: 0,
            fan_rpm: 0,
            ac_state: 0,
            screensaver: false,
            fan_read_cmd: None,
        }
    }

    // Tweak 6.1: Return &str instead of cloning String
    pub fn get_name(&self) -> &str {
        &self.name
    }

    // Tweak 1.1: Accepts &str, zero allocation
    pub fn have_feature(&self, fch: &str) -> bool {
        self.features.iter().any(|f| f == fch)
    }

    #[allow(dead_code)]
    pub fn set_screensaver(&mut self, active: bool) {
        self.screensaver = active;
    }

    pub fn set_ac_state(&mut self, online: bool) -> usize {
        self.ac_state = online as u8;
        self.ac_state as usize
    }

    #[allow(dead_code)]
    pub fn get_ac_state(&mut self) -> usize {
        self.ac_state as usize
    }

    pub fn set_config(&mut self, config: config::PowerConfig) -> bool {
        let mut ret = false;
        if !self.screensaver {
            ret |= self.set_brightness(config.brightness);
            ret |= self.set_logo_led_state(config.logo_state);
        } else {
            ret |= self.set_brightness(0);
            ret |= self.set_logo_led_state(0);
        }
        ret |= self.set_power_mode(config.power_mode, config.cpu_boost, config.gpu_boost);
        ret |= self.set_fan_rpm(config.fan_rpm as u16);
        ret
    }

    // Tweak 2.1: Bounds guard to prevent slice panic if JSON lacks limits
    fn clamp_fan(&self, rpm: u16) -> u8 {
        if self.fan.len() < 2 {
            return (rpm / 100) as u8;
        }

        let min = self.fan[0];
        let max = self.fan[1];

        if rpm > max {
            return (max / 100) as u8;
        }
        if rpm < min {
            return (min / 100) as u8;
        }
        (rpm / 100) as u8
    }

    pub fn set_standard_effect(&mut self, effect_id: u8, params: Vec<u8>) -> bool {
        let mut report = RazerPacket::new(0x03, 0x0a, 80);
        report.args[0] = effect_id;
        // Tweak 7.1: take(79) to prevent out-of-bounds array access
        for (idx, &p) in params.iter().take(79).enumerate() {
            report.args[idx + 1] = p;
        }
        self.send_report(report).is_some()
    }

    pub fn set_custom_frame_data(&mut self, row: u8, data: Vec<u8>) {
        if data.len() == 45 {
            let mut report = RazerPacket::new(0x03, 0x0b, 0x34);
            report.args[0] = 0xff;
            report.args[1] = row;
            report.args[2] = 0x00;
            report.args[3] = 0x0f;
            for (idx, &b) in data.iter().enumerate() {
                report.args[idx + 7] = b;
            }
            self.send_report(report);
        }
    }

    pub fn set_custom_frame(&mut self) -> bool {
        let mut report = RazerPacket::new(0x03, 0x0a, 0x02);
        report.args[0] = RazerLaptop::CUSTOMFRAME;
        report.args[1] = RazerLaptop::NOSTORE;
        self.send_report(report).is_some()
    }

    #[allow(dead_code)]
    pub fn get_power_mode(&mut self, zone: u8) -> u8 {
        let mut report = RazerPacket::new(0x0d, 0x82, 0x04);
        report.args[0] = 0x00;
        report.args[1] = zone;
        self.send_report(report).map(|r| r.args[2]).unwrap_or(0)
    }

    fn set_power(&mut self, zone: u8) -> bool {
        let mut report = RazerPacket::new(0x0d, 0x02, 0x04);
        report.args[0] = 0x00;
        report.args[1] = zone;
        report.args[2] = self.power;
        report.args[3] = if self.fan_rpm != 0 { 0x01 } else { 0x00 };
        self.send_report(report).is_some()
    }

    #[allow(dead_code)]
    pub fn get_cpu_boost(&mut self) -> u8 {
        let mut report = RazerPacket::new(0x0d, 0x87, 0x03);
        report.args[0] = 0x00;
        report.args[1] = 0x01;
        self.send_report(report).map(|r| r.args[2]).unwrap_or(0)
    }

    fn set_cpu_boost(&mut self, mut boost: u8) -> bool {
        if boost == 3 && !self.have_feature("boost") {
            boost = 2;
        }
        let mut report = RazerPacket::new(0x0d, 0x07, 0x03);
        report.args[0] = 0x00;
        report.args[1] = 0x01;
        report.args[2] = boost;
        self.send_report(report).is_some()
    }

    #[allow(dead_code)]
    pub fn get_gpu_boost(&mut self) -> u8 {
        let mut report = RazerPacket::new(0x0d, 0x87, 0x03);
        report.args[0] = 0x00;
        report.args[1] = 0x02;
        self.send_report(report).map(|r| r.args[2]).unwrap_or(0)
    }

    fn set_gpu_boost(&mut self, boost: u8) -> bool {
        let mut report = RazerPacket::new(0x0d, 0x07, 0x03);
        report.args[0] = 0x00;
        report.args[1] = 0x02;
        report.args[2] = boost;
        self.send_report(report).is_some()
    }

    pub fn set_power_mode(&mut self, mode: u8, cpu_boost: u8, gpu_boost: u8) -> bool {
        let mode = mode.min(4);
        let cpu_boost = if self.have_feature("boost") {
            cpu_boost.min(3)
        } else {
            cpu_boost.min(2)
        };
        let gpu_boost = gpu_boost.min(2);
        self.power = mode;
        if mode <= 3 {
            self.set_power(0x01);
            self.set_power(0x02);
        } else {
            self.fan_rpm = 0;
            self.set_power(0x01);
            self.set_cpu_boost(cpu_boost);
            self.set_power(0x02);
            self.set_gpu_boost(gpu_boost);
        }
        true
    }

    fn set_rpm(&mut self, zone: u8) -> bool {
        let mut report = RazerPacket::new(0x0d, 0x01, 0x03);
        report.args[0] = 0x00;
        report.args[1] = zone;
        report.args[2] = self.fan_rpm;
        self.send_report(report).is_some()
    }

    pub fn set_fan_rpm(&mut self, value: u16) -> bool {
        self.fan_rpm = if value == 0 { 0 } else { self.clamp_fan(value) };
        if self.power == 4 {
            return true; // Custom mode: firmware manages fan
        }
        self.set_power(0x01);
        if value != 0 {
            self.set_rpm(0x01);
        }
        self.set_power(0x02);
        if value != 0 {
            self.set_rpm(0x02);
        }
        true
    }

    /// Read the *actual measured* fan RPM from the EC (tachometer).
    ///
    /// On the first call the method probes which EC command the firmware supports
    /// (0x0D/0x88 tachometer → 0x0D/0x81 set-point → none) and caches the result
    /// so every subsequent poll is a single HID round-trip with no log spam.
    ///
    /// The result: `info!` logged once at detection time; `warn!` if neither command
    /// works (so you can diagnose it at default log level without `RAZER_LOG=debug`).
    pub fn get_fan_tachometer(&mut self) -> u16 {
        // Lazy probe: try 0x88 (tachometer) then 0x81 (set-point) once.
        if self.fan_read_cmd.is_none() {
            // Sentinel: Some(0) means "nothing works, use cached"
            self.fan_read_cmd = Some(0);
            for &cmd_id in &[0x88u8, 0x81u8] {
                let mut probe = RazerPacket::new(0x0d, cmd_id, 0x02);
                probe.args[0] = 0x00;
                probe.args[1] = 0x01;
                if self.send_report(probe).map(|r| r.args[2] > 0).unwrap_or(false) {
                    let label = if cmd_id == 0x88 { "tachometer" } else { "set-point" };
                    info!("fan_rpm: EC 0x0D/0x{:02X} ({}) selected for {}", cmd_id, label, self.name);
                    self.fan_read_cmd = Some(cmd_id);
                    break;
                }
            }
            if self.fan_read_cmd == Some(0) {
                warn!("fan_rpm: no EC command supported on {} — showing configured target", self.name);
            }
        }

        match self.fan_read_cmd {
            Some(cmd_id) if cmd_id > 0 => {
                let mut report = RazerPacket::new(0x0d, cmd_id, 0x02);
                report.args[0] = 0x00;
                report.args[1] = 0x01;
                self.send_report(report)
                    .map(|r| r.args[2] as u16 * 100)
                    .filter(|&rpm| rpm > 0)
                    .unwrap_or(self.fan_rpm as u16 * 100)
            }
            _ => self.fan_rpm as u16 * 100,
        }
    }

    pub fn set_logo_led_state(&mut self, mode: u8) -> bool {
        if mode > 0 {
            let mut report = RazerPacket::new(0x03, 0x02, 0x03);
            report.args[0] = RazerLaptop::VARSTORE;
            report.args[1] = RazerLaptop::LOGO_LED;
            report.args[2] = if mode == 1 { 0x00 } else { 0x02 };
            self.send_report(report);
        }
        let mut report = RazerPacket::new(0x03, 0x00, 0x03);
        report.args[0] = RazerLaptop::VARSTORE;
        report.args[1] = RazerLaptop::LOGO_LED;
        // Built-in clamp instead of custom mutable method
        report.args[2] = mode.clamp(0x00, 0x01);
        self.send_report(report).is_some()
    }

    pub fn set_brightness(&mut self, brightness: u8) -> bool {
        let mut report = RazerPacket::new(0x0E, 0x04, 0x02);
        report.args[0] = 0x01;
        report.args[1] = brightness;
        self.send_report(report).is_some()
    }

    pub fn get_brightness(&mut self) -> u8 {
        let mut report = RazerPacket::new(0x0E, 0x84, 0x02);
        report.args[0] = 0x01;
        self.send_report(report).map(|r| r.args[1]).unwrap_or(0)
    }

    pub fn get_bho(&mut self) -> Option<u8> {
        if !self.have_feature("bho") {
            return None;
        }
        let mut report = RazerPacket::new(0x07, 0x92, 0x01);
        report.args[0] = 0x00;
        self.send_report(report).map(|r| r.args[0])
    }

    pub fn set_bho(&mut self, is_on: bool, threshold: u8) -> bool {
        if !self.have_feature("bho") {
            warn!("BHO not supported on this device");
            return false;
        }
        let threshold = threshold.clamp(50, 80);
        let mut report = RazerPacket::new(0x07, 0x12, 0x01);
        report.args[0] = bho_to_byte(is_on, threshold);
        self.send_report(report).map_or(false, |r| {
            debug!("BHO response: {:?}", r);
            true
        })
    }

    pub fn get_fn_swap(&mut self) -> Option<bool> {
        let mut report = RazerPacket::new(0x02, 0x86, 0x02);
        report.id = 0xFF;
        report.args[0] = 0x00;
        self.send_report(report).map(|r| r.args[1] != 0)
    }

    pub fn set_fn_swap(&mut self, swap: bool) -> bool {
        let mut report = RazerPacket::new(0x02, 0x06, 0x02);
        report.id = 0xFF;
        report.args[0] = 0x00;
        report.args[1] = swap as u8;
        self.send_report(report).is_some()
    }

    /// Read the EC's feature-report response.
    /// The 1 ms sleep is the minimum guard that lets the EC finish processing
    /// the preceding SET_REPORT before we issue a GET_REPORT.  Without it,
    /// `HidD_GetFeature` returns the stale buffer from the previous command.
    fn read_response(&self, buf: &mut [u8; REPORT_SIZE]) -> Option<usize> {
        thread::sleep(Duration::from_millis(1));
        match self.device.get_feature_report(buf) {
            Ok(size) if size == REPORT_SIZE => Some(size),
            _ => None,
        }
    }

    fn send_report(&mut self, mut report: RazerPacket) -> Option<RazerPacket> {
        let mut temp_buf = [0x00; REPORT_SIZE];

        // Serialize ONCE per command to drop heap-allocation overhead during retries
        let packet_payload = report.calc_crc_and_serialize();

        for attempt in 0..3 {
            match self.device.send_feature_report(&packet_payload) {
                Ok(_) => {
                    if self.read_response(&mut temp_buf).is_some() {
                        match bincode::deserialize::<RazerPacket>(&temp_buf) {
                            Ok(response) => {
                                if response.command_id == 0x92 {
                                    return Some(response);
                                }
                                if response.remaining_packets != report.remaining_packets
                                    || response.command_class != report.command_class
                                    || response.command_id != report.command_id
                                {
                                    warn!(
                                        "HID response mismatch: expected class=0x{:02X} cmd=0x{:02X}",
                                        report.command_class, report.command_id
                                    );
                                } else if response.status == RazerPacket::RAZER_CMD_SUCCESSFUL {
                                    return Some(response);
                                } else if response.status == RazerPacket::RAZER_CMD_NOT_SUPPORTED {
                                    debug!(
                                        "HID command not supported: class=0x{:02X} cmd=0x{:02X}",
                                        report.command_class, report.command_id
                                    );
                                    return None;
                                }
                            }
                            // Better contextual logging
                            Err(e) => warn!(
                                "HID deserialize (attempt {} for class 0x{:02X} cmd 0x{:02X}): {}", 
                                attempt + 1, report.command_class, report.command_id, e
                            ),
                        }
                    } else {
                        warn!("HID read timeout (attempt {})", attempt + 1);
                    }
                }
                Err(e) => error!("HID write (attempt {}): {}", attempt + 1, e),
            }
            
            // Tweak: Exponential backoff (1ms, 2ms, 4ms) instead of large fixed steps
            thread::sleep(Duration::from_millis(1 << attempt));
        }
        
        error!(
            "HID command failed after 3 attempts: class=0x{:02X} cmd=0x{:02X}",
            report.command_class, report.command_id
        );
        None
    }
}


// ── BHO encoding helpers ───────────────────────────────────────────────────

fn byte_to_bho(u: u8) -> (bool, u8) {
    (u & (1 << 7) != 0, u & 0b0111_1111)
}

fn bho_to_byte(is_on: bool, threshold: u8) -> u8 {
    if is_on { threshold | 0b1000_0000 } else { threshold }
}
