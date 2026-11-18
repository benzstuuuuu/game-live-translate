//! Capture the subtitle rect of the Consolation window with ScreenCaptureKit and OCR each frame.
//! usage: [VIEWER=obs] live_capture <window|display> [seconds=20] [fps=4]
//! prints `t_s \t ocr_ms \t text` per frame, then CPU% of this process; dumps first frame to out/live_first.bgra
use apple_vision::prelude::*;
use screencapturekit::cg::{CGPoint, CGRect, CGSize};
use screencapturekit::prelude::*;
use screencapturekit::CVPixelBuffer;
use std::sync::Mutex;
use std::time::Instant;

// Subtitle rect as a fraction of the 16:9 game picture (Genshin dialogue box, measured on the fixture).
const RECT: (f64, f64, f64, f64) = (240.0 / 1920.0, 805.0 / 1080.0, 1440.0 / 1920.0, 150.0 / 1080.0);
const OUT_W: u32 = 1440;
const OUT_H: u32 = 150;

struct Ocr {
    rec: TextRecognizer,
    start: Instant,
    first: Mutex<bool>,
    ms: Mutex<Vec<f64>>,
}

impl SCStreamOutputTrait for Ocr {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, _t: SCStreamOutputType) {
        let Some(pb) = sample.pixel_buffer() else { return };
        if std::mem::take(&mut *self.first.lock().unwrap()) {
            dump(&pb);
        }
        let t = Instant::now();
        let obs = match self.rec.recognize_in_pixel_buffer(&pb) {
            Ok(o) => o,
            Err(e) => return eprintln!("ocr error: {e:?}"),
        };
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.ms.lock().unwrap().push(ms);
        let mut obs = obs;
        obs.sort_by(|a, b| b.bounding_box.y.total_cmp(&a.bounding_box.y));
        let text = obs.iter().map(|o| o.text.as_str()).collect::<Vec<_>>().join(" | ");
        println!("{:.2}\t{:.1}\t{}", self.start.elapsed().as_secs_f64(), ms, text);
    }
}

fn dump(pb: &CVPixelBuffer) {
    let Ok(g) = pb.lock_read_only() else { return };
    let (w, h, bpr) = (g.width(), g.height(), g.bytes_per_row());
    let Some(src) = (unsafe { g.as_slice() }) else { return };
    let rows: Vec<u8> = (0..h).flat_map(|y| src[y * bpr..y * bpr + w * 4].iter().copied()).collect();
    let _ = std::fs::write("out/live_first.bgra", rows);
    eprintln!("first frame {w}x{h} -> out/live_first.bgra");
}

fn cpu_secs() -> f64 {
    let mut ru: libc_rusage = unsafe { std::mem::zeroed() };
    unsafe { getrusage(0, &mut ru) };
    let tv = |s: i64, us: i32| s as f64 + us as f64 / 1e6;
    tv(ru.ru_utime.0, ru.ru_utime.1) + tv(ru.ru_stime.0, ru.ru_stime.1)
}
#[repr(C)]
struct libc_rusage { ru_utime: (i64, i32), ru_stime: (i64, i32), _rest: [i64; 14] }
extern "C" { fn getrusage(who: i32, usage: *mut libc_rusage) -> i32; }

extern "C" { fn sc_initialize_core_graphics(); }

fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe { sc_initialize_core_graphics() };
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "window".into());
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    let fps: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4);

    // OBS: only its "Projector - …" window shows the game; the main OBS window is the editor.
    let obs = std::env::var("VIEWER").is_ok_and(|v| v == "obs");
    let bundle = if obs { "com.obsproject.obs-studio" } else { "org.centennialoss.consolation" };
    let content = SCShareableContent::get()?;
    let window = content
        .windows()
        .into_iter()
        .filter(|w| w.owning_application().is_some_and(|a| a.bundle_identifier() == bundle))
        .filter(|w| !obs || w.title().is_some_and(|t| t.starts_with("Projector")))
        .max_by(|a, b| (a.frame().size.width * a.frame().size.height).total_cmp(&(b.frame().size.width * b.frame().size.height)))
        .ok_or("viewer window not found")?;
    let wf = window.frame();
    eprintln!("window id={} on_screen={} frame=({},{} {}x{})", window.window_id(), window.is_on_screen(), wf.origin.x, wf.origin.y, wf.size.width, wf.size.height);

    // Fit a 16:9 picture inside the window (letterbox), then take RECT of that picture.
    let pic_w = wf.size.width.min(wf.size.height * 16.0 / 9.0);
    let pic_h = pic_w * 9.0 / 16.0;
    let (px, py) = ((wf.size.width - pic_w) / 2.0, (wf.size.height - pic_h) / 2.0);
    let local = CGRect {
        origin: CGPoint { x: px + RECT.0 * pic_w, y: py + RECT.1 * pic_h },
        size: CGSize { width: RECT.2 * pic_w, height: RECT.3 * pic_h },
    };

    let (filter, src) = if mode == "display" {
        let display = content
            .displays()
            .into_iter()
            .find(|d| {
                let f = d.frame();
                wf.origin.x >= f.origin.x && wf.origin.x < f.origin.x + f.size.width
                    && wf.origin.y >= f.origin.y && wf.origin.y < f.origin.y + f.size.height
            })
            .ok_or("display for window not found")?;
        let df = display.frame();
        eprintln!("display id={} frame=({},{} {}x{})", display.display_id(), df.origin.x, df.origin.y, df.size.width, df.size.height);
        let src = CGRect {
            origin: CGPoint { x: wf.origin.x - df.origin.x + local.origin.x, y: wf.origin.y - df.origin.y + local.origin.y },
            size: local.size,
        };
        (SCContentFilter::create().with_display(&display).with_excluding_windows(&[]).build()?, src)
    } else {
        (SCContentFilter::create().with_window(&window).build()?, local)
    };
    eprintln!("source_rect=({:.0},{:.0} {:.0}x{:.0})", src.origin.x, src.origin.y, src.size.width, src.size.height);

    let config = SCStreamConfiguration::new()
        .with_width(OUT_W)
        .with_height(OUT_H)
        .with_source_rect(src)
        .with_pixel_format(PixelFormat::BGRA)
        .with_shows_cursor(false)
        .with_minimum_frame_interval(&CMTime { value: 1, timescale: fps, flags: 1, epoch: 0 });

    let handler = Ocr {
        rec: TextRecognizer::new().with_recognition_level(RecognitionLevel::Accurate).with_language_correction(true),
        start: Instant::now(),
        first: Mutex::new(true),
        ms: Mutex::new(Vec::new()),
    };
    let mut stream = SCStream::new(&filter, &config)?;
    stream.add_output_handler(handler, SCStreamOutputType::Screen)?;
    let (wall, cpu0) = (Instant::now(), cpu_secs());
    stream.start_capture()?;
    std::thread::sleep(std::time::Duration::from_secs(secs));
    stream.stop_capture()?;
    let cpu = (cpu_secs() - cpu0) / wall.elapsed().as_secs_f64() * 100.0;
    eprintln!("cpu {cpu:.1}% of one core over {secs}s");
    Ok(())
}
