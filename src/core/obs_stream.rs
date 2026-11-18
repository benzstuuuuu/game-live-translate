//! Pure decisions for the stream half of "ตั้ง OBS ให้" (FR-ONB-07, FR-STR-04):
//! Browser Source `Yokoten overlay` full frame on top of the Program scene,
//! Audio Input Capture `Yokoten card audio` of the card set to Monitor Only,
//! and every other Audio Input Capture (OBS's Mic/Aux) muted. Yokoten's own
//! objects are found by these fixed names; nothing is deleted or renamed and
//! nothing is ever unmuted except `Yokoten card audio` itself.

use super::obs::{Action, SceneItem};
use serde_json::{json, Value};

/// Browser Source that shows /overlay on the stream (FR-STR-01).
pub const OVERLAY: &str = "Yokoten overlay";
pub const OVERLAY_KIND: &str = "browser_source";
/// Size of the /overlay page.
pub const OVERLAY_SIZE: (u32, u32) = (1920, 1080);
/// Audio Input Capture of the card: the card's video source carries no sound
/// (spike #13), so game sound reaches the stream through monitoring.
pub const CARD_AUDIO: &str = "Yokoten card audio";
/// OBS input kind of an Audio Input Capture on macOS; every other input of
/// this kind is a Mic/Aux to mute.
pub const AUDIO_KIND: &str = "coreaudio_input_capture";

/// Where an item sits on the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Position of the item's alignment point.
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
    /// The alignment point is the item's top-left corner.
    pub top_left: bool,
    /// Size on the canvas: the bounds box, or source size × scale without one.
    pub width: f64,
    pub height: f64,
    pub cropped: bool,
}

impl Placement {
    /// What "fit to canvas" gives: the whole `canvas`, nothing cut.
    pub fn full(canvas: (u32, u32)) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            rotation: 0.0,
            top_left: true,
            width: f64::from(canvas.0),
            height: f64::from(canvas.1),
            cropped: false,
        }
    }

    /// Covers exactly the whole `canvas`.
    pub fn fills(&self, canvas: (u32, u32)) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() < 0.5;
        let full = Self::full(canvas);
        self.top_left
            && !self.cropped
            && near(self.x, 0.0)
            && near(self.y, 0.0)
            && near(self.rotation % 360.0, 0.0)
            && near(self.width, full.width)
            && near(self.height, full.height)
    }
}

/// `Yokoten card audio` as OBS has it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInput {
    pub device_id: Option<String>,
    pub monitor_only: bool,
    pub muted: bool,
}

/// An Audio Input Capture that is not Yokoten's.
#[derive(Debug, Clone, PartialEq)]
pub struct Mic {
    pub name: String,
    pub muted: bool,
}

/// An audio device OBS offers to an Audio Input Capture.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioDevice {
    pub name: String,
    /// `device_id` setting value (CoreAudio UID).
    pub id: String,
}

/// What OBS looks like for the stream setup.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamView {
    /// Scene that is Program now.
    pub program: String,
    /// Items of the Program scene, bottom first.
    pub items: Vec<SceneItem>,
    /// Placement of the topmost `OVERLAY` item in Program.
    pub overlay_placement: Option<Placement>,
    /// Settings of input `OVERLAY`; None when there is no such input.
    pub overlay: Option<Value>,
    pub card_audio: Option<AudioInput>,
    pub mics: Vec<Mic>,
    pub devices: Vec<AudioDevice>,
}

impl StreamView {
    /// The Program scene shows `source` (one of its items is on).
    pub fn shows(&self, source: &str) -> bool {
        self.items.iter().any(|i| i.enabled && i.source == source)
    }
}

/// The audio device of the card, by the same device name as its video.
pub fn find_audio_device<'a>(devices: &'a [AudioDevice], device: &str) -> Option<&'a AudioDevice> {
    let want = device.trim();
    devices
        .iter()
        .find(|d| d.name.trim().eq_ignore_ascii_case(want))
}

