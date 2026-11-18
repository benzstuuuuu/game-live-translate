//! Game profiles (FR-GAME-05, 07, 08 · FR-SET-04): list, create, rename,
//! delete, clear history, and pick the one in use. Picking one while
//! translating ends the session and starts a new one at once.

use crate::app::{App, AppState, Status};
use crate::core::error::ErrorCode;
use crate::core::obs::{Action, Crop, Region, CROP_FILTER, OCR_SCENE};
use crate::core::profile::{clean_name, NameError};
use crate::db::{self, ProfileRef};
use crate::obs;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One row of the /setup list and the /reader picker.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub source: String,
    /// The profile has a subtitle box (FR-REG).
    pub has_region: bool,
    /// Lines kept across sessions (FR-GAME-05); cleared by FR-GAME-07.
    pub line_count: i64,
}

#[derive(Debug, PartialEq)]
pub enum ProfileError {
    NotFound,
    Name(NameError),
    Db(String),
}

impl From<rusqlite::Error> for ProfileError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl IntoResponse for ProfileError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "ไม่มีโปรไฟล์นี้".to_string()),
            Self::Name(e) => (StatusCode::BAD_REQUEST, e.message()),
            Self::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// Every profile, oldest first so the list does not jump around.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Profile>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.source, p.region_w IS NOT NULL AND p.region_h IS NOT NULL,
                (SELECT COUNT(*) FROM lines l WHERE l.profile_id = p.id)
         FROM profiles p ORDER BY p.created_at, p.rowid",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Profile {
            id: r.get(0)?,
            name: r.get(1)?,
            source: r.get(2)?,
            has_region: r.get(3)?,
            line_count: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn find(conn: &Connection, id: &str) -> rusqlite::Result<Option<ProfileRef>> {
    conn.query_row(
        "SELECT id, name, source FROM profiles WHERE id = ?1",
        [id],
        |r| {
            Ok(ProfileRef {
                id: r.get(0)?,
                name: r.get(1)?,
                source: r.get(2)?,
            })
        },
    )
    .optional()
}

fn other_names(conn: &Connection, except: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT name FROM profiles WHERE id != ?1")?;
    let names = stmt.query_map([except], |r| r.get(0))?;
    names.collect()
}

/// New profile with no subtitle box yet: /setup asks for one to be dragged
/// on the card's picture (#19). It does not become the last used one until
/// it is picked.
pub fn create(conn: &Connection, raw_name: &str) -> Result<ProfileRef, ProfileError> {
    let name = clean_name(raw_name, &other_names(conn, "")?).map_err(ProfileError::Name)?;
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO profiles (id, name, created_at, last_used_at) VALUES (?1, ?2, ?3, 0)",
        params![id, name, db::now_ms()],
    )?;
    find(conn, &id)?.ok_or(ProfileError::NotFound)
}

pub fn rename(conn: &Connection, id: &str, raw_name: &str) -> Result<ProfileRef, ProfileError> {
    find(conn, id)?.ok_or(ProfileError::NotFound)?;
    let name = clean_name(raw_name, &other_names(conn, id)?).map_err(ProfileError::Name)?;
    conn.execute(
        "UPDATE profiles SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    find(conn, id)?.ok_or(ProfileError::NotFound)
}

/// Deletes the profile and, through the foreign key, its history.
pub fn delete(conn: &Connection, id: &str) -> Result<(), ProfileError> {
    match conn.execute("DELETE FROM profiles WHERE id = ?1", [id])? {
        0 => Err(ProfileError::NotFound),
        _ => Ok(()),
    }
}

/// FR-GAME-07: forget every line of the profile. Returns how many went.
pub fn clear_lines(conn: &Connection, id: &str) -> Result<usize, ProfileError> {
    find(conn, id)?.ok_or(ProfileError::NotFound)?;
    Ok(conn.execute("DELETE FROM lines WHERE profile_id = ?1", [id])?)
}

/// Marks the profile as the last used one; Yokoten opens with it (FR-GAME-08).
/// Always later than any other, even within the same millisecond.
fn touch(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE profiles SET last_used_at =
           MAX(?2, (SELECT MAX(last_used_at) FROM profiles) + 1) WHERE id = ?1",
        params![id, db::now_ms()],
    )?;
    Ok(())
}

