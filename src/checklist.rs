//! GET /api/checklist (FR-ONB-06): gathers the facts that
//! `core::checklist` judges. Only reads; never changes OBS or `AppState`.

use crate::app::App;
use crate::core::checklist::{
    evaluate, ready, scene_state, stream_facts, Facts, Item, ObsFacts, ProfileFacts,
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
    // Asks the engine directly: a checklist must not stop a session.
    let translator = app
        .translator()
        .check()
        .await
        .map_err(|e| AppError::translator_unavailable(e.message, &e.fix));
    Facts {
        obs,
        card_device: app.obs().map(|l| l.card_device.clone()).unwrap_or_default(),
        translator,
        profile: profile.map(|p| ProfileFacts {
            name: p.name,
            has_region: region.is_some(),
        }),
    }
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