pub fn overlay_settings(url: &str) -> Value {
    json!({ "url": url, "width": OVERLAY_SIZE.0, "height": OVERLAY_SIZE.1 })
}

fn overlay_settings_match(settings: &Value, url: &str) -> bool {
    let num = |k: &str| settings.get(k).and_then(Value::as_f64);
    settings.get("url").and_then(Value::as_str) == Some(url)
        && num("width") == Some(f64::from(OVERLAY_SIZE.0))
        && num("height") == Some(f64::from(OVERLAY_SIZE.1))
}

/// Topmost item of `source` in Program: (position bottom-first, item).
fn topmost<'a>(items: &'a [SceneItem], source: &str) -> Option<(usize, &'a SceneItem)> {
    items
        .iter()
        .enumerate()
        .rev()
        .find(|(_, i)| i.source == source)
}

/// Program shows `source`: add an item, or switch the topmost one on.
fn ensure_item(items: &[SceneItem], source: &str) -> Vec<Action> {
    match topmost(items, source) {
        None => vec![Action::AddItem {
            source: source.to_owned(),
        }],
        Some((_, it)) if !it.enabled => vec![Action::SetItemEnabled {
            id: it.id,
            enabled: true,
        }],
        Some(_) => vec![],
    }
}

/// `CARD_AUDIO` on `device_id`, Monitor Only, not muted, shown in Program
/// (only active sources are heard).
pub fn plan_audio(view: &StreamView, device_id: &str) -> Vec<Action> {
    let settings = json!({ "device_id": device_id });
    let Some(audio) = &view.card_audio else {
        return vec![
            Action::CreateInput {
                name: CARD_AUDIO.into(),
                kind: AUDIO_KIND.into(),
                settings,
            },
            Action::SetMonitorOnly {
                name: CARD_AUDIO.into(),
            },
        ];
    };
    let mut out = Vec::new();
    if audio.device_id.as_deref() != Some(device_id) {
        out.push(Action::SetInputSettings {
            name: CARD_AUDIO.into(),
            settings,
        });
    }
    if !audio.monitor_only {
        out.push(Action::SetMonitorOnly {
            name: CARD_AUDIO.into(),
        });
    }
    if audio.muted {
        out.push(Action::SetMuted {
            name: CARD_AUDIO.into(),
            muted: false,
        });
    }
    out.extend(ensure_item(&view.items, CARD_AUDIO));
    out
}

/// Mute every Mic/Aux that is not muted yet.
pub fn plan_mics(view: &StreamView) -> Vec<Action> {
    view.mics
        .iter()
        .filter(|m| !m.muted)
        .map(|m| Action::SetMuted {
            name: m.name.clone(),
            muted: true,
        })
        .collect()
}

/// `OVERLAY` showing `url` at 1920×1080, on, full frame and on top of
/// Program. `pushed_down`: something else is added to Program first.
pub fn plan_overlay(
    view: &StreamView,
    url: &str,
    canvas: (u32, u32),
    pushed_down: bool,
) -> Vec<Action> {
    let fit = Action::FitToCanvas {
        source: OVERLAY.into(),
        canvas,
    };
    let mut out = Vec::new();
    match &view.overlay {
        None => {
            return vec![
                Action::CreateInput {
                    name: OVERLAY.into(),
                    kind: OVERLAY_KIND.into(),
                    settings: overlay_settings(url),
                },
                fit,
            ]
        }
        Some(s) if !overlay_settings_match(s, url) => out.push(Action::SetInputSettings {
            name: OVERLAY.into(),
            settings: overlay_settings(url),
        }),
        Some(_) => {}
    }
    let Some((pos, item)) = topmost(&view.items, OVERLAY) else {
        out.extend([
            Action::AddItem {
                source: OVERLAY.into(),
            },
            fit,
        ]);
        return out;
    };
    out.extend(ensure_item(&view.items, OVERLAY));
    if !view.overlay_placement.is_some_and(|p| p.fills(canvas)) {
        out.push(fit);
    }
    if pushed_down || pos + 1 != view.items.len() {
        out.push(Action::MoveToTop { id: item.id });
    }
    out
}

