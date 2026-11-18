//! The real OBS: obs-websocket 5 through obws (pinned =0.15.0, which checks
//! OBS ≥ 30.2 and obs-websocket ^5.5 on connect).

use super::{Connect, Obs, ObsError};
use crate::core::error::AppError;
use crate::core::obs::{
    Action, Filter, Input, SceneItem, SceneView, CARD_KINDS, CROP_FILTER, CROP_KIND,
};
use crate::core::obs_stream::{
    AudioDevice, AudioInput, Mic, Placement, StreamView, AUDIO_KIND, CARD_AUDIO, OVERLAY,
};
use base64::Engine;
use futures_util::future::BoxFuture;
use obws::common::{Alignment, BoundsType, MonitorType};
use obws::requests::filters::{
    Create as CreateFilter, SetEnabled as SetFilterEnabled, SetSettings as SetFilterSettings,
};
use obws::requests::inputs::{Create as CreateInput, InputId, SetSettings as SetInputSettings};
use obws::requests::scene_items::{
    Bounds, CreateSceneItem, Crop as ItemCrop, Position, Scale, SceneItemTransform,
    SetEnabled as SetItemEnabled, SetIndex, SetTransform,
};
use obws::requests::scenes::SceneId;
use obws::requests::sources::{SourceId, TakeScreenshot};
use obws::responses::scene_items::SceneItemTransform as Transform;
use obws::responses::StatusCode;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

/// A request that takes longer than this counts as OBS gone.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// OBS on this Mac: ws://localhost:4455, password from obs-websocket's config.
pub struct Local;

impl Connect for Local {
    fn connect(&self) -> BoxFuture<'_, Result<Box<dyn Obs>, AppError>> {
        Box::pin(async {
            let client = tokio::time::timeout(
                REQUEST_TIMEOUT,
                obws::Client::connect("localhost", 4455, password()),
            )
            .await
            .map_err(|_| AppError::obs_unreachable())?
            .map_err(|e| match e {
                obws::error::Error::ObsStudioVersion(v, _)
                | obws::error::Error::ObsWebsocketVersion(v, _) => {
                    AppError::obs_version(&v.to_string())
                }
                obws::error::Error::RpcVersion { negotiated, .. } => {
                    AppError::obs_version(&format!("rpc {negotiated}"))
                }
                _ => AppError::obs_unreachable(),
            })?;
            Ok(Box::new(Ws(client)) as Box<dyn Obs>)
        })
    }
}

/// `server_password` when obs-websocket requires auth.
fn password() -> Option<String> {
    let path = PathBuf::from(std::env::var_os("HOME")?)
        .join("Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json");
    let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    if !cfg["auth_required"].as_bool()? {
        return None;
    }
    cfg["server_password"].as_str().map(str::to_owned)
}

struct Ws(obws::Client);

async fn settings(c: &obws::Client, name: &str) -> Result<serde_json::Value, ObsError> {
    Ok(call(
        c.inputs()
            .settings::<serde_json::Value>(InputId::Name(name)),
    )
    .await?
    .settings)
}

fn placement(t: Transform) -> Placement {
    let (width, height) = if t.bounds_type == BoundsType::None {
        (t.width, t.height)
    } else {
        (t.bounds_width, t.bounds_height)
    };
    Placement {
        x: t.position_x.into(),
        y: t.position_y.into(),
        rotation: t.rotation.into(),
        top_left: t.alignment == Alignment::LEFT | Alignment::TOP,
        width: width.into(),
        height: height.into(),
        cropped: t.crop_left + t.crop_right + t.crop_top + t.crop_bottom > 0,
    }
}

/// Top-left at (0, 0), stretched to the canvas, nothing cut or turned.
fn full_frame(canvas: (u32, u32)) -> SceneItemTransform {
    SceneItemTransform {
        position: Some(Position {
            x: Some(0.0),
            y: Some(0.0),
        }),
        rotation: Some(0.0),
        scale: Some(Scale {
            x: Some(1.0),
            y: Some(1.0),
        }),
        alignment: Some(Alignment::LEFT | Alignment::TOP),
        bounds: Some(Bounds {
            r#type: Some(BoundsType::Stretch),
            alignment: Some(Alignment::CENTER),
            width: Some(canvas.0 as f32),
            height: Some(canvas.1 as f32),
        }),
        crop: Some(ItemCrop {
            left: Some(0),
            right: Some(0),
            top: Some(0),
            bottom: Some(0),
        }),
    }
}

