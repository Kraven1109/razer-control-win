/// Windows power / AC-state helper.
///
/// Uses GetSystemPowerStatus (Win32) to read whether the machine is running
/// on AC power.  This replaces the D-Bus UPower calls in the Linux build.

#[cfg(windows)]
pub fn is_on_ac() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    unsafe {
        let mut status = SYSTEM_POWER_STATUS::default();
        let _ = GetSystemPowerStatus(&mut status);
        // ACLineStatus: 0 = offline, 1 = online, 255 = unknown
        status.ACLineStatus == 1
    }
}

#[cfg(not(windows))]
pub fn is_on_ac() -> bool {
    true
}
