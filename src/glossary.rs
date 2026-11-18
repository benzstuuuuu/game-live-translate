//! Glossary in two layers (FR-GLS-01, 03, 05): terms in SQLite, the API
//! behind /setup, and the placeholder round trip for engines without a
//! prompt. Matching and placeholders are pure, in `core::glossary`.
//!
//! The glossary is read for every line, so a term added in /setup applies
//! from the next line on.

use crate::app::App;
use crate::core::glossary::{self as core, Term, TermError};
use crate::translate::Translator;
use crate::{db, profiles};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// `text` in Thai with the glossary terms swapped for `[1]`, `[2]` … on the
/// way in and back to Thai on the way out (FR-GLS-05). When a placeholder
/// goes missing the line is translated again as it is, English terms left
/// in the result are replaced, and the miss is logged; the player sees no
/// error.
pub async fn with_placeholders<T: Translator + ?Sized>(
    engine: &T,
    text: &str,
    earlier: &[String],
    glossary: &[Term],
) -> Result<String, String> {
    let protected = core::protect(text, glossary);
    if protected.is_empty() {
        return engine.translate_in_context(text, earlier).await;
    }
    let out = engine
        .translate_in_context(&protected.text, earlier)
        .await?;
    if let Some(done) = protected.restore(&out) {
        return Ok(done);
    }
    eprintln!(
        "glossary: {} lost a placeholder, translating the line again without them",
        engine.engine()
    );
    let plain = engine.translate_in_context(text, earlier).await?;
    Ok(core::replace_terms(&plain, protected.terms()))
}

/// One row of `glossary_terms` (SRS 7.2) as the API shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub id: String,
    /// Empty for the shared glossary.
    pub profile_id: String,
    pub source: String,
    pub target: String,
    pub created_at: i64,
}

#[derive(Debug, PartialEq)]
pub enum GlossaryError {
    NotFound,
    NoProfile,
    Term(TermError),
    /// The layer already has this source.
    Taken,
    Db(String),
}

impl From<rusqlite::Error> for GlossaryError {
    fn from(e: rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::ConstraintViolation) => Self::Taken,
            _ => Self::Db(e.to_string()),
        }
    }
}

impl IntoResponse for GlossaryError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "ไม่มีคำนี้ใน glossary".to_string()),
            Self::NoProfile => (StatusCode::NOT_FOUND, "ไม่มีโปรไฟล์นี้".to_string()),
            Self::Term(e) => (StatusCode::BAD_REQUEST, e.message()),
            Self::Taken => (
                StatusCode::CONFLICT,
                "มีคำนี้ใน glossary ชั้นนี้แล้ว แก้คำเดิมแทน".to_string(),
            ),
            Self::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

const COLUMNS: &str = "id, profile_id, source, target, created_at";

fn entry(r: &rusqlite::Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        profile_id: r.get(1)?,
        source: r.get(2)?,
        target: r.get(3)?,
        created_at: r.get(4)?,
    })
}

/// Every term of both layers, by source.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<Entry>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM glossary_terms ORDER BY source, profile_id"
    ))?;
    let rows = stmt.query_map([], entry)?;
    rows.collect()
}

pub fn find(conn: &Connection, id: &str) -> rusqlite::Result<Option<Entry>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM glossary_terms WHERE id = ?1"),
        [id],
        entry,
    )
    .optional()
}

/// Adds a term to the shared glossary (`profile_id` empty) or a game's.
pub fn add(
    conn: &Connection,
    profile_id: &str,
    source: &str,
    target: &str,
) -> Result<Entry, GlossaryError> {
    let term = core::clean(source, target).map_err(GlossaryError::Term)?;
    if !profile_id.is_empty() && profiles::find(conn, profile_id)?.is_none() {
        return Err(GlossaryError::NoProfile);
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO glossary_terms (id, profile_id, source, target, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, profile_id, term.source, term.target, db::now_ms()],
    )?;
    find(conn, &id)?.ok_or(GlossaryError::NotFound)
}

/// Changes the source and target of a term; its layer stays.
pub fn edit(
    conn: &Connection,
    id: &str,
    source: &str,
    target: &str,
) -> Result<Entry, GlossaryError> {
    let term = core::clean(source, target).map_err(GlossaryError::Term)?;
    let n = conn.execute(
        "UPDATE glossary_terms SET source = ?2, target = ?3 WHERE id = ?1",
        params![id, term.source, term.target],
    )?;
    if n == 0 {
        return Err(GlossaryError::NotFound);
    }
    find(conn, id)?.ok_or(GlossaryError::NotFound)
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), GlossaryError> {
    match conn.execute("DELETE FROM glossary_terms WHERE id = ?1", [id])? {
        0 => Err(GlossaryError::NotFound),
        _ => Ok(()),
    }
}

/// The glossary a game translates with: shared terms, overridden by the
/// game's own (FR-GLS-01).
pub fn terms(conn: &Connection, profile_id: &str) -> rusqlite::Result<Vec<Term>> {
    let mut stmt = conn.prepare(
        "SELECT profile_id, source, target FROM glossary_terms
         WHERE profile_id IN ('', ?1) ORDER BY created_at, rowid",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Term::new(&r.get::<_, String>(1)?, &r.get::<_, String>(2)?),
        ))
    })?;
    let (shared, game): (Vec<_>, Vec<_>) = rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .partition(|(p, _)| p.is_empty());
    let strip = |v: Vec<(String, Term)>| v.into_iter().map(|(_, t)| t).collect();
    Ok(core::merge(strip(shared), strip(game)))
}

