# Razer Control Win — Developer Memo

> Living document. Update after every meaningful session.

---

## Project Overview

Windows-native Razer laptop control daemon (Synapse replacement).  
Three binaries: `razer-daemon` (background service), `razer-cli` (shell control), `razer-gui` (eframe 0.29 tray app).

**Hardware**: Blade 16 2023 — RZ09-0483, RTX 4090 Laptop (10de:2757)

---

## Architecture

```
razer-daemon   ←──IPC (bincode/TCP)──→  razer-gui / razer-cli
    │
    ├── kbd/       HID keyboard (brightness, effects)
    ├── power.rs   ACPI power profile (WMI / HID)
    └── gpu.rs     PDH-first GPU monitor:
                   • PDH query is persistent (OnceLock, init once at daemon start)
                   • Gate: VRAM > 256 MB OR temp > 0 °C → dGPU awake
                   •   awake: NVML (per-poll init+drop) → nvidia-smi → PDH
                   •   sleeping: sanitize PDH → name="NVIDIA GPU (Sleeping)"
                   •   PDH failed: fall back to nvidia-smi unconditionally
                   • NVML handle dropped after each poll (no persistent connection)
```

IPC message types defined in `src/lib.rs` (`DaemonCommand` / `DaemonResponse`).

---

## Verified TGP Values (FurMark load, AC)

| Profile  | Observed TGP | CPU boost |
|----------|-------------|-----------|
| Silent   | ~115 W      | Eco       |
| Balanced | ~130-135 W  | Normal    |
| Gaming   | ~150-154 W  | High      |
| Custom   | user-set    | user-set  |

Linux reference: Silent=115W, Balanced=135W, Gaming=150W, DynBoost max=175W  
(`D:\quang_dev\razer_control\memo.md`)

---

## Bug Fixes / Improvements (Session 2025-07 — power & UX polish)

### GPU power drain (20-30 W idle)
**Root cause A — GUI renderer**: eframe/glow loops at vsync by default; `App::update()` was
being called at ~60 fps even with no visible change.
**Fix**: `ctx.request_repaint_after(Duration::from_secs(3))` at the END of `App::update()`.
The GUI now goes idle between poll cycles; user interaction still wakes it immediately.

**Root cause B — wgpu/glow GPU selection**: NVIDIA Optimus may assign the dGPU as the render
device for any app that doesn't opt out.
**Fix**: Added at the top of `src/gui/main.rs`:
```rust
#[no_mangle] pub static NvOptimusEnablement: i32 = 0;
#[no_mangle] pub static AmdPowerXpressRequestHighPerformance: i32 = 0;
```
These standard OS-level hints force both NVIDIA Optimus and AMD PowerXpress to assign the iGPU
for rendering. Confirmed by NVIDIA driver documentation.

**Root cause C — daemon nvidia-smi polling wakes dGPU**: `gpu.rs` was spawning nvidia-smi every
3 seconds regardless of GPU activity, preventing D3 sleep on the dGPU.
**Fix**: PDH-first query strategy in `query_gpu()`:
1. Always query PDH first (passive Windows Performance Counters — no GPU wake).
2. Only call nvidia-smi if PDH reports activity (util > 0% OR VRAM > 500 MB used).
3. nvidia-smi path caching (resolved once at startup, not re-discovered each poll).

### Chart / metric tile color consistency
All tile accent colours now match the corresponding chart line colours via shared `CH_*` constants:
- `CH_GPU`   (`#44FFA1`, green)   — GPU utilization
- `CH_TEMP`  (`#FF934F`, orange)  — GPU temperature
- `CH_VRAM`  (`#C772FF`, purple)  — VRAM utilization
- `CH_POWER` (`#50C8FF`, cyan)    — TGP / power draw
Old `TEMP` constant removed (now superseded by `CH_TEMP`).

### Startup / autostart integration
- **GUI**: "Start on Boot" toggle row added to the System tab via `draw_startup_card()`.
  Calls `tray::is_autostart_enabled()` / `tray::toggle_autostart()`.
- **CLI**: New `razer-cli startup enable|disable|status` subcommand.
  Operates on `HKCU\...\Run\RazerBladeControl` using the Windows registry API.
  Does **not** require a running daemon.

### Code quality (main.rs review)
- `request_repaint_after` moved to end of `update()` (cleaner frame semantics).
- `NAV_ITEMS`, `BADGE_ACTIVE_ALPHA`, `BADGE_IDLE_ALPHA` extracted as module-level constants.
- `resp.on_hover_cursor()` replaces `ctx.set_cursor_icon()` (proper egui idiom, scoped to widget).
- Tray IPC handlers DRY'd with internal closures.
- Detached chart window: clones removed — fields borrowed directly using Rust 2021 disjoint capture.
- `egui::Id`-based temp data hack for chart-close replaced with direct `self.chart_detached = false`.

