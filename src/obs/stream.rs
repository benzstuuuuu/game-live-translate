//! The stream half of "ตั้ง OBS ให้" (FR-ONB-07, FR-STR-04): overlay on top of
//! the Program scene, card audio Monitor Only, Mic/Aux muted. What to change
//! is decided in `core::obs_stream`; this reads OBS and applies it.

use super::{lost, Link};
use crate::core::error::AppError;
use crate::core::obs::{Action, OCR_SCENE};
use crate::core::obs_stream::{find_audio_device, plan_stream, CARD_AUDIO, OVERLAY};
use serde::Serialize;

/// What the stream half of the button did.
#[derive(Debug, Serialize)]
pub struct StreamDone {
    /// Program scene the overlay and card audio are in.
    pub program: String,
    pub overlay: &'static str,
    pub card_audio: &'static str,
    /// Mic/Aux inputs this press muted.
    pub muted: Vec<String>,
    /// Number of changes made; 0 when OBS was already right.
    pub changes: usize,
}

/// Create or repair Yokoten's overlay and card audio in the scene that is
/// Program now, and mute every Mic/Aux. Pressing it twice changes nothing
/// more; other sources are never touched and nothing is unmuted.
pub async fn setup(link: &Link) -> Result<StreamDone, AppError> {
    let obs = link.connect.connect().await?;
    let canvas = obs.canvas().await.map_err(lost)?;
    let view = obs.stream().await.map_err(lost)?;
    // Our OCR scene shows only the card; an overlay there would be switched
    // off again by its own setup.
    if view.program == OCR_SCENE {
        return Err(AppError::program_is_ocr_scene());
    }
    let device = find_audio_device(&view.devices, &link.card_device)
        .ok_or_else(|| AppError::card_audio_not_found(&link.card_device))?;
    let actions = plan_stream(&view, &link.overlay_url, &device.id, canvas);
    for a in &actions {
        obs.apply(&view.program, a).await.map_err(lost)?;
    }
    let muted = actions
        .iter()
        .filter_map(|a| match a {
            Action::SetMuted { name, muted: true } => Some(name.clone()),
            _ => None,
        })
        .collect();
    Ok(StreamDone {
        program: view.program,
        overlay: OVERLAY,
        card_audio: CARD_AUDIO,
        muted,
        changes: actions.len(),
    })
}