/// Everything the button changes for the stream, in order: card audio first
/// so the overlay ends up above anything added.
pub fn plan_stream(
    view: &StreamView,
    url: &str,
    device_id: &str,
    canvas: (u32, u32),
) -> Vec<Action> {
    let mut out = plan_audio(view, device_id);
    let added = out
        .iter()
        .any(|a| matches!(a, Action::AddItem { .. } | Action::CreateInput { .. }));
    out.extend(plan_mics(view));
    out.extend(plan_overlay(view, url, canvas, added));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "http://127.0.0.1:8790/overlay";
    const DEVICE_ID: &str = "AppleUSBAudioEngine:UGREEN:15390:1";
    const CANVAS: (u32, u32) = (1920, 1080);

    fn item(id: i64, source: &str, enabled: bool) -> SceneItem {
        SceneItem {
            id,
            source: source.into(),
            enabled,
        }
    }

    fn mic(name: &str, muted: bool) -> Mic {
        Mic {
            name: name.into(),
            muted,
        }
    }

    /// Program "Game" with the card, ready as the button leaves it.
    fn right() -> StreamView {
        StreamView {
            program: "Game".into(),
            items: vec![
                item(1, "Capture Card Device 2", true),
                item(2, CARD_AUDIO, true),
                item(3, OVERLAY, true),
            ],
            overlay_placement: Some(Placement::full(CANVAS)),
            overlay: Some(overlay_settings(URL)),
            card_audio: Some(AudioInput {
                device_id: Some(DEVICE_ID.into()),
                monitor_only: true,
                muted: false,
            }),
            mics: vec![mic("Mic/Aux", true)],
            devices: vec![],
        }
    }

    /// The streamer's OBS before the button: a game scene and a live mic.
    fn fresh() -> StreamView {
        StreamView {
            program: "Game".into(),
            items: vec![item(1, "Capture Card Device 2", true)],
            mics: vec![mic("Mic/Aux", false), mic("Card Audio", false)],
            ..StreamView::default()
        }
    }

    #[test]
    fn right_obs_needs_nothing() {
        assert_eq!(plan_stream(&right(), URL, DEVICE_ID, CANVAS), vec![]);
        assert!(right().shows(OVERLAY));
    }

    #[test]
    fn fresh_obs_gets_audio_mutes_and_overlay() {
        assert_eq!(
            plan_stream(&fresh(), URL, DEVICE_ID, CANVAS),
            vec![
                Action::CreateInput {
                    name: CARD_AUDIO.into(),
                    kind: AUDIO_KIND.into(),
                    settings: json!({ "device_id": DEVICE_ID }),
                },
                Action::SetMonitorOnly {
                    name: CARD_AUDIO.into()
                },
                Action::SetMuted {
                    name: "Mic/Aux".into(),
                    muted: true
                },
                Action::SetMuted {
                    name: "Card Audio".into(),
                    muted: true
                },
                Action::CreateInput {
                    name: OVERLAY.into(),
                    kind: OVERLAY_KIND.into(),
                    settings: json!({ "url": URL, "width": 1920, "height": 1080 }),
                },
                Action::FitToCanvas {
                    source: OVERLAY.into(),
                    canvas: CANVAS
                },
            ]
        );
    }

    #[test]
    fn changed_card_audio_is_set_back() {
        let mut v = right();
        v.card_audio = Some(AudioInput {
            device_id: Some("BuiltInMicrophoneDevice".into()),
            monitor_only: false,
            muted: true,
        });
        v.items[1].enabled = false;
        assert_eq!(
            plan_audio(&v, DEVICE_ID),
            vec![
                Action::SetInputSettings {
                    name: CARD_AUDIO.into(),
                    settings: json!({ "device_id": DEVICE_ID }),
                },
                Action::SetMonitorOnly {
                    name: CARD_AUDIO.into()
                },
                Action::SetMuted {
                    name: CARD_AUDIO.into(),
                    muted: false
                },
                Action::SetItemEnabled {
                    id: 2,
                    enabled: true
                },
            ]
        );
    }

    #[test]
    fn card_audio_missing_from_program_is_added_and_overlay_moved_back_on_top() {
        let mut v = right();
        v.items.remove(1);
        assert_eq!(
            plan_stream(&v, URL, DEVICE_ID, CANVAS),
            vec![
                Action::AddItem {
                    source: CARD_AUDIO.into()
                },
                Action::MoveToTop { id: 3 },
            ]
        );
    }

    #[test]
    fn only_unmuted_mics_are_muted_and_none_unmuted() {
        let mut v = right();
        v.mics = vec![mic("Mic/Aux", true), mic("Mic/Aux 2", false)];
        assert_eq!(
            plan_mics(&v),
            vec![Action::SetMuted {
                name: "Mic/Aux 2".into(),
                muted: true
            }]
        );
    }

    #[test]
    fn changed_overlay_is_set_back() {
        let mut v = right();
        v.overlay = Some(json!({ "url": "http://example.com", "width": 800 }));
        v.items[2].enabled = false;
        v.overlay_placement = Some(Placement {
            x: 100.0,
            ..Placement::full(CANVAS)
        });
        v.items.swap(1, 2);
        assert_eq!(
            plan_overlay(&v, URL, CANVAS, false),
            vec![
                Action::SetInputSettings {
                    name: OVERLAY.into(),
                    settings: overlay_settings(URL),
                },
                Action::SetItemEnabled {
                    id: 3,
                    enabled: true
                },
                Action::FitToCanvas {
                    source: OVERLAY.into(),
                    canvas: CANVAS
                },
                Action::MoveToTop { id: 3 },
            ]
        );
    }

    #[test]
    fn overlay_input_from_another_scene_is_added_to_program() {
        let mut v = right();
        v.items.pop();
        v.overlay_placement = None;
        assert_eq!(
            plan_overlay(&v, URL, CANVAS, false),
            vec![
                Action::AddItem {
                    source: OVERLAY.into()
                },
                Action::FitToCanvas {
                    source: OVERLAY.into(),
                    canvas: CANVAS
                },
            ]
        );
    }

    #[test]
    fn overlay_settings_with_float_size_and_extra_keys_match() {
        let mut v = right();
        v.overlay = Some(json!({ "url": URL, "width": 1920.0, "height": 1080.0, "css": "" }));
        assert_eq!(plan_overlay(&v, URL, CANVAS, false), vec![]);
    }

    #[test]
    fn placement_fills_only_the_exact_canvas() {
        let full = Placement::full(CANVAS);
        assert!(full.fills(CANVAS));
        assert!(Placement {
            rotation: 360.0,
            x: 0.2,
            ..full
        }
        .fills(CANVAS));
        assert!(!full.fills((1280, 720)));
        assert!(!Placement { y: 4.0, ..full }.fills(CANVAS));
        assert!(!Placement {
            top_left: false,
            ..full
        }
        .fills(CANVAS));
        assert!(!Placement {
            cropped: true,
            ..full
        }
        .fills(CANVAS));
        assert!(!Placement {
            rotation: 90.0,
            ..full
        }
        .fills(CANVAS));
        assert!(!Placement {
            width: 1280.0,
            height: 720.0,
            ..full
        }
        .fills(CANVAS));
    }

    #[test]
    fn audio_device_is_found_by_name() {
        let devices = [
            AudioDevice {
                name: "MacBook Pro Microphone".into(),
                id: "BuiltInMicrophoneDevice".into(),
            },
            AudioDevice {
                name: "UGREEN 15390".into(),
                id: DEVICE_ID.into(),
            },
        ];
        assert_eq!(
            find_audio_device(&devices, " ugreen 15390 ").unwrap().id,
            DEVICE_ID
        );
        assert!(find_audio_device(&devices, "Elgato HD60").is_none());
    }
}