### Code quality (device.rs review — critical CRC fix + HID improvements)
- **CRC bug fixed** (`calc_crc_and_serialize`): old `calc_crc()` serialized the packet BEFORE
  writing `self.crc`, so every outgoing HID packet had CRC=0x00 in the wire bytes. Fixed by
  serializing a second time after `self.crc = res` to embed the computed byte.
- **HidApi cached** via `lazy_static!`: avoids full SetupAPI re-enumeration on each
  `discover_devices()` call; the handle lives for the daemon's lifetime.
- **Usage-page filter** added (`d.usage_page() == 0xFF00`): skips boot-keyboard/mouse HID
  interfaces that lack a Feature report descriptor and cause `HidD_SetFeature` to fail.
- **`find_supported_device`** rewritten with `.iter().find()` + `.cloned()` (idiomatic Rust).
- **`get_name(&self) -> &str`** — returns `&self.name` instead of cloning String.
- **`have_feature(&self, fch: &str)`** — takes `&str` not `String`; zero per-call allocation.
- **`clamp_fan`** Guards against empty `self.fan` (would panic on index if JSON lacks limits).
- **`set_standard_effect`** adds `.take(79)` to prevent out-of-bounds array access.
- **`read_response` method** with 15-iteration `hint::spin_loop()` first pass: cuts HID
  feature-report latency from ~1.2 ms to ~0.3 ms; 1 ms sleep fallback if EC is busy.
- **`send_report` rewrite**: packet serialized once before retry loop (no repeated heap allocs);
  exponential backoff 1 ms→2 ms→4 ms instead of fixed 5/10/15 ms; better contextual log messages.

### Fan safety (Session 2025-07 — earlier)
- On daemon startup `config.reset_fan_profiles_to_auto()` is called; any previously-saved
  manual RPM is cleared to 0 (auto).
- `set_fan_rpm` writes only to `DeviceManager::fan_overrides[ac]` (session-only, not persisted).
- `set_ac_state` re-applies the session override after each AC/BAT transition.

---

### 1. IPC spam on profile switch
**Root cause**: Old `draw_power` used snapshot/compare pattern. `apply_poll()` sets
`app.ac.cpu` from device (e.g., 2 = High boost). Reset block `if mode != 4 { cpu=0; gpu=0 }`
ran every frame → `new != old` every frame → IPC fired at 60 fps.

**Fix**: Rewrote `src/gui/tabs/power.rs` completely. Each widget fires IPC only inside
its `changed()` / `drag_stopped()` / `lost_focus()` handler. Zero render-time state mutation.

### 2. Chart polygon artifact (fan/cross pattern)
**Root cause**: `egui::Shape::convex_polygon` builds a convex hull from points.
Non-convex sparkline data → crossing triangle fill / fan artifact.

**Fix**: Replace with `egui::Shape::Path(egui::epaint::PathShape { closed: true, ... })`.
Applied in `src/gui/tabs/system.rs`.

### 3. VRAM shows "300 / 0 MB"
**Root cause**: PDH counter `\GPU Adapter Memory(*)\Dedicated Limit` doesn't exist on
NVIDIA GeForce consumer GPUs (only Quadro/enterprise).

**Fix**: Guard the label: `if mem_total_mb > 0 { "X / Y MB" } else { "X MB" }`.  
Long-term: use DXGI (`Win32_Graphics_Dxgi` feature) to query total VRAM.

### 4. Chart always 1-column (VRAM hidden)
**Root cause**: `chart_cols = if has_power && has_temp { 2 } else { 1 }` — required
BOTH metrics to show VRAM chart.

**Fix**: Always 2-column layout in `draw_chart_body`. Row 1: GPU% | VRAM%. Row 2:
Temp | TGP only when data available.

### 5. Detach shows chart in both windows
**Root cause**: History card was always rendered; `draw_chart_body` called regardless of
`app.chart_detached`.

**Fix**: Wrap History card in `if app.chart_detached { hint + attach-back button } else { chart }`.

### 6. App icon
**Before**: Procedurally-generated 32×32 diamond in code.  
**After**: Rasterized from `data/razer-blade-control.svg` at 64×64 using `resvg 0.47`.

---

## New Features (Session 2025-07 — Synapse Replacement)

### 1. Fn Key Swap — NOT SUPPORTED (removed)
Attempted and removed. Do not re-add without solving the root issues.

