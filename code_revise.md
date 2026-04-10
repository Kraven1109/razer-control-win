### 2. Lấy nhiệt độ CPU, SSD, Mainboard (An toàn & Không wake dGPU)
Các linh kiện này sử dụng các bus giao tiếp hoàn toàn khác (SMBus, I2C, hoặc tích hợp thẳng trong CPU package) và không liên quan gì đến bus PCIe của card rời. Bạn có thể đọc chúng thoải mái mỗi 1-2 giây mà không lo đánh thức dGPU.

Có 3 con đường để ứng dụng Rust của bạn lấy dữ liệu này trên Windows:

**Cách 1: Đọc qua Windows Performance Data Helper (PDH)**
* **Ưu điểm:** Native API, không cần quyền Admin sâu, cực nhẹ (bạn đã có sẵn code PDH ở file `query_pdh_gpu`).
* **Nhược điểm:** Không phải mainboard/máy tính nào cũng expose nhiệt độ CPU/SSD lên PDH.
* **Target:** `\Thermal Zone Information(*)\Temperature`.

**Cách 2: Đọc qua WMI (Windows Management Instrumentation)**
* **Ưu điểm:** Chuẩn của Windows, có thể lấy được qua API `MSAcpi_ThermalZoneTemperature` hoặc namespace của nhà sản xuất (như Razer/Dell/Lenovo).
* **Nhược điểm:** Query WMI bằng Rust đôi khi hơi cồng kềnh (có thể dùng crate `wmi`), và một số máy trả về nhiệt độ theo đơn vị 1/10 Kelvin.

**Cách 3: Đọc trực tiếp từ thanh ghi phần cứng (Cách các app Pro như HWiNFO, LibreHardwareMonitor làm)**
* **Ưu điểm:** Đọc được chính xác đến từng core CPU, lấy được cả nhiệt độ SSD (SMART), cực kỳ chi tiết.
* **Nhược điểm:** Cần cài một driver Ring0 (như `WinRing0.sys` hoặc `inpoutx64.sys`) để có quyền chọc vào phần cứng. Với một tool cá nhân, việc tự maintain cái này bằng Rust rất cực.

---

### 3. Đề xuất Kiến trúc cho `razer-daemon` của bạn

Để ứng dụng của bạn chuyên nghiệp như các công cụ của bên thứ 3 (và giữ pin laptop trâu nhất có thể), hãy kết hợp **Gatekeeper** (ở bài trước) với thư viện hệ thống:

1. **Với dGPU:**
   * Dùng cái Gatekeeper VRAM (> 256MB) làm chốt chặn. 
   * Nếu Gatekeeper đóng (dGPU ngủ): Trả về `temp_c = 0`. Giao diện hiện chữ "Zzz" hoặc "Offline".
   * Nếu Gatekeeper mở (dGPU thức): Gọi `nvidia-smi` để lấy temp chuẩn.
