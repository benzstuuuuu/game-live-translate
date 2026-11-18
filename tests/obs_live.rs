//! Against the real OBS on this Mac (needs the UGREEN card source).
//! Run by hand: cargo test --test obs_live -- --ignored --nocapture
//! It only creates or repairs scene yokoten-ocr and its filter yokoten-crop.

use std::sync::Arc;
use yokoten::core::obs::{Region, CROP_FILTER, OCR_SCENE, SHOT_SIZE};
use yokoten::obs::{self, ws, Link};

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
