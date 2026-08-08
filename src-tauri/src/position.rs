//! Pill placement math. All inputs/outputs are PHYSICAL pixels; the spec's
//! sizes (pill 120x36, margin 26) are logical and get multiplied by the
//! monitor scale factor here.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub const PILL_LOGICAL_W: f64 = 120.0;
pub const PILL_LOGICAL_H: f64 = 36.0;
pub const TASKBAR_MARGIN_LOGICAL: f64 = 26.0;

/// Top-left position for the pill: horizontally centred in `work_area`,
/// bottom edge `26 * scale` px above the work-area bottom (the work area
/// already excludes the taskbar).
pub fn pill_position(work_area: Rect, scale: f64) -> (i32, i32) {
    let pw = (PILL_LOGICAL_W * scale).round() as i32;
    let ph = (PILL_LOGICAL_H * scale).round() as i32;
    let margin = (TASKBAR_MARGIN_LOGICAL * scale).round() as i32;
    let x = work_area.x + (work_area.width as i32 - pw) / 2;
    let y = work_area.y + work_area.height as i32 - ph - margin;
    (x, y)
}

/// Half-open containment: the right/bottom edges are outside.
pub fn contains(r: Rect, px: f64, py: f64) -> bool {
    px >= r.x as f64
        && py >= r.y as f64
        && px < r.x as f64 + r.width as f64
        && py < r.y as f64 + r.height as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centres_pill_at_100_percent_scale() {
        // 1920x1040 work area (1080 minus a 40px taskbar):
        // pill 120x36, margin 26 → x=(1920-120)/2=900, y=1040-36-26=978
        let wa = Rect { x: 0, y: 0, width: 1920, height: 1040 };
        assert_eq!(pill_position(wa, 1.0), (900, 978));
    }

    #[test]
    fn scales_pill_and_margin_at_150_percent() {
        // pill 180x54, margin 39 → x=(2560-180)/2=1190, y=1352-54-39=1259
        let wa = Rect { x: 0, y: 0, width: 2560, height: 1352 };
        assert_eq!(pill_position(wa, 1.5), (1190, 1259));
    }

    #[test]
    fn respects_monitor_origin_offset() {
        // second monitor to the right and slightly lower
        let wa = Rect { x: 1920, y: 200, width: 1920, height: 1040 };
        assert_eq!(pill_position(wa, 1.0), (2820, 1178));
    }

    #[test]
    fn contains_point_inside_and_outside() {
        let r = Rect { x: 10, y: 10, width: 100, height: 50 };
        assert!(contains(r, 10.0, 10.0));
        assert!(contains(r, 109.9, 59.9));
        assert!(!contains(r, 110.0, 60.0));
        assert!(!contains(r, 5.0, 30.0));
    }
}