/// Make `id` the profile in use (FR-GAME-08). While translating, the old
/// session ends and a new one starts at once with the new subtitle box;
/// otherwise /reader is cleared and waits for start.
pub async fn select(app: &Arc<App>, id: &str) -> Result<AppState, ProfileError> {
    let (profile, region) = {
        let conn = app.db();
        let profile = find(&conn, id)?.ok_or(ProfileError::NotFound)?;
        touch(&conn, id)?;
        (profile, db::profile_region(&conn, id)?)
    };
    let translating = app.state.borrow().status == Status::Translating;
    if translating {
        app.stop();
    }
    if let (Some(link), Some(region)) = (app.obs(), region) {
        apply_crop(link, region).await;
    }
    app.state.send_modify(|s| {
        s.profile = Some(profile);
        s.session_id = None;
        // A failure of the old profile's session says nothing about this one;
        // a translator problem still holds.
        if s.status == Status::Error
            && s.error
                .as_ref()
                .is_none_or(|e| e.code != ErrorCode::TranslatorUnavailable)
        {
            s.status = Status::Idle;
            s.error = None;
        }
    });
    if translating {
        // Start problems (no box, OBS gone) land in AppState.error for /reader.
        let _ = app.start();
    }
    Ok(app.state.borrow().clone())
}

/// Point the existing `yokoten-crop` at the new profile's box, if OBS is up
/// and the filter is there. Creating the scene or filter stays the job of
/// the "ตั้ง OBS ให้" button; anything missing shows on the checklist.
async fn apply_crop(link: &obs::Link, region: Region) {
    let Ok(conn) = link.connect.connect().await else {
        return;
    };
    let has_filter = matches!(
        conn.scene(OCR_SCENE).await,
        Ok(Some(view)) if view.filters.iter().any(|f| f.name == CROP_FILTER)
    );
    let Ok(canvas) = conn.canvas().await else {
        return;
    };
    if has_filter {
        let settings = Crop::of(region, canvas).settings();
        if let Err(e) = conn
            .apply(OCR_SCENE, &Action::SetCropSettings { settings })
            .await
        {
            eprintln!("OBS crop: {e:?}");
        }
    }
}

/// Routes of this module; `server::router` merges them under its guard.
pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/profiles", get(list_handler).post(create_handler))
        .route(
            "/api/profiles/{id}",
            put(rename_handler).delete(delete_handler),
        )
        .route(
            "/api/profiles/{id}/lines",
            axum::routing::delete(clear_handler),
        )
        .route("/api/profile", put(select_handler))
}

async fn list_handler(State(app): State<Arc<App>>) -> Result<Json<Vec<Profile>>, ProfileError> {
    Ok(Json(list(&app.db())?))
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

async fn create_handler(
    State(app): State<Arc<App>>,
    Json(body): Json<NameBody>,
) -> Result<Response, ProfileError> {
    let p = create(&app.db(), &body.name)?;
    Ok((StatusCode::CREATED, Json(p)).into_response())
}

async fn rename_handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(body): Json<NameBody>,
) -> Result<Json<ProfileRef>, ProfileError> {
    let p = rename(&app.db(), &id, &body.name)?;
    // /reader shows the name of the profile in use on its header.
    app.state.send_if_modified(|s| match s.profile.as_mut() {
        Some(cur) if cur.id == p.id => {
            cur.name = p.name.clone();
            true
        }
        _ => false,
    });
    Ok(Json(p))
}

async fn delete_handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<AppState>, ProfileError> {
    let in_use = app
        .state
        .borrow()
        .profile
        .as_ref()
        .is_some_and(|p| p.id == id);
    if in_use {
        app.stop();
    }
    let next = {
        let conn = app.db();
        delete(&conn, &id)?;
        db::last_profile(&conn)?
    };
    if in_use {
        match next {
            Some(p) => {
                select(&app, &p.id).await?;
            }
            None => app.state.send_modify(|s| {
                s.profile = None;
                s.session_id = None;
            }),
        }
    }
    Ok(Json(app.state.borrow().clone()))
}

