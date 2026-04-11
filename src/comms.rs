/// IPC between daemon and CLI/GUI — TCP loopback (replaces Unix socket).
///
/// Windows does not have Unix domain sockets in all configurations, so we
/// use a simple TCP listener on localhost.  The serialisation format (bincode)
/// and command/response enums are identical to the Linux build so any
/// tooling that communicates with the daemon is portable.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

pub const DAEMON_ADDR: &str = "127.0.0.1:29494";

// ── Commands sent TO the daemon ────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
pub enum DaemonCommand {
    SetFanSpeed { ac: usize, rpm: i32 },
    GetFanSpeed { ac: usize },
    SetPowerMode { ac: usize, pwr: u8, cpu: u8, gpu: u8 },
    GetPwrLevel { ac: usize },
    GetCPUBoost { ac: usize },
    GetGPUBoost { ac: usize },
    SetLogoLedState { ac: usize, logo_state: u8 },
    GetLogoLedState { ac: usize },
    GetKeyboardRGB { layer: i32 },
    SetEffect { name: String, params: Vec<u8> },
    SetStandardEffect { name: String, params: Vec<u8> },
    SetBrightness { ac: usize, val: u8 },
    SetIdle { ac: usize, val: u32 },
    GetBrightness { ac: usize },
    SetSync { sync: bool },
    GetSync(),
    SetBatteryHealthOptimizer { is_on: bool, threshold: u8 },
    GetBatteryHealthOptimizer(),
    GetDeviceName,
    GetGpuStatus,
    GetPowerLimits { ac: usize },
    SetPowerLimits { ac: usize, pl1_watts: u32, pl2_watts: u32 },
    GetCurrentEffect,
    SetFnSwap { swap: bool },
    GetFnSwap(),
    SetGamingMode { win_key: bool, alt_tab: bool, alt_f4: bool },
    /// Live fan RPM from the EC tachometer (model-agnostic).
    GetFanTachometer,
}

// ── Responses sent FROM the daemon ────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
pub enum DaemonResponse {
    SetFanSpeed { result: bool },
    GetFanSpeed { rpm: i32 },
    SetPowerMode { result: bool },
    GetPwrLevel { pwr: u8 },
    GetCPUBoost { cpu: u8 },
    GetGPUBoost { gpu: u8 },
    SetLogoLedState { result: bool },
    GetLogoLedState { logo_state: u8 },
    GetKeyboardRGB { layer: i32, rgbdata: Vec<u8> },
    SetEffect { result: bool },
    SetStandardEffect { result: bool },
    SetBrightness { result: bool },
    SetIdle { result: bool },
    GetBrightness { result: u8 },
    SetSync { result: bool },
    GetSync { sync: bool },
    SetBatteryHealthOptimizer { result: bool },
    GetBatteryHealthOptimizer { is_on: bool, threshold: u8 },
    GetDeviceName { name: String },
    GetGpuStatus {
        name: String,
        temp_c: i32,
        gpu_util: u8,
        mem_util: u8,
        stale: bool,
        power_w: f32,
        power_limit_w: f32,
        power_max_limit_w: f32,
        mem_used_mb: u32,
        mem_total_mb: u32,
        clock_gpu_mhz: u32,
        clock_mem_mhz: u32,
    },
    GetPowerLimits {
        pl1_watts: u32,
        pl2_watts: u32,
        pl1_max_watts: u32,
    },
    SetPowerLimits { result: bool },
    GetCurrentEffect { name: String, args: Vec<u8> },
    SetFnSwap { result: bool },
    GetFnSwap { swap: bool },
    SetGamingMode { result: bool },
    /// Live fan RPM from the EC tachometer.
    GetFanTachometer { rpm: i32 },
}

// ── Client helpers ─────────────────────────────────────────────────────────

/// Connect to the daemon.  Returns `None` if the daemon is not running.
#[allow(dead_code)]
pub fn bind() -> Option<TcpStream> {
    TcpStream::connect(DAEMON_ADDR).ok()
}

#[allow(dead_code)]
pub fn try_bind() -> std::io::Result<TcpStream> {
    TcpStream::connect(DAEMON_ADDR)
}

/// Returns `true` when the daemon is reachable (used by CLI).
#[allow(dead_code)]
pub fn is_daemon_running() -> bool {
    TcpStream::connect(DAEMON_ADDR).is_ok()
}

// ── Server helper ──────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn create() -> Option<TcpListener> {
    match TcpListener::bind(DAEMON_ADDR) {
        Ok(l) => Some(l),
        Err(e) => {
            eprintln!("Cannot bind TCP listener on {}: {}", DAEMON_ADDR, e);
            eprintln!("Is another daemon instance already running?");
            None
        }
    }
}

// ── Send/receive helpers ───────────────────────────────────────────────────

#[allow(dead_code)]
pub fn send_to_daemon(command: DaemonCommand, mut sock: TcpStream) -> Option<DaemonResponse> {
    let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(1500)));
    let _ = sock.set_write_timeout(Some(std::time::Duration::from_millis(1000)));

    let encoded = bincode::serialize(&command).ok()?;
    sock.write_all(&encoded).ok()?;

    let mut buf = [0u8; 4096];
    match sock.read(&mut buf) {
        Ok(n) if n > 0 => read_from_socket_resp(&buf[..n]),
        Ok(_) => {
            eprintln!("No response from daemon");
            None
        }
        Err(e) => {
            eprintln!("Read failed: {}", e);
            None
        }
    }
}

pub fn read_from_socket_resp(bytes: &[u8]) -> Option<DaemonResponse> {
    match bincode::deserialize::<DaemonResponse>(bytes) {
        Ok(res) => Some(res),
        Err(e) => {
            eprintln!("RES deserialize error: {}", e);
            None
        }
    }
}

#[allow(dead_code)]
pub fn read_from_socket_req(bytes: &[u8]) -> Option<DaemonCommand> {
    match bincode::deserialize::<DaemonCommand>(bytes) {
        Ok(res) => Some(res),
        Err(e) => {
            eprintln!("REQ deserialize error: {}", e);
            None
        }
    }
}
