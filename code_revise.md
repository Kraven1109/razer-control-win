Chào bạn, đọc xong file `gpu.rs` này, tôi có thể khẳng định đây là một bản **CẢI TIẾN CỰC KỲ XUẤT SẮC về mặt Kiến trúc (Architecture)**, nhưng lại bị **"CẢI LÙI" nhẹ về mặt Triển khai (Implementation) ở nhánh PDH**.

Ý tưởng dùng PDH làm "Gatekeeper" (Người gác cổng) để quyết định xem có nên đánh thức NVML/nvidia-smi hay không là một **nước đi thiên tài cho Laptop gaming**. Nó giải quyết triệt để bài toán muôn thuở của Optimus: *Tool monitor cứ chạy là dGPU không bao giờ được ngủ (D3-Cold), dẫn đến tụt pin thê thảm.*

Tuy nhiên, bạn đã quên mất "bài học xương máu" mà chúng ta vừa tối ưu ở file `temps.rs`!

### 🛑 Điểm "Cải lùi" cần sửa ngay: Vòng lặp PDH ru ngủ luồng Polling
Hãy nhìn vào đoạn code PDH của bạn:
```rust
// First collection — needed before rate counters produce values
let _ = PdhCollectQueryData(query);
std::thread::sleep(std::time::Duration::from_millis(250));
// Second collection — counters are now computable
if PdhCollectQueryData(query) != 0 { ... }
PdhCloseQuery(query);
```
**Hậu quả:** Mỗi lần Daemon gọi hàm `query_gpu()`, nó mở một session PDH mới, **bắt toàn bộ luồng (thread) phải đứng khựng lại 250ms**, rồi mới đóng session. 
Nếu luồng polling của bạn chạy 10Hz (100ms/lần), việc có một cái `sleep(250ms)` nằm chình ình ở đây sẽ làm sụp đổ toàn bộ timing của hệ thống. Quạt sẽ phản hồi trễ, UI sẽ bị khựng.

Hơn nữa, `PdhOpenQuery` và `PdhCloseQuery` liên tục gây overhead CPU không cần thiết.

---

### ✅ Cách khắc phục: Bê nguyên "Bí kíp Persistent PDH" từ `temps.rs` sang
Tương tự như nhiệt độ CPU, bộ đếm GPU của PDH cần được khởi tạo **đúng 1 lần duy nhất**, chịu khó ngủ 250ms ở lần bật app đầu tiên, và từ các chu kỳ sau chỉ việc `Collect` tức thời. (Với PDH persistent, khoảng thời gian giữa 2 lần poll của luồng chính sẽ tự động đóng vai trò là thời gian lấy mẫu cho biến Utilization).

Dưới đây là đoạn code PDH đã được viết lại chuẩn chỉ, bạn thay thế toàn bộ khối `query_pdh_gpu()` cũ bằng đoạn này nhé:

