/// System temperature queries for the daemon.
///
/// Priority chain:
///   1. IOCTL_THERMAL_QUERY_INFORMATION via SetupDi (kernel thermal driver, ACPI zones).
///   2. sysinfo WMI (all available system components).
///
/// ⚠  On Razer Blade 16 (2023) all known Windows user-mode temperature paths ultimately
///    read ACPI `_TMP`, which is firmware-pinned to a static thermal trip-point (~45 °C).
///    The only way to read the real die temperature on this hardware is via ring-0 MSR
///    reads (IA32_THERM_STATUS 0x19C) — which HWiNFO64 / Core Temp do with their own
///    kernel-mode drivers.  Without such a driver we can only surface what Windows exposes.
///
///    The first-run log will enumerate ALL sysinfo components; if any sensor is reading
///    a live temperature it will be visible there.

use std::sync::{Mutex, OnceLock};

// ── Public entry point ────────────────────────────────────────────────────

pub fn query_sys_temps() -> (f32, f32) {
    (query_cpu_temp(), 0.0)
}

fn query_cpu_temp() -> f32 {
    let t = query_cpu_temp_thermal_zones();
    if t > 0.0 { return t; }
    query_cpu_temp_sysinfo()
}

// ── 1. IOCTL_THERMAL_QUERY_INFORMATION ───────────────────────────────────
//
// Two bugs fixed from the previous version:
//  • access = 0  →  GENERIC_READ (0x80000000)
//    The IOCTL requires at least FILE_READ_ACCESS.  Passing 0 lets CreateFileW
//    succeed but DeviceIoControl returns ERROR_ACCESS_DENIED silently.
//  • bytes guard >= size_of::<ThermalInfo>() (84 B)  →  >= 32 B
//    CurrentTemperature is at offset 28.  Many ACPI drivers return only the
//    first 32–44 bytes; the old strict guard discarded all valid readings.

#[cfg(windows)]
static THERMAL_ZONE_PATHS: OnceLock<Vec<Vec<u16>>> = OnceLock::new();

