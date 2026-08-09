//! Combo-aware hold-to-dictate logic. The combo is fixed at Ctrl+Win and read
//! from config at startup; there is no rebind UI. Rules: at least two keys, at
//! least one modifier, at most one non-modifier — enforced by parse_combo so an
//! invalid config can never produce a broken tracker.
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

}