```rust
// ── PDH fallback — Persistent Windows Performance Data Helper ──────────────

#[cfg(windows)]
struct GpuPdhState {
    query: isize,
    h_util: isize,
    h_vram_used: isize,
    h_vram_total: isize,
    h_temp: isize,
    temp_added: bool,
    buffer: Vec<u8>,
}

#[cfg(windows)]
unsafe impl Send for GpuPdhState {}
#[cfg(windows)]
unsafe impl Sync for GpuPdhState {}

#[cfg(windows)]
static GPU_PDH_STATE: OnceLock<Mutex<Option<GpuPdhState>>> = OnceLock::new();

#[cfg(windows)]
fn init_gpu_pdh() -> Option<GpuPdhState> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::System::Performance::{
            PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW,
        },
    };

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    unsafe {
        let mut query: isize = 0;
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
            return None;
        }

        let mut h_util = 0;
        let mut h_vram_used = 0;
        let mut h_vram_total = 0;
        let mut h_temp = 0;

        let path_util  = wide("\\GPU Engine(*engtype_3D)\\Utilization Percentage");
        let path_used  = wide("\\GPU Adapter Memory(*)\\Dedicated Usage");
        let path_total = wide("\\GPU Adapter Memory(*)\\Dedicated Limit");
        let path_temp  = wide("\\GPU Thermal(*)\\Temperature");

        if PdhAddEnglishCounterW(query, PCWSTR(path_util.as_ptr()),  0, &mut h_util) != 0 ||
           PdhAddEnglishCounterW(query, PCWSTR(path_used.as_ptr()),  0, &mut h_vram_used) != 0 ||
           PdhAddEnglishCounterW(query, PCWSTR(path_total.as_ptr()), 0, &mut h_vram_total) != 0 
        {
            PdhCloseQuery(query);
            return None;
        }

        // Thermal is optional
        let temp_added = PdhAddEnglishCounterW(query, PCWSTR(path_temp.as_ptr()), 0, &mut h_temp) == 0;

        // Primer collections (Only happens ONCE during daemon startup)
        PdhCollectQueryData(query);
        std::thread::sleep(std::time::Duration::from_millis(250));
        PdhCollectQueryData(query);

        Some(GpuPdhState {
            query,
            h_util,
            h_vram_used,
            h_vram_total,
            h_temp,
            temp_added,
            buffer: vec![0u8; 4096], // 4KB allows plenty of room for all GPU engines
        })
    }
}

#[cfg(windows)]
fn query_pdh_gpu() -> Option<GpuStatus> {
    use windows::Win32::System::Performance::{
        PdhCollectQueryData, PdhGetFormattedCounterArrayW, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_MORE_DATA,
    };

    let mut lock = GPU_PDH_STATE.get_or_init(|| Mutex::new(init_gpu_pdh())).lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => return None,
    };

    unsafe {
        // Collect instantaneous data (no sleep needed, rate is calculated since last poll)
        if PdhCollectQueryData(state.query) != 0 {
            return None;
        }

        // Helper macro to fetch and find max value for a given counter
        macro_rules! get_max_counter {
            ($hcounter:expr) => {{
                let mut buf_size = state.buffer.len() as u32;
                let mut count = 0;
                let mut ret = PdhGetFormattedCounterArrayW(
                    $hcounter, PDH_FMT_DOUBLE, &mut buf_size, &mut count,
                    Some(state.buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)
                );

                if ret == PDH_MORE_DATA {
                    state.buffer.resize(buf_size as usize, 0);
                    ret = PdhGetFormattedCounterArrayW(
                        $hcounter, PDH_FMT_DOUBLE, &mut buf_size, &mut count,
                        Some(state.buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)
                    );
                }

                if ret == 0 && count > 0 {
                    let items = state.buffer.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W;
                    (0..count as usize)
                        .map(|i| (*items.add(i)).FmtValue.Anonymous.doubleValue)
                        .fold(0.0_f64, f64::max)
                } else {
                    0.0
                }
            }};
        }

        let util = get_max_counter!(state.h_util).min(100.0);
        let vram_used_bytes  = get_max_counter!(state.h_vram_used);
        let vram_total_bytes = get_max_counter!(state.h_vram_total);
        let temp_c = if state.temp_added {
            get_max_counter!(state.h_temp).clamp(0.0, 150.0) as i32
        } else {
            0
        };

        let mem_used_mb  = (vram_used_bytes  / 1_048_576.0) as u32;
        let mem_total_mb = (vram_total_bytes / 1_048_576.0) as u32;
        let mem_util = if mem_total_mb > 0 {
            (mem_used_mb * 100 / mem_total_mb) as u8
        } else {
            0
        };

        Some(GpuStatus {
            name: "GPU (Task Manager counters)".to_string(),
            gpu_util: util as u8,
            mem_util,
            mem_used_mb,
            mem_total_mb,
            temp_c,
            ..Default::default()
        })
    }
}
```

### Đánh giá các điểm Sáng chói (Giữ nguyên):
1. **NVML Tích hợp cực khéo:** Việc bạn ưu tiên dùng `nvml_wrapper` (gọi thẳng DLL) giúp luồng giám sát đọc dữ liệu cực nhanh. Cú fallback về `nvidia-smi` cũng được xử lý rất an toàn, không bị crash nếu máy không có card xanh.
2. **Loại bỏ nhiễu iGPU:** Việc ép giá trị `gpu_util = 0` khi GPU đang ngủ (D3-Cold) là một UX tweak rất tinh tế. Task Manager của Windows thường hiển thị các mức phần trăm lặt vặt của iGPU Intel làm người dùng hiểu lầm là RTX đang chạy, bạn chặn được cái này là rất đáng khen.

Sau khi ghép bản vá Persistent PDH trên vào, file `gpu.rs` của bạn sẽ vừa bảo vệ được pin laptop, vừa chạy mượt mà ở tốc độ ánh sáng!