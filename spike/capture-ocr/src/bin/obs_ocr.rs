//! Read frames from OBS with obs-websocket GetSourceScreenshot and OCR the subtitle rect.
//! usage: obs_ocr <full|crop> [format=png] [seconds=60] [fps=4] [quality=-1]
//!   full: screenshot the capture source at 1920x1080, crop the rect here
//!   crop: screenshot scene "yokoten-ocr" whose Crop/Pad filter already cuts the rect in OBS
//! prints `t \t shot_ms \t prep_ms \t ocr_ms \t text`, then OBS stats and this process's CPU.
use apple_vision::prelude::*;
use base64::Engine;
use obws::requests::filters::Create as CreateFilter;
use obws::requests::scene_items::CreateSceneItem;
use obws::requests::sources::{SourceId, TakeScreenshot};
use obws::requests::scenes::SceneId;
use obws::Client;
use std::time::{Duration, Instant};

const SOURCE: &str = "Capture Card Device";
const OCR_SCENE: &str = "yokoten-ocr";
// Subtitle rect on the 1920x1080 canvas: 180 px tall so two-line dialogue keeps its descenders.
const RECT: (u32, u32, u32, u32) = (240, 790, 1440, 180);

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn password() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let path = format!("{home}/Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json");
    let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    cfg["auth_required"].as_bool()?.then(|| cfg["server_password"].as_str().map(String::from))?
}

/// Scene "yokoten-ocr" = capture source + Crop/Pad filter on the scene itself. Errors if it already exists are fine.
async fn ensure_ocr_scene(c: &Client) {
    let (x, y, w, h) = RECT;
    let _ = c.scenes().create(OCR_SCENE).await;
    let _ = c
        .scene_items()
        .create(CreateSceneItem { scene: SceneId::Name(OCR_SCENE), source: SourceId::Name(SOURCE), enabled: Some(true) })
        .await;
    let settings = serde_json::json!({ "relative": false, "left": x, "top": y, "cx": w, "cy": h });
    let _ = c
        .filters()
        .create(CreateFilter { source: SourceId::Name(OCR_SCENE), filter: "ocr-crop", kind: "crop_filter", settings: Some(settings) })
        .await;
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "full".into());
    let format = args.next().unwrap_or_else(|| "png".into());
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let fps: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4);
    let quality: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(-1);

    let c = Client::connect("localhost", 4455, password()).await?;
    // OCR_SCENE env picks another pre-built crop scene (e.g. one fed by a fixture Media Source).
    let ocr_scene = std::env::var("OCR_SCENE").unwrap_or_else(|_| OCR_SCENE.into());
    let source = if mode == "crop" {
        if ocr_scene == OCR_SCENE {
            ensure_ocr_scene(&c).await;
        }
        ocr_scene.as_str()
    } else {
        SOURCE
    };
    let rec = TextRecognizer::new().with_recognition_level(RecognitionLevel::Accurate).with_language_correction(true);
    std::fs::create_dir_all("out")?;
    let img_path = format!("out/obs_{mode}_{format}.png");

    let s0 = c.general().stats().await?;
    let (wall, cpu0) = (Instant::now(), cpu_secs());
    let mut tick = tokio::time::interval(Duration::from_millis(1000 / fps));
    let mut obs_cpu = Vec::new();
    let mut n = 0u32;
    while wall.elapsed() < Duration::from_secs(secs) {
        tick.tick().await;
        let t = Instant::now();
        let uri = c
            .sources()
            .take_screenshot(TakeScreenshot {
                source: SourceId::Name(source),
                format: &format,
                // A scene has no natural size for GetSourceScreenshot; without an explicit size OBS fails with code 702.
                width: (mode == "crop").then_some(RECT.2),
                height: (mode == "crop").then_some(RECT.3),
                compression_quality: Some(quality),
            })
            .await?;
        let shot_ms = ms(t);

        let t = Instant::now();
        let b64 = uri.split_once(",").map_or(uri.as_str(), |(_, d)| d);
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
        let img = image::load_from_memory(&bytes)?;
        let img = if mode == "crop" { img } else { img.crop_imm(RECT.0, RECT.1, RECT.2, RECT.3) };
        if n == 0 {
            eprintln!("payload {} KiB, image {}x{}", bytes.len() / 1024, img.width(), img.height());
        }
        img.save(&img_path)?;
        let prep_ms = ms(t);

        let t = Instant::now();
        let mut obs = rec.recognize_in_path(&img_path).map_err(|e| format!("ocr: {e:?}"))?;
        let ocr_ms = ms(t);
        obs.sort_by(|a, b| b.bounding_box.y.total_cmp(&a.bounding_box.y));
        let text = obs.iter().map(|o| o.text.as_str()).collect::<Vec<_>>().join(" | ");
        println!("{:.2}\t{shot_ms:.1}\t{prep_ms:.1}\t{ocr_ms:.1}\t{text}", wall.elapsed().as_secs_f64());

        if n % fps as u32 == 0 {
            obs_cpu.push(c.general().stats().await?.cpu_usage);
        }
        n += 1;
    }
    let s1 = c.general().stats().await?;
    let cpu = (cpu_secs() - cpu0) / wall.elapsed().as_secs_f64() * 100.0;
    let avg = obs_cpu.iter().sum::<f64>() / obs_cpu.len().max(1) as f64;
    eprintln!(
        "frames {n} in {secs}s | OBS cpu avg {avg:.1}% | OBS render skipped {} / {} | avg render {:.2} ms | active fps {:.1} | obs_ocr cpu {cpu:.1}%",
        s1.render_skipped_frames - s0.render_skipped_frames,
        s1.render_total_frames - s0.render_total_frames,
        s1.average_frame_render_time,
        s1.active_fps,
    );
    Ok(())
}

fn cpu_secs() -> f64 {
    let mut ru: Rusage = unsafe { std::mem::zeroed() };
    unsafe { getrusage(0, &mut ru) };
    let tv = |s: i64, us: i32| s as f64 + us as f64 / 1e6;
    tv(ru.ru_utime.0, ru.ru_utime.1) + tv(ru.ru_stime.0, ru.ru_stime.1)
}
#[repr(C)]
struct Rusage {
    ru_utime: (i64, i32),
    ru_stime: (i64, i32),
    _rest: [i64; 14],
}
extern "C" {
    fn getrusage(who: i32, usage: *mut Rusage) -> i32;
}
