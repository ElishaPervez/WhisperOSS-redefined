//! Global low-level keyboard hook (spec §4): real key-down/key-up events,
//! no polling, no third-party hotkey library. The hook runs on a dedicated
//! thread with its own message loop, does minimal work, and never blocks —
//! Windows silently removes hooks that are slow.

use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
    TranslateMessage, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkey_logic::KeyEvent;

static SENDER: OnceLock<Sender<KeyEvent>> = OnceLock::new();

// Virtual-key codes: generic/left/right Ctrl, left/right Win.
const VK_CONTROL: u32 = 0x11;
const VK_LCONTROL: u32 = 0xA2;
const VK_RCONTROL: u32 = 0xA3;
const VK_LWIN: u32 = 0x5B;
const VK_RWIN: u32 = 0x5C;

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let t = kb.time as u64;
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        let ev = match kb.vkCode {
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL if down => Some(KeyEvent::CtrlDown(t)),
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL if up => Some(KeyEvent::CtrlUp(t)),
            VK_LWIN | VK_RWIN if down => Some(KeyEvent::WinDown(t)),
            VK_LWIN | VK_RWIN if up => Some(KeyEvent::WinUp(t)),
            _ => None,
        };
        if let (Some(ev), Some(tx)) = (ev, SENDER.get()) {
            let _ = tx.send(ev);
        }
    }
    // Never swallow keys — Ctrl and Win must keep working normally.
    CallNextHookEx(None, code, wparam, lparam)
}

pub fn spawn(tx: Sender<KeyEvent>) {
    SENDER.set(tx).expect("hook::spawn called twice");
    std::thread::spawn(|| unsafe {
        let _hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)
            .expect("failed to install keyboard hook");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