2. **Với CPU/SSD/RAM:**
   * Thay vì tự code lại từ đầu, bạn có thể dùng crate [sysinfo](https://github.com/GuillaumeGomez/sysinfo) của Rust. Nó hỗ trợ Windows khá tốt trong việc lấy nhiệt độ CPU thông qua các sensor phổ thông mà không đụng chạm đến dGPU.
   * Ví dụ:
     ```rust
     use sysinfo::{System, SystemExt, ComponentExt};
     
     let mut sys = System::new_all();
     sys.refresh_components();
     
     for component in sys.components() {
         if component.label().contains("CPU") {
             println!("{} temp: {}°C", component.label(), component.temperature());
         }
     }
     ```

File `poll.rs` của bạn có cấu trúc luồng (thread), channels và cơ chế đánh thức (wake early) rất chuẩn. Tuy nhiên, nếu bạn đang nhắm tới một app **"Zero Overhead"** để tiết kiệm pin tối đa cho laptop, thì file này đang chứa một **"Sát thủ ngầm" tàn phá CPU C-states**.

Dưới đây là review chi tiết và cách tinh chỉnh để phần nền của GUI chạy nhẹ như một chiếc lông hồng.

### 1. Sát thủ ngầm: Query Static Data trong vòng lặp (Critical)

Hãy nhìn vào hàm `collect_sys()` của bạn, nó được gọi mỗi 3 giây trong `do_poll()`:

```rust
const BIOS_KEY: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
let laptop_model = reg_read_sz(BIOS_KEY, "SystemProductName");
let bios_version = reg_read_sz(BIOS_KEY, "BIOSVersion");
let bios_date    = reg_read_sz(BIOS_KEY, "BIOSReleaseDate");

let cpu_name  = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
let host_name = System::host_name().unwrap_or_default();
let os_name   = System::long_os_version().unwrap_or_default();
```

**Vấn đề:** Bạn đang thực hiện **3 lệnh đọc Registry Windows** và hàng loạt phép parse string của `sysinfo` (OS name, CPU name, Hostname) **mỗi 3 giây**. 
Những thông số này (Tên máy, tên CPU, BIOS) là **Dữ liệu tĩnh (Static Data)** — nó không bao giờ thay đổi trong suốt vòng đời app chạy. Việc liên tục chọc ngoáy vào Registry và gọi OS API mỗi 3 giây sẽ ngăn CPU của bạn rơi vào trạng thái ngủ sâu (C8/C10 state), làm tăng nhiệt độ và ngốn pin vô ích.

**Cách Fix:** Sử dụng `std::sync::OnceLock` (Rust 1.70+) để cache dữ liệu tĩnh này ngay lần đầu tiên gọi, và tái sử dụng nó vĩnh viễn.

### 2. Tối ưu Memory Refresh của `sysinfo`

```rust
.with_memory(MemoryRefreshKind::everything())
```
Hàm `everything()` của `sysinfo` sẽ query cả dung lượng Swap/Pagefile. Trên Windows, đọc Pagefile có thể sinh ra I/O disk request siêu nhỏ. Vì bạn chỉ cần `ram_used_mb` và `ram_total_mb`, hãy đổi thành:
`.with_memory(MemoryRefreshKind::new().with_ram())`

### 3. Vấn đề IPC Chatty (Kiến trúc)
Trong `do_poll()`, bạn gọi hàm `send()` khoảng **18 lần** (1 lần lấy tên, 6 lần x2 cho AC/BAT, 5 lần lấy BHO/Fn/Sync...).
Mặc dù IPC qua local socket khá nhanh, nhưng việc ping-pong 18 lần mỗi 3 giây tốn khá nhiều context-switch của OS. 
* *Lời khuyên (Không ép buộc phải sửa ngay):* Ở version sau, bạn nên gom tất cả API này thành một lệnh duy nhất ở daemon, ví dụ: `comms::DaemonCommand::GetAllStatus`, daemon gom 1 cục struct bự ném về cho GUI. Code GUI sẽ sạch và nhanh hơn gấp chục lần.

---

### Bản Revise `poll.rs` Tối ưu

Đây là đoạn code đã được tinh chỉnh lại với `OnceLock` và tối ưu `sysinfo`. Bạn thay thế phần từ đầu đến hàm `collect_sys()` bằng đoạn này:

```rust
/// Background poll thread and IPC send helper.

use crate::app::{PollData, SysMetrics, SysStatic};
use crate::app::{GpuInfo, Pwr};
use crate::comms;
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

// Persistent sysinfo System — kept alive across polls so CPU usage delta works.
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

lazy_static::lazy_static! {
    static ref SYS: std::sync::Mutex<System> = {
        let s = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::new().with_cpu_usage())
                // Tweak: Chỉ refresh RAM vật lý, bỏ qua Swap/Pagefile để tránh disk I/O wakeup
                .with_memory(MemoryRefreshKind::new().with_ram()),
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

// Tweak: Cache các dữ liệu tĩnh (Static Data) để không bao giờ phải đọc Registry 
// hay query OS config trong vòng lặp 3 giây.
static STATIC_SYS_INFO: OnceLock<SysStatic> = OnceLock::new();

fn get_sys_static(sys: &System) -> SysStatic {
    STATIC_SYS_INFO.get_or_init(|| {
        const BIOS_KEY: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
        let laptop_model = reg_read_sz(BIOS_KEY, "SystemProductName");
        let bios_version = reg_read_sz(BIOS_KEY, "BIOSVersion");
        let bios_date    = reg_read_sz(BIOS_KEY, "BIOSReleaseDate");

        let cpu_name  = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
        let host_name = System::host_name().unwrap_or_default();
        let os_name   = System::long_os_version().unwrap_or_default();
        
        SysStatic {
            cpu_name,
            host_name,
            os_name,
            laptop_model,
            bios_version,
            bios_date,
            uptime_secs: 0, // Sẽ được update liên tục bên ngoài
        }
    }).clone()
}

fn collect_sys() -> (SysMetrics, SysStatic) {
    let mut sys = SYS.lock().unwrap_or_else(|e| e.into_inner());
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpu_pct      = sys.global_cpu_usage();
    let ram_used_mb  = sys.used_memory() / 1024 / 1024;
    let ram_total_mb = sys.total_memory() / 1024 / 1024;
    let metrics = SysMetrics { cpu_pct, ram_used_mb, ram_total_mb };

    // Lấy bản clone của static info (cực nhanh vì chỉ copy string)
    let mut statics = get_sys_static(&sys);
    // Uptime là thông số động duy nhất cần update
    statics.uptime_secs = System::uptime(); 

    (metrics, statics)
}
```

Các phần phía dưới (như `send()`, `poll_pwr_slot`, `do_poll` và `start_poll_thread`) của bạn đã viết rất gọn gàng và chuẩn chỉ, bạn có thể giữ nguyên không cần thay đổi. Với fix này, background thread của bạn sẽ trở thành một "ninja" thực thụ — hoạt động hiệu quả mà không để lại gánh nặng cho CPU.