//! Subtitle box on /setup (FR-REG-02 to 06): a still of the card, live OCR
//! of a box on it, and saving the box to the current profile and to OBS
//! filter `yokoten-crop`. Loopback-only, like every API not on the LAN list.

use crate::app::App;
use crate::core::obs::{Crop, Region, SHOT_SIZE};
use crate::core::region::{cut_edges, fit, hint};
use crate::{db, obs, ocr};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use image::imageops::{self, FilterType};
use image::RgbaImage;
use serde::Serialize;
use serde_json::json;
use std::sync::{Arc, Mutex};

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/obs/still", get(take_still))
        .route("/api/ocr-preview", post(preview))
        .route("/api/region", get(current).put(save))
}

/// The last card still, as the JPG OBS sent: decoded it is 8 MB at 1080p, so
/// that is done for each preview (#102). Kept in memory only, never on disk
/// (NFR-05).
#[derive(Default)]
pub struct Still(Mutex<Option<Arc<[u8]>>>);

impl Still {
    fn get(&self) -> Option<Arc<[u8]>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, jpg: &[u8]) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(jpg.into());
    }
}

/// GET /api/obs/still: JPG of the card at canvas size; its pixels are the
/// canvas pixels `yokoten-crop` cuts.
async fn take_still(State(app): State<Arc<App>>) -> Response {
    let Some(link) = app.obs().cloned() else {
        return conflict("ไม่ได้ต่อ OBS");
    };
    let jpg = match obs::still(&link).await {
        Ok(jpg) => jpg,
        Err(e) => return (StatusCode::CONFLICT, Json(json!({ "error": e }))).into_response(),
    };
    match image::load_from_memory(&jpg) {
        Ok(_) => app.still.set(&jpg),
        Err(e) => return conflict(&format!("อ่านภาพจาก OBS ไม่ได้: {e}")),
    }
    (
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        jpg,
    )
        .into_response()
}

#[derive(Debug, Serialize)]
struct Preview {
    /// What OCR reads in the box, top to bottom, as the session would.
    lines: Vec<String>,
    /// What to change; null when the box reads whole lines.
    hint: Option<String>,
}

/// POST /api/ocr-preview with a box: OCR of that box on the last still.
async fn preview(State(app): State<Arc<App>>, Json(region): Json<Region>) -> Response {
    let Some(jpg) = app.still.get() else {
        return conflict("ยังไม่มีภาพจากการ์ด กด “ถ่ายภาพใหม่” ก่อน");
    };
    let region = region.clamped();
    let reading = tokio::task::spawn_blocking(move || {
        let img = image::load_from_memory(&jpg).map_err(|e| e.to_string())?;
        read_box(&img.to_rgba8(), region)
    });
    match reading.await {
        Ok(Ok(p)) => Json(p).into_response(),
        Ok(Err(e)) => conflict(&format!("อ่านกรอบนี้ไม่ได้: {e}")),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Reads `region` of `img` the way a session will (cropped, then scaled like
/// OBS's 1440×180 screenshot), then looks around it for text the box cuts.
fn read_box(img: &RgbaImage, region: Region) -> Result<Preview, String> {
    let size = img.dimensions();
    let c = Crop::of(region, size);
    let (w, h) = fit((c.cx, c.cy), SHOT_SIZE);
    let inside = imageops::crop_imm(img, c.left, c.top, c.cx, c.cy).to_image();
    let inside = imageops::resize(&inside, w, h, FilterType::Triangle);
    let lines: Vec<String> = ocr::read_pixels(&inside)?
        .into_iter()
        .map(|s| s.text)
        .collect();

    let a = Crop::of(c.region(size).around(), size);
    let area = a.region(size);
    let around = imageops::crop_imm(img, a.left, a.top, a.cx, a.cy).to_image();
    let found: Vec<Region> = ocr::read_pixels(&around)?
        .into_iter()
        .map(|s| s.at.within(area))
        .collect();
    let cut = cut_edges(c.region(size), &found);
    Ok(Preview {
        hint: hint(!lines.is_empty(), &cut),
        lines,
    })
}

/// GET /api/region: the current profile's box, or null.
async fn current(State(app): State<Arc<App>>) -> Response {
    let Some(id) = profile_id(&app) else {
        return Json(json!({ "region": null })).into_response();
    };
    match db::profile_region(&app.db(), &id) {
        Ok(region) => Json(json!({ "region": region })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/region with a box ("ใช้กรอบนี้"): store it in the current
/// profile, then set `yokoten-crop` to match (FR-REG-04, 06). The box stays
/// saved even when OBS cannot be reached; the reply says so.
async fn save(State(app): State<Arc<App>>, Json(region): Json<Region>) -> Response {
    let region = region.clamped();
    let Some(id) = profile_id(&app) else {
        return conflict("ยังไม่มีโปรไฟล์เกม");
    };
    let stored = db::set_region(&app.db(), &id, region);
    match stored {
        Ok(true) => {}
        Ok(false) => return conflict("ไม่เจอโปรไฟล์นี้แล้ว"),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
    let Some(link) = app.obs().cloned() else {
        return Json(json!({ "region": region, "crop": null })).into_response();
    };
    match obs::setup(&link, region).await {
        Ok(done) => Json(json!({ "region": region, "crop": done.crop })).into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(json!({ "region": region, "error": e })),
        )
            .into_response(),
    }
}

fn profile_id(app: &App) -> Option<String> {
    app.state.borrow().profile.as_ref().map(|p| p.id.clone())
}

fn conflict(reason: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": { "message": reason } })),
    )
        .into_response()
}
