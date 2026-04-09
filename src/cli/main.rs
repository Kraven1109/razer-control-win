#[path = "../comms.rs"]
mod comms;

use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(version = "0.1.0", about = "Razer laptop control CLI — Windows", name = "razer-cli")]
struct Cli {
    #[command(subcommand)]
    args: Args,
}

#[derive(Subcommand)]
enum Args {
    /// Read the current configuration of the device
    Read {
        #[command(subcommand)]
        attr: ReadAttr,
    },
    /// Write a new configuration to the device
    Write {
        #[command(subcommand)]
        attr: WriteAttr,
    },
    /// Set a hardware (standard) keyboard effect
    StandardEffect {
        #[command(subcommand)]
        effect: StandardEffect,
    },
    /// Set a software (animated) keyboard effect
    Effect {
        #[command(subcommand)]
        effect: Effect,
    },
    /// Show NVIDIA GPU status
    Gpu,
    /// Read stored CPU power limits (informational — RAPL not applied on Windows)
    Pdl,
    /// Store CPU power limits (informational — RAPL not applied on Windows)
    SetPdl(PdlParams),
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum OnOff {
    On,
    Off,
}

impl OnOff {
    fn is_on(&self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Subcommand)]
enum ReadAttr {
    Fan(AcStateParam),
    Power(AcStateParam),
    Brightness(AcStateParam),
    Logo(AcStateParam),
    FnSwap,
    Sync,
    Bho,
}

#[derive(Subcommand)]
enum WriteAttr {
    Fan(FanParams),
    Power(PowerParams),
    Brightness(BrightnessParams),
    Logo(LogoParams),
    FnSwap(FnSwapParams),
    Sync(SyncParams),
    Bho(BhoParams),
}

#[derive(Parser)]
struct PowerParams {
    ac_state: AcState,
    pwr: u8,
    cpu_mode: Option<u8>,
    gpu_mode: Option<u8>,
}

#[derive(Parser)]
struct FanParams {
    ac_state: AcState,
    speed: i32,
}

#[derive(Parser)]
struct BrightnessParams {
    ac_state: AcState,
    brightness: i32,
}

#[derive(Parser)]
struct LogoParams {
    ac_state: AcState,
    logo_state: i32,
}

#[derive(Parser)]
struct SyncParams {
    sync_state: OnOff,
}

#[derive(Parser)]
struct FnSwapParams {
    state: OnOff,
}

#[derive(Parser)]
struct BhoParams {
    state: OnOff,
    threshold: Option<u8>,
}

#[derive(Parser)]
struct PdlParams {
    pl1: u32,
    pl2: u32,
}

#[derive(ValueEnum, Clone)]
enum AcState {
    Bat,
    Ac,
}

#[derive(Parser, Clone)]
struct AcStateParam {
    ac_state: AcState,
}

#[derive(Subcommand)]
enum StandardEffect {
    Off,
    Wave(WaveParams),
    Reactive(ReactiveParams),
    Breathing(BreathingParams),
    Spectrum,
    Static(StaticParams),
    Starlight(StarlightParams),
}

#[derive(Parser)]
struct WaveParams {
    direction: u8,
}

#[derive(Parser)]
struct ReactiveParams {
    speed: u8,
    red: u8,
    green: u8,
    blue: u8,
}

#[derive(Parser)]
struct BreathingParams {
    kind: u8,
    red1: u8,
    green1: u8,
    blue1: u8,
    red2: u8,
    green2: u8,
    blue2: u8,
}

#[derive(Parser)]
struct StarlightParams {
    kind: u8,
    speed: u8,
    red1: u8,
    green1: u8,
    blue1: u8,
    red2: u8,
    green2: u8,
    blue2: u8,
}

#[derive(Subcommand)]
enum Effect {
    Static(StaticParams),
    StaticGradient(StaticGradientParams),
    WaveGradient(WaveGradientParams),
    BreathingSingle(BreathingSingleParams),
    BreathingDual(BreathingDualParams),
    SpectrumCycle(SpectrumCycleParams),
    RainbowWave(RainbowWaveParams),
    Starlight(StarlightEffectParams),
    Ripple(RippleParams),
    Wheel(WheelParams),
}

#[derive(Parser)]
struct WheelParams {
    speed: u8,
    direction: u8,
}

#[derive(Parser)]
struct StaticParams {
    red: u8,
    green: u8,
    blue: u8,
}

#[derive(Parser)]
struct StaticGradientParams {
    red1: u8,
    green1: u8,
    blue1: u8,
    red2: u8,
    green2: u8,
    blue2: u8,
}

#[derive(Parser)]
struct WaveGradientParams {
    red1: u8,
    green1: u8,
    blue1: u8,
    red2: u8,
    green2: u8,
    blue2: u8,
}

#[derive(Parser)]
struct BreathingSingleParams {
    red: u8,
    green: u8,
    blue: u8,
    duration: u8,
}

#[derive(Parser)]
struct BreathingDualParams {
    red1: u8,
    green1: u8,
    blue1: u8,
    red2: u8,
    green2: u8,
    blue2: u8,
    duration: u8,
}

#[derive(Parser)]
struct SpectrumCycleParams {
    speed: u8,
}

#[derive(Parser)]
struct RainbowWaveParams {
    speed: u8,
    direction: u8,
}

#[derive(Parser)]
struct StarlightEffectParams {
    red: u8,
    green: u8,
    blue: u8,
    density: u8,
}

#[derive(Parser)]
struct RippleParams {
    red: u8,
    green: u8,
    blue: u8,
    speed: u8,
}

// ── main ───────────────────────────────────────────────────────────────────

fn main() {
    if !comms::is_daemon_running() {
        eprintln!(
            "Error: cannot connect to daemon on {}.\n\
             Is razer-daemon running (elevated)?",
            comms::DAEMON_ADDR
        );
        std::process::exit(1);
    }

    let cli = Cli::parse();

    match cli.args {
        Args::Read { attr } => match attr {
            ReadAttr::Fan(AcStateParam { ac_state }) => read_fan_rpm(ac_state as usize),
            ReadAttr::Power(AcStateParam { ac_state }) => read_power_mode(ac_state as usize),
            ReadAttr::Brightness(AcStateParam { ac_state }) => read_brightness(ac_state as usize),
            ReadAttr::Logo(AcStateParam { ac_state }) => read_logo_mode(ac_state as usize),
            ReadAttr::FnSwap => read_fn_swap(),
            ReadAttr::Sync => read_sync(),
            ReadAttr::Bho => read_bho(),
        },
        Args::Write { attr } => match attr {
            WriteAttr::Fan(FanParams { ac_state, speed }) => write_fan_speed(ac_state as usize, speed),
            WriteAttr::Power(PowerParams { ac_state, pwr, cpu_mode, gpu_mode }) => {
                write_pwr_mode(ac_state as usize, pwr, cpu_mode, gpu_mode)
            }
            WriteAttr::Brightness(BrightnessParams { ac_state, brightness }) => {
                write_brightness(ac_state as usize, brightness as u8)
            }
            WriteAttr::FnSwap(FnSwapParams { state }) => write_fn_swap(state.is_on()),
            WriteAttr::Sync(SyncParams { sync_state }) => write_sync(sync_state.is_on()),
            WriteAttr::Logo(LogoParams { ac_state, logo_state }) => {
                write_logo_mode(ac_state as usize, logo_state as u8)
            }
            WriteAttr::Bho(BhoParams { state, threshold }) => {
                validate_and_write_bho(threshold, state)
            }
        },
        Args::Effect { effect } => match effect {
            Effect::Static(p) => send_effect("static".into(), vec![p.red, p.green, p.blue]),
            Effect::StaticGradient(p) => send_effect(
                "static_gradient".into(),
                vec![p.red1, p.green1, p.blue1, p.red2, p.green2, p.blue2],
            ),
            Effect::WaveGradient(p) => send_effect(
                "wave_gradient".into(),
                vec![p.red1, p.green1, p.blue1, p.red2, p.green2, p.blue2],
            ),
            Effect::BreathingSingle(p) => {
                send_effect("breathing_single".into(), vec![p.red, p.green, p.blue, p.duration])
            }
            Effect::BreathingDual(p) => send_effect(
                "breathing_dual".into(),
                vec![p.red1, p.green1, p.blue1, p.red2, p.green2, p.blue2, p.duration],
            ),
            Effect::SpectrumCycle(p) => send_effect("spectrum_cycle".into(), vec![p.speed]),
            Effect::RainbowWave(p) => send_effect("rainbow_wave".into(), vec![p.speed, p.direction]),
            Effect::Starlight(p) => {
                send_effect("starlight".into(), vec![p.red, p.green, p.blue, p.density])
            }
            Effect::Ripple(p) => send_effect("ripple".into(), vec![p.red, p.green, p.blue, p.speed]),
            Effect::Wheel(p) => send_effect("wheel".into(), vec![p.speed, p.direction]),
        },
        Args::StandardEffect { effect } => match effect {
            StandardEffect::Off => send_standard_effect("off".into(), vec![]),
            StandardEffect::Spectrum => send_standard_effect("spectrum".into(), vec![]),
            StandardEffect::Wave(p) => send_standard_effect("wave".into(), vec![p.direction]),
            StandardEffect::Reactive(p) => {
                send_standard_effect("reactive".into(), vec![p.speed, p.red, p.green, p.blue])
            }
            StandardEffect::Breathing(p) => send_standard_effect(
                "breathing".into(),
                vec![p.kind, p.red1, p.green1, p.blue1, p.red2, p.green2, p.blue2],
            ),
            StandardEffect::Static(p) => {
                send_standard_effect("static".into(), vec![p.red, p.green, p.blue])
            }
            StandardEffect::Starlight(p) => send_standard_effect(
                "starlight".into(),
                vec![p.kind, p.speed, p.red1, p.green1, p.blue1, p.red2, p.green2, p.blue2],
            ),
        },
        Args::Gpu => read_gpu_status(),
        Args::Pdl => read_power_limits(),
        Args::SetPdl(p) => write_power_limits(p.pl1, p.pl2),
    }
}

// ── IPC helpers ────────────────────────────────────────────────────────────

fn send_data(cmd: comms::DaemonCommand) -> Option<comms::DaemonResponse> {
    match comms::bind() {
        Some(socket) => comms::send_to_daemon(cmd, socket),
        None => {
            eprintln!("Error: cannot connect to daemon");
            None
        }
    }
}

fn send_effect(name: String, params: Vec<u8>) {
    match send_data(comms::DaemonCommand::SetEffect { name, params }) {
        Some(comms::DaemonResponse::SetEffect { result: true }) => println!("Effect set OK!"),
        Some(comms::DaemonResponse::SetEffect { result: false }) => eprintln!("Effect set FAIL!"),
        _ => eprintln!("Unexpected daemon response"),
    }
}

fn send_standard_effect(name: String, params: Vec<u8>) {
    match send_data(comms::DaemonCommand::SetStandardEffect { name, params }) {
        Some(comms::DaemonResponse::SetStandardEffect { result: true }) => println!("Effect set OK!"),
        Some(comms::DaemonResponse::SetStandardEffect { result: false }) => eprintln!("Effect set FAIL!"),
        _ => eprintln!("Unexpected daemon response"),
    }
}

// ── Read commands ──────────────────────────────────────────────────────────

fn read_fan_rpm(ac: usize) {
    match send_data(comms::DaemonCommand::GetFanSpeed { ac }) {
        Some(comms::DaemonResponse::GetFanSpeed { rpm }) => {
            let desc = match rpm {
                f if f < 0 => "Unknown".to_string(),
                0 => "Auto (0)".to_string(),
                _ => format!("{} RPM", rpm),
            };
            println!("Current fan setting: {}", desc);
        }
        _ => eprintln!("Failed to read fan speed"),
    }
}

fn read_logo_mode(ac: usize) {
    match send_data(comms::DaemonCommand::GetLogoLedState { ac }) {
        Some(comms::DaemonResponse::GetLogoLedState { logo_state }) => {
            let desc = match logo_state {
                0 => "Off",
                1 => "On",
                2 => "Breathing",
                _ => "Unknown",
            };
            println!("Current logo: {}", desc);
        }
        _ => eprintln!("Failed to read logo state"),
    }
}

fn read_power_mode(ac: usize) {
    if let Some(comms::DaemonResponse::GetPwrLevel { pwr }) =
        send_data(comms::DaemonCommand::GetPwrLevel { ac })
    {
        let desc = match pwr {
            0 => "Balanced",
            1 => "Gaming",
            2 => "Creator",
            3 => "Silent",
            4 => "Custom",
            _ => "Unknown",
        };
        println!("Current power mode: {}", desc);
        if pwr == 4 {
            if let Some(comms::DaemonResponse::GetCPUBoost { cpu }) =
                send_data(comms::DaemonCommand::GetCPUBoost { ac })
            {
                println!(
                    "CPU boost: {}",
                    match cpu {
                        0 => "Low",
                        1 => "Medium",
                        2 => "High",
                        3 => "Boost",
                        _ => "Unknown",
                    }
                );
            }
            if let Some(comms::DaemonResponse::GetGPUBoost { gpu }) =
                send_data(comms::DaemonCommand::GetGPUBoost { ac })
            {
                println!(
                    "GPU boost: {}",
                    match gpu {
                        0 => "Low",
                        1 => "Medium",
                        2 => "High",
                        _ => "Unknown",
                    }
                );
            }
        }
    } else {
        eprintln!("Failed to read power mode");
    }
}

fn read_brightness(ac: usize) {
    match send_data(comms::DaemonCommand::GetBrightness { ac }) {
        Some(comms::DaemonResponse::GetBrightness { result }) => {
            println!("Current brightness: {}", result)
        }
        _ => eprintln!("Failed to read brightness"),
    }
}

fn read_sync() {
    match send_data(comms::DaemonCommand::GetSync()) {
        Some(comms::DaemonResponse::GetSync { sync }) => println!("Sync: {}", sync),
        _ => eprintln!("Failed to read sync"),
    }
}

fn read_fn_swap() {
    match send_data(comms::DaemonCommand::GetFnSwap()) {
        Some(comms::DaemonResponse::GetFnSwap { swap }) => {
            println!(
                "Fn swap: {}",
                if swap {
                    "ON (media keys primary)"
                } else {
                    "OFF (F-keys primary)"
                }
            );
        }
        _ => eprintln!("Failed to read Fn swap state"),
    }
}

fn read_bho() {
    match send_data(comms::DaemonCommand::GetBatteryHealthOptimizer()) {
        Some(comms::DaemonResponse::GetBatteryHealthOptimizer { is_on, threshold }) => {
            if is_on {
                println!("Battery Health Optimizer: ON (threshold {}%)", threshold);
            } else {
                println!("Battery Health Optimizer: OFF");
            }
        }
        _ => eprintln!("Failed to read BHO status"),
    }
}

fn read_gpu_status() {
    match send_data(comms::DaemonCommand::GetGpuStatus) {
        Some(comms::DaemonResponse::GetGpuStatus {
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
        }) => {
            println!("GPU:         {}{}", name, if stale { " (cached)" } else { "" });
            if temp_c > 0 {
                println!("Temperature: {}°C", temp_c);
            }
            println!("GPU Usage:   {}%", gpu_util);
            if mem_total_mb > 0 {
                println!("VRAM Usage:  {}% ({} / {} MiB)", mem_util, mem_used_mb, mem_total_mb);
            }
            if power_w > 0.0 {
                println!(
                    "Power Draw:  {:.1} W  (TGP: {:.0} W / max: {:.0} W)",
                    power_w, power_limit_w, power_max_limit_w
                );
            }
            if clock_gpu_mhz > 0 {
                println!("GPU Clock:   {} MHz", clock_gpu_mhz);
                println!("Mem Clock:   {} MHz", clock_mem_mhz);
            }
        }
        _ => eprintln!("GPU status unavailable (daemon GPU monitor not yet populated)"),
    }
}

fn read_power_limits() {
    // AC state detection on Windows: default 1 (AC) since we can't read sysfs here.
    match send_data(comms::DaemonCommand::GetPowerLimits { ac: 1 }) {
        Some(comms::DaemonResponse::GetPowerLimits { pl1_watts, pl2_watts, .. }) => {
            println!("PL1 (sustained): {} W", pl1_watts);
            println!("PL2 (boost):     {} W", pl2_watts);
            println!("Note: RAPL limits are not applied on Windows.");
        }
        _ => eprintln!("Failed to read power limits"),
    }
}

// ── Write commands ─────────────────────────────────────────────────────────

fn write_pwr_mode(ac: usize, pwr_mode: u8, cpu_mode: Option<u8>, gpu_mode: Option<u8>) {
    if pwr_mode > 4 {
        Cli::command()
            .error(ErrorKind::InvalidValue, "Power mode must be 0-4")
            .exit()
    }
    let cm = if pwr_mode == 4 {
        cpu_mode.expect("CPU mode required when power mode is 4")
    } else {
        cpu_mode.unwrap_or(0)
    };
    if cm > 3 {
        Cli::command()
            .error(ErrorKind::InvalidValue, "CPU mode must be 0-3")
            .exit()
    }
    let gm = if pwr_mode == 4 {
        gpu_mode.expect("GPU mode required when power mode is 4")
    } else {
        gpu_mode.unwrap_or(0)
    };
    if gm > 2 {
        Cli::command()
            .error(ErrorKind::InvalidValue, "GPU mode must be 0-2")
            .exit()
    }
    match send_data(comms::DaemonCommand::SetPowerMode {
        ac,
        pwr: pwr_mode,
        cpu: cm,
        gpu: gm,
    }) {
        Some(_) => read_power_mode(ac),
        None => eprintln!("Failed to set power mode"),
    }
}

fn write_fan_speed(ac: usize, rpm: i32) {
    match send_data(comms::DaemonCommand::SetFanSpeed { ac, rpm }) {
        Some(_) => read_fan_rpm(ac),
        None => eprintln!("Failed to set fan speed"),
    }
}

fn write_brightness(ac: usize, val: u8) {
    match send_data(comms::DaemonCommand::SetBrightness { ac, val }) {
        Some(_) => read_brightness(ac),
        None => eprintln!("Failed to set brightness"),
    }
}

fn write_logo_mode(ac: usize, logo_state: u8) {
    match send_data(comms::DaemonCommand::SetLogoLedState { ac, logo_state }) {
        Some(_) => read_logo_mode(ac),
        None => eprintln!("Failed to set logo"),
    }
}

fn write_sync(sync: bool) {
    match send_data(comms::DaemonCommand::SetSync { sync }) {
        Some(_) => read_sync(),
        None => eprintln!("Failed to set sync"),
    }
}

fn write_fn_swap(swap: bool) {
    match send_data(comms::DaemonCommand::SetFnSwap { swap }) {
        Some(comms::DaemonResponse::SetFnSwap { result: true }) => read_fn_swap(),
        Some(comms::DaemonResponse::SetFnSwap { result: false }) => eprintln!(
            "Fn swap write was rejected or did not persist on this device"
        ),
        _ => eprintln!("Failed to set Fn swap"),
    }
}

fn write_power_limits(pl1: u32, pl2: u32) {
    match send_data(comms::DaemonCommand::SetPowerLimits {
        ac: 1,
        pl1_watts: pl1,
        pl2_watts: pl2,
    }) {
        Some(comms::DaemonResponse::SetPowerLimits { result: true }) => {
            println!("Stored PL1={} W, PL2={} W (RAPL not applied on Windows)", pl1, pl2)
        }
        _ => eprintln!("Failed to store power limits"),
    }
}

fn validate_and_write_bho(threshold: Option<u8>, state: OnOff) {
    match threshold {
        Some(t) => {
            if !valid_bho_threshold(t) {
                Cli::command()
                    .error(
                        ErrorKind::InvalidValue,
                        "Threshold must be a multiple of 5 between 50 and 80",
                    )
                    .exit()
            }
            write_bho(state.is_on(), t)
        }
        None => {
            if state.is_on() {
                Cli::command()
                    .error(ErrorKind::MissingRequiredArgument, "Threshold required when BHO is on")
                    .exit()
            }
            write_bho(false, 80)
        }
    }
}

fn valid_bho_threshold(t: u8) -> bool {
    t % 5 == 0 && t >= 50 && t <= 80
}

fn write_bho(on: bool, threshold: u8) {
    match send_data(comms::DaemonCommand::SetBatteryHealthOptimizer { is_on: on, threshold }) {
        Some(comms::DaemonResponse::SetBatteryHealthOptimizer { result: true }) => {
            if on {
                println!("BHO enabled at {}%", threshold);
            } else {
                println!("BHO disabled");
            }
        }
        _ => eprintln!("Failed to set BHO"),
    }
}
