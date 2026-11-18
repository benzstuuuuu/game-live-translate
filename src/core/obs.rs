//! Pure OBS decisions: which input is the card (FR-CAP-13), what the
//! "ตั้ง OBS ให้" button must change (FR-ONB-07, FR-CAP-12), and when the card
//! picture has been black too long. No network here; `crate::obs` does the I/O.

use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Scene Yokoten owns: only the card, never the Program scene (FR-CAP-12).
pub const OCR_SCENE: &str = "yokoten-ocr";
/// Crop/Pad filter on `OCR_SCENE` that cuts out the subtitle box.
pub const CROP_FILTER: &str = "yokoten-crop";
pub const CROP_KIND: &str = "crop_filter";
/// OBS input kinds of a macOS video capture device.
pub const CARD_KINDS: &[&str] = &["macos-avcapture", "macos-avcapture-fast"];
/// Screenshot of `OCR_SCENE` for OCR (FR-CAP-11). OBS keeps the aspect ratio.
pub const SHOT_SIZE: (u32, u32) = (1440, 180);
/// Tiny screenshot of the whole card, only to tell whether it is black.
pub const PROBE_SIZE: (u32, u32) = (64, 36);
/// Black for longer than this stops translating (FR-CAP-13).
pub const BLACK_LIMIT: Duration = Duration::from_secs(3);

/// An OBS input as far as finding the card goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub name: String,
    /// Unversioned input kind, e.g. `macos-avcapture-fast`.
    pub kind: String,
    /// `device_name` setting of capture inputs; the device ID changes on
    /// replug but the name does not.
    pub device_name: Option<String>,
}

/// The capture input showing `device`, if any.
pub fn find_card<'a>(inputs: &'a [Input], device: &str) -> Option<&'a Input> {
    let want = device.trim();
    inputs.iter().find(|i| {
        CARD_KINDS.contains(&i.kind.as_str())
            && i.device_name
                .as_deref()
                .is_some_and(|d| d.trim().eq_ignore_ascii_case(want))
    })
}

/// Subtitle box of a profile, as fractions 0–1 of the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Crop/Pad filter values in canvas pixels (`relative: false`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Crop {
    pub left: u32,
    pub top: u32,
    pub cx: u32,
    pub cy: u32,
}

impl Crop {
    /// Pixel crop of `region` on a `canvas` of (width, height). Stays inside
    /// the canvas and is at least 1×1.
    pub fn of(region: Region, canvas: (u32, u32)) -> Self {
        let (cw, ch) = canvas;
        let px = |f: f64, size: u32| (f.clamp(0.0, 1.0) * f64::from(size)).round() as u32;
        let left = px(region.x, cw).min(cw.saturating_sub(1));
        let top = px(region.y, ch).min(ch.saturating_sub(1));
        Self {
            left,
            top,
            cx: px(region.w, cw).clamp(1, (cw - left).max(1)),
            cy: px(region.h, ch).clamp(1, (ch - top).max(1)),
        }
    }

    pub fn settings(&self) -> Value {
        json!({ "relative": false, "left": self.left, "top": self.top, "cx": self.cx, "cy": self.cy })
    }

    /// True when filter `settings` already crop exactly this.
    fn matches(&self, settings: &Value) -> bool {
        let num = |k: &str| settings.get(k).and_then(Value::as_f64);
        settings.get("relative").and_then(Value::as_bool) == Some(false)
            && num("left") == Some(f64::from(self.left))
            && num("top") == Some(f64::from(self.top))
            && num("cx") == Some(f64::from(self.cx))
            && num("cy") == Some(f64::from(self.cy))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneItem {
    pub id: i64,
    pub source: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub settings: Value,
}

/// What `OCR_SCENE` holds now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneView {
    pub items: Vec<SceneItem>,
    pub filters: Vec<Filter>,
}

impl SceneView {
    /// The scene shows `card` (one of its items is on).
    pub fn shows(&self, card: &str) -> bool {
        self.items.iter().any(|i| i.enabled && i.source == card)
    }
}

/// One change to `OCR_SCENE`. Nothing here deletes or renames anything.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    CreateScene,
    AddItem { source: String },
    SetItemEnabled { id: i64, enabled: bool },
    CreateCrop { settings: Value },
    SetCropSettings { settings: Value },
    SetFilterEnabled { name: String, enabled: bool },
}

