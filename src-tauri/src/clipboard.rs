//! Privacy paste (spec §4). The transcript is staged with DELAYED RENDERING:
//! we put a promise (not the text) on the clipboard, plus three formats that
//! keep it out of Win+V history and the cloud clipboard. When the target app
//! pastes, Windows asks us to render (WM_RENDERFORMAT) — that message IS the
//! paste confirmation, which makes restore sequenced instead of a timer.
//! Restore only happens if we still own the clipboard, so a user copy in
//! between is never clobbered (fixes v1's bug).
//!
//! The snapshot carries every HGLOBAL-backed format (text, images, copied
//! files, rich text). Oversized clipboards fall back to text only.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardOwner, GetOpenClipboardWindow, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClassNameW, GetForegroundWindow,
    GetMessageW, GetWindowTextW, GetWindowThreadProcessId, PostMessageW, RegisterClassW,
    HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_RENDERFORMAT, WNDCLASSW,
};

use crate::applog;

const CF_UNICODETEXT: u32 = 13;
const WM_STAGE: u32 = WM_APP + 1;
const WM_RESTORE: u32 = WM_APP + 2;

static PENDING: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static RESTORE_TO: Mutex<Option<Snapshot>> = Mutex::new(None);
static STAGE_OK: AtomicBool = AtomicBool::new(false);
static STAGE_DONE: AtomicBool = AtomicBool::new(false);
static RENDERED: AtomicBool = AtomicBool::new(false);
static OWNER_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn to_utf16z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn describe_window(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return "none".to_string();
    }
    let mut pid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    let mut title_buf = [0u16; 128];
    let title_len = unsafe { GetWindowTextW(hwnd, &mut title_buf) };
    let title = if title_len > 0 {
        String::from_utf16_lossy(&title_buf[..title_len as usize])
    } else {
        String::new()
    };
    let mut class_buf = [0u16; 64];
    let class_len = unsafe { GetClassNameW(hwnd, &mut class_buf) };
    let class_name = if class_len > 0 {
        String::from_utf16_lossy(&class_buf[..class_len as usize])
    } else {
        String::new()
    };
    let exe = if pid != 0 {
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut path_buf = [0u16; 512];
                let mut size = path_buf.len() as u32;
                let res = QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_FORMAT(0),
                    PWSTR(path_buf.as_mut_ptr()),
                    &mut size,
                );
                let _ = CloseHandle(handle);
                if res.is_ok() && size > 0 {
                    let full_path = String::from_utf16_lossy(&path_buf[..size as usize]);
                    std::path::Path::new(&full_path)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or(full_path)
                } else {
                    format!("pid-{pid}")
                }
            } else {
                format!("pid-{pid}(access-denied)")
            }
        }
    } else {
        "no-pid".to_string()
    };
    format!("hwnd={:?} pid={} exe={:?} class={:?} title={:?}", hwnd.0, pid, exe, class_name, title)
}

pub fn describe_clipboard_blocker(context: &str, last_err: u32) -> String {
    let open_hwnd = unsafe { GetOpenClipboardWindow().unwrap_or_default() };
    let owner_hwnd = unsafe { GetClipboardOwner().unwrap_or_default() };
    let fg_hwnd = unsafe { GetForegroundWindow() };

    let open_info = describe_window(open_hwnd);
    let owner_info = describe_window(owner_hwnd);
    let fg_info = describe_window(fg_hwnd);

    format!(
        "clipboard-lock-failed ctx={context} win32_err={last_err} open_window=({open_info}) owner_window=({owner_info}) foreground=({fg_info})"
    )
}

const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;
/// A clipboard bigger than this is not cloned into memory; text survives,
/// the rest is let go. Stops a copied video from doubling its RAM.
const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

/// Everything on the user's clipboard that can be put back after our paste.
pub struct Snapshot {
    /// (format id, raw bytes) — only formats whose data lives in an HGLOBAL.
    entries: Vec<(u32, Vec<u8>)>,
}

