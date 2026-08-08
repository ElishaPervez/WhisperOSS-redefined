//! Milestone-0 synthetic driver: emits fake mic levels so the overlay can be
//! validated without any audio code. Replaced by the real recorder in M2.

use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter};

/// Deterministic pseudo-random 0.0..1.0 (no rand dependency).
fn lcg_next(state: &mut u32) -> f64 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 8) as f64 / 16_777_216.0
}

/// Synthetic mic level: slow sine envelope plus jitter, clamped to 0..=1.
pub fn synth_level(t: f64, jitter: f64) -> f64 {
    let envelope = 0.35 + 0.35 * (t * 1.7).sin() + 0.2 * (t * 0.4).sin();
    (envelope + 0.25 * (jitter - 0.5)).clamp(0.0, 1.0)
}

/// Emit `"level"` events at 30 Hz forever.
pub fn spawn_demo(app: AppHandle) {
    thread::spawn(move || {
        let mut t = 0.0_f64;
        let mut rng = 0x2026_0808_u32;
        loop {
            t += 1.0 / 30.0;
            let level = synth_level(t, lcg_next(&mut rng));
            let _ = app.emit("level", level);
            thread::sleep(Duration::from_millis(33));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_level_stays_in_unit_range() {
        let mut rng = 1u32;
        for i in 0..10_000 {
            let j = lcg_next(&mut rng);
            let v = synth_level(i as f64 * 0.033, j);
            assert!((0.0..=1.0).contains(&v), "out of range at {i}: {v}");
        }
    }
}
