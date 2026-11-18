//! OBS as a frame source (FR-CAP-11 to 13) and the "ตั้ง OBS ให้" button
//! (FR-ONB-07, scene + crop part). Everything that talks to OBS sits behind
//! [`Obs`] so tests can use a fake; [`ws::Local`] is the real obs-websocket.

pub mod ws;

use crate::core::checklist::{scene_state, SceneState};
use crate::core::error::AppError;
use crate::core::obs::{
    find_card, plan, Action, BlackWatch, Crop, Input, Region, SceneView, OCR_SCENE, PROBE_SIZE,
    SHOT_SIZE,
};
use crate::frames::{Frame, FrameSource};
use futures_util::future::BoxFuture;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Why an OBS request failed.
#[derive(Debug, Clone, PartialEq)]
pub enum ObsError {
    /// No answer: OBS closed, server off, or the link dropped.
    Unreachable,
    /// OBS has no source/scene of that name.
    NotFound,
    /// OBS answered with another error.
    Failed(String),
}

/// The OBS requests Yokoten needs, on one open connection.
pub trait Obs: Send + Sync {
    fn inputs(&self) -> BoxFuture<'_, Result<Vec<Input>, ObsError>>;
    /// Base canvas (width, height).
    fn canvas(&self) -> BoxFuture<'_, Result<(u32, u32), ObsError>>;
    /// Items and filters of scene `name`, or None when there is no such scene.
    fn scene<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<SceneView>, ObsError>>;
    fn apply<'a>(
        &'a self,
        scene: &'a str,
        action: &'a Action,
    ) -> BoxFuture<'a, Result<(), ObsError>>;
    /// JPG of `source` scaled to fit `size`, in memory.
    fn screenshot<'a>(
        &'a self,
        source: &'a str,
        size: (u32, u32),
    ) -> BoxFuture<'a, Result<Vec<u8>, ObsError>>;
}

/// Opens a connection; Err is already the cause card to show.
pub trait Connect: Send + Sync {
    fn connect(&self) -> BoxFuture<'_, Result<Box<dyn Obs>, AppError>>;
}

/// How to reach OBS and which capture card to look for.
#[derive(Clone)]
pub struct Link {
    pub connect: Arc<dyn Connect>,
    /// Device name of the capture card, e.g. "UGREEN 15390" (FR-CAP-13).
    pub card_device: String,
}

/// What the setup button did.
#[derive(Debug, Serialize)]
pub struct SetupDone {
    /// OBS input that is the card.
    pub card: String,
    pub scene: &'static str,
    pub filter: &'static str,
    pub crop: Crop,
    /// Number of changes made; 0 when OBS was already right.
    pub changes: usize,
}

/// Create or repair scene `yokoten-ocr` (card only) and filter
/// `yokoten-crop` from `region`. Pressing it twice changes nothing more.
/// Only touches that scene; never the Program scene or other sources.
pub async fn setup(link: &Link, region: Region) -> Result<SetupDone, AppError> {
    let obs = link.connect.connect().await?;
    let card = find_card_name(obs.as_ref(), &link.card_device).await?;
    let canvas = obs.canvas().await.map_err(lost)?;
    let crop = Crop::of(region, canvas);
    let view = obs.scene(OCR_SCENE).await.map_err(lost)?;
    let actions = plan(view.as_ref(), &card, crop);
    for a in &actions {
        obs.apply(OCR_SCENE, a).await.map_err(|e| match e {
            ObsError::NotFound => AppError::card_not_found(&link.card_device),
            other => lost(other),
        })?;
    }
    Ok(SetupDone {
        card,
        scene: OCR_SCENE,
        filter: crate::core::obs::CROP_FILTER,
        crop,
        changes: actions.len(),
    })
}

/// Connect and check the card, the scene and its crop for `region`, then read
/// frames from them. Only checks; fixing is the setup button's job (#38).
pub async fn open(link: &Link, region: Region) -> Result<Box<dyn FrameSource>, AppError> {
    let obs = link.connect.connect().await?;
    let card = find_card_name(obs.as_ref(), &link.card_device).await?;
    let canvas = obs.canvas().await.map_err(lost)?;
    let view = obs.scene(OCR_SCENE).await.map_err(lost)?;
    match scene_state(view.as_ref(), &card, Crop::of(region, canvas)) {
        SceneState::Missing => return Err(AppError::scene_missing()),
        SceneState::Differs => return Err(AppError::scene_differs()),
        SceneState::Right => {}
    }
    Ok(Box::new(ObsFrames {
        obs,
        card,
        device: link.card_device.clone(),
        black: BlackWatch::default(),
    }))
}

async fn find_card_name(obs: &dyn Obs, device: &str) -> Result<String, AppError> {
    let inputs = obs.inputs().await.map_err(lost)?;
    find_card(&inputs, device)
        .map(|i| i.name.clone())
        .ok_or_else(|| AppError::card_not_found(device))
}

/// Any failure while talking to OBS that is not "not found" means the link
/// is gone or broken.
fn lost(e: ObsError) -> AppError {
    if e != ObsError::Unreachable {
        eprintln!("OBS: {e:?}");
    }
    AppError::obs_unreachable()
}

/// GetSourceScreenshot of `yokoten-ocr` at 1440×180, plus a tiny screenshot of
/// the card to notice it going black.
struct ObsFrames {
    obs: Box<dyn Obs>,
    card: String,
    device: String,
    black: BlackWatch,
}

impl ObsFrames {
    /// Is the card picture black right now?
    async fn card_is_black(&self) -> Result<bool, AppError> {
        match self.obs.screenshot(&self.card, PROBE_SIZE).await {
            Ok(jpg) => Ok(image::load_from_memory(&jpg).map_or(true, |img| {
                crate::core::obs::is_black(img.to_rgb8().as_raw(), 3)
            })),
            Err(ObsError::NotFound) => Err(AppError::card_not_found(&self.device)),
            Err(ObsError::Unreachable) => Err(AppError::obs_unreachable()),
            // A source with no picture (device gone) cannot be rendered.
            Err(ObsError::Failed(_)) => Ok(true),
        }
    }
}

impl FrameSource for ObsFrames {
    fn next_frame(&mut self) -> BoxFuture<'_, Result<Option<Frame>, AppError>> {
        Box::pin(async move {
            loop {
                let black = self.card_is_black().await?;
                if self.black.see(black, Instant::now()) {
                    return Err(AppError::card_black());
                }
                match self.obs.screenshot(OCR_SCENE, SHOT_SIZE).await {
                    Ok(jpg) => return Ok(Some(Frame::Image(jpg))),
                    Err(ObsError::NotFound) => return Err(AppError::scene_missing()),
                    Err(ObsError::Unreachable) => return Err(AppError::obs_unreachable()),
                    Err(ObsError::Failed(e)) => {
                        eprintln!("OBS screenshot: {e}");
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
            }
        })
    }
}