/// Formats worth carrying across a paste. The four standard ones are byte
/// buffers; 0xC000 and up are app-registered names (HTML, RTF, drop effects)
/// and are byte buffers by convention. Everything else is either synthesized
/// by Windows from a kept format or is a handle that cannot be copied.
fn should_snapshot(format: u32) -> bool {
    matches!(format, CF_UNICODETEXT | CF_DIB | CF_HDROP | CF_DIBV5) || format >= 0xC000
}

/// Copy bytes into an HGLOBAL and put it on the (already open) clipboard.
unsafe fn write_format(format: u32, bytes: &[u8]) -> bool {
    let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
    let ptr = GlobalLock(hmem);
    if ptr.is_null() {
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
    let _ = GlobalUnlock(hmem);
    SetClipboardData(format, Some(HANDLE(hmem.0))).is_ok()
}

/// Copy text into an HGLOBAL and put it on the (already open) clipboard.
unsafe fn set_unicode_text(text: &[u16]) -> bool {
    let bytes = std::slice::from_raw_parts(text.as_ptr() as *const u8, text.len() * 2);
    write_format(CF_UNICODETEXT, bytes)
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
            let err = GetLastError().0;
            let msg = format!("privacy-format-reg-failed name={name} win32_err={err}");
            applog::log(&msg);
            eprintln!("[WhisperOSS Clipboard Warning] {msg}");
            return false;
        }
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, 4) else {
            applog::log(&format!("privacy-format-alloc-failed name={name}"));
            return false;
        };
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            applog::log(&format!("privacy-format-lock-failed name={name}"));
            return false;
        }
        std::ptr::copy_nonoverlapping(value.to_le_bytes().as_ptr(), ptr as *mut u8, 4);
        let _ = GlobalUnlock(hmem);
        if let Err(err) = SetClipboardData(id, Some(HANDLE(hmem.0))) {
            let msg = format!("privacy-format-set-failed name={name} win32_err={err}");
            applog::log(&msg);
            eprintln!("[WhisperOSS Clipboard Warning] {msg}");
            return false;
        }
    }
    true
}