**Goal**: Toggle F-keys primary / media-keys primary on Blade 16 2023.

**What was tried**:
1. **EC HID** — Class 0x02, cmd 0x06 (set) / 0x86 (get). EC acknowledges but hardware is
   unaffected on Blade 16 2023 (PID 0x029F). OpenRazer doesn't list `fn_toggle` for this model.
2. **Elevated in-process WH_KEYBOARD_LL in daemon** — Hook sees VK_VOLUME_MUTE etc. and suppresses
   them. But `SendInput(VK_F1)` from elevated daemon is blocked by Windows UIPI before reaching
   non-elevated foreground windows. Key becomes a no-op.
3. **Out-of-process medium-IL helper `razer-kbd-hook.exe`** — spawned via `explorer.exe` launcher
   (to avoid `SE_ASSIGNPRIMARYTOKEN` needed by `CreateProcessWithTokenW`). Daemon ↔ helper via
   TCP 127.0.0.1:29495. Helper installs WH_KEYBOARD_LL at medium IL, `SendInput` should work.
   Tested. Still did not work — exact reason unknown (Razer driver may handle the keys below
   WH_KEYBOARD_LL, or the helper was not receiving them reliably).

**Conclusion**: Not feasible with current knowledge. All fn_swap code removed from codebase.

**If revisiting**:
- Confirm with AutoHotKey: `#IfWinActive` + `Volume_Mute::F1` — if AHK can do it, our hook can too.
- If AHK can also NOT intercept it, Razer driver handles the key at kernel level below user-mode hooks.
- Consider kernel driver (filter driver) or Razer SDK path if they release one.
- Medium-IL helper + WH_KEYBOARD_LL is the correct architecture IF the key reaches user-mode hooks.

### 2. Gaming Mode (Keyboard Hooks)
Block Win key, Alt+Tab, Alt+F4 while gaming using Windows low-level keyboard hooks.
- `SetWindowsHookExW(WH_KEYBOARD_LL)` with `ll_keyboard_proc` callback.
- AtomicBool flags: `BLOCK_WIN_KEY`, `BLOCK_ALT_TAB`, `BLOCK_ALT_F4`.
- Reliable version runs the hook on a dedicated thread with its own Windows message loop and explicit Alt-state tracking.
- The hook ownership was moved behind daemon IPC (`SetGamingMode`) so the hook now lives in the elevated long-lived `razer-daemon` process instead of the GUI process.
- Files: `src/gui/gaming_mode.rs`, `src/comms.rs`, `src/daemon/main.rs`, `src/gui/app.rs`, `src/gui/tabs/power.rs`.

### 3. Battery Refresh Rate Switching
Auto-switch display to lowest available refresh rate on battery, highest on AC.
- `EnumDisplaySettingsW` / `ChangeDisplaySettingsExW` for refresh rate enumeration and switching.
- Triggers on AC ↔ battery transitions detected via `GetSystemPowerStatus`.
- File: `src/gui/display.rs`.

### 4. Low-Battery Lighting Auto-Off
Dim keyboard + logo LED when battery drops below configurable threshold.
- Threshold slider 5–50% (default 20%).
- Restores previous brightness when AC plugged or battery recovers above threshold.
- File: `src/gui/app.rs` (apply_poll).

### 5. Settings Persistence
GUI-only settings persisted to `%APPDATA%\razercontrol\gui.json`.
- Saved: gaming mode flags, battery refresh toggle, low-battery lighting + threshold.
- Restored on startup including re-applying gaming mode through the daemon.
- File: `src/gui/gui_config.rs`.

### 6. System Tray Icon
Context menu with quick access to power profiles, KB brightness, start-on-boot, and exit.
- Uses `tray-icon` 0.19 + `muda` 0.15 crates.
- Menu: Show Window, Power Profile submenu, KB Brightness submenu, Start on boot (CheckMenuItem), Exit.
- File: `src/gui/tray.rs`.

### 7. Start on Boot (Registry)
Toggle auto-start via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- Current exe path written as REG_SZ value "RazerBladeControl".
- File: `src/gui/tray.rs` (toggle_autostart, is_autostart_enabled).

### 8. Dynamic Power Profile Descriptions
Each power mode shows a brief description (Balanced, Gaming, Creator, Silent, Custom).
- File: `src/gui/tabs/power.rs`.

---

## Known Issues / TODOs

- **VRAM total on GeForce** — PDH `Dedicated Limit` unavailable; show just "X MB used" for now.
  Proper fix: add `Win32_Graphics_Dxgi` feature and query adapter memory via DXGI.