/// The glossary of the profile in use; empty when there is none or it
/// cannot be read (the line is still translated, the error is logged).
pub fn current(app: &App) -> Vec<Term> {
    let Some(id) = app.state.borrow().profile.as_ref().map(|p| p.id.clone()) else {
        return Vec::new();
    };
    terms(&app.db(), &id).unwrap_or_else(|e| {
        eprintln!("glossary: {e}");
        Vec::new()
    })
}

/// Routes of this module; `server::router` merges them under its guard.
/// POST answers on the LAN too (add a term from a line on /reader); the rest
/// is loopback-only (`core::access`).
pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/glossary", get(list_handler).post(add_handler))
        .route(
            "/api/glossary/{id}",
            get(get_handler).put(edit_handler).delete(delete_handler),
        )
}

async fn list_handler(State(app): State<Arc<App>>) -> Result<Json<Vec<Entry>>, GlossaryError> {
    Ok(Json(list(&app.db())?))
}

async fn get_handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Entry>, GlossaryError> {
    Ok(Json(find(&app.db(), &id)?.ok_or(GlossaryError::NotFound)?))
}

#[derive(Deserialize)]
struct NewTerm {
    source: String,
    target: String,
    /// Missing, null or empty: the shared glossary.
    #[serde(default)]
    profile_id: Option<String>,
}

async fn add_handler(
    State(app): State<Arc<App>>,
    Json(body): Json<NewTerm>,
) -> Result<Response, GlossaryError> {
    let profile = body.profile_id.unwrap_or_default();
    let e = add(&app.db(), &profile, &body.source, &body.target)?;
    Ok((StatusCode::CREATED, Json(e)).into_response())
}

#[derive(Deserialize)]
struct Change {
    source: String,
    target: String,
}

async fn edit_handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(body): Json<Change>,
) -> Result<Json<Entry>, GlossaryError> {
    Ok(Json(edit(&app.db(), &id, &body.source, &body.target)?))
}

async fn delete_handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<StatusCode, GlossaryError> {
    delete(&app.db(), &id)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> (Connection, String) {
        let c = Connection::open_in_memory().unwrap();
        db::prepare(&c).unwrap();
        let id = db::last_profile(&c).unwrap().unwrap().id;
        (c, id)
    }

    #[test]
    fn game_term_overrides_shared_one_for_that_game_only() {
        let (c, game) = conn();
        let other = crate::profiles::create(&c, "Other").unwrap().id;
        add(&c, "", "Warden", "ผู้คุม").unwrap();
        add(&c, "", "Paimon", "ไพมอน").unwrap();
        add(&c, &game, "warden", "วอร์เดน").unwrap();
        assert_eq!(
            terms(&c, &game).unwrap(),
            vec![Term::new("Paimon", "ไพมอน"), Term::new("warden", "วอร์เดน")]
        );
        assert_eq!(
            terms(&c, &other).unwrap(),
            vec![Term::new("Warden", "ผู้คุม"), Term::new("Paimon", "ไพมอน")]
        );
    }

    #[test]
    fn source_is_unique_per_layer_ignoring_case() {
        let (c, game) = conn();
        add(&c, "", "Warden", "ผู้คุม").unwrap();
        assert_eq!(add(&c, "", "WARDEN", "x"), Err(GlossaryError::Taken));
        add(&c, &game, "Warden", "วอร์เดน").unwrap();
        let rhee = add(&c, "", "Rhee", "รี").unwrap();
        assert_eq!(edit(&c, &rhee.id, "warden", "x"), Err(GlossaryError::Taken));
    }

    #[test]
    fn add_checks_term_and_profile() {
        let (c, _) = conn();
        assert_eq!(
            add(&c, "", " ", "ก"),
            Err(GlossaryError::Term(TermError::EmptySource))
        );
        assert_eq!(add(&c, "nope", "A", "ก"), Err(GlossaryError::NoProfile));
        let e = add(&c, "", "  Captain   Rhee ", " กัปตันรี ").unwrap();
        assert_eq!(
            (e.source.as_str(), e.target.as_str()),
            ("Captain Rhee", "กัปตันรี")
        );
        assert_eq!(e.profile_id, "");
    }

    #[test]
    fn edit_find_and_delete() {
        let (c, game) = conn();
        let e = add(&c, &game, "Warden", "ผู้คุม").unwrap();
        let changed = edit(&c, &e.id, "Warden", "วอร์เดน").unwrap();
        assert_eq!(changed.target, "วอร์เดน");
        assert_eq!(changed.profile_id, game);
        assert_eq!(find(&c, &e.id).unwrap(), Some(changed));
        assert_eq!(list(&c).unwrap().len(), 1);
        assert_eq!(edit(&c, "nope", "A", "ก"), Err(GlossaryError::NotFound));
        assert_eq!(
            edit(&c, &e.id, "A", ""),
            Err(GlossaryError::Term(TermError::EmptyTarget))
        );
        delete(&c, &e.id).unwrap();
        assert_eq!(delete(&c, &e.id), Err(GlossaryError::NotFound));
        assert!(list(&c).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_profile_takes_its_terms_but_not_shared_ones() {
        let (c, _) = conn();
        let p = crate::profiles::create(&c, "B").unwrap().id;
        add(&c, &p, "Warden", "วอร์เดน").unwrap();
        add(&c, "", "Paimon", "ไพมอน").unwrap();
        crate::profiles::delete(&c, &p).unwrap();
        let left: Vec<String> = list(&c).unwrap().into_iter().map(|e| e.source).collect();
        assert_eq!(left, vec!["Paimon"]);
    }
}