#[cfg(windows)]
fn init_thermal_zone_paths() -> Vec<Vec<u16>> {
    use std::mem;
    use windows::{
        core::GUID,
        Win32::Devices::DeviceAndDriverInstallation::{
            SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
            SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
            DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
            SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
        },
    };

    // GUID_DEVINTERFACE_THERMAL_ZONE = {4AFA3D52-74A7-11D0-BE5E-00A0C9062857}
    let guid = GUID {
        data1: 0x4AFA3D52,
        data2: 0x74A7,
        data3: 0x11d0,
        data4: [0xbe, 0x5e, 0x00, 0xA0, 0xC9, 0x06, 0x28, 0x57],
    };
    let mut paths: Vec<Vec<u16>> = Vec::new();
    unsafe {
        let devs = match SetupDiGetClassDevsW(
            Some(&guid), None, None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) {
            Ok(h) if !h.is_invalid() => h,
            _ => return paths,
        };
        let mut idx = 0u32;
        loop {
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(devs, None, &guid, idx, &mut iface).is_err() { break; }
            idx += 1;

            let mut req = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(devs, &iface, None, 0, Some(&mut req), None);
            if req == 0 { continue; }

            let mut buf = vec![0u8; req as usize];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(devs, &iface, Some(detail), req, None, None).is_ok() {
                let ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
                let mut len = 0;
                while *ptr.add(len) != 0 { len += 1; }
                let mut v = Vec::with_capacity(len + 1);
                v.extend_from_slice(std::slice::from_raw_parts(ptr, len + 1));
                paths.push(v);
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(devs);
    }
    log::debug!("Thermal zone devices: {}", paths.len());
    paths
}

#[cfg(windows)]
fn query_cpu_temp_thermal_zones() -> f32 {
    use std::mem;
    use windows::Win32::{
        Foundation::CloseHandle,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::IO::DeviceIoControl,
    };

    // CTL_CODE(FILE_DEVICE_BATTERY=0x29, fn=0x12, METHOD_BUFFERED, FILE_READ_ACCESS)
    const IOCTL_THERMAL_QUERY_INFORMATION: u32 = 0x0029_4048;
    const GENERIC_READ: u32 = 0x8000_0000;  // FIX: was 0 — IOCTL needs read access

    // THERMAL_INFORMATION layout (x64, WDK ntddtherm.h):
    //   0  ThermalStamp          u32
    //   4  ThermalConstant1      u32
    //   8  ThermalConstant2      u32
    //  12  [4-byte padding]          ← aligns KAFFINITY (u64) to offset 16
    //  16  Processors (KAFFINITY)   u64
    //  24  SamplingPeriod            u32
    //  28  CurrentTemperature        u32  ← value we read (1/10 Kelvin)
    //  32  …rest of struct…
    #[repr(C)]
    struct ThermalInfo {
        _s: u32, _c1: u32, _c2: u32, _pad: u32,
        _proc: u64, _period: u32,
        current_temperature: u32,
        _rest: [u32; 13],   // PassiveTripPoint through ActiveTripPoint[10]
    }
    const _: () = assert!(std::mem::offset_of!(ThermalInfo, current_temperature) == 28);

    let paths = THERMAL_ZONE_PATHS.get_or_init(init_thermal_zone_paths);
    let mut max_c = 0.0_f32;

    for path in paths {
        let Ok(h) = (unsafe {
            CreateFileW(
                windows::core::PCWSTR(path.as_ptr()),
                GENERIC_READ,               // FIX: was 0
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, None,
            )
        }) else { continue; };
        if h.is_invalid() { continue; }

        let mut info = mem::MaybeUninit::<ThermalInfo>::zeroed();
        let mut bytes = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                h,
                IOCTL_THERMAL_QUERY_INFORMATION,
                None, 0,
                Some(info.as_mut_ptr() as *mut _),
                mem::size_of::<ThermalInfo>() as u32,
                Some(&mut bytes),
                None,
            )
        };
        unsafe { let _ = CloseHandle(h); }

        // FIX: was >= size_of::<ThermalInfo>() (84 B).  Only need >= 32 B
        // to safely read CurrentTemperature at offset 28.
        if ok.is_ok() && bytes as usize >= 32 {
            let t = unsafe { info.assume_init() }.current_temperature;
            if t > 0 {
                let c = (t as f32 / 10.0) - 273.15;  // 1/10 K → °C
                if c > 0.0 && c < 120.0 { max_c = max_c.max(c); }
            }
        }
    }
    max_c
}

#[cfg(not(windows))]
fn query_cpu_temp_thermal_zones() -> f32 { 0.0 }

// ── 2. sysinfo WMI (MSAcpi_ThermalZoneTemperature) ───────────────────────

static SYS_COMPS: OnceLock<Mutex<sysinfo::Components>> = OnceLock::new();

fn query_cpu_temp_sysinfo() -> f32 {
    let mutex = SYS_COMPS
        .get_or_init(|| {
            let comps = sysinfo::Components::new_with_refreshed_list();
            // Log every visible component once at startup so we can see what
            // sensors Windows actually exposes on this hardware.
            log::info!("Thermal sensors found by sysinfo ({} total):", comps.len());
            for c in comps.iter() {
                log::info!("  [{:.1}°C] {:?}", c.temperature(), c.label());
            }
            Mutex::new(comps)
        });
    let Ok(mut comps) = mutex.lock() else { return 0.0; };
    // Windows WMI note: refresh() re-reads from an already-instantiated WbemClassObject
    // which can return stale cached values.  refresh_list() re-executes the WQL query,
    // forcing the ACPI WMI Bridge to provide a fresh CurrentTemperature read.
    comps.refresh_list();
    comps.iter()
        .map(|c| c.temperature())
        .filter(|&t| t > 20.0 && t < 110.0)
        .fold(0.0_f32, f32::max)
}
