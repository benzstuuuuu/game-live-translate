//! The /setup checklist (FR-ONB-06), M1 items only (#29): OBS reachable,
//! card source found, scene and crop right, translator ready, a profile with
//! a subtitle box. Overlay and Mic/Aux are M2. Facts in, items out; the I/O
//! that gathers the facts lives in `crate::checklist`.

use super::error::AppError;
use super::obs::{plan, Crop, SceneView, CROP_FILTER, OCR_SCENE};
use serde::Serialize;

/// What OBS looked like when asked.
#[derive(Debug, Clone, PartialEq)]
pub enum ObsFacts {
    /// Frames come from a folder (`--frames`); OBS is not used.
    Folder,
    /// Could not talk to OBS; the error says why and how to fix it.
    Down(AppError),
    Up {
        /// OBS input that is the card, if found.
        card: Option<String>,
        /// None when it cannot be judged yet (no card or no subtitle box).
        scene: Option<SceneState>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneState {
    /// No `yokoten-ocr`, or it does not show the card.
    Missing,
    /// It shows the card but the crop or other items/filters are off.
    Differs,
    Right,
}

/// Judge `yokoten-ocr` the way the "ตั้ง OBS ให้" button would: right when
/// pressing it would change nothing.
pub fn scene_state(view: Option<&SceneView>, card: &str, crop: Crop) -> SceneState {
    match view {
        Some(v) if v.shows(card) => {
            if plan(Some(v), card, crop).is_empty() {
                SceneState::Right
            } else {
                SceneState::Differs
            }
        }
        _ => SceneState::Missing,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileFacts {
    pub name: String,
    pub has_region: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    pub obs: ObsFacts,
    /// Device name of the capture card, e.g. "UGREEN 15390".
    pub card_device: String,
    pub translator: Result<(), AppError>,
    pub profile: Option<ProfileFacts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Ok,
    Fail,
    /// Not checked: an earlier item must pass first, or it does not apply.
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub id: &'static str,
    pub label: String,
    pub state: State,
    /// What was found, or what is wrong, in Thai.
    pub detail: String,
    /// What to do about it, in Thai; only when `state` is `fail`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
    /// Anchor on /setup of the section that fixes it.
    pub anchor: &'static str,
}

impl Item {
    fn new(id: &'static str, label: String, anchor: &'static str) -> Self {
        Self {
            id,
            label,
            state: State::Ok,
            detail: String::new(),
            fix: None,
            anchor,
        }
    }

    fn ok(self, detail: impl Into<String>) -> Self {
        Self {
            state: State::Ok,
            detail: detail.into(),
            ..self
        }
    }

    fn skip(self, detail: impl Into<String>) -> Self {
        Self {
            state: State::Skip,
            detail: detail.into(),
            ..self
        }
    }

    fn fail(self, detail: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            state: State::Fail,
            detail: detail.into(),
            fix: Some(fix.into()),
            ..self
        }
    }

    fn fail_with(self, e: &AppError) -> Self {
        self.fail(e.message.clone(), e.fix.clone())
    }
}

const OBS_ANCHOR: &str = "obs-title";
const SETUP_FIX: &str = "กด “ตั้ง OBS ให้” ในส่วน OBS";

/// The M1 checklist, in the order /setup shows it.
pub fn evaluate(f: &Facts) -> Vec<Item> {
    vec![
        obs_item(&f.obs),
        card_item(&f.obs, &f.card_device),
        scene_item(&f.obs, f.profile.as_ref()),
        translator_item(&f.translator),
        profile_item(f.profile.as_ref()),
    ]
}

/// True when nothing failed: starting a session should work.
pub fn ready(items: &[Item]) -> bool {
    items.iter().all(|i| i.state != State::Fail)
}

fn obs_item(obs: &ObsFacts) -> Item {
    let item = Item::new("obs", "ต่อ OBS ได้".into(), OBS_ANCHOR);
    match obs {
        ObsFacts::Folder => item.skip("อ่านภาพจากโฟลเดอร์ (--frames) ไม่ได้ใช้ OBS"),
        ObsFacts::Down(e) => item.fail_with(e),
        ObsFacts::Up { .. } => item.ok("obs-websocket ตอบแล้ว"),
    }
}

fn card_item(obs: &ObsFacts, device: &str) -> Item {
    let item = Item::new("card", "เจอ source การ์ดใน OBS".into(), OBS_ANCHOR);
    match obs {
        ObsFacts::Folder => item.skip("ไม่ได้ใช้ OBS"),
        ObsFacts::Down(_) => item.skip("ต่อ OBS ให้ได้ก่อน"),
        ObsFacts::Up { card: Some(c), .. } => item.ok(format!("“{c}” ({device})")),
        ObsFacts::Up { card: None, .. } => item.fail_with(&AppError::card_not_found(device)),
    }
}

fn scene_item(obs: &ObsFacts, profile: Option<&ProfileFacts>) -> Item {
    let item = Item::new(
        "scene",
        format!("scene {OCR_SCENE} และ filter {CROP_FILTER} ถูก"),
        OBS_ANCHOR,
    );
    match obs {
        ObsFacts::Folder => item.skip("ไม่ได้ใช้ OBS"),
        ObsFacts::Down(_) => item.skip("ต่อ OBS ให้ได้ก่อน"),
        ObsFacts::Up { card: None, .. } => item.skip("หา source การ์ดให้เจอก่อน"),
        ObsFacts::Up { scene: None, .. } => item.skip("โปรไฟล์ต้องมีกรอบ subtitle ก่อน"),
        ObsFacts::Up {
            scene: Some(state), ..
        } => match state {
            SceneState::Right => item.ok(match profile {
                Some(p) => format!("ตัดกรอบตามโปรไฟล์ “{}”", p.name),
                None => "ตัดกรอบตามโปรไฟล์".into(),
            }),
            SceneState::Missing => {
                item.fail(format!("ยังไม่มี scene {OCR_SCENE} ที่มี source การ์ด"), SETUP_FIX)
            }
            SceneState::Differs => item.fail(
                format!("{OCR_SCENE} หรือ {CROP_FILTER} ไม่ตรงกับกรอบของโปรไฟล์นี้"),
                SETUP_FIX,
            ),
        },
    }
}

fn translator_item(t: &Result<(), AppError>) -> Item {
    let item = Item::new("translator", "ตัวแปลพร้อม".into(), "translator-title");
    match t {
        Ok(()) => item.ok("พร้อมแปล"),
        Err(e) => item.fail_with(e),
    }
}

fn profile_item(p: Option<&ProfileFacts>) -> Item {
    let item = Item::new("profile", "มีโปรไฟล์และกรอบ subtitle".into(), "game");
    match p {
        None => item.fail("ยังไม่ได้เลือกโปรไฟล์เกม", "สร้างหรือเลือกโปรไฟล์ในส่วนเกม"),
        Some(p) if !p.has_region => Item {
            anchor: "region",
            ..item.fail(
                format!("โปรไฟล์ “{}” ยังไม่มีกรอบ subtitle", p.name),
                "ลากกรอบบนภาพจากการ์ดในส่วนกรอบ subtitle",
            )
        },
        Some(p) => item.ok(format!("“{}”", p.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::obs::{Filter, Region, SceneItem};

    const DEVICE: &str = "UGREEN 15390";
    const CARD: &str = "Capture Card Device 2";

    fn crop() -> Crop {
        Crop::of(
            Region {
                x: 0.125,
                y: 0.74,
                w: 0.75,
                h: 0.16,
            },
            (1920, 1080),
        )
    }

    fn right_view() -> SceneView {
        SceneView {
            items: vec![SceneItem {
                id: 1,
                source: CARD.into(),
                enabled: true,
            }],
            filters: vec![Filter {
                name: CROP_FILTER.into(),
                kind: "crop_filter".into(),
                enabled: true,
                settings: crop().settings(),
            }],
        }
    }

    fn all_good() -> Facts {
        Facts {
            obs: ObsFacts::Up {
                card: Some(CARD.into()),
                scene: Some(SceneState::Right),
            },
            card_device: DEVICE.into(),
            translator: Ok(()),
            profile: Some(ProfileFacts {
                name: "Genshin".into(),
                has_region: true,
            }),
        }
    }

    fn states(items: &[Item]) -> Vec<(&'static str, State)> {
        items.iter().map(|i| (i.id, i.state)).collect()
    }

    fn item<'a>(items: &'a [Item], id: &str) -> &'a Item {
        items.iter().find(|i| i.id == id).unwrap()
    }

    #[test]
    fn scene_right_when_setup_would_change_nothing() {
        assert_eq!(
            scene_state(Some(&right_view()), CARD, crop()),
            SceneState::Right
        );
    }

    #[test]
    fn scene_missing_without_scene_or_card_item() {
        assert_eq!(scene_state(None, CARD, crop()), SceneState::Missing);
        let mut v = right_view();
        v.items[0].enabled = false;
        assert_eq!(scene_state(Some(&v), CARD, crop()), SceneState::Missing);
    }

    #[test]
    fn scene_differs_when_crop_is_off_or_missing() {
        let mut v = right_view();
        v.filters[0].settings = Crop::of(
            Region {
                x: 0.0,
                y: 0.0,
                w: 0.5,
                h: 0.5,
            },
            (1920, 1080),
        )
        .settings();
        assert_eq!(scene_state(Some(&v), CARD, crop()), SceneState::Differs);
        v.filters.clear();
        assert_eq!(scene_state(Some(&v), CARD, crop()), SceneState::Differs);
    }

    #[test]
    fn five_m1_items_all_ok() {
        let items = evaluate(&all_good());
        assert_eq!(
            states(&items),
            vec![
                ("obs", State::Ok),
                ("card", State::Ok),
                ("scene", State::Ok),
                ("translator", State::Ok),
                ("profile", State::Ok),
            ]
        );
        assert!(ready(&items));
        assert!(items.iter().all(|i| i.fix.is_none()));
        assert!(item(&items, "card").detail.contains(CARD));
        assert!(item(&items, "scene").detail.contains("Genshin"));
    }

    #[test]
    fn obs_down_fails_obs_and_skips_what_needs_it() {
        let f = Facts {
            obs: ObsFacts::Down(AppError::obs_unreachable()),
            ..all_good()
        };
        let items = evaluate(&f);
        assert_eq!(item(&items, "obs").state, State::Fail);
        assert_eq!(item(&items, "obs").detail, "ต่อ OBS ไม่ได้");
        assert!(item(&items, "obs").fix.as_ref().unwrap().contains("4455"));
        assert_eq!(item(&items, "card").state, State::Skip);
        assert_eq!(item(&items, "scene").state, State::Skip);
        assert_eq!(item(&items, "translator").state, State::Ok);
        assert!(!ready(&items));
    }

    #[test]
    fn missing_card_names_the_device() {
        let f = Facts {
            obs: ObsFacts::Up {
                card: None,
                scene: None,
            },
            ..all_good()
        };
        let items = evaluate(&f);
        let card = item(&items, "card");
        assert_eq!(card.state, State::Fail);
        assert!(card.detail.contains(DEVICE));
        assert_eq!(item(&items, "scene").state, State::Skip);
    }

    #[test]
    fn scene_problems_point_to_the_setup_button() {
        for (state, word) in [
            (SceneState::Missing, "ยังไม่มี"),
            (SceneState::Differs, "ไม่ตรง"),
        ] {
            let f = Facts {
                obs: ObsFacts::Up {
                    card: Some(CARD.into()),
                    scene: Some(state),
                },
                ..all_good()
            };
            let items = evaluate(&f);
            let scene = item(&items, "scene");
            assert_eq!(scene.state, State::Fail);
            assert!(scene.detail.contains(word), "{}", scene.detail);
            assert!(scene.fix.as_ref().unwrap().contains("ตั้ง OBS ให้"));
            assert_eq!(scene.anchor, OBS_ANCHOR);
        }
    }

    #[test]
    fn scene_waits_for_a_subtitle_box() {
        let f = Facts {
            obs: ObsFacts::Up {
                card: Some(CARD.into()),
                scene: None,
            },
            profile: Some(ProfileFacts {
                name: "ใหม่".into(),
                has_region: false,
            }),
            ..all_good()
        };
        let items = evaluate(&f);
        assert_eq!(item(&items, "scene").state, State::Skip);
        let p = item(&items, "profile");
        assert_eq!(p.state, State::Fail);
        assert!(p.detail.contains("ใหม่"));
        assert_eq!(p.anchor, "region");
    }

    #[test]
    fn no_profile_fails_profile() {
        let f = Facts {
            profile: None,
            ..all_good()
        };
        let items = evaluate(&f);
        let p = item(&items, "profile");
        assert_eq!(p.state, State::Fail);
        assert_eq!(p.anchor, "game");
        assert!(item(&items, "scene").detail.contains("ตัดกรอบ"));
    }

    #[test]
    fn translator_problem_carries_its_fix() {
        let f = Facts {
            translator: Err(AppError::translator_unavailable(
                "ยังไม่มีชุดภาษาอังกฤษ → ไทย".into(),
                "ดาวน์โหลดใน Language & Region",
            )),
            ..all_good()
        };
        let items = evaluate(&f);
        let t = item(&items, "translator");
        assert_eq!(t.state, State::Fail);
        assert_eq!(t.fix.as_deref(), Some("ดาวน์โหลดใน Language & Region"));
        assert!(!ready(&items));
    }

    #[test]
    fn folder_mode_skips_obs_items_but_stays_ready() {
        let f = Facts {
            obs: ObsFacts::Folder,
            ..all_good()
        };
        let items = evaluate(&f);
        for id in ["obs", "card", "scene"] {
            assert_eq!(item(&items, id).state, State::Skip, "{id}");
        }
        assert!(ready(&items));
    }

    #[test]
    fn items_serialize_with_lowercase_state() {
        let f = Facts {
            obs: ObsFacts::Down(AppError::obs_unreachable()),
            ..all_good()
        };
        let v = serde_json::to_value(evaluate(&f)).unwrap();
        assert_eq!(v[0]["state"], "fail");
        assert_eq!(v[1]["state"], "skip");
        assert_eq!(v[3]["state"], "ok");
        assert!(v[3].get("fix").is_none());
    }
}
