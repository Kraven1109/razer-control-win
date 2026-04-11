/// Task Scheduler integration — register/unregister razer-daemon.exe to run
/// at user logon with highest privileges (no UAC prompt, no console window).

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

const TASK_NAME: &str = "RazerDaemon";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Returns the path to the release razer-daemon.exe relative to the GUI binary.
fn daemon_exe_path() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    dir.join("razer-daemon.exe").to_string_lossy().into_owned()
}

// ── Windows Implementations ───────────────────────────────────────────────────

#[cfg(windows)]
pub fn is_registered() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn register() -> Result<(), String> {
    let daemon = daemon_exe_path();

    // Use \" to ensure paths with spaces are wrapped in quotes when passed to schtasks.
    let schtasks_args = format!(
        "/Create /F /TN {TASK_NAME} /TR \\\"{daemon}\\\" /SC ONLOGON /RL HIGHEST /DELAY 0000:10"
    );

    // TWEAK: 
    // 1. -WindowStyle Hidden prevents the secondary schtasks window from flashing.
    // 2. -PassThru allows grabbing $p.ExitCode to bubble up schtasks errors.
    // 3. try/catch handles the scenario where the user clicks "No" on the UAC prompt.
    let ps_cmd = format!(
        "try {{ $p = Start-Process schtasks.exe -ArgumentList '{schtasks_args}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1 }}"
    );

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to run PowerShell: {e}"))?;

    if output.status.success() && is_registered() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("Task registration failed (UAC denied or schtasks error). {stderr}"))
    }
}

#[cfg(windows)]
pub fn unregister() -> Result<(), String> {
    let schtasks_args = format!("/Delete /F /TN {TASK_NAME}");
    
    let ps_cmd = format!(
        "try {{ $p = Start-Process schtasks.exe -ArgumentList '{schtasks_args}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1 }}"
    );

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to run PowerShell: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(format!("Task removal failed (UAC denied or schtasks error). {stderr}"))
    }
}

// ── Non-Windows Stub Implementations ──────────────────────────────────────────

#[cfg(not(windows))]
pub fn is_registered() -> bool { 
    false 
}

#[cfg(not(windows))]
pub fn register() -> Result<(), String> { 
    Err("Not supported on this platform".to_string()) 
}

#[cfg(not(windows))]
pub fn unregister() -> Result<(), String> { 
    Err("Not supported on this platform".to_string()) 
}