async fn call<T>(req: impl Future<Output = obws::error::Result<T>>) -> Result<T, ObsError> {
    match tokio::time::timeout(REQUEST_TIMEOUT, req).await {
        Err(_) => Err(ObsError::Unreachable),
        Ok(Ok(v)) => Ok(v),
        Ok(Err(obws::error::Error::Api {
            code: StatusCode::ResourceNotFound,
            ..
        })) => Err(ObsError::NotFound),
        Ok(Err(obws::error::Error::Api { code, message })) => Err(ObsError::Failed(format!(
            "{code:?} {}",
            message.unwrap_or_default()
        ))),
        Ok(Err(_)) => Err(ObsError::Unreachable),
    }
}

impl Obs for Ws {
    fn inputs(&self) -> BoxFuture<'_, Result<Vec<Input>, ObsError>> {
        Box::pin(async move {
            let mut out = Vec::new();
            for i in call(self.0.inputs().list(None)).await? {
                let device_name = if CARD_KINDS.contains(&i.unversioned_kind.as_str()) {
                    let s = call(
                        self.0
                            .inputs()
                            .settings::<serde_json::Value>(InputId::Name(&i.id.name)),
                    )
                    .await?;
                    s.settings["device_name"].as_str().map(str::to_owned)
                } else {
                    None
                };
                out.push(Input {
                    name: i.id.name,
                    kind: i.unversioned_kind,
                    device_name,
                });
            }
            Ok(out)
        })
    }

    fn canvas(&self) -> BoxFuture<'_, Result<(u32, u32), ObsError>> {
        Box::pin(async move {
            let v = call(self.0.config().video_settings()).await?;
            Ok((v.base_width, v.base_height))
        })
    }

    fn scene<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<SceneView>, ObsError>> {
        Box::pin(async move {
            let scenes = call(self.0.scenes().list()).await?;
            if !scenes.scenes.iter().any(|s| s.id.name == name) {
                return Ok(None);
            }
            let mut items = Vec::new();
            for it in call(self.0.scene_items().list(SceneId::Name(name))).await? {
                let enabled =
                    call(self.0.scene_items().enabled(SceneId::Name(name), it.id)).await?;
                items.push(SceneItem {
                    id: it.id,
                    source: it.source_name,
                    enabled,
                });
            }
            let filters = call(self.0.filters().list(SourceId::Name(name)))
                .await?
                .into_iter()
                .map(|f| Filter {
                    name: f.name,
                    kind: f.kind,
                    enabled: f.enabled,
                    settings: f.settings,
                })
                .collect();
            Ok(Some(SceneView { items, filters }))
        })
    }

    fn stream(&self) -> BoxFuture<'_, Result<StreamView, ObsError>> {
        Box::pin(async move {
            let c = &self.0;
            let program = call(c.scenes().current_program_scene()).await?.id.name;
            let mut listed = call(c.scene_items().list(SceneId::Name(&program))).await?;
            listed.sort_by_key(|it| it.index);
            let mut items = Vec::new();
            for it in listed {
                let enabled = call(c.scene_items().enabled(SceneId::Name(&program), it.id)).await?;
                items.push(SceneItem {
                    id: it.id,
                    source: it.source_name,
                    enabled,
                });
            }
            let overlay_placement = match items.iter().rev().find(|i| i.source == OVERLAY) {
                Some(it) => Some(placement(
                    call(c.scene_items().transform(SceneId::Name(&program), it.id)).await?,
                )),
                None => None,
            };
            let mut view = StreamView {
                program,
                items,
                overlay_placement,
                ..StreamView::default()
            };
            let mut ask_devices = None;
            for i in call(c.inputs().list(None)).await? {
                let name = i.id.name;
                if name == OVERLAY {
                    view.overlay = Some(settings(c, &name).await?);
                }
                if i.unversioned_kind != AUDIO_KIND {
                    continue;
                }
                let muted = call(c.inputs().muted(InputId::Name(&name))).await?;
                if name == CARD_AUDIO {
                    view.card_audio = Some(AudioInput {
                        device_id: settings(c, &name).await?["device_id"]
                            .as_str()
                            .map(str::to_owned),
                        monitor_only: call(c.inputs().audio_monitor_type(InputId::Name(&name)))
                            .await?
                            == MonitorType::MonitorOnly,
                        muted,
                    });
                    ask_devices = Some(name);
                } else {
                    ask_devices.get_or_insert_with(|| name.clone());
                    view.mics.push(Mic { name, muted });
                }
            }
            // OBS lists devices only through an existing Audio Input Capture.
            if let Some(name) = ask_devices {
                view.devices = call(
                    c.inputs()
                        .properties_list_property_items(InputId::Name(&name), "device_id"),
                )
                .await?
                .into_iter()
                .filter_map(|d| {
                    Some(AudioDevice {
                        id: d.value.as_str()?.to_owned(),
                        name: d.name,
                    })
                })
                .collect();
            }
            Ok(view)
        })
    }

    fn apply<'a>(
        &'a self,
        scene: &'a str,
        action: &'a Action,
    ) -> BoxFuture<'a, Result<(), ObsError>> {
        Box::pin(async move {
            let c = &self.0;
            match action {
                Action::CreateInput {
                    name,
                    kind,
                    settings,
                } => call(c.inputs().create(CreateInput {
                    scene: SceneId::Name(scene),
                    input: name,
                    kind,
                    settings: Some(settings),
                    enabled: Some(true),
                }))
                .await
                .map(drop),
                Action::SetInputSettings { name, settings } => {
                    call(c.inputs().set_settings(SetInputSettings {
                        input: InputId::Name(name),
                        settings,
                        overlay: Some(true),
                    }))
                    .await
                }
                Action::FitToCanvas { source, canvas } => {
                    let items = call(c.scene_items().list(SceneId::Name(scene))).await?;
                    let top = items
                        .iter()
                        .filter(|i| i.source_name == *source)
                        .max_by_key(|i| i.index)
                        .ok_or(ObsError::NotFound)?;
                    call(c.scene_items().set_transform(SetTransform {
                        scene: SceneId::Name(scene),
                        item_id: top.id,
                        transform: full_frame(*canvas),
                    }))
                    .await
                }
                Action::MoveToTop { id } => {
                    let count = call(c.scene_items().list(SceneId::Name(scene)))
                        .await?
                        .len();
                    call(c.scene_items().set_index(SetIndex {
                        scene: SceneId::Name(scene),
                        item_id: *id,
                        index: u32::try_from(count.saturating_sub(1)).unwrap_or(u32::MAX),
                    }))
                    .await
                }
                Action::SetMonitorOnly { name } => {
                    call(
                        c.inputs()
                            .set_audio_monitor_type(InputId::Name(name), MonitorType::MonitorOnly),
                    )
                    .await
                }
                Action::SetMuted { name, muted } => {
                    call(c.inputs().set_muted(InputId::Name(name), *muted)).await
                }
                Action::CreateScene => call(c.scenes().create(scene)).await.map(drop),
                Action::AddItem { source } => call(c.scene_items().create(CreateSceneItem {
                    scene: SceneId::Name(scene),
                    source: SourceId::Name(source),
                    enabled: Some(true),
                }))
                .await
                .map(drop),
                Action::SetItemEnabled { id, enabled } => {
                    call(c.scene_items().set_enabled(SetItemEnabled {
                        scene: SceneId::Name(scene),
                        item_id: *id,
                        enabled: *enabled,
                    }))
                    .await
                }
                Action::CreateCrop { settings } => {
                    call(c.filters().create(CreateFilter {
                        source: SourceId::Name(scene),
                        filter: CROP_FILTER,
                        kind: CROP_KIND,
                        settings: Some(settings),
                    }))
                    .await
                }
                Action::SetCropSettings { settings } => {
                    call(c.filters().set_settings(SetFilterSettings {
                        source: SourceId::Name(scene),
                        filter: CROP_FILTER,
                        settings,
                        overlay: Some(true),
                    }))
                    .await
                }
                Action::SetFilterEnabled { name, enabled } => {
                    call(c.filters().set_enabled(SetFilterEnabled {
                        source: SourceId::Name(scene),
                        filter: name,
                        enabled: *enabled,
                    }))
                    .await
                }
            }
        })
    }

    fn screenshot<'a>(
        &'a self,
        source: &'a str,
        size: (u32, u32),
    ) -> BoxFuture<'a, Result<Vec<u8>, ObsError>> {
        Box::pin(async move {
            let uri = call(self.0.sources().take_screenshot(TakeScreenshot {
                source: SourceId::Name(source),
                format: "jpg",
                // A scene has no size of its own: without one OBS fails (702).
                width: Some(size.0),
                height: Some(size.1),
                compression_quality: None,
            }))
            .await?;
            let b64 = uri.split_once(',').map_or(uri.as_str(), |(_, d)| d);
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| ObsError::Failed(format!("screenshot base64: {e}")))
        })
    }
}
