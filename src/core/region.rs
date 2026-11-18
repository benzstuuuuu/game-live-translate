//! Subtitle box maths for the picker on /setup (FR-REG-02 to 06): keep a
//! dragged box inside the frame, convert between fractions and pixels, and
//! tell when the box cuts through dialogue.

use super::obs::{Crop, Region};

/// Smallest box side, as a fraction of the frame.
pub const MIN_SIDE: f64 = 0.01;
/// Extra width `Region::around` looks at on each side, as a fraction.
const AROUND_SIDE: f64 = 0.05;
/// Vision boxes run loose: a line may poke this share of its height past an
/// edge and still count as inside.
const LOOSE: f64 = 0.25;
/// Profile boxes are fractions of a 16:9 frame (FR-REG-04): height / width.
const FRAME_H_PER_W: f64 = 9.0 / 16.0;

impl Region {
    /// The same box made whole: each side at least `MIN_SIDE` and at most the
    /// frame, moved back inside the frame if it hangs over an edge.
    pub fn clamped(self) -> Self {
        let w = self.w.clamp(MIN_SIDE, 1.0);
        let h = self.h.clamp(MIN_SIDE, 1.0);
        Self {
            x: self.x.clamp(0.0, 1.0 - w),
            y: self.y.clamp(0.0, 1.0 - h),
            w,
            h,
        }
    }

    /// `self` is relative to `outer` (fractions of `outer`); the same box as
    /// fractions of the whole frame.
    pub fn within(self, outer: Region) -> Self {
        Self {
            x: outer.x + self.x * outer.w,
            y: outer.y + self.y * outer.h,
            w: self.w * outer.w,
            h: self.h * outer.h,
        }
    }

    /// A Vision bounding box (origin bottom-left) with the origin top-left.
    pub fn from_vision(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self {
            x,
            y: 1.0 - y - h,
            w,
            h,
        }
    }

    /// The box plus its own height above and below and a little to each
    /// side, inside the frame: where dialogue the box cuts off would be.
    pub fn around(self) -> Self {
        let left = (self.x - AROUND_SIDE).max(0.0);
        let top = (self.y - self.h).max(0.0);
        let right = (self.x + self.w + AROUND_SIDE).min(1.0);
        let bottom = (self.y + 2.0 * self.h).min(1.0);
        Self {
            x: left,
            y: top,
            w: right - left,
            h: bottom - top,
        }
    }

    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }
}

impl Crop {
    /// The fractions of `canvas` this crop cuts out; `Crop::of` of the
    /// result is this crop again.
    pub fn region(&self, canvas: (u32, u32)) -> Region {
        let (cw, ch) = (f64::from(canvas.0.max(1)), f64::from(canvas.1.max(1)));
        Region {
            x: f64::from(self.left) / cw,
            y: f64::from(self.top) / ch,
            w: f64::from(self.cx) / cw,
            h: f64::from(self.cy) / ch,
        }
    }
}

/// `size` scaled to fit inside `max`, keeping its aspect ratio, as OBS does
/// for GetSourceScreenshot. Never smaller than 1×1.
pub fn fit(size: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    let (w, h) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
    let scale = (f64::from(max.0) / w).min(f64::from(max.1) / h);
    let side = |v: f64| ((v * scale).round() as u32).max(1);
    (side(w), side(h))
}

