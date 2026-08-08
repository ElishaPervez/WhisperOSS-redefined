//! Pure hold-to-dictate logic, driven by timestamped key events from the
//! OS hook (Task 5). Recording starts the instant both keys are down — the
//! 0.5 s pre-roll covers anything earlier. A release before MIN_HOLD_MS
//! counts as an accidental tap and cancels (spec §5).

pub const MIN_HOLD_MS: u64 = 150;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyEvent {
    CtrlDown(u64),
    CtrlUp(u64),
    WinDown(u64),
    WinUp(u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Start,
    Finish { held_ms: u64 },
    Cancel,
}

pub struct HoldTracker {
    ctrl: bool,
    win: bool,
    started_at: Option<u64>,
}

impl HoldTracker {
    pub fn new() -> Self {
        Self { ctrl: false, win: false, started_at: None }
    }

    pub fn on_event(&mut self, ev: KeyEvent) -> Action {
        match ev {
            KeyEvent::CtrlDown(t) => {
                self.ctrl = true;
                self.maybe_start(t)
            }
            KeyEvent::WinDown(t) => {
                self.win = true;
                self.maybe_start(t)
            }
            KeyEvent::CtrlUp(t) => {
                self.ctrl = false;
                self.finish_if_active(t)
            }
            KeyEvent::WinUp(t) => {
                self.win = false;
                self.finish_if_active(t)
            }
        }
    }

    fn maybe_start(&mut self, t: u64) -> Action {
        if self.ctrl && self.win && self.started_at.is_none() {
            self.started_at = Some(t);
            Action::Start
        } else {
            Action::None
        }
    }

    fn finish_if_active(&mut self, t: u64) -> Action {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_both_then_release_finishes() {
        let mut t = HoldTracker::new();
        assert_eq!(t.on_event(KeyEvent::CtrlDown(0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::WinDown(10)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::CtrlUp(400)),
                   Action::Finish { held_ms: 390 });
    }

    #[test]
    fn reverse_press_order_also_works() {
        let mut t = HoldTracker::new();
        assert_eq!(t.on_event(KeyEvent::WinDown(0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::CtrlDown(5)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::WinUp(300)),
                   Action::Finish { held_ms: 295 });
    }

    #[test]
    fn tap_shorter_than_150ms_cancels() {
        let mut t = HoldTracker::new();
        t.on_event(KeyEvent::CtrlDown(0));
        assert_eq!(t.on_event(KeyEvent::WinDown(20)), Action::Start);
        assert_eq!(t.on_event(KeyEvent::WinUp(100)), Action::Cancel);
    }

    #[test]
    fn key_repeat_does_not_double_start() {
        let mut t = HoldTracker::new();
        t.on_event(KeyEvent::CtrlDown(0));
        assert_eq!(t.on_event(KeyEvent::WinDown(10)), Action::Start);
        // Windows auto-repeats key-down while held:
        assert_eq!(t.on_event(KeyEvent::CtrlDown(50)), Action::None);
        assert_eq!(t.on_event(KeyEvent::WinDown(60)), Action::None);
        assert_eq!(t.on_event(KeyEvent::CtrlUp(500)),
                   Action::Finish { held_ms: 490 });
    }

    #[test]
    fn single_key_never_starts() {
        let mut t = HoldTracker::new();
        assert_eq!(t.on_event(KeyEvent::WinDown(0)), Action::None);
        assert_eq!(t.on_event(KeyEvent::WinUp(500)), Action::None);
    }

    #[test]
    fn second_release_after_finish_is_ignored() {
        let mut t = HoldTracker::new();
        t.on_event(KeyEvent::CtrlDown(0));
        t.on_event(KeyEvent::WinDown(10));
        assert!(matches!(t.on_event(KeyEvent::CtrlUp(400)), Action::Finish { .. }));
        assert_eq!(t.on_event(KeyEvent::WinUp(450)), Action::None);
    }
}