/// Changes that make `OCR_SCENE` show only `card`, cropped to `crop`.
/// `scene` is None when the scene does not exist yet. Running the plan and
/// planning again yields nothing, so the button can be pressed twice.
/// Other items and filters in our scene are switched off, never removed.
pub fn plan(scene: Option<&SceneView>, card: &str, crop: Crop) -> Vec<Action> {
    let Some(view) = scene else {
        return vec![
            Action::CreateScene,
            Action::AddItem {
                source: card.to_owned(),
            },
            Action::CreateCrop {
                settings: crop.settings(),
            },
        ];
    };
    let mut out = Vec::new();
    let card_items: Vec<&SceneItem> = view.items.iter().filter(|i| i.source == card).collect();
    match card_items.first() {
        None => out.push(Action::AddItem {
            source: card.to_owned(),
        }),
        Some(first) if !card_items.iter().any(|i| i.enabled) => {
            out.push(Action::SetItemEnabled {
                id: first.id,
                enabled: true,
            });
        }
        Some(_) => {}
    }
    out.extend(
        view.items
            .iter()
            .filter(|i| i.enabled && i.source != card)
            .map(|i| Action::SetItemEnabled {
                id: i.id,
                enabled: false,
            }),
    );
    match view.filters.iter().find(|f| f.name == CROP_FILTER) {
        None => out.push(Action::CreateCrop {
            settings: crop.settings(),
        }),
        Some(f) => {
            if !crop.matches(&f.settings) {
                out.push(Action::SetCropSettings {
                    settings: crop.settings(),
                });
            }
            if !f.enabled {
                out.push(Action::SetFilterEnabled {
                    name: CROP_FILTER.to_owned(),
                    enabled: true,
                });
            }
        }
    }
    out.extend(
        view.filters
            .iter()
            .filter(|f| f.enabled && f.name != CROP_FILTER)
            .map(|f| Action::SetFilterEnabled {
                name: f.name.clone(),
                enabled: false,
            }),
    );
    out
}

/// True when an RGB(A) picture is black: at most 1% of pixels brighter than
/// a dim grey, so JPEG noise and a stray dot do not count as picture.
pub fn is_black(pixels: &[u8], channels: usize) -> bool {
    const DIM: u32 = 32;
    if channels < 3 {
        return true;
    }
    let mut total = 0usize;
    let mut lit = 0usize;
    for p in pixels.chunks_exact(channels) {
        total += 1;
        // Rec. 601 luma, integer.
        let luma = (299 * u32::from(p[0]) + 587 * u32::from(p[1]) + 114 * u32::from(p[2])) / 1000;
        if luma > DIM {
            lit += 1;
        }
    }
    lit * 100 <= total
}

/// Tracks how long the card has been black.
#[derive(Debug, Default)]
pub struct BlackWatch {
    since: Option<Instant>,
}