- **`nvidia-powerd` equivalent** — Windows enforces TGP via WMI/HID; verify the daemon
  path on fresh installs (no Synapse required once daemon installs WMI provider).
- **Intel XTU undervolting** — Razer uses IntelOverclockingSDK.dll (.NET managed) for PL1/PL2/
  core voltage offset. Too complex to integrate directly; document as future work.
- **Fn key primary on Blade 16 2023** — legacy EC HID packet is not sufficient even with verified `0xFF` transaction_id.
  Synapse persists `functionKeyPrimary` in Chromium IndexedDB under `Default\IndexedDB\https_apps.razer.com_0.indexeddb.leveldb` (observed in `001047.log`), while the cached app bundle references both `functionKeyPrimary` and `bladeNativeAction`.
  Fresh reset logs on 2026-04-09 show the live profile value flips between `functionKeyPrimary:"func"` and `functionKeyPrimary:"multi"` on this model, not `media`.
  The same toggle rewrites the stored top-row `defaultMappings`: scancodes `59-68, 87, 88` move from `hypershift:true` to `hypershift:false`, and `mapping_engine.log` records `localStorageSetItem` plus `deviceModeChangedCallbackEvent ... event[3] json_event[{"isEnabled":true|false}]` immediately around the change.
  Combined with the USB captures on the real Blade HID interface (device address `2`, endpoint `0x82`) showing only inbound report IDs `01` and `04`, this points to a Windows software mapping-engine mode flip layered on top of the HID reports, not a standalone firmware/EC Fn-primary setter.
  `key_cap_3.pcapng` is the strongest capture so far: it contains four repeated top-row passes, and both `multi` and `func` runs emit the same `0x82` HID sequence (`01003a..010045`, `01004c`, plus report-`04` `040b/0400` transitions) with no convincing outbound mode-set packet on the Blade device. Treat Fn-primary as unsupported in the hardware UI until a real Windows remap or proprietary control path is implemented.
  `Logfile1.CSV` from Procmon confirms the userspace side of that theory: `RazerAppEngine.exe` mutates both `Default\Local Storage\leveldb` and `Default\IndexedDB\https_apps.razer.com_0.indexeddb.leveldb` during the sequence, enumerates the `VID_1532&PID_029F` / `RZCONTROL` registry nodes, and loads `User Data\Apps\Common\bladeCommon\bladeNative_v1.0.13.1.dll`; however, this capture still does not expose a clear standalone HID/device write we can reproduce yet.
- **Dual display mode** — Blade 16 supports 1920×1200 ↔ 3840×2400 native OLED switching.
  Requires bladeNative.dll research; deferred.
- **Battery profile** — IPC path works; needs more real-world testing on battery discharge.

---

## Session 2026-04 — GPU display + power + UX fixes

### GPU info not showing (root cause: PDH gatekeeper bug)
**Root cause**: `query_gpu()` used `pdh.as_ref().map_or(false, ...)` — when PDH init failed
(empty OnceLock → None returned), `gpu_active` was false. The final `pdh.map(...)` also
returned None so the cache was never populated.
**Fix**: Two-pronged:
1. `map_or(false, …)` kept (preserves D3-Cold gating) but a new `if pdh.is_none()` branch
   tries nvidia-smi as fallback before giving up. nvidia-smi exits after each query, so it
   cannot hold the GPU awake.
2. `start_gpu_monitor_task()` now does one immediate query before the first 3-second sleep,
   so the cache is populated before the first GUI poll arrives.

### NVML per-poll (no persistent library handle)
**Root cause**: `NVML_HANDLE: OnceLock<Option<Nvml>>` kept the NVML connection open for the
daemon's lifetime. On some Optimus / Advanced-Optimus systems a persistent NVML handle can
prevent the dGPU from entering D3-Cold, causing 20-30 W idle draw.
**Fix**: Removed the OnceLock. `query_nvml()` now calls `Nvml::init()` at the start of each
poll and drops the handle at function return (nvmlShutdown via Drop). Overhead < 5 ms per
poll, negligible vs. the 3-second interval.
**Note**: DWM-caused P0 (NVIDIA Overlay / Advanced-Optimus display path) is a Windows/driver
issue unrelated to the daemon — confirmed by GPU staying at P0 after stopping our binaries.

### GUI "Connecting to daemon…" on load
**Root cause**: `App::ok` starts false; before the first poll result arrives, the central
panel immediately showed "Cannot connect to razer-daemon".
**Fix**: Added `App::first_poll_received: bool` (default false, set true by `apply_poll`).
- `!ok && !first_poll_received` → "Connecting to razer-daemon…" panel
- `!ok && first_poll_received`  → "Cannot connect" error panel
- Sidebar status dot shows amber `...` while connecting.

