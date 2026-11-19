//! Stream bar of the current profile (FR-STR-03): /overlay reads it, /setup
//! moves it. A move reaches /overlay at once as the SSE event `bar`.
//! `Hider` hides the bar of an audio session once speech stops.

use crate::app::{App, Push};
use crate::core::bar::{self, Bar};
use crate::core::bar_hide::HideTimer;
use crate::db;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;

pub fn routes() -> Router<Arc<App>> {
    Router::new().route("/api/bar", get(current).put(save))
}

/// The bar the profile uses (`Bar::of_profile`); None when there is no such
/// profile.
pub fn profile_bar(conn: &Connection, profile_id: &str) -> rusqlite::Result<Option<Bar>> {
    let stored = conn
        .query_row(
            "SELECT bar_x, bar_y, bar_w FROM profiles WHERE id = ?1",
            [profile_id],
            |r| {
                Ok(match (r.get(0)?, r.get(1)?, r.get(2)?) {
                    (Some(x), Some(y), Some(w)) => Some(Bar { x, y, w }),
                    _ => None,
                })
            },
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let region = db::profile_region(conn, profile_id)?;
    Ok(Some(Bar::of_profile(stored, region)))
}

/// Store the bar of a profile; false when there is no such profile.
pub fn set_bar(conn: &Connection, profile_id: &str, b: Bar) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE profiles SET bar_x = ?2, bar_y = ?3, bar_w = ?4 WHERE id = ?1",
        params![profile_id, b.x, b.y, b.w],
    )?;
    Ok(n > 0)
}

/// GET /api/bar: the current profile's bar, or the default with no profile.
async fn current(State(app): State<Arc<App>>) -> Response {
    let Some(id) = profile_id(&app) else {
        return Json(bar::DEFAULT).into_response();
    };
    match profile_bar(&app.db(), &id) {
        Ok(b) => Json(b.unwrap_or(bar::DEFAULT)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/bar: store the bar in the current profile and move /overlay.
/// Replies with the bar as stored, kept inside the frame.
async fn save(State(app): State<Arc<App>>, Json(b): Json<Bar>) -> Response {
    let b = b.clamped();
    let Some(id) = profile_id(&app) else {
        return conflict("ยังไม่มีโปรไฟล์เกม");
    };
    let stored = set_bar(&app.db(), &id, b);
    match stored {
        Ok(true) => {
            let _ = app.events.send(Push::Bar(b));
            Json(b).into_response()
        }
        Ok(false) => conflict("ไม่เจอโปรไฟล์นี้แล้ว"),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Hides the bar of an audio session (FR-AUD-06): each audio line pushed is
/// told to it, and `subtitle-gone` goes out `HIDE_AFTER` after a closed line
/// unless speech starts again. The wait runs in its own task, which ends
/// once the session's lines are dropped and nothing is due.
pub struct Hider(mpsc::UnboundedSender<(bool, Instant)>);

impl Hider {
    pub fn spawn(events: broadcast::Sender<Push>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(hide_bar(events, rx));
        Self(tx)
    }

    /// An audio line was pushed; `done` when it closed.
    pub fn line(&self, done: bool) {
        let _ = self.0.send((done, Instant::now()));
    }
}

async fn hide_bar(
    events: broadcast::Sender<Push>,
    mut lines: mpsc::UnboundedReceiver<(bool, Instant)>,
) {
    let mut timer = HideTimer::default();
    let mut open = true;
    loop {
        let due = timer.due();
        if due.is_none() && !open {
            return;
        }
        // No timer at all while nothing is due, so paused test time does
        // not jump ahead to it.
        let wait = async {
            match due {
                Some(due) => tokio::time::sleep_until(Instant::from_std(due)).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            got = lines.recv(), if open => match got {
                Some((done, at)) => timer.line(done, at.into_std()),
                None => open = false,
            },
            () = wait => {
                if timer.fire(Instant::now().into_std()) {
                    let _ = events.send(Push::SubtitleGone);
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::obs::Region;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        db::prepare(&c).unwrap();
        c
    }

    #[test]
    fn seeded_profile_has_the_default_bar() {
        let c = conn();
        let id = db::last_profile(&c).unwrap().unwrap().id;
        assert_eq!(profile_bar(&c, &id).unwrap(), Some(bar::DEFAULT));
    }

    #[test]
    fn new_profile_bar_sits_above_its_box() {
        let c = conn();
        let p = crate::profiles::create(&c, "B").unwrap();
        assert_eq!(profile_bar(&c, &p.id).unwrap(), Some(bar::DEFAULT));
        let region = Region {
            x: 0.2,
            y: 0.7,
            w: 0.6,
            h: 0.1,
        };
        db::set_region(&c, &p.id, region).unwrap();
        assert_eq!(profile_bar(&c, &p.id).unwrap(), Some(Bar::above(region)));
    }

    #[test]
    fn set_bar_stores_per_profile() {
        let c = conn();
        let first = db::last_profile(&c).unwrap().unwrap().id;
        let other = crate::profiles::create(&c, "B").unwrap().id;
        let moved = Bar {
            x: 0.2,
            y: 0.1,
            w: 0.5,
        };
        assert!(set_bar(&c, &other, moved).unwrap());
        assert_eq!(profile_bar(&c, &other).unwrap(), Some(moved));
        assert_eq!(profile_bar(&c, &first).unwrap(), Some(bar::DEFAULT));
        assert!(!set_bar(&c, "nope", moved).unwrap());
        assert_eq!(profile_bar(&c, "nope").unwrap(), None);
    }
}
