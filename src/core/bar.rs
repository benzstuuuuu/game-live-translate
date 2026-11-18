//! Stream bar on /overlay (FR-STR-01, 03): where the Thai text sits on the
//! 1920×1080 frame OBS streams. Stored per profile as `bar_x`, `bar_y`,
//! `bar_w`, fractions 0–1 of that frame; `y` is the top of the bar.

use super::obs::Region;
use serde::{Deserialize, Serialize};

/// Two lines of 64 px text at 88 px line height, as a fraction of 1080.
pub const HEIGHT: f64 = 2.0 * 88.0 / 1080.0;
/// Narrowest bar: a fifth of the frame, about six Thai words at 64 px.
pub const MIN_W: f64 = 0.2;
/// Above the default dialogue box when a profile has neither bar nor box.
pub const DEFAULT: Bar = Bar {
    x: 0.1,
    y: 0.62,
    w: 0.8,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub x: f64,
    pub y: f64,
    pub w: f64,
}

impl Bar {
    /// The same bar made whole: at least `MIN_W` wide, and both lines inside
    /// the frame.
    pub fn clamped(self) -> Self {
        let w = self.w.clamp(MIN_W, 1.0);
        Self {
            x: self.x.clamp(0.0, 1.0 - w),
            y: self.y.clamp(0.0, 1.0 - HEIGHT),
            w,
        }
    }

    /// Just above the subtitle box, centred on it and as wide as it
    /// (FR-STR-03).
    pub fn above(region: Region) -> Self {
        let w = region.w.max(MIN_W);
        Self {
            x: region.x + (region.w - w) / 2.0,
            y: region.y - HEIGHT,
            w,
        }
        .clamped()
    }

    /// The bar a profile uses: the stored one, else above its subtitle box,
    /// else `DEFAULT`. Profiles made in /setup start with no bar.
    pub fn of_profile(stored: Option<Bar>, region: Option<Region>) -> Self {
        match (stored, region) {
            (Some(bar), _) => bar.clamped(),
            (None, Some(region)) => Self::above(region),
            (None, None) => DEFAULT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x: f64, y: f64, w: f64) -> Bar {
        Bar { x, y, w }
    }

    fn close(a: Bar, e: Bar) -> bool {
        (a.x - e.x).abs() < 1e-9 && (a.y - e.y).abs() < 1e-9 && (a.w - e.w).abs() < 1e-9
    }

    #[test]
    fn height_is_two_lines_of_88_px() {
        assert!((HEIGHT * 1080.0 - 176.0).abs() < 1e-9);
    }

    #[test]
    fn clamped_keeps_a_good_bar() {
        assert_eq!(DEFAULT.clamped(), DEFAULT);
        let edge = b(0.0, 1.0 - HEIGHT, 1.0);
        assert!(close(edge.clamped(), edge));
    }

    #[test]
    fn clamped_moves_bar_back_inside_the_frame() {
        assert!(close(
            b(0.5, 0.95, 0.8).clamped(),
            b(0.2, 1.0 - HEIGHT, 0.8)
        ));
        assert!(close(b(-0.1, -0.2, 0.5).clamped(), b(0.0, 0.0, 0.5)));
    }

    #[test]
    fn clamped_keeps_width_between_min_and_frame() {
        assert!(close(b(0.4, 0.5, 0.01).clamped(), b(0.4, 0.5, MIN_W)));
        assert!(close(b(0.3, 0.5, 1.4).clamped(), b(0.0, 0.5, 1.0)));
    }

    #[test]
    fn above_sits_on_top_of_the_box_as_wide_as_it() {
        let region = Region {
            x: 0.125,
            y: 0.74,
            w: 0.75,
            h: 0.16,
        };
        assert!(close(Bar::above(region), b(0.125, 0.74 - HEIGHT, 0.75)));
    }

    #[test]
    fn above_a_box_at_the_top_stays_inside() {
        let region = Region {
            x: 0.45,
            y: 0.05,
            w: 0.1,
            h: 0.1,
        };
        assert!(close(Bar::above(region), b(0.4, 0.0, MIN_W)));
    }

    #[test]
    fn profile_bar_prefers_stored_then_box_then_default() {
        let region = Region {
            x: 0.2,
            y: 0.7,
            w: 0.6,
            h: 0.1,
        };
        let stored = b(0.1, 0.1, 0.5);
        assert_eq!(Bar::of_profile(Some(stored), Some(region)), stored);
        assert!(close(
            Bar::of_profile(None, Some(region)),
            b(0.2, 0.7 - HEIGHT, 0.6)
        ));
        assert_eq!(Bar::of_profile(None, None), DEFAULT);
    }

    #[test]
    fn stored_bar_is_clamped_on_read() {
        assert!(close(
            Bar::of_profile(Some(b(0.9, 0.9, 0.5)), None),
            b(0.5, 1.0 - HEIGHT, 0.5)
        ));
    }

    #[test]
    fn json_is_x_y_w() {
        let json = serde_json::to_value(DEFAULT).unwrap();
        assert_eq!(json, serde_json::json!({ "x": 0.1, "y": 0.62, "w": 0.8 }));
        let back: Bar = serde_json::from_value(json).unwrap();
        assert_eq!(back, DEFAULT);
    }
}
