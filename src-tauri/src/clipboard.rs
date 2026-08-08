//! Privacy paste (spec §4). The transcript is staged with DELAYED RENDERING:
//! we put a promise (not the text) on the clipboard, plus three formats that
//! keep it out of Win+V history and the cloud clipboard. When the target app
//! pastes, Windows asks us to render (WM_RENDERFORMAT) — that message IS the
//! paste confirmation, which makes restore sequenced instead of a timer.
//! Restore only happens if we still own the clipboard, so a user copy in
//! between is never clobbered (fixes v1's bug).
//!
//! M1 limitation (documented in the plan header): snapshot/restore is plain
//! text only. A non-text clipboard (image, files) is logged and not restored.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
    OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
    PostMessageW, RegisterClassW, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_RENDERFORMAT, WNDCLASSW,
};

use crate::applog;

const CF_UNICODETEXT: u32 = 13;
const WM_STAGE: u32 = WM_APP + 1;
const WM_RESTORE: u32 = WM_APP + 2;

static PENDING: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static RESTORE_TO: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static STAGE_OK: AtomicBool = AtomicBool::new(false);
static STAGE_DONE: AtomicBool = AtomicBool::new(false);
static RENDERED: AtomicBool = AtomicBool::new(false);
static OWNER_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn to_utf16z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copy text into an HGLOBAL and put it on the (already open) clipboard.
unsafe fn set_unicode_text(text: &[u16]) -> bool {
    let bytes = text.len() * 2;
    let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) else { return false };
    let ptr = GlobalLock(hmem);
    if ptr.is_null() {
        return false;
    }
    std::ptr::copy_nonoverlapping(text.as_ptr() as *const u8, ptr as *mut u8, bytes);
    let _ = GlobalUnlock(hmem);
    SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hmem.0))).is_ok()
}

unsafe fn set_privacy_formats() -> bool {
    // Format name → DWORD value. Presence of the first alone excludes the
    // entry from clipboard monitors; the other two must be 0 (spec §4 / v1).
    let formats: [(&str, u32); 3] = [
        ("ExcludeClipboardContentFromMonitorProcessing", 0),
        ("CanIncludeInClipboardHistory", 0),
        ("CanUploadToCloudClipboard", 0),
    ];
    for (name, value) in formats {
        let wname = to_utf16z(name);
        let id = RegisterClipboardFormatW(PCWSTR(wname.as_ptr()));
        if id == 0 {
            return false;
        }
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, 4) else { return false };
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), ptr as *mut u8, 4);
        let _ = GlobalUnlock(hmem);
        if SetClipboardData(id, Some(HANDLE(hmem.0))).is_err() {
            return false;
        }
    }
    true
}

unsafe fn open_clipboard_retrying(hwnd: HWND) -> bool {
    // Another app may hold the clipboard briefly; retry 60 x 10 ms (as v1).
    for _ in 0..60 {
        if OpenClipboard(Some(hwnd)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_RENDERFORMAT => {
            // The target app is pasting RIGHT NOW. The clipboard is already
            // open for us here — SetClipboardData directly, no OpenClipboard.
            if let Some(text) = PENDING.lock().unwrap().clone() {
                let _ = set_unicode_text(&text);
            }
            RENDERED.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_STAGE => {
            let ok = 'stage: {
                if !open_clipboard_retrying(hwnd) {
                    break 'stage false;
                }
                let ok = EmptyClipboard().is_ok()
                    // NULL handle = delayed rendering. The call reports an
                    // error for NULL by design — ignore its return value.
                    && { let _ = SetClipboardData(CF_UNICODETEXT, None); true }
                    && set_privacy_formats();
                let _ = CloseClipboard();
                ok
            };
            STAGE_OK.store(ok, Ordering::SeqCst);
            STAGE_DONE.store(true, Ordering::SeqCst);
            LRESULT(0)
        }
        WM_RESTORE => {
            let owner = GetClipboardOwner().unwrap_or_default();
            if owner.0 as isize != OWNER_HWND.load(Ordering::SeqCst) {
                // Someone else owns the clipboard now (user copied something
                // since our paste) — leave it alone.
                applog::log("clipboard-restore-skipped-not-owner");
                return LRESULT(0);
            }
            if open_clipboard_retrying(hwnd) {
                let _ = EmptyClipboard();
                if let Some(prev) = RESTORE_TO.lock().unwrap().take() {
                    let _ = set_unicode_text(&prev);
                }
                let _ = CloseClipboard();
                applog::log("clipboard-restored");
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Spawn the hidden clipboard-owner window and its message loop. Call once.
pub fn init() {
    std::thread::spawn(|| unsafe {
        let class_name = to_utf16z("WhisperOSSClipboard");
        let hinstance = GetModuleHandleW(None).expect("module handle");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WINDOW_STYLE(0),
            0, 0, 0, 0,
            Some(HWND_MESSAGE), // message-only window: invisible, no taskbar
            None,
            Some(hinstance.into()),
            None,
        )
        .expect("clipboard window");
        OWNER_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
    // Give the window a moment to exist before first use.
    for _ in 0..100 {
        if OWNER_HWND.load(Ordering::SeqCst) != 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn post(msg: u32) {
    let hwnd = HWND(OWNER_HWND.load(Ordering::SeqCst) as *mut _);
    unsafe {
        let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
    }
}

/// Read current clipboard text (None if empty or non-text).
pub fn snapshot_text() -> Option<String> {
    unsafe {
        let hwnd = HWND(OWNER_HWND.load(Ordering::SeqCst) as *mut _);
        if !open_clipboard_retrying(hwnd) {
            return None;
        }
        let result = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let ptr = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(HGLOBAL(h.0));
            Some(s)
        });
        let _ = CloseClipboard();
        result
    }
}

/// Stage `text` for a privacy paste. Returns false if the privacy formats
/// could not be set — the caller MUST abort the paste in that case.
pub fn stage(text: &str, restore_to: Option<String>) -> bool {
    *PENDING.lock().unwrap() = Some(to_utf16z(text));
    *RESTORE_TO.lock().unwrap() = restore_to.map(|s| to_utf16z(&s));
    RENDERED.store(false, Ordering::SeqCst);
    STAGE_DONE.store(false, Ordering::SeqCst);
    post(WM_STAGE);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !STAGE_DONE.load(Ordering::SeqCst) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    STAGE_OK.load(Ordering::SeqCst)
}

/// True once the target app has actually pulled our text (WM_RENDERFORMAT).
pub fn wait_pasted(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if RENDERED.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

pub fn restore() {
    post(WM_RESTORE);
}

/// Synthetic Ctrl+V into the focused app.
pub fn send_ctrl_v() {
    fn key(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
    let inputs = [
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYBD_EVENT_FLAGS(0)),
        key(VK_V, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16z_is_null_terminated_utf16() {
        let v = to_utf16z("Hi ✓");
        assert_eq!(v.last(), Some(&0u16));
        assert_eq!(String::from_utf16_lossy(&v[..v.len() - 1]), "Hi ✓");
    }
}
