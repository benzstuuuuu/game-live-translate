//! The translation section of /setup (FR-SET-03, 07): settings, the Gemini
//! key, the Claude key (FR-TRN-14) and the models of the Claude Code and
//! Codex CLIs (FR-TRN-15). Loopback-only like every API not
//! listed in `core::access` (FR-SET-08). No response ever carries a key
//! (NFR-07).

use crate::app::App;
use crate::core::claude::MODEL;
use crate::core::gemini::Patch;
use crate::translate::{Claude, ClaudeCli, CodexCli, Gemini};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type Shared = State<(Arc<App>, Gemini)>;

/// Routes to merge into `server::router_with`.
pub fn routes(app: Arc<App>, gemini: Gemini) -> Router {
    Router::new()
        .route("/api/settings", get(read_settings).put(change_settings))
        .route("/api/secret/gemini-key", put(save_key))
        .route("/api/secret/gemini-key/test", post(test_key))
        .with_state((app, gemini))
}

async fn read_settings(State((_, gemini)): Shared) -> Json<Value> {
    Json(gemini.view().await)
}

/// Changes only the keys sent (FR-SET-06).
async fn change_settings(State((_, gemini)): Shared, Json(body): Json<Value>) -> Response {
    let Ok(patch) = serde_json::from_value::<Patch>(body) else {
        return bad_request("ค่าตั้งไม่ถูกต้อง".into());
    };
    match gemini.update(patch) {
        Ok(_) => Json(gemini.view().await).into_response(),
        Err(reason) => bad_request(reason),
    }
}

#[derive(Deserialize)]
struct KeyBody {
    key: Option<String>,
}

async fn save_key(State((app, gemini)): Shared, Json(body): Json<KeyBody>) -> Response {
    if let Err(reason) = gemini
        .save_key(body.key.as_deref().unwrap_or_default())
        .await
    {
        return bad_request(reason);
    }
    // A missing key may be why Gemini was marked unavailable; look again.
    if app.state.borrow().translator == "gemini" {
        let _ = app.check_translator().await;
    }
    Json(json!({ "key_saved": true })).into_response()
}

/// Tests the key typed in the field, or the saved one when the field is empty.
async fn test_key(State((_, gemini)): Shared, body: Option<Json<KeyBody>>) -> Response {
    let typed = body.and_then(|Json(b)| b.key);
    match gemini.test_key(typed.as_deref()).await {
        Ok(result) => Json(result).into_response(),
        Err(reason) => bad_request(reason),
    }
}

type WithClaude = State<(Arc<App>, Claude)>;

/// The Claude key's routes, to merge beside `routes`. Tone, context lines
/// and extra prompt stay in /api/settings: Claude reads the same ones.
pub fn claude_routes(app: Arc<App>, claude: Claude) -> Router {
    Router::new()
        .route(
            "/api/secret/anthropic-key",
            get(claude_key_state).put(save_claude_key),
        )
        .route("/api/secret/anthropic-key/test", post(test_claude_key))
        .with_state((app, claude))
}

async fn claude_key_state(State((_, claude)): WithClaude) -> Json<Value> {
    Json(json!({ "key_saved": claude.key_saved().await, "model": MODEL }))
}

async fn save_claude_key(State((app, claude)): WithClaude, Json(body): Json<KeyBody>) -> Response {
    if let Err(reason) = claude
        .save_key(body.key.as_deref().unwrap_or_default())
        .await
    {
        return bad_request(reason);
    }
    // A missing key may be why Claude was marked unavailable; look again.
    if app.state.borrow().translator == "claude" {
        let _ = app.check_translator().await;
    }
    Json(json!({ "key_saved": true })).into_response()
}

/// Tests the key typed in the field, or the saved one when the field is empty.
async fn test_claude_key(State((_, claude)): WithClaude, body: Option<Json<KeyBody>>) -> Response {
    let typed = body.and_then(|Json(b)| b.key);
    match claude.test_key(typed.as_deref()).await {
        Ok(result) => Json(result).into_response(),
        Err(reason) => bad_request(reason),
    }
}

type WithCli = State<(Arc<App>, ClaudeCli)>;

/// The Claude Code CLI's model and readiness (FR-TRN-15), to merge beside
/// `routes`. It has no key: it uses the login of the `claude` on this Mac.
pub fn claude_cli_routes(app: Arc<App>, cli: ClaudeCli) -> Router {
    Router::new()
        .route("/api/claude-cli", get(cli_state).put(change_cli))
        .with_state((app, cli))
}

async fn cli_state(State((_, cli)): WithCli) -> Json<Value> {
    Json(cli.view().await)
}

#[derive(Deserialize)]
struct CliBody {
    model: Option<String>,
}

async fn change_cli(State((app, cli)): WithCli, Json(body): Json<CliBody>) -> Response {
    if let Err(reason) = cli.set_model(body.model.as_deref().unwrap_or_default()) {
        return bad_request(reason);
    }
    // The CLI may have been fixed meanwhile; look again if it is in use.
    if app.state.borrow().translator == "claude-cli" {
        let _ = app.check_translator().await;
    }
    Json(cli.view().await).into_response()
}

type WithCodex = State<(Arc<App>, CodexCli)>;

/// The Codex CLI's model and readiness (FR-TRN-15), to merge beside
/// `routes`. It has no key: it uses the login of the `codex` on this Mac.
pub fn codex_cli_routes(app: Arc<App>, codex: CodexCli) -> Router {
    Router::new()
        .route("/api/codex-cli", get(codex_state).put(change_codex))
        .with_state((app, codex))
}

async fn codex_state(State((_, codex)): WithCodex) -> Json<Value> {
    Json(codex.view().await)
}

async fn change_codex(State((app, codex)): WithCodex, Json(body): Json<CliBody>) -> Response {
    if let Err(reason) = codex.set_model(body.model.as_deref().unwrap_or_default()) {
        return bad_request(reason);
    }
    // Codex may have been fixed meanwhile; look again if it is in use.
    if app.state.borrow().translator == "codex-cli" {
        let _ = app.check_translator().await;
    }
    Json(codex.view().await).into_response()
}

fn bad_request(reason: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": reason }))).into_response()
}
