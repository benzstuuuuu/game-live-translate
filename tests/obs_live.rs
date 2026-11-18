//! Against the real OBS on this Mac (needs the UGREEN card source).
//! Run by hand: cargo test --test obs_live -- --ignored --nocapture
//! The first two only create or repair scene yokoten-ocr and its filter
//! yokoten-crop. The stream one changes the Program scene like the button
//! does: it mutes every Mic/Aux. Do not run it while streaming.

use std::sync::Arc;
use yokoten::core::obs::{Region, CROP_FILTER, OCR_SCENE, SHOT_SIZE};
use yokoten::obs::{self, ws, Link};

const OVERLAY_URL: &str = "http://127.0.0.1:8790/overlay";

const DEFAULT_REGION: Region = Region {
    x: 0.125,
    y: 0.74,
    w: 0.75,
    h: 0.16,
};

#[tokio::test]
#[ignore = "needs OBS running with the capture card source"]
async fn press_setup_twice_gives_one_scene_and_one_filter() {
    let link = Link {
        connect: Arc::new(ws::Local),
        card_device: "UGREEN 15390".into(),
        overlay_url: OVERLAY_URL.into(),
    };
    let first = obs::setup(&link, DEFAULT_REGION).await.unwrap();
    println!("first press: {first:?}");
    let second = obs::setup(&link, DEFAULT_REGION).await.unwrap();
    println!("second press: {second:?}");
    assert_eq!(second.changes, 0, "second press must change nothing");

    let o = link.connect.connect().await.unwrap();
    let view = o.scene(OCR_SCENE).await.unwrap().expect("scene exists");
    let shown: Vec<&str> = view
        .items
        .iter()
        .filter(|i| i.enabled)
        .map(|i| i.source.as_str())
        .collect();
    assert_eq!(shown, vec![first.card.as_str()], "only the card is shown");
    let crops: Vec<_> = view
        .filters
        .iter()
        .filter(|f| f.name == CROP_FILTER)
        .collect();
    assert_eq!(crops.len(), 1);
    assert!(crops[0].enabled);
    assert_eq!(
        view.filters.iter().filter(|f| f.enabled).count(),
        1,
        "only yokoten-crop is on"
    );

    // One frame, in memory only.
    let jpg = o.screenshot(OCR_SCENE, SHOT_SIZE).await.unwrap();
    let img = image::load_from_memory(&jpg).unwrap();
    println!(
        "frame {}x{}, {} bytes",
        img.width(),
        img.height(),
        jpg.len()
    );
    assert_eq!(img.width(), SHOT_SIZE.0);
    assert!(img.height() <= SHOT_SIZE.1);
    println!("OCR: {:?}", yokoten::ocr::read_image(&jpg).unwrap());
}

/// "ใช้กรอบนี้" against the real OBS (#34): the still is canvas-sized, the
/// filter follows the box, and the box crop is read. Puts the old crop back.
#[tokio::test]
#[ignore = "needs OBS running with the capture card source"]
async fn region_box_sets_yokoten_crop_and_still_matches_canvas() {
    use yokoten::core::obs::Crop;
    let link = Link {
        connect: Arc::new(ws::Local),
        card_device: "UGREEN 15390".into(),
        overlay_url: OVERLAY_URL.into(),
    };
    let o = link.connect.connect().await.unwrap();
    let canvas = o.canvas().await.unwrap();
    let still = image::load_from_memory(&obs::still(&link).await.unwrap()).unwrap();
    println!(
        "canvas {canvas:?}, still {}x{}",
        still.width(),
        still.height()
    );
    assert_eq!((still.width(), still.height()), canvas);

    let crop_now = || async {
        let view = o.scene(OCR_SCENE).await.unwrap();
        view.and_then(|v| v.filters.into_iter().find(|f| f.name == CROP_FILTER))
            .map(|f| f.settings)
    };
    let before = crop_now().await;
    let old = before
        .as_ref()
        .map(|s| {
            let n = |k: &str| s[k].as_u64().unwrap_or(0) as u32;
            Crop {
                left: n("left"),
                top: n("top"),
                cx: n("cx"),
                cy: n("cy"),
            }
            .region(canvas)
        })
        .unwrap_or(DEFAULT_REGION);

    let b = Region {
        x: 0.1,
        y: 0.7,
        w: 0.8,
        h: 0.2,
    };
    let done = obs::setup(&link, b).await.unwrap();
    assert_eq!(crop_now().await, Some(Crop::of(b, canvas).settings()));
    let jpg = o.screenshot(OCR_SCENE, SHOT_SIZE).await.unwrap();
    println!(
        "box {:?}: OCR {:?}",
        done.crop,
        yokoten::ocr::read_image(&jpg).unwrap()
    );

    obs::setup(&link, old).await.unwrap();
    println!("restored {:?}", crop_now().await);
}

