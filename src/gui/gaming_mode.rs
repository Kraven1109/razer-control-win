/// Gaming Mode — low-level keyboard hook to block Win key, Alt+Tab, Alt+F4.
///
/// Runs the `WH_KEYBOARD_LL` hook on a dedicated thread with its own message
/// loop so shell shortcuts are intercepted reliably.

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::mpsc;
#[cfg(windows)]
use std::thread;

#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, KBDLLHOOKSTRUCT, MSG,
    PM_NOREMOVE, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_F4, VK_LMENU, VK_LWIN, VK_MENU, VK_RMENU, VK_RWIN, VK_TAB,
};
#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentThreadId;

// ── Shared state (accessed from hook callback) ─────────────────────────────

static BLOCK_WIN_KEY: AtomicBool = AtomicBool::new(false);
static BLOCK_ALT_TAB: AtomicBool = AtomicBool::new(false);
static BLOCK_ALT_F4: AtomicBool = AtomicBool::new(false);
static ALT_HELD: AtomicBool = AtomicBool::new(false);
static SUPPRESS_ALT_UP: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
struct HookThread {
    join: thread::JoinHandle<()>,
    thread_id: u32,
}

#[cfg(windows)]
static HOOK_THREAD: std::sync::Mutex<Option<HookThread>> = std::sync::Mutex::new(None);

// ── Public API ──────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn is_win_key_blocked() -> bool {
    BLOCK_WIN_KEY.load(Ordering::Relaxed)
}
#[allow(dead_code)]
pub fn is_alt_tab_blocked() -> bool {
    BLOCK_ALT_TAB.load(Ordering::Relaxed)
}
#[allow(dead_code)]
pub fn is_alt_f4_blocked() -> bool {
    BLOCK_ALT_F4.load(Ordering::Relaxed)
}

/// Returns true if any gaming-mode block is active.
#[allow(dead_code)]
pub fn is_any_active() -> bool {
    BLOCK_WIN_KEY.load(Ordering::Relaxed)
        || BLOCK_ALT_TAB.load(Ordering::Relaxed)
        || BLOCK_ALT_F4.load(Ordering::Relaxed)
}

/// Update which keys are blocked. Installs the hook if at least one block is
/// on, removes it when all are off.
pub fn set_blocks(win_key: bool, alt_tab: bool, alt_f4: bool) {
    BLOCK_WIN_KEY.store(win_key, Ordering::Relaxed);
    BLOCK_ALT_TAB.store(alt_tab, Ordering::Relaxed);
    BLOCK_ALT_F4.store(alt_f4, Ordering::Relaxed);

    if win_key || alt_tab || alt_f4 {
        install_hook();
    } else {
        remove_hook();
    }
}

// ── Hook install / remove ───────────────────────────────────────────────────

#[cfg(windows)]
fn install_hook() {
    let mut guard = HOOK_THREAD.lock().unwrap();
    if guard.is_some() {
        return;
    }

    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let join = thread::spawn(move || {
        let mut msg = MSG::default();
        unsafe {
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        }

        let thread_id = unsafe { GetCurrentThreadId() };
        let module = unsafe { GetModuleHandleW(PCWSTR::null()) }
            .map(|handle| windows::Win32::Foundation::HINSTANCE(handle.0))
            .unwrap_or_default();
        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), module, 0) };

        match hook {
            Ok(hook) => {
                let _ = ready_tx.send(Ok(thread_id));
                loop {
                    let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                    if result.0 <= 0 {
                        break;
                    }
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                unsafe {
                    let _ = UnhookWindowsHookEx(hook);
                }
            }
            Err(err) => {
                let _ = ready_tx.send(Err(err.to_string()));
            }
        }
    });

    match ready_rx.recv() {
        Ok(Ok(thread_id)) => {
            *guard = Some(HookThread { join, thread_id });
        }
        Ok(Err(err)) => {
            let _ = join.join();
            eprintln!("Failed to install keyboard hook: {err}");
        }
        Err(err) => {
            let _ = join.join();
            eprintln!("Keyboard hook thread failed to initialize: {err}");
        }
    }
}

#[cfg(windows)]
fn remove_hook() {
    let mut guard = HOOK_THREAD.lock().unwrap();
    if let Some(hook) = guard.take() {
        unsafe {
            let _ = PostThreadMessageW(hook.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = hook.join.join();
    }
    ALT_HELD.store(false, Ordering::Relaxed);
    SUPPRESS_ALT_UP.store(false, Ordering::Relaxed);
}

#[cfg(not(windows))]
fn install_hook() {}
#[cfg(not(windows))]
fn remove_hook() {}

// ── Low-level keyboard hook callback ────────────────────────────────────────

#[cfg(windows)]
fn alt_is_down() -> bool {
    ALT_HELD.load(Ordering::Relaxed)
        || unsafe { (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 }
}

#[cfg(windows)]
unsafe extern "system" fn ll_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(info.vkCode as u16);
        let msg = wparam.0 as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

        if vk == VK_MENU || vk == VK_LMENU || vk == VK_RMENU {
            if is_down {
                ALT_HELD.store(true, Ordering::Relaxed);
            } else if is_up {
                ALT_HELD.store(false, Ordering::Relaxed);
                if SUPPRESS_ALT_UP.swap(false, Ordering::Relaxed) {
                    return LRESULT(1);
                }
            }
        }

        // Start opens on the Win key gesture itself, so swallow every Win event.
        if BLOCK_WIN_KEY.load(Ordering::Relaxed)
            && (vk == VK_LWIN || vk == VK_RWIN)
        {
            return LRESULT(1);
        }

        let alt_held = alt_is_down() || (info.flags.0 & 0x20 != 0);

        if BLOCK_ALT_TAB.load(Ordering::Relaxed) && alt_held && vk == VK_TAB
            && (is_down || is_up)
        {
            if is_down {
                SUPPRESS_ALT_UP.store(true, Ordering::Relaxed);
            }
            return LRESULT(1);
        }

        if BLOCK_ALT_F4.load(Ordering::Relaxed) && alt_held && vk == VK_F4
            && (is_down || is_up)
        {
            if is_down {
                SUPPRESS_ALT_UP.store(true, Ordering::Relaxed);
            }
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