impl BlackWatch {
    /// Record one probe at `now`; true once black has lasted past `BLACK_LIMIT`.
    pub fn see(&mut self, black: bool, now: Instant) -> bool {
        if !black {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.duration_since(since) > BLACK_LIMIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, kind: &str, device: Option<&str>) -> Input {
        Input {
            name: name.into(),
            kind: kind.into(),
            device_name: device.map(Into::into),
        }
    }

    #[test]
    fn card_is_found_by_device_name_not_input_name() {
        let inputs = [
            input("Mic/Aux", "coreaudio_input_capture", None),
            input("Webcam", "macos-avcapture", Some("FaceTime HD Camera")),
            input(
                "Capture Card Device 2",
                "macos-avcapture-fast",
                Some("UGREEN 15390"),
            ),
        ];
        let card = find_card(&inputs, "UGREEN 15390").unwrap();
        assert_eq!(card.name, "Capture Card Device 2");
        assert_eq!(
            find_card(&inputs, " ugreen 15390 ").unwrap().name,
            card.name
        );
        assert!(find_card(&inputs, "Elgato HD60").is_none());
    }

    #[test]
    fn audio_input_with_card_name_is_not_the_card() {
        let inputs = [input(
            "Card Audio",
            "coreaudio_input_capture",
            Some("UGREEN 15390"),
        )];
        assert!(find_card(&inputs, "UGREEN 15390").is_none());
    }

    const DEFAULT: Region = Region {
        x: 0.125,
        y: 0.74,
        w: 0.75,
        h: 0.16,
    };

    #[test]
    fn crop_of_default_region_on_1080p() {
        let c = Crop::of(DEFAULT, (1920, 1080));
        assert_eq!(
            c,
            Crop {
                left: 240,
                top: 799,
                cx: 1440,
                cy: 173
            }
        );
        assert_eq!(
            c.settings(),
            json!({"relative": false, "left": 240, "top": 799, "cx": 1440, "cy": 173})
        );
    }

    #[test]
    fn crop_stays_inside_canvas() {
        let wild = Region {
            x: 0.9,
            y: -0.5,
            w: 2.0,
            h: 0.0,
        };
        let c = Crop::of(wild, (1920, 1080));
        assert_eq!(c.left, 1728);
        assert_eq!(c.top, 0);
        assert_eq!(c.left + c.cx, 1920);
        assert_eq!(c.cy, 1);
        let edge = Crop::of(
            Region {
                x: 1.0,
                y: 1.0,
                w: 0.5,
                h: 0.5,
            },
            (1920, 1080),
        );
        assert_eq!((edge.left, edge.top, edge.cx, edge.cy), (1919, 1079, 1, 1));
    }

    fn crop() -> Crop {
        Crop::of(DEFAULT, (1920, 1080))
    }

    fn item(id: i64, source: &str, enabled: bool) -> SceneItem {
        SceneItem {
            id,
            source: source.into(),
            enabled,
        }
    }

    fn filter(name: &str, enabled: bool, settings: Value) -> Filter {
        Filter {
            name: name.into(),
            kind: CROP_KIND.into(),
            enabled,
            settings,
        }
    }

    #[test]
    fn no_scene_creates_scene_item_and_crop() {
        assert_eq!(
            plan(None, "Card", crop()),
            vec![
                Action::CreateScene,
                Action::AddItem {
                    source: "Card".into()
                },
                Action::CreateCrop {
                    settings: crop().settings()
                },
            ]
        );
    }

    #[test]
    fn correct_scene_needs_nothing() {
        let view = SceneView {
            items: vec![item(5, "Card", true)],
            filters: vec![filter(CROP_FILTER, true, crop().settings())],
        };
        assert!(view.shows("Card"));
        assert_eq!(plan(Some(&view), "Card", crop()), vec![]);
    }

    #[test]
    fn crop_with_float_values_and_extra_keys_still_matches() {
        let settings = json!({"relative": false, "left": 240.0, "top": 799.0, "cx": 1440.0, "cy": 173.0, "bottom": 0});
        let view = SceneView {
            items: vec![item(5, "Card", true)],
            filters: vec![filter(CROP_FILTER, true, settings)],
        };
        assert_eq!(plan(Some(&view), "Card", crop()), vec![]);
    }

    #[test]
    fn empty_scene_gets_card_and_crop() {
        let view = SceneView::default();
        assert!(!view.shows("Card"));
        assert_eq!(
            plan(Some(&view), "Card", crop()),
            vec![
                Action::AddItem {
                    source: "Card".into()
                },
                Action::CreateCrop {
                    settings: crop().settings()
                },
            ]
        );
    }

    #[test]
    fn hidden_card_is_shown_and_wrong_crop_reset() {
        let view = SceneView {
            items: vec![item(7, "Card", false), item(8, "Card", false)],
            filters: vec![filter(
                CROP_FILTER,
                false,
                json!({"relative": true, "left": 0}),
            )],
        };
        assert_eq!(
            plan(Some(&view), "Card", crop()),
            vec![
                Action::SetItemEnabled {
                    id: 7,
                    enabled: true
                },
                Action::SetCropSettings {
                    settings: crop().settings()
                },
                Action::SetFilterEnabled {
                    name: CROP_FILTER.into(),
                    enabled: true
                },
            ]
        );
    }

    #[test]
    fn other_items_and_filters_in_our_scene_are_switched_off_not_removed() {
        let view = SceneView {
            items: vec![
                item(1, "Card", true),
                item(2, "Yokoten overlay", true),
                item(3, "Old image", false),
            ],
            filters: vec![
                filter("ocr-crop", true, json!({})),
                filter("off already", false, json!({})),
                filter(CROP_FILTER, true, crop().settings()),
            ],
        };
        assert_eq!(
            plan(Some(&view), "Card", crop()),
            vec![
                Action::SetItemEnabled {
                    id: 2,
                    enabled: false
                },
                Action::SetFilterEnabled {
                    name: "ocr-crop".into(),
                    enabled: false
                },
            ]
        );
    }

    #[test]
    fn black_means_almost_no_lit_pixels() {
        let black = vec![0u8; 64 * 36 * 3];
        assert!(is_black(&black, 3));
        // JPEG noise near black.
        let noisy: Vec<u8> = (0..64 * 36 * 4).map(|i| (i % 9) as u8).collect();
        assert!(is_black(&noisy, 4));
        // A few white dots (≤1%) still count as black.
        let mut dots = black.clone();
        for p in 0..23 {
            dots[p * 3..p * 3 + 3].copy_from_slice(&[255, 255, 255]);
        }
        assert!(is_black(&dots, 3));
        // A subtitle line is picture.
        let mut text = black.clone();
        for p in 0..200 {
            text[p * 3..p * 3 + 3].copy_from_slice(&[240, 240, 240]);
        }
        assert!(!is_black(&text, 3));
        assert!(!is_black(&[200, 10, 10], 3));
        assert!(is_black(&[255], 1));
    }

    #[test]
    fn black_watch_trips_only_after_limit_of_continuous_black() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut w = BlackWatch::default();
        assert!(!w.see(true, at(0)));
        assert!(!w.see(true, at(3000)));
        assert!(w.see(true, at(3250)));
        // Picture comes back: the clock restarts.
        assert!(!w.see(false, at(3500)));
        assert!(!w.see(true, at(4000)));
        assert!(!w.see(true, at(6900)));
        assert!(w.see(true, at(7100)));
    }
}
