//! Measure what Discord Go Live would see: capture an OBS projector window + OBS app audio with
//! ScreenCaptureKit, find flash (video) and beep (audio) onsets, and map them to the Media Source
//! cursor polled over obs-websocket. All times are host-clock seconds (mach_absolute_time).
//! usage: av_probe <media input name> [seconds=40]
//! prints: v/a onset times, and per flash: video delay vs media time, audio - video offset.
use obws::Client;
use screencapturekit::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

extern "C" {
    fn sc_initialize_core_graphics();
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut [u32; 2]) -> i32;
}

fn host_secs() -> f64 {
    let mut tb = [0u32; 2];
    unsafe { mach_timebase_info(&mut tb) };
    (unsafe { mach_absolute_time() }) as f64 * tb[0] as f64 / tb[1] as f64 / 1e9
}

fn cm_secs(t: CMTime) -> f64 {
    t.value as f64 / t.timescale as f64
}

#[derive(Default)]
struct Onsets {
    video: Vec<f64>,
    audio: Vec<f64>,
    v_on: bool,
    a_last: f64,
    v_max: f64,
    a_bufs: u32,
    a_max: f32,
}

fn thr() -> f32 {
    std::env::var("THR").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05)
}

fn tone_amp(b: &[f32], f: f32, sr: f32) -> f32 {
    let w = 2.0 * std::f32::consts::PI * f / sr;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &x in b {
        let s0 = x + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    2.0 * (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / b.len() as f32
}

struct Probe(Arc<Mutex<Onsets>>);

impl SCStreamOutputTrait for Probe {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, kind: SCStreamOutputType) {
        let pts = cm_secs(sample.presentation_timestamp());
        let mut o = self.0.lock().unwrap();
        match kind {
            SCStreamOutputType::Screen => {
                let Some(pb) = sample.pixel_buffer() else { return };
                let Ok(g) = pb.lock_read_only() else { return };
                let Some(px) = (unsafe { g.as_slice() }) else { return };
                // mean of the green channel (BGRA) over the whole tiny frame
                let mean = px.iter().skip(1).step_by(4).map(|&v| v as u64).sum::<u64>() as f64 / (px.len() / 4) as f64;
                o.v_max = o.v_max.max(mean);
                if !o.v_on && mean > 60.0 {
                    o.video.push(pts);
                }
                o.v_on = mean > 60.0;
            }
            SCStreamOutputType::Audio | SCStreamOutputType::Microphone => {
                let Ok(list) = sample.audio_buffer_list() else { return };
                let Some(buf) = list.get(0) else { return };
                let data = buf.data();
                let samples: &[f32] = unsafe { std::slice::from_raw_parts(data.as_ptr().cast(), data.len() / 4) };
                o.a_bufs += 1;
                o.a_max = samples.iter().fold(o.a_max, |m, s| m.max(s.abs()));
                // 1 kHz tone amplitude per 5 ms block (Goertzel) so room noise doesn't trigger
                if std::env::var("DUMP").is_ok() {
                    for (k, b) in samples.chunks(240).enumerate() {
                        let a = tone_amp(b, 1000.0, 48_000.0);
                        if a > 0.001 {
                            eprintln!("amp {:.3} {:.4}", pts + (k * 240) as f64 / 48_000.0, a);
                        }
                    }
                }
                let hit = samples.chunks(240).position(|b| tone_amp(b, 1000.0, 48_000.0) > thr()).map(|k| k * 240);
                if let Some(i) = hit {
                    let t = pts + i as f64 / 48_000.0;
                    if t - o.a_last > 1.0 {
                        o.audio.push(t);
                    }
                    o.a_last = t;
                }
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe { sc_initialize_core_graphics() };
    let mut args = std::env::args().skip(1);
    let media = args.next().ok_or("media input name required")?;
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(40);

    let content = SCShareableContent::get()?;
    let window = content
        .windows()
        .into_iter()
        .filter(|w| w.owning_application().is_some_and(|a| a.bundle_identifier() == "com.obsproject.obs-studio"))
        .find(|w| w.title().is_some_and(|t| t.starts_with("Projector")))
        .ok_or("OBS projector window not found")?;
    eprintln!("window {:?}", window.title());
    let filter = SCContentFilter::create().with_window(&window).build()?;
    let config = SCStreamConfiguration::new()
        .with_width(64)
        .with_height(36)
        .with_pixel_format(PixelFormat::BGRA)
        .with_minimum_frame_interval(&CMTime { value: 1, timescale: 60, flags: 1, epoch: 0 })
        .with_captures_audio(true)
        .with_sample_rate(48_000)
        .with_channel_count(1)
        .with_excludes_current_process_audio(true);

    let onsets = Arc::new(Mutex::new(Onsets::default()));
    let mut stream = SCStream::new(&filter, &config)?;
    stream.add_output_handler(Probe(onsets.clone()), SCStreamOutputType::Screen)?;
    // A window-only filter delivered silent audio; app audio needs a display filter that includes the app.
    let app = window.owning_application().ok_or("no app")?;
    let display = content.displays().into_iter().next().ok_or("no display")?;
    let afilter = if std::env::var("ALL_AUDIO").is_ok() {
        SCContentFilter::create().with_display(&display).with_excluding_windows(&[]).build()?
    } else {
        SCContentFilter::create().with_display(&display).with_including_applications(&[&app], &[]).build()?
    };
    // MIC=1: acoustic loopback (OBS monitoring -> speakers -> mic) when system-audio capture is not permitted
    let mic = std::env::var("MIC").is_ok();
    let aconfig = if mic { config.clone().with_captures_microphone(true)? } else { config.clone() };
    let mut astream = SCStream::new(&afilter, &aconfig)?;
    let akind = if mic { SCStreamOutputType::Microphone } else { SCStreamOutputType::Audio };
    astream.add_output_handler(Probe(onsets.clone()), akind)?;

    // password: same lookup as obs_ocr
    let home = std::env::var("HOME")?;
    let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!(
        "{home}/Library/Application Support/obs-studio/plugin_config/obs-websocket/config.json"
    ))?)?;
    let c = Client::connect("localhost", 4455, cfg["server_password"].as_str()).await?;

    stream.start_capture()?;
    astream.start_capture()?;
    let mut cursor: Vec<(f64, f64)> = Vec::new(); // (host secs, media ms)
    let end = host_secs() + secs as f64;
    while host_secs() < end {
        let t0 = host_secs();
        let st = c.media_inputs().status(obws::requests::inputs::InputId::Name(&media)).await?;
        let t1 = host_secs();
        if let Some(cur) = st.cursor {
            cursor.push(((t0 + t1) / 2.0, cur.as_seconds_f64() * 1000.0));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stream.stop_capture()?;
    astream.stop_capture()?;

    let o = onsets.lock().unwrap();
    // host time -> media ms via nearest cursor sample (cursor advances 1:1 with host time while playing)
    let media_at = |h: f64| {
        let (ch, cm) = cursor.iter().min_by(|a, b| (a.0 - h).abs().total_cmp(&(b.0 - h).abs())).copied().unwrap();
        cm + (h - ch) * 1000.0
    };
    eprintln!("video onsets {} (max luma {:.0}) audio onsets {} (bufs {} max {:.3}) cursor samples {}", o.video.len(), o.v_max, o.audio.len(), o.a_bufs, o.a_max, cursor.len());
    for &v in &o.video {
        let m = media_at(v);
        // flashes happen at media 0, 10000, 20000 ... ms: delay = media time elapsed since the most recent one
        let delay = m.rem_euclid(10000.0);
        let a = o.audio.iter().map(|&a| a - v).min_by(|x, y| x.abs().total_cmp(&y.abs())).unwrap_or(f64::NAN);
        println!("flash@media {:>7.0} ms  video_delay {:>6.0} ms  audio-video {:>+6.0} ms  host {v:.4}", m - delay, delay, a * 1000.0);
    }
    Ok(())
}
