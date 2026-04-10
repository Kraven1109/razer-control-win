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
                   • Always query PDH (passive, no GPU wake)
                   • Gate: VRAM > 256 MB OR temp > 0 °C → dGPU awake
                   • If awake: call nvidia-smi for full telemetry
                   • If sleeping (D3-Cold): sanitize PDH, zero util/power/clocks,
                     set name = "NVIDIA GPU (Sleeping)"
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

### 1. Fn Key Swap (EC HID)
Toggle between F-keys primary and media-keys primary via EC HID.
- Class 0x02, cmd 0x06 (set) / 0x86 (get).
- Legacy OpenRazer-compatible packet uses transaction_id `0xFF`.
- Daemon IPC: `SetFnSwap { swap: bool }` / `GetFnSwap()`.
- The daemon now treats the write as successful only if a read-back matches the requested state.
- On Blade 16 2023 the EC may still ACK the legacy packet without changing keyboard behaviour; this strongly suggests Synapse uses a newer proprietary path for `functionKeyPrimary` on this model.
- Files: `device.rs` (get_fn_swap/set_fn_swap), `comms.rs`, `daemon/main.rs`.

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
- **PDH handle caching** — `query_pdh_gpu()` currently opens a fresh PDH query, registers
  counters, and sleeps 250 ms every 3 s poll cycle.  For lower CPU overhead, cache the
  `PDH_HQUERY` and `PDH_HCOUNTER` handles in a `lazy_static` struct and call only
  `PdhCollectQueryData` each cycle (open/close once at startup, not per-poll).  Low priority
  since the daemon idles between polls.
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

# CLI test
.\target\debug\razer-cli.exe write power ac 2 0 0   # Gaming, no custom boost
.\target\debug\razer-cli.exe read power
.\target\debug\razer-cli.exe read fn-swap
.\target\debug\razer-cli.exe write fn-swap on
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