/// The stream half of "ตั้ง OBS ให้" (#42): pressing twice creates nothing
/// twice, the user's sources keep their state, and values changed on
/// Yokoten's own objects are set back.
#[tokio::test]
#[ignore = "needs OBS with the card; mutes Mic/Aux in the real OBS"]
async fn stream_setup_twice_keeps_user_sources_and_resets_ours() {
    use yokoten::core::checklist::{stream_facts, SceneState};
    use yokoten::core::obs::Action;
    use yokoten::core::obs_stream::{CARD_AUDIO, OVERLAY};
    let link = Link {
        connect: Arc::new(ws::Local),
        card_device: "UGREEN 15390".into(),
        overlay_url: OVERLAY_URL.into(),
    };
    let o = link.connect.connect().await.unwrap();
    let before = o.stream().await.unwrap();
    let user_items: Vec<_> = before
        .items
        .iter()
        .filter(|i| i.source != OVERLAY && i.source != CARD_AUDIO)
        .cloned()
        .collect();

    let first = obs::stream::setup(&link).await.unwrap();
    println!("first press: {first:?}");
    let second = obs::stream::setup(&link).await.unwrap();
    assert_eq!(second.changes, 0, "second press must change nothing");

    let after = o.stream().await.unwrap();
    let ours = |src: &str| after.items.iter().filter(|i| i.source == src).count();
    assert_eq!(ours(OVERLAY), 1);
    assert_eq!(ours(CARD_AUDIO), 1);
    assert_eq!(
        after.items.last().unwrap().source,
        OVERLAY,
        "overlay on top"
    );
    let kept: Vec<_> = after
        .items
        .iter()
        .filter(|i| i.source != OVERLAY && i.source != CARD_AUDIO)
        .cloned()
        .collect();
    assert_eq!(kept, user_items, "user's items untouched");
    assert!(after.mics.iter().all(|m| m.muted));

    // Change Yokoten's own objects the way a user might, then press again.
    let overlay_id = after.items.last().unwrap().id;
    for a in [
        Action::SetInputSettings {
            name: OVERLAY.into(),
            settings: serde_json::json!({"url": "about:blank", "width": 800}),
        },
        Action::SetItemEnabled {
            id: overlay_id,
            enabled: false,
        },
        Action::SetMuted {
            name: CARD_AUDIO.into(),
            muted: true,
        },
    ] {
        o.apply(&after.program, &a).await.unwrap();
    }
    let reset = obs::stream::setup(&link).await.unwrap();
    println!("reset press: {reset:?}");
    assert_eq!(reset.changes, 3);
    let canvas = o.canvas().await.unwrap();
    let f = stream_facts(
        &o.stream().await.unwrap(),
        OVERLAY_URL,
        &link.card_device,
        canvas,
    );
    assert_eq!(f.overlay, SceneState::Right);
    assert_eq!(f.audio, Some(SceneState::Right));
    assert!(f.unmuted.is_empty());
}