/// A side of the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Sides of `region` that cut text off: a line crosses that side, or (top
/// and bottom) sits less than its own height outside it, like the second line
/// of dialogue below a box that is too short. `lines` are text boxes found
/// around `region`, as fractions of the frame. Sorted, no repeats.
pub fn cut_edges(region: Region, lines: &[Region]) -> Vec<Edge> {
    let r = region;
    let mut out = Vec::new();
    for l in lines {
        let tol_y = LOOSE * l.h;
        // The same number of pixels across, on a 16:9 frame.
        let tol_x = tol_y * FRAME_H_PER_W;
        let over_x = l.x < r.right() && l.right() > r.x;
        let over_y = l.y < r.bottom() && l.bottom() > r.y;
        if over_x && l.y < r.y - tol_y && l.bottom() > r.y - l.h {
            out.push(Edge::Top);
        }
        if over_x && l.bottom() > r.bottom() + tol_y && l.y < r.bottom() + l.h {
            out.push(Edge::Bottom);
        }
        if over_y && l.x < r.x - tol_x && l.right() > r.x {
            out.push(Edge::Left);
        }
        if over_y && l.right() > r.right() + tol_x && l.x < r.right() {
            out.push(Edge::Right);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// What to tell the player under the live OCR text; None when the box reads
/// whole lines. `read_any` is whether OCR found text inside the box.
pub fn hint(read_any: bool, cut: &[Edge]) -> Option<String> {
    let names: Vec<&str> = cut
        .iter()
        .map(|e| match e {
            Edge::Top => "ขอบบน",
            Edge::Bottom => "ขอบล่าง",
            Edge::Left => "ขอบซ้าย",
            Edge::Right => "ขอบขวา",
        })
        .collect();
    match names.split_last() {
        Some((last, [])) => Some(format!("ข้อความขาดที่{last} ลาก{last}ออกให้คลุมทั้งบรรทัด")),
        Some((last, rest)) => {
            let sides = format!("{}และ{last}", rest.join(" "));
            Some(format!("ข้อความขาดที่{sides} ขยายกรอบออกทางนั้นให้คลุมทั้งบรรทัด"))
        }
        None if read_any => None,
        None => Some("ยังไม่เห็นข้อความในกรอบ ลากกรอบให้คลุมบรรทัดบทพูด".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Region {
        Region { x, y, w, h }
    }

    fn close(a: Region, b: Region) -> bool {
        [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h]
            .iter()
            .all(|d| d.abs() < 1e-9)
    }

    #[test]
    fn clamped_keeps_a_good_box() {
        let b = r(0.125, 0.74, 0.75, 0.16);
        assert_eq!(b.clamped(), b);
    }

    #[test]
    fn clamped_moves_box_back_inside_frame() {
        assert!(close(
            r(0.9, 0.95, 0.3, 0.1).clamped(),
            r(0.7, 0.9, 0.3, 0.1)
        ));
        assert!(close(
            r(-0.2, -0.1, 0.5, 0.2).clamped(),
            r(0.0, 0.0, 0.5, 0.2)
        ));
    }

    #[test]
    fn clamped_limits_size() {
        assert!(close(
            r(0.3, 0.3, 2.0, 0.0).clamped(),
            r(0.0, 0.3, 1.0, MIN_SIDE)
        ));
        assert!(close(
            r(0.5, 0.5, -1.0, 1.5).clamped(),
            r(0.5, 0.0, MIN_SIDE, 1.0)
        ));
    }

    #[test]
    fn crop_and_region_convert_both_ways() {
        let canvas = (1920, 1080);
        // Default box: 180 px tall on 1080p (FR-REG-04).
        let b = r(0.125, 0.74, 0.75, 180.0 / 1080.0);
        let c = Crop::of(b, canvas);
        assert_eq!(
            c,
            Crop {
                left: 240,
                top: 799,
                cx: 1440,
                cy: 180
            }
        );
        let back = c.region(canvas);
        assert!((back.x - b.x).abs() < 1.0 / 1920.0);
        assert!((back.y - b.y).abs() < 1.0 / 1080.0);
        assert!(close(
            back,
            r(240.0 / 1920.0, 799.0 / 1080.0, 0.75, 180.0 / 1080.0)
        ));
        assert_eq!(Crop::of(back, canvas), c);
    }

    #[test]
    fn crop_round_trips_on_other_canvases() {
        for canvas in [(1280, 720), (2560, 1440), (1920, 1200)] {
            for c in [
                Crop {
                    left: 0,
                    top: 0,
                    cx: canvas.0,
                    cy: canvas.1,
                },
                Crop {
                    left: 17,
                    top: 403,
                    cx: 999,
                    cy: 131,
                },
                Crop {
                    left: canvas.0 - 1,
                    top: canvas.1 - 1,
                    cx: 1,
                    cy: 1,
                },
            ] {
                assert_eq!(Crop::of(c.region(canvas), canvas), c, "{canvas:?} {c:?}");
            }
        }
    }

    #[test]
    fn within_maps_inner_box_to_frame() {
        let outer = r(0.1, 0.6, 0.8, 0.4);
        assert!(close(r(0.0, 0.0, 1.0, 1.0).within(outer), outer));
        assert!(close(
            r(0.5, 0.25, 0.25, 0.5).within(outer),
            r(0.5, 0.7, 0.2, 0.2)
        ));
    }

    #[test]
    fn vision_box_flips_to_top_left_origin() {
        // A line in the bottom fifth of the image.
        assert!(close(
            Region::from_vision(0.1, 0.0, 0.8, 0.2),
            r(0.1, 0.8, 0.8, 0.2)
        ));
        assert!(close(
            Region::from_vision(0.0, 0.7, 1.0, 0.3),
            r(0.0, 0.0, 1.0, 0.3)
        ));
    }

    #[test]
    fn around_grows_by_box_height_and_stays_inside() {
        let b = r(0.125, 0.5, 0.75, 0.1);
        assert!(close(b.around(), r(0.075, 0.4, 0.85, 0.3)));
        // At the bottom-right corner it stops at the frame.
        assert!(close(
            r(0.5, 0.8, 0.5, 0.2).around(),
            r(0.45, 0.6, 0.55, 0.4)
        ));
        assert!(close(r(0.0, 0.0, 1.0, 1.0).around(), r(0.0, 0.0, 1.0, 1.0)));
    }

    #[test]
    fn fit_keeps_aspect_like_obs_screenshot() {
        assert_eq!(fit((1440, 180), (1440, 180)), (1440, 180));
        assert_eq!(fit((1920, 180), (1440, 180)), (1440, 135));
        assert_eq!(fit((720, 180), (1440, 180)), (720, 180));
        assert_eq!(fit((720, 45), (1440, 180)), (1440, 90));
        assert_eq!(fit((10_000, 1), (1440, 180)), (1440, 1));
    }

    /// Default box on 1080p: rows 799 to 979.
    fn dialogue_box() -> Region {
        r(0.125, 799.0 / 1080.0, 0.75, 180.0 / 1080.0)
    }

    /// A text line `top` to `top + 40` px, x 0.3 to 0.7.
    fn line(top: f64) -> Region {
        r(0.3, top / 1080.0, 0.4, 40.0 / 1080.0)
    }

    #[test]
    fn lines_inside_cut_nothing() {
        let lines = [line(820.0), line(876.0), line(932.0)];
        assert_eq!(cut_edges(dialogue_box(), &lines), vec![]);
        assert_eq!(cut_edges(dialogue_box(), &[]), vec![]);
    }

    #[test]
    fn box_edge_through_a_line_is_cut() {
        assert_eq!(
            cut_edges(dialogue_box(), &[line(820.0), line(960.0)]),
            vec![Edge::Bottom]
        );
        assert_eq!(cut_edges(dialogue_box(), &[line(780.0)]), vec![Edge::Top]);
    }

    #[test]
    fn loose_vision_box_a_little_past_the_edge_is_not_cut() {
        // 8 px over the bottom of a 40 px line: Vision boxes run loose.
        assert_eq!(cut_edges(dialogue_box(), &[line(947.0)]), vec![]);
    }

    #[test]
    fn second_line_just_below_a_short_box_is_cut() {
        // Box too short: rows 799 to 907; next line starts at 932.
        let short = r(0.125, 799.0 / 1080.0, 0.75, 108.0 / 1080.0);
        assert_eq!(
            cut_edges(short, &[line(820.0), line(932.0)]),
            vec![Edge::Bottom]
        );
        // Text far below (a HUD prompt) does not count.
        assert_eq!(cut_edges(short, &[line(820.0), line(1000.0)]), vec![]);
    }

    #[test]
    fn text_beside_the_box_does_not_count_but_text_through_a_side_does() {
        let b = dialogue_box();
        let beside = r(0.9, 880.0 / 1080.0, 0.08, 40.0 / 1080.0);
        assert_eq!(cut_edges(b, &[beside]), vec![]);
        let wide = r(0.05, 880.0 / 1080.0, 0.9, 40.0 / 1080.0);
        assert_eq!(cut_edges(b, &[wide]), vec![Edge::Left, Edge::Right]);
        // Off to the side and far below: neither.
        let corner = r(0.0, 1040.0 / 1080.0, 0.05, 40.0 / 1080.0);
        assert_eq!(cut_edges(b, &[corner]), vec![]);
    }

    #[test]
    fn edges_are_sorted_without_repeats() {
        let b = dialogue_box();
        let lines = [line(960.0), line(780.0), line(965.0)];
        assert_eq!(cut_edges(b, &lines), vec![Edge::Top, Edge::Bottom]);
    }

    #[test]
    fn hint_names_the_sides_to_widen() {
        assert_eq!(hint(true, &[]), None);
        let bottom = hint(true, &[Edge::Bottom]).unwrap();
        assert!(bottom.contains("ขอบล่าง"), "{bottom}");
        assert!(!bottom.contains("ขอบบน"), "{bottom}");
        let both = hint(false, &[Edge::Top, Edge::Bottom]).unwrap();
        assert!(both.contains("ขอบบนและขอบล่าง"), "{both}");
        let sides = hint(true, &[Edge::Left, Edge::Right]).unwrap();
        assert!(sides.contains("ขอบซ้ายและขอบขวา"), "{sides}");
    }

    #[test]
    fn hint_when_nothing_is_read() {
        let none = hint(false, &[]).unwrap();
        assert!(none.contains("ยังไม่เห็นข้อความ"), "{none}");
    }
}
