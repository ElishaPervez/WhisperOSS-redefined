//! Combo-aware hold-to-dictate logic. The combo comes from config
//! ("ctrl"+"win" by default; user-rebindable in M3c). Rules: at least two
//! keys, at least one modifier, at most one non-modifier — enforced by
//! parse_combo so an invalid config can never produce a broken tracker.
//! Recording starts when EVERY combo key is down; the first release of any
//! combo key finishes (or cancels under MIN_HOLD_MS — accidental tap).

pub const MIN_HOLD_MS: u64 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Ctrl,
    Win,
    Alt,
    Shift,
    Other(u32),
}

impl Key {
    pub fn is_modifier(&self) -> bool {
        !matches!(self, Key::Other(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyEvent {
    Down(Key, u64),
    Up(Key, u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Start,
    Finish { held_ms: u64 },
    Cancel,
}

/// Collapse left/right/generic virtual-key variants into one logical key.
pub fn key_from_vk(vk: u32) -> Key {
    match vk {
        0x11 | 0xA2 | 0xA3 => Key::Ctrl,
        0x5B | 0x5C => Key::Win,
        0x12 | 0xA4 | 0xA5 => Key::Alt,
        0x10 | 0xA0 | 0xA1 => Key::Shift,
        other => Key::Other(other),
    }
}

fn key_from_name(name: &str) -> Option<Key> {
    match name {
        "ctrl" => Some(Key::Ctrl),
        "win" => Some(Key::Win),
        "alt" => Some(Key::Alt),
        "shift" => Some(Key::Shift),
        "space" => Some(Key::Other(0x20)),
        "tab" => Some(Key::Other(0x09)),
        "capslock" => Some(Key::Other(0x14)),
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap().to_ascii_uppercase();
            if c.is_ascii_uppercase() || c.is_ascii_digit() {
                Some(Key::Other(c as u32))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Validate and parse a combo from config names. None = invalid combo.
pub fn parse_combo(names: &[String]) -> Option<Vec<Key>> {
    let keys: Option<Vec<Key>> = names.iter().map(|n| key_from_name(n)).collect();
    let keys = keys?;
    let modifiers = keys.iter().filter(|k| k.is_modifier()).count();
    let others = keys.len() - modifiers;
    if keys.len() >= 2 && modifiers >= 1 && others <= 1 {
        Some(keys)
    } else {
        None
    }
}

/// The regular key the OS hook must swallow while the combo is held, if any.
pub fn combo_other_vk(combo: &[Key]) -> Option<u32> {
    combo.iter().find_map(|k| match k {
        Key::Other(vk) => Some(*vk),
        _ => None,
    })
}

/// How long the app will listen for a new combo before giving up. While it
/// listens it swallows every key on the machine, so this is a hard ceiling,
/// not a preference.
#[allow(dead_code)]
pub const CAPTURE_TIMEOUT_MS: u64 = 6_000;

const VK_ESCAPE: u32 = 0x1B;

fn rank(k: &Key) -> u8 {
    match k {
        Key::Ctrl => 0,
        Key::Win => 1,
        Key::Alt => 2,
        Key::Shift => 3,
        Key::Other(_) => 4,
    }
}

/// One fixed order for every combo, so pressing Space-then-Ctrl and
/// Ctrl-then-Space save and display identically.
#[allow(dead_code)]
pub fn canonical(mut keys: Vec<Key>) -> Vec<Key> {
    keys.sort_by_key(rank);
    keys
}

fn name_of(k: &Key) -> Option<String> {
    Some(match k {
        Key::Ctrl => "ctrl".into(),
        Key::Win => "win".into(),
        Key::Alt => "alt".into(),
        Key::Shift => "shift".into(),
        Key::Other(0x20) => "space".into(),
        Key::Other(0x09) => "tab".into(),
        Key::Other(0x14) => "capslock".into(),
        Key::Other(vk) => {
            let c = char::from_u32(*vk)?;
            if c.is_ascii_uppercase() || c.is_ascii_digit() {
                c.to_ascii_lowercase().to_string()
            } else {
                return None;
            }
        }
    })
}

/// Inverse of key_from_name. None when any key has no config name (F-keys,
/// media keys): such a combo cannot be persisted, so it must be refused at
/// capture time rather than silently lost on the next launch.
#[allow(dead_code)]
pub fn combo_names(keys: &[Key]) -> Option<Vec<String>> {
    keys.iter().map(name_of).collect()
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum Capture {
    /// Keys held so far — repaint the preview.
    Pending(Vec<Key>),
    /// A usable combo, in canonical order. Persist and apply it.
    Done(Vec<Key>),
    /// Two or more keys that cannot work as a hotkey. Keep the old one.
    Invalid,
    Cancelled,
}

/// Watches keys during a rebind. The combo is whatever is held down when the
/// user lets go of the first key.
#[allow(dead_code)]
pub struct CaptureBuffer {
    keys: Vec<Key>,
}

#[allow(dead_code)]
impl CaptureBuffer {
    pub fn new() -> Self {
        CaptureBuffer { keys: Vec::new() }
    }

    pub fn on_event(&mut self, ev: KeyEvent) -> Capture {
        match ev {
            KeyEvent::Down(Key::Other(VK_ESCAPE), _) => Capture::Cancelled,
            KeyEvent::Down(key, _) => {
                if !self.keys.contains(&key) {
                    self.keys.push(key);
                }
                Capture::Pending(canonical(self.keys.clone()))
            }
            KeyEvent::Up(_, _) => {
                if self.keys.len() < 2 {
                    // A stray tap of one key: forget it and keep listening.
                    self.keys.clear();
                    return Capture::Pending(Vec::new());
                }
                let keys = canonical(std::mem::take(&mut self.keys));
                match combo_names(&keys) {
                    Some(names) if parse_combo(&names).is_some() => Capture::Done(keys),
                    _ => Capture::Invalid,
                }
            }
        }
    }
}

pub struct HoldTracker {
    combo: Vec<Key>,
    down: Vec<bool>,
    started_at: Option<u64>,
}

impl HoldTracker {
    pub fn new(combo: Vec<Key>) -> Self {
        let n = combo.len();
        Self { combo, down: vec![false; n], started_at: None }
    }

    fn index_of(&self, key: Key) -> Option<usize> {
        self.combo.iter().position(|&k| k == key)
    }

    pub fn on_event(&mut self, ev: KeyEvent) -> Action {
        match ev {
            KeyEvent::Down(key, t) => {
                let Some(i) = self.index_of(key) else { return Action::None };
                self.down[i] = true;
                if self.down.iter().all(|&d| d) && self.started_at.is_none() {
                    self.started_at = Some(t);
                    Action::Start
                } else {
                    Action::None
                }
            }
            KeyEvent::Up(key, t) => {
                let Some(i) = self.index_of(key) else { return Action::None };
                self.down[i] = false;
                match self.started_at.take() {
                    None => Action::None,
                    Some(s) => {
                        let held_ms = t.saturating_sub(s);
                        if held_ms >= MIN_HOLD_MS {
                            Action::Finish { held_ms }
                        } else {
                            Action::Cancel
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_win() -> Vec<Key> {
        parse_combo(&["ctrl".into(), "win".into()]).unwrap()
    }

    #[test]
    fn parse_combo_accepts_and_rejects_per_rules() {
        assert!(parse_combo(&["ctrl".into(), "win".into()]).is_some());
        assert!(parse_combo(&["ctrl".into(), "alt".into(), "shift".into()]).is_some());
        assert!(parse_combo(&["ctrl".into(), "space".into()]).is_some());
        assert!(parse_combo(&["alt".into(), "d".into()]).is_some());
        // rejected: single key, no modifier, two non-modifiers, unknown name
        assert!(parse_combo(&["ctrl".into()]).is_none());
        assert!(parse_combo(&["a".into(), "b".into()]).is_none());
        assert!(parse_combo(&["ctrl".into(), "a".into(), "b".into()]).is_none());
        assert!(parse_combo(&["ctrl".into(), "banana".into()]).is_none());
    }

    #[test]
    fn key_names_map_to_expected_keys() {
        assert_eq!(parse_combo(&["ctrl".into(), "space".into()]).unwrap()[1],
                   Key::Other(0x20));
        assert_eq!(parse_combo(&["win".into(), "d".into()]).unwrap()[1],
                   Key::Other(0x44));
    }

    #[test]
    fn vk_variants_collapse_to_one_key() {
        assert_eq!(key_from_vk(0x11), Key::Ctrl);
        assert_eq!(key_from_vk(0xA2), Key::Ctrl);
        assert_eq!(key_from_vk(0xA3), Key::Ctrl);
        assert_eq!(key_from_vk(0x5B), Key::Win);
        assert_eq!(key_from_vk(0x5C), Key::Win);
        assert_eq!(key_from_vk(0x12), Key::Alt);
        assert_eq!(key_from_vk(0xA0), Key::Shift);
        assert_eq!(key_from_vk(0x20), Key::Other(0x20));
    }

    #[test]
    fn combo_other_vk_extraction() {
        assert_eq!(combo_other_vk(&ctrl_win()), None);
        let c = parse_combo(&["ctrl".into(), "space".into()]).unwrap();
        assert_eq!(combo_other_vk(&c), Some(0x20));
    }

    #[test]
    fn default_combo_full_cycle() {
        let mut t = HoldTracker::new(ctrl_win());
        assert_eq!(t.on_event(KeyEvent::Down(Key::Ctrl, 0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::Down(Key::Win, 10)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::Up(Key::Ctrl, 400)),
                   Action::Finish { held_ms: 390 });
        // release of the second key after finish: ignored
        assert_eq!(t.on_event(KeyEvent::Up(Key::Win, 450)), Action::None);
    }

    #[test]
    fn short_tap_cancels() {
        let mut t = HoldTracker::new(ctrl_win());
        t.on_event(KeyEvent::Down(Key::Win, 0));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Ctrl, 20)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::Up(Key::Ctrl, 100)), Action::Cancel);
    }

    #[test]
    fn key_repeat_does_not_double_start() {
        let mut t = HoldTracker::new(ctrl_win());
        t.on_event(KeyEvent::Down(Key::Ctrl, 0));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Win, 10)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::Down(Key::Ctrl, 50)), Action::None);
        assert!(matches!(t.on_event(KeyEvent::Up(Key::Win, 500)),
                         Action::Finish { .. }));
    }

    #[test]
    fn unrelated_keys_do_not_disturb_hold() {
        let mut t = HoldTracker::new(ctrl_win());
        t.on_event(KeyEvent::Down(Key::Ctrl, 0));
        t.on_event(KeyEvent::Down(Key::Win, 10));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Other(0x41), 50)), Action::None);
        assert_eq!(t.on_event(KeyEvent::Up(Key::Other(0x41), 90)), Action::None);
        assert!(matches!(t.on_event(KeyEvent::Up(Key::Ctrl, 400)),
                         Action::Finish { .. }));
    }

    #[test]
    fn three_key_combo_requires_all_three() {
        let c = parse_combo(&["ctrl".into(), "alt".into(), "space".into()]).unwrap();
        let mut t = HoldTracker::new(c);
        t.on_event(KeyEvent::Down(Key::Ctrl, 0));
        assert_eq!(t.on_event(KeyEvent::Down(Key::Other(0x20), 5)), Action::None);
        assert_eq!(t.on_event(KeyEvent::Down(Key::Alt, 10)), Action::Start);
        assert!(matches!(t.on_event(KeyEvent::Up(Key::Other(0x20), 400)),
                         Action::Finish { .. }));
    }

    fn down(k: Key) -> KeyEvent { KeyEvent::Down(k, 0) }
    fn up(k: Key) -> KeyEvent { KeyEvent::Up(k, 0) }

    #[test]
    fn canonical_order_is_press_order_independent() {
        let a = canonical(vec![Key::Other(0x20), Key::Ctrl]);
        let b = canonical(vec![Key::Ctrl, Key::Other(0x20)]);
        assert_eq!(a, b);
        assert_eq!(a, vec![Key::Ctrl, Key::Other(0x20)]);
        assert_eq!(
            canonical(vec![Key::Shift, Key::Win, Key::Ctrl]),
            vec![Key::Ctrl, Key::Win, Key::Shift]
        );
    }

    #[test]
    fn combo_names_maps_every_supported_key() {
        assert_eq!(
            combo_names(&[Key::Ctrl, Key::Win, Key::Alt, Key::Shift]).unwrap(),
            vec!["ctrl", "win", "alt", "shift"]
        );
        assert_eq!(combo_names(&[Key::Other(0x20)]).unwrap(), vec!["space"]);
        assert_eq!(combo_names(&[Key::Other(0x09)]).unwrap(), vec!["tab"]);
        assert_eq!(combo_names(&[Key::Other(0x14)]).unwrap(), vec!["capslock"]);
        assert_eq!(combo_names(&[Key::Other(0x44)]).unwrap(), vec!["d"]);
        assert_eq!(combo_names(&[Key::Other(0x35)]).unwrap(), vec!["5"]);
    }

    #[test]
    fn combo_names_rejects_keys_with_no_config_name() {
        // F1 has no name in key_from_name, so it can never be persisted.
        assert!(combo_names(&[Key::Ctrl, Key::Other(0x70)]).is_none());
    }

    #[test]
    fn combo_names_round_trips_through_parse_combo() {
        for input in [
            vec!["ctrl".to_string(), "win".to_string()],
            vec!["ctrl".to_string(), "space".to_string()],
            vec!["alt".to_string(), "shift".to_string(), "d".to_string()],
        ] {
            let keys = parse_combo(&input).unwrap();
            assert_eq!(combo_names(&canonical(keys)).unwrap(), input);
        }
    }

    #[test]
    fn capture_completes_on_first_release() {
        let mut c = CaptureBuffer::new();
        assert_eq!(c.on_event(down(Key::Ctrl)), Capture::Pending(vec![Key::Ctrl]));
        assert_eq!(
            c.on_event(down(Key::Win)),
            Capture::Pending(vec![Key::Ctrl, Key::Win])
        );
        assert_eq!(
            c.on_event(up(Key::Ctrl)),
            Capture::Done(vec![Key::Ctrl, Key::Win])
        );
    }

    #[test]
    fn capture_is_press_order_independent() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Other(0x20)));
        c.on_event(down(Key::Ctrl));
        assert_eq!(
            c.on_event(up(Key::Other(0x20))),
            Capture::Done(vec![Key::Ctrl, Key::Other(0x20)])
        );
    }

    #[test]
    fn capture_ignores_key_repeat() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        c.on_event(down(Key::Ctrl));
        c.on_event(down(Key::Win));
        assert_eq!(
            c.on_event(up(Key::Win)),
            Capture::Done(vec![Key::Ctrl, Key::Win])
        );
    }

    #[test]
    fn escape_cancels_capture() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        assert_eq!(c.on_event(down(Key::Other(0x1B))), Capture::Cancelled);
    }

    #[test]
    fn stray_single_tap_keeps_listening() {
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        assert_eq!(c.on_event(up(Key::Ctrl)), Capture::Pending(Vec::new()));
        // still usable afterwards
        c.on_event(down(Key::Alt));
        c.on_event(down(Key::Other(0x44)));
        assert_eq!(
            c.on_event(up(Key::Alt)),
            Capture::Done(vec![Key::Alt, Key::Other(0x44)])
        );
    }

    #[test]
    fn unusable_combos_are_rejected() {
        // two regular keys, no modifier
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Other(0x41)));
        c.on_event(down(Key::Other(0x42)));
        assert_eq!(c.on_event(up(Key::Other(0x41))), Capture::Invalid);
        // a key that cannot be written to config.json
        let mut c = CaptureBuffer::new();
        c.on_event(down(Key::Ctrl));
        c.on_event(down(Key::Other(0x70)));
        assert_eq!(c.on_event(up(Key::Ctrl)), Capture::Invalid);
    }
}