async fn clear_handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ProfileError> {
    let deleted = clear_lines(&app.db(), &id)?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

#[derive(Deserialize)]
struct Pick {
    id: String,
}

/// FR-GAME-08 / FR-WEB-03: pick from /reader (LAN) or /setup.
async fn select_handler(
    State(app): State<Arc<App>>,
    Json(pick): Json<Pick>,
) -> Result<Json<AppState>, ProfileError> {
    Ok(Json(select(&app, &pick.id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        db::prepare(&c).unwrap();
        c
    }

    #[test]
    fn first_run_lists_the_default_with_its_box() {
        let c = conn();
        let all = list(&c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "เกมแรก");
        assert!(all[0].has_region);
        assert_eq!(all[0].line_count, 0);
    }

    #[test]
    fn created_profile_has_no_box_and_is_not_last_used() {
        let c = conn();
        let first = db::last_profile(&c).unwrap().unwrap();
        let p = create(&c, "  Genshin  Impact ").unwrap();
        assert_eq!(p.name, "Genshin Impact");
        assert_eq!(p.source, "subtitle");
        assert_eq!(db::profile_region(&c, &p.id).unwrap(), None);
        assert_eq!(db::last_profile(&c).unwrap().unwrap(), first);
        let names: Vec<String> = list(&c).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["เกมแรก", "Genshin Impact"]);
    }

    #[test]
    fn names_must_be_unique_and_present() {
        let c = conn();
        assert_eq!(
            create(&c, "เกมแรก"),
            Err(ProfileError::Name(NameError::Taken))
        );
        assert_eq!(create(&c, " "), Err(ProfileError::Name(NameError::Empty)));
        let p = create(&c, "A").unwrap();
        assert_eq!(rename(&c, &p.id, "a").unwrap().name, "a");
        assert_eq!(
            rename(&c, &p.id, "เกมแรก"),
            Err(ProfileError::Name(NameError::Taken))
        );
        assert_eq!(rename(&c, "nope", "B"), Err(ProfileError::NotFound));
    }

    #[test]
    fn delete_takes_the_history_with_it() {
        let c = conn();
        let p = create(&c, "B").unwrap();
        c.execute(
            "INSERT INTO lines (id, profile_id, session_id, source, source_text, target_text,
               engine, created_at) VALUES ('l1', ?1, 's', 'subtitle', 'a', 'ก', 'fake', 1)",
            [&p.id],
        )
        .unwrap();
        assert_eq!(list(&c).unwrap()[1].line_count, 1);
        delete(&c, &p.id).unwrap();
        let lines: i64 = c
            .query_row("SELECT COUNT(*) FROM lines", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lines, 0);
        assert_eq!(delete(&c, &p.id), Err(ProfileError::NotFound));
    }

    #[test]
    fn clear_lines_keeps_the_profile() {
        let c = conn();
        let id = db::last_profile(&c).unwrap().unwrap().id;
        for n in 0..3 {
            c.execute(
                "INSERT INTO lines (id, profile_id, session_id, source, source_text, target_text,
                   engine, created_at) VALUES (?1, ?2, 's', 'subtitle', 'a', 'ก', 'fake', 1)",
                params![format!("l{n}"), id],
            )
            .unwrap();
        }
        assert_eq!(clear_lines(&c, &id).unwrap(), 3);
        assert_eq!(list(&c).unwrap()[0].line_count, 0);
        assert_eq!(clear_lines(&c, "nope"), Err(ProfileError::NotFound));
    }

    #[test]
    fn touch_makes_it_the_last_used() {
        let c = conn();
        let p = create(&c, "C").unwrap();
        touch(&c, &p.id).unwrap();
        assert_eq!(db::last_profile(&c).unwrap().unwrap(), p);
    }
}
