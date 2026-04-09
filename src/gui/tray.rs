/// System tray icon with context menu.
///
/// Provides quick access to power profiles, keyboard brightness, and app controls
/// without opening the main window.

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, CheckMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, Icon};
use std::sync::mpsc;

/// Actions the tray menu can generate.
#[derive(Debug, Clone)]
pub enum TrayAction {
    ShowWindow,
    SetProfile(u8),           // 0=Balanced, 1=Gaming, 2=Creator, 3=Silent
    SetBrightness(u8),        // 0, 25, 50, 75, 100
    ToggleStartOnBoot,
    Exit,
}

/// Builds the tray icon and context menu. Returns a receiver for menu actions.
/// Must be called from a thread with a Windows message pump.
pub fn create_tray() -> (TrayIcon, mpsc::Receiver<TrayAction>) {
    let (tx, rx) = mpsc::channel();

    let icon = load_icon();

    // Build menu
    let menu = Menu::new();

    let show_item = MenuItem::new("Show Window", true, None);
    let _ = menu.append(&show_item);
    let _ = menu.append(&PredefinedMenuItem::separator());

    let profile_sub = Submenu::new("Power Profile", true);
    let balanced = MenuItem::new("Balanced", true, None);
    let gaming = MenuItem::new("Gaming", true, None);
    let creator = MenuItem::new("Creator", true, None);
    let silent = MenuItem::new("Silent", true, None);
    let _ = profile_sub.append(&balanced);
    let _ = profile_sub.append(&gaming);
    let _ = profile_sub.append(&creator);
    let _ = profile_sub.append(&silent);
    let _ = menu.append(&profile_sub);

    let brightness_sub = Submenu::new("KB Brightness", true);
    let br_off = MenuItem::new("Off", true, None);
    let br_25 = MenuItem::new("25%", true, None);
    let br_50 = MenuItem::new("50%", true, None);
    let br_75 = MenuItem::new("75%", true, None);
    let br_100 = MenuItem::new("100%", true, None);
    let _ = brightness_sub.append(&br_off);
    let _ = brightness_sub.append(&br_25);
    let _ = brightness_sub.append(&br_50);
    let _ = brightness_sub.append(&br_75);
    let _ = brightness_sub.append(&br_100);
    let _ = menu.append(&brightness_sub);

    let _ = menu.append(&PredefinedMenuItem::separator());

    let boot_item = CheckMenuItem::new("Start on boot", true, false, None);
    // Check current autostart state.
    let autostart_enabled = is_autostart_enabled();
    boot_item.set_checked(autostart_enabled);
    let _ = menu.append(&boot_item);

    let _ = menu.append(&PredefinedMenuItem::separator());
    let exit_item = MenuItem::new("Exit", true, None);
    let _ = menu.append(&exit_item);

    // Build tray icon
    let tray = TrayIconBuilder::new()
        .with_icon(icon)
        .with_tooltip("Razer Blade Control")
        .with_menu(Box::new(menu))
        .build()
        .expect("Failed to create tray icon");

    // Spawn menu event listener on a background thread.
    let show_id = show_item.id().clone();
    let balanced_id = balanced.id().clone();
    let gaming_id = gaming.id().clone();
    let creator_id = creator.id().clone();
    let silent_id = silent.id().clone();
    let br_off_id = br_off.id().clone();
    let br_25_id = br_25.id().clone();
    let br_50_id = br_50.id().clone();
    let br_75_id = br_75.id().clone();
    let br_100_id = br_100.id().clone();
    let boot_id = boot_item.id().clone();
    let exit_id = exit_item.id().clone();

    std::thread::spawn(move || {
        loop {
            if let Ok(event) = MenuEvent::receiver().recv() {
                let action = if event.id == show_id {
                    Some(TrayAction::ShowWindow)
                } else if event.id == balanced_id {
                    Some(TrayAction::SetProfile(0))
                } else if event.id == gaming_id {
                    Some(TrayAction::SetProfile(1))
                } else if event.id == creator_id {
                    Some(TrayAction::SetProfile(2))
                } else if event.id == silent_id {
                    Some(TrayAction::SetProfile(3))
                } else if event.id == br_off_id {
                    Some(TrayAction::SetBrightness(0))
                } else if event.id == br_25_id {
                    Some(TrayAction::SetBrightness(25))
                } else if event.id == br_50_id {
                    Some(TrayAction::SetBrightness(50))
                } else if event.id == br_75_id {
                    Some(TrayAction::SetBrightness(75))
                } else if event.id == br_100_id {
                    Some(TrayAction::SetBrightness(100))
                } else if event.id == boot_id {
                    Some(TrayAction::ToggleStartOnBoot)
                } else if event.id == exit_id {
                    Some(TrayAction::Exit)
                } else {
                    None
                };
                if let Some(a) = action {
                    if tx.send(a).is_err() {
                        break; // receiver dropped
                    }
                }
            }
        }
    });

    (tray, rx)
}

fn load_icon() -> Icon {
    const SVG: &[u8] = include_bytes!("../../data/razer-blade-control.svg");
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(SVG, &options).expect("icon SVG parse failed");
    let size = 32u32;
    let sx = size as f32 / tree.size().width();
    let sy = size as f32 / tree.size().height();
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size, size).expect("pixmap alloc");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    Icon::from_rgba(pixmap.data().to_vec(), size, size).expect("icon from rgba")
}

// ── Autostart (registry) ────────────────────────────────────────────────────

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const APP_NAME: &str = "RazerBladeControl";

#[cfg(windows)]
pub fn is_autostart_enabled() -> bool {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ,
    };
    let key = HSTRING::from(RUN_KEY);
    let val = HSTRING::from(APP_NAME);
    let mut buf = [0u16; 512];
    let mut len = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &val,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
        .is_ok()
    }
}

#[cfg(windows)]
pub fn toggle_autostart() {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
    };

    if is_autostart_enabled() {
        // Remove
        let key = HSTRING::from(RUN_KEY);
        let val = HSTRING::from(APP_NAME);
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &val);
        }
    } else {
        // Add — use the current executable path
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let key = HSTRING::from(RUN_KEY);
        let val = HSTRING::from(APP_NAME);
        let wide: Vec<u16> = exe.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &key,
                &val,
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                (wide.len() * 2) as u32,
            );
        }
    }
}

#[cfg(not(windows))]
pub fn is_autostart_enabled() -> bool { false }
#[cfg(not(windows))]
pub fn toggle_autostart() {}