unsafe fn open_clipboard_retrying(hwnd: Option<HWND>, context: &str) -> bool {
    // Another app may hold the clipboard briefly; retry 60 x 10 ms (as v1).
    let mut last_err = 0u32;
    for _ in 0..60 {
        if OpenClipboard(hwnd).is_ok() {
            return true;
        }
        last_err = GetLastError().0;
        std::thread::sleep(Duration::from_millis(10));
    }
    let diag = describe_clipboard_blocker(context, last_err);
    applog::log(&diag);
    eprintln!("[WhisperOSS Clipboard Error] {diag}");
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
                if !open_clipboard_retrying(Some(hwnd), "stage") {
                    break 'stage false;
                }
                let empty_ok = EmptyClipboard().is_ok();
                if !empty_ok {
                    let err = GetLastError().0;
                    let msg = format!("stage-empty-clipboard-failed win32_err={err}");
                    applog::log(&msg);
                    eprintln!("[WhisperOSS Clipboard Warning] {msg}");
                }
                // NULL handle = delayed rendering. The call reports an
                // error for NULL by design — ignore its return value.
                let _ = SetClipboardData(CF_UNICODETEXT, None);
                let priv_ok = set_privacy_formats();
                let _ = CloseClipboard();
                empty_ok && priv_ok
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
                let owner_info = describe_window(owner);
                applog::log(&format!("clipboard-restore-skipped-not-owner current_owner=({owner_info})"));
                return LRESULT(0);
            }
            if open_clipboard_retrying(Some(hwnd), "restore") {
                let _ = EmptyClipboard();
                if let Some(snap) = RESTORE_TO.lock().unwrap().take() {
                    for (format, bytes) in &snap.entries {
                        let _ = write_format(*format, bytes);
                    }
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

/// Bytes of one HGLOBAL-backed clipboard format. The clipboard must be open.
unsafe fn read_format(format: u32) -> Option<Vec<u8>> {
    let handle = GetClipboardData(format).ok()?;
    let hglobal = HGLOBAL(handle.0);
    let size = GlobalSize(hglobal);
    if size == 0 {
        return None;
    }
    let ptr = GlobalLock(hglobal) as *const u8;
    if ptr.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
    let _ = GlobalUnlock(hglobal);
    Some(bytes)
}

/// Copy every restorable format off the clipboard. None means there is
/// nothing we can put back. An oversized clipboard keeps only its text.
pub fn snapshot() -> Option<Snapshot> {
    unsafe {
        if !open_clipboard_retrying(None, "snapshot") {
            return None;
        }
        let mut entries: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut total = 0usize;
        let mut oversized = false;
        let mut format = EnumClipboardFormats(0);
        while format != 0 {
            if should_snapshot(format) {
                if let Some(bytes) = read_format(format) {
                    total += bytes.len();
                    if total > MAX_SNAPSHOT_BYTES {
                        oversized = true;
                    } else {
                        entries.push((format, bytes));
                    }
                }
            }
            format = EnumClipboardFormats(format);
        }
        let _ = CloseClipboard();
        if oversized {
            applog::log("clipboard-snapshot-oversized-keeping-text-only");
            entries.retain(|(f, _)| *f == CF_UNICODETEXT);
        }
        if entries.is_empty() {
            None
        } else {
            Some(Snapshot { entries })
        }
    }
}

/// Stage `text` for a privacy paste. Returns false if the privacy formats
/// could not be set — the caller MUST abort the paste in that case.
pub fn stage(text: &str, restore_to: Option<Snapshot>) -> bool {
    *PENDING.lock().unwrap() = Some(to_utf16z(text));
    *RESTORE_TO.lock().unwrap() = restore_to;
    RENDERED.store(false, Ordering::SeqCst);
    STAGE_DONE.store(false, Ordering::SeqCst);
    post(WM_STAGE);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !STAGE_DONE.load(Ordering::SeqCst) {
        if Instant::now() > deadline {
            let msg = "paste-aborted-stage-wndproc-timeout-2s";
            applog::log(msg);
            eprintln!("[WhisperOSS Clipboard Error] {msg}");
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let ok = STAGE_OK.load(Ordering::SeqCst);
    if !ok {
        let msg = "paste-aborted-stage-flag-false";
        applog::log(msg);
        eprintln!("[WhisperOSS Clipboard Error] {msg}");
    }
    ok
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

    #[test]
    fn snapshot_format_filter() {
        // kept: the four standard formats whose bytes can be copied
        assert!(should_snapshot(13)); // Unicode text
        assert!(should_snapshot(8));  // DIB image (screenshots)
        assert!(should_snapshot(17)); // DIBv5 image
        assert!(should_snapshot(15)); // copied files (HDROP)
        // kept: everything app-registered (HTML, RTF, drop effects)
        assert!(should_snapshot(0xC000));
        assert!(should_snapshot(0xC123));
        // dropped: synthesized or handle-based formats
        assert!(!should_snapshot(1));      // ANSI text — synthesized from Unicode
        assert!(!should_snapshot(2));      // bitmap — a GDI handle, not bytes
        assert!(!should_snapshot(3));      // metafile
        assert!(!should_snapshot(14));     // enhanced metafile
        assert!(!should_snapshot(16));     // locale — synthesized
        assert!(!should_snapshot(0x0083)); // owner-display range
    }

    #[test]
    fn test_clipboard_lifecycle() {
        init();
        let snap = snapshot();
        let staged = stage("hello test", snap);
        assert!(staged, "Delayed rendering stage should succeed");
        restore();
    }
}
