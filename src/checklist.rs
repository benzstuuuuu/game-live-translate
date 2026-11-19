//! GET /api/checklist (FR-ONB-06): gathers the facts that
//! `core::checklist` judges. Only reads; never changes OBS or `AppState`.

use crate::app::App;
use crate::audio::Card;
use crate::core::checklist::{
    evaluate, ready, scene_state, stream_facts, CardInput, Facts, Item, ObsFacts, ProfileFacts,
};
use crate::core::error::AppError;
use crate::core::obs::{find_card, Crop, Region, OCR_SCENE};
use crate::db;
use crate::obs::Link;
use axum::extract::State;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct Checklist {
    pub items: Vec<Item>,
    /// Nothing failed: starting should work.
    pub ready: bool,
}

pub async fn handler(State(app): State<Arc<App>>) -> Json<Checklist> {
    let items = evaluate(&facts(&app).await);
    Json(Checklist {
        ready: ready(&items),
        items,
    })
}

pub async fn facts(app: &App) -> Facts {
    let profile = app.state.borrow().profile.clone();
    let region = profile
        .as_ref()
        .and_then(|p| db::profile_region(&app.db(), &p.id).ok().flatten());
    let obs = match (app.reads_folder(), app.obs()) {
        (true, _) => ObsFacts::Folder,
        (false, None) => ObsFacts::Down(AppError::obs_unreachable()),
        (false, Some(link)) => obs_facts(link, region).await,
    };
    let audio = profile.as_ref().is_some_and(|p| p.source == "audio");
    // Asks directly: a checklist must not stop a session. Audio profiles use
    // Live Translate whatever the translator (FR-AUD-04).
    let ready = if audio {
        app.check_live().await
    } else {
        app.translator().check().await
    };
    let translator = ready.map_err(|e| AppError::translator_unavailable(e.message, &e.fix));
    let card = app.live().and_then(|l| l.card.clone());
    let card_input = match app.live() {
        Some(link) if audio && link.audio_file.is_none() => card_input(card.clone()).await,
        _ => None,
    };
    let card_device = match (app.obs(), &card) {
        (Some(link), _) => link.card_device.clone(),
        (None, Some(card)) => card.device().to_owned(),
        (None, None) => String::new(),
    };
    Facts {
        obs,
        card_device,
        translator,
        profile: profile.map(|p| ProfileFacts {
            name: p.name,
            has_region: region.is_some(),
            audio,
        }),
        card_input,
    }
}

/// Whether the Mac has the card's audio input and may hear it. Asks
/// CoreAudio off the async threads; never opens the card or asks for
/// permission.
async fn card_input(card: Option<Arc<dyn Card>>) -> Option<CardInput> {
    let card = card?;
    let read = move || CardInput {
        found: card.found(),
        permission: card.permission(),
    };
    tokio::task::spawn_blocking(read).await.ok()
}

async fn obs_facts(link: &Link, region: Option<Region>) -> ObsFacts {
    let conn = match link.connect.connect().await {
        Ok(c) => c,
        Err(e) => return ObsFacts::Down(e),
    };
    let (Ok(inputs), Ok(canvas), Ok(view)) = (
        conn.inputs().await,
        conn.canvas().await,
        conn.stream().await,
    ) else {
        return ObsFacts::Down(AppError::obs_unreachable());
    };
    let stream = stream_facts(&view, &link.overlay_url, &link.card_device, canvas);
    let Some(card) = find_card(&inputs, &link.card_device).map(|i| i.name.clone()) else {
        return ObsFacts::Up {
            card: None,
            scene: None,
            stream,
        };
    };
    let scene = match region {
        None => None,
        Some(r) => match conn.scene(OCR_SCENE).await {
            Ok(view) => Some(scene_state(view.as_ref(), &card, Crop::of(r, canvas))),
            Err(_) => return ObsFacts::Down(AppError::obs_unreachable()),
        },
    };
    ObsFacts::Up {
        card: Some(card),
        scene,
        stream,
    }
}
