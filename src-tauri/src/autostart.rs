//! Start-with-Windows via the per-user Run key (spec §4). Per-user only —
//! no admin rights involved. reconcile() runs at startup and after the
//! toggle changes (M3b) so the registry always matches the config.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegDeleteValueW, RegGetValueW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE,
    REG_SZ, RRF_RT_REG_SZ,
};

use crate::applog;
use crate::clipboard::to_utf16z;

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "WhisperOSS";

fn open_run_key(_access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    unsafe {
        let mut hkey = HKEY::default();
        let path = to_utf16z(RUN_KEY);
        let ok = RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), &mut hkey);
        if ok.is_ok() { Some(hkey) } else { None }
    }
}

pub(crate) fn set_run_value(name: &str, command: &str) {
    unsafe {
        if let Some(hkey) = open_run_key(KEY_SET_VALUE) {
            let wname = to_utf16z(name);
            let wval = to_utf16z(command);
            let bytes = std::slice::from_raw_parts(
                wval.as_ptr() as *const u8,
                wval.len() * 2,
            );
            let _ = RegSetValueExW(hkey, PCWSTR(wname.as_ptr()), None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(hkey);
        }
    }
}

pub(crate) fn query_run_value(name: &str) -> Option<String> {
    unsafe {
        let path = to_utf16z(RUN_KEY);
        let wname = to_utf16z(name);
        let mut len: u32 = 0;
        let probe = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            PCWSTR(wname.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut len),
        );
        if probe.is_err() || len == 0 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize) / 2 + 1];
        let mut len2 = len;
        let ok = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            PCWSTR(wname.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len2),
        );
        if ok.is_err() {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

pub(crate) fn remove_run_value(name: &str) {
    unsafe {
        if let Some(hkey) = open_run_key(KEY_SET_VALUE) {
            let wname = to_utf16z(name);
            let _ = RegDeleteValueW(hkey, PCWSTR(wname.as_ptr()));
            let _ = RegCloseKey(hkey);
        }
    }
}

pub fn is_enabled() -> bool {
    query_run_value(VALUE_NAME).is_some()
}

/// Make the registry match the config. Called at startup and on toggle.
pub fn reconcile(enabled: bool) {
    if enabled {
        if let Ok(exe) = std::env::current_exe() {
            set_run_value(VALUE_NAME, &format!("\"{}\"", exe.display()));
            applog::log("autostart-enabled");
        }
    } else if is_enabled() {
        remove_run_value(VALUE_NAME);
        applog::log("autostart-disabled");
    }
    let _ = KEY_READ; // feature-used marker; remove if unused warning appears
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Integration test against the real per-user registry, using a
    /// throwaway value name so the real "WhisperOSS" entry is untouched.
    #[test]
    fn set_query_remove_roundtrip() {
        const TEST_NAME: &str = "WhisperOSSAutostartTest";
        set_run_value(TEST_NAME, "\"C:\\test\\fake.exe\"");
        assert_eq!(query_run_value(TEST_NAME).as_deref(), Some("\"C:\\test\\fake.exe\""));
        remove_run_value(TEST_NAME);
        assert_eq!(query_run_value(TEST_NAME), None);
    }
}
