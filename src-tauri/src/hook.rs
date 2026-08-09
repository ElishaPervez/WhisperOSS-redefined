//! Global low-level keyboard hook. Forwards every key transition (as
//! logical Keys) to the pipeline. If the active combo contains one regular
//! key (e.g. Space in Ctrl+Space), that key is SWALLOWED while all the
//! combo's modifier keys are held — otherwise holding the combo would type
//! into the focused app. Modifier keys are never swallowed.
//! PRIVACY: key events are never logged — only forwarded in memory.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::OnceLock;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
    TranslateMessage, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkey_logic::{key_from_vk, Key, KeyEvent};

static SENDER: OnceLock<Sender<KeyEvent>> = OnceLock::new();

// Suppression state, written by the pipeline, read synchronously in the
// hook. SUPPRESS_VK == 0 means "suppress nothing".
static SUPPRESS_VK: AtomicU32 = AtomicU32::new(0);
static REQUIRED_MODS: AtomicU32 = AtomicU32::new(0);
static MODS_DOWN: AtomicU32 = AtomicU32::new(0);

const CTRL_BIT: u32 = 1;
const WIN_BIT: u32 = 2;
const ALT_BIT: u32 = 4;
const SHIFT_BIT: u32 = 8;

fn modifier_bit(key: Key) -> u32 {
    match key {
        Key::Ctrl => CTRL_BIT,
        Key::Win => WIN_BIT,
        Key::Alt => ALT_BIT,
        Key::Shift => SHIFT_BIT,
        Key::Other(_) => 0,
    }
}

pub fn set_suppression(other_vk: Option<u32>, required_modifiers: &[Key]) {
    let mask = required_modifiers.iter().map(|&k| modifier_bit(k)).fold(0, |a, b| a | b);
    REQUIRED_MODS.store(mask, Ordering::SeqCst);
    SUPPRESS_VK.store(other_vk.unwrap_or(0), Ordering::SeqCst);
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let t = kb.time as u64;
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let up = matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        if down || up {
            let key = key_from_vk(kb.vkCode);

            // Track which modifiers are physically held.
            let bit = modifier_bit(key);
            if bit != 0 {
                if down {
                    MODS_DOWN.fetch_or(bit, Ordering::SeqCst);
                } else {
                    MODS_DOWN.fetch_and(!bit, Ordering::SeqCst);
                }
            }

            let ev = if down {
                KeyEvent::Down(key, t)
            } else {
                KeyEvent::Up(key, t)
            };
            if let Some(tx) = SENDER.get() {
                let _ = tx.send(ev);
            }

            // Swallow the combo's regular key while its modifiers are held.
            let target = SUPPRESS_VK.load(Ordering::SeqCst);
            if target != 0 && kb.vkCode == target {
                let required = REQUIRED_MODS.load(Ordering::SeqCst);
                if MODS_DOWN.load(Ordering::SeqCst) & required == required {
                    return LRESULT(1);
                }
            }
        }
    }
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
