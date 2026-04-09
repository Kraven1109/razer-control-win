/// Display refresh-rate switching for battery savings.
///
/// Enumerates the primary monitor's supported modes and offers toggling
/// between the highest available refresh rate (AC) and the lowest (battery).

#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplaySettingsW, DEVMODEW, CDS_TYPE,
    DISP_CHANGE_SUCCESSFUL, DM_PELSWIDTH, DM_PELSHEIGHT, DM_DISPLAYFREQUENCY,
    ENUM_CURRENT_SETTINGS,
};

/// Enumerate all distinct refresh rates available at the current resolution.
/// Returns a sorted list (ascending) of Hz values.
#[cfg(windows)]
pub fn available_refresh_rates() -> Vec<u32> {
    let mut rates = Vec::new();
    let mut devmode: DEVMODEW = unsafe { std::mem::zeroed() };
    devmode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;

    // Get current resolution so we only list rates for this resolution.
    let ok = unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut devmode) };
    if !ok.as_bool() {
        return rates;
    }
    let cur_w = devmode.dmPelsWidth;
    let cur_h = devmode.dmPelsHeight;

    let mut i = 0u32;
    loop {
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        let ok = unsafe {
            EnumDisplaySettingsW(None, windows::Win32::Graphics::Gdi::ENUM_DISPLAY_SETTINGS_MODE(i), &mut dm)
        };
        if !ok.as_bool() {
            break;
        }
        if dm.dmPelsWidth == cur_w && dm.dmPelsHeight == cur_h && dm.dmDisplayFrequency > 0 {
            if !rates.contains(&dm.dmDisplayFrequency) {
                rates.push(dm.dmDisplayFrequency);
            }
        }
        i += 1;
    }
    rates.sort();
    rates
}

/// Get the current display refresh rate (Hz).
#[cfg(windows)]
#[allow(dead_code)]
pub fn current_refresh_rate() -> u32 {
    let mut devmode: DEVMODEW = unsafe { std::mem::zeroed() };
    devmode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    let ok = unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut devmode) };
    if ok.as_bool() { devmode.dmDisplayFrequency } else { 0 }
}

/// Set the display refresh rate (Hz). Returns true on success.
#[cfg(windows)]
pub fn set_refresh_rate(hz: u32) -> bool {
    let mut devmode: DEVMODEW = unsafe { std::mem::zeroed() };
    devmode.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    let ok = unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut devmode) };
    if !ok.as_bool() {
        return false;
    }
    devmode.dmDisplayFrequency = hz;
    devmode.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    let result = unsafe { ChangeDisplaySettingsExW(None, Some(&devmode as *const _), None, CDS_TYPE(0), None) };
    result == DISP_CHANGE_SUCCESSFUL
}

#[cfg(not(windows))]
pub fn available_refresh_rates() -> Vec<u32> { vec![] }
#[cfg(not(windows))]
#[allow(dead_code)]
pub fn current_refresh_rate() -> u32 { 0 }
#[cfg(not(windows))]
pub fn set_refresh_rate(_hz: u32) -> bool { false }