### UAC manifest for razer-daemon.exe
Added `build.rs` with:
```rust
println!("cargo:rustc-link-arg-bin=razer-daemon=/MANIFESTUAC:level='requireAdministrator'");
```
Windows now auto-prompts for elevation when razer-daemon.exe is launched from Explorer.

### "GPU: cached" label corrected
`src/gui/widgets.rs` showed `"GPU: cached"` for all non-PDH GPU names. The cache is always
fresh (updated every 3 s), so the label was misleading. Changed to `"GPU: live"`.

---

## EC Probe Tool (`tools/ec_probe.py`)

Zero-dependency Python script (pure ctypes, `hid.dll` + `setupapi.dll`).

```powershell
# Stop daemon first — it holds mi_02 exclusively
uv run tools/ec_probe.py --fan-temp          # quick targeted probe
uv run tools/ec_probe.py --dump 0x0D 0x88    # dump single command, all zones
# Full scan: uv run tools/ec_probe.py
```

Results saved to `tools/razer_ec_map.json` + `tools/razer_ec_map.txt`.

### Blade 16 (2023) HID interface layout

| Interface | Status | Notes |
|-----------|--------|-------|
| `mi_00\kbd` | blocked | keyboard driver claims it |
| `mi_00&col03`, `mi_01&colXX` | opens OK | no feature reports; SetFeature fails |
| **`mi_02`** | **EC control** | daemon opens exclusively; supports feature reports |

### Windows HID / ctypes pitfalls (64-bit)
- `SetupDiGetClassDevsW.restype = c_void_p` — **required** on 64-bit; default `c_int` truncates pointer
- `SP_DEVICE_INTERFACE_DATA.Reserved` must be `c_size_t` (8 bytes = ULONG_PTR), not `c_ulong` (4 bytes)
- `SetupDiGetDeviceInterfaceDetailW` third arg = raw `c_void_p` buffer; `cbSize` field = 8 on 64-bit
- Two-step pattern: first call with NULL to get required buffer size, then allocate and retry
- `wstring_at(addr + 4)` reads DevicePath (offset 4 = sizeof DWORD `cbSize`)

---

## Build Commands

```powershell
# Debug build (all binaries)
cargo build

# Release
cargo build --release

# Release validation without touching a locked running daemon binary
cargo build --release --target-dir target-verify

# GUI only (faster iteration)
cargo build --bin razer-gui

# Run daemon (requires admin for HID + WMI)
.\target\debug\razer-daemon.exe
# NOTE: release build has /SUBSYSTEM:WINDOWS so no console window appears.
# Logs go to %APPDATA%\razercontrol\daemon.log

# CLI test
.\target\debug\razer-cli.exe write power ac 2 0 0   # Gaming, no custom boost
.\target\debug\razer-cli.exe read power

```

## Startup (no-console autostart)

`razer-daemon.exe` (release) is built with `/SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup` so
it runs with no console window — exactly like a background service.

**One-time setup** (run from elevated PowerShell after `cargo build --release`):
```powershell
# Register as a Task Scheduler task (at logon, highest privileges)
.\scripts\install-daemon-task.ps1

# Start immediately without rebooting:
Start-ScheduledTask -TaskName "RazerDaemon"
```

Logs: `%APPDATA%\razercontrol\daemon.log`

To view live:
```powershell
Get-Content "$env:APPDATA\razercontrol\daemon.log" -Wait -Tail 30
```

To stop:
```powershell
Stop-ScheduledTask -TaskName "RazerDaemon"  # or kill the process
```

---

## Code Conventions

- `src/gui/widgets.rs`: `row()`, `card()`, `metric_tile()`, `chip()` — zero return from closures,
  capture widget responses via `let mut flag = false; row(ui, ..., |ui| { flag = widget.changed(); })`.
- Color palette constants in `src/gui/constants.rs`:
  `ACCENT` (green), `WARN` (yellow), `ERR` (red), `OK`, `DIM`, `SOFT`, `TEXT`.
  Chart/tile colour constants: `CH_GPU`, `CH_TEMP`, `CH_VRAM`, `CH_POWER` — always use these
  for both chart lines and the corresponding metric tile accent so they stay in sync.
- IPC send helper: `send(DaemonCommand::...)` returns `Option<DaemonResponse>`. Always match;
  push label to `issues: Vec<&str>` on failure; show banner after all widgets.
- Chart fill: use `egui::Shape::Path(egui::epaint::PathShape { closed: true, ... })` — never
  `convex_polygon` for sparklines.
