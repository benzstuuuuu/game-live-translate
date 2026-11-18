//! Apple Vision OCR. Every call runs inside an autorelease pool: without it
//! the spike leaked 1.4 GB in 11 minutes.

use crate::core::obs::Region;
use crate::frames::Frame;
use apple_cf::cv::CVPixelBuffer;
use apple_vision::prelude::*;
use std::path::Path;

/// kCVPixelFormatType_32BGRA.
const BGRA: u32 = 0x4247_5241;

/// Text lines of a frame, top to bottom.
pub fn read_frame(frame: &Frame) -> Result<Vec<String>, String> {
    match frame {
        Frame::File(path) => read(path),
        Frame::Image(bytes) => read_image(bytes),
    }
}

/// Text lines of an image file, top to bottom.
pub fn read(path: &Path) -> Result<Vec<String>, String> {
    let obs = objc2::rc::autoreleasepool(|_| recognizer().recognize_in_path(path))
        .map_err(|e| format!("OCR {}: {e:?}", path.display()))?;
    Ok(top_to_bottom(obs))
}

/// Text lines of encoded JPG/PNG bytes, decoded in memory: no file on disk.
pub fn read_image(bytes: &[u8]) -> Result<Vec<String>, String> {
    let rgba = image::load_from_memory(bytes)
        .map_err(|e| format!("decode frame: {e}"))?
        .to_rgba8();
    Ok(read_pixels(&rgba)?.into_iter().map(|s| s.text).collect())
}

/// A line of text and where it sits, as fractions of the image (origin
/// top-left).
#[derive(Debug, Clone)]
pub struct Seen {
    pub text: String,
    pub at: Region,
}

/// Text lines of an in-memory picture with their boxes, top to bottom.
pub fn read_pixels(img: &image::RgbaImage) -> Result<Vec<Seen>, String> {
    let buffer = pixel_buffer(img)?;
    let obs = objc2::rc::autoreleasepool(|_| recognizer().recognize_in_pixel_buffer(&buffer))
        .map_err(|e| format!("OCR frame: {e:?}"))?;
    Ok(sorted(obs)
        .into_iter()
        .map(|o| {
            let b = o.bounding_box;
            Seen {
                text: o.text,
                at: Region::from_vision(b.x, b.y, b.width, b.height),
            }
        })
        .collect())
}

fn recognizer() -> TextRecognizer {
    TextRecognizer::new()
        .with_recognition_level(RecognitionLevel::Accurate)
        .with_language_correction(true)
}

fn top_to_bottom(obs: Vec<RecognizedText>) -> Vec<String> {
    sorted(obs).into_iter().map(|o| o.text).collect()
}

fn sorted(mut obs: Vec<RecognizedText>) -> Vec<RecognizedText> {
    // Vision's y origin is bottom-left: higher y is higher on screen.
    obs.sort_by(|a, b| b.bounding_box.y.total_cmp(&a.bounding_box.y));
    obs
}

/// Copy RGBA pixels into a new BGRA CVPixelBuffer, row by row (CoreVideo may
/// pad rows).
fn pixel_buffer(img: &image::RgbaImage) -> Result<CVPixelBuffer, String> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let buffer = CVPixelBuffer::create(w, h, BGRA).map_err(|s| format!("CVPixelBuffer {s}"))?;
    let mut guard = buffer
        .lock_read_write()
        .map_err(|s| format!("lock CVPixelBuffer {s}"))?;
    let stride = guard.bytes_per_row();
    let base = guard
        .base_address_mut()
        .filter(|p| !p.is_null())
        .ok_or("CVPixelBuffer has no memory")?;
    if stride < w * 4 {
        return Err(format!("CVPixelBuffer row {stride} < {}", w * 4));
    }
    for (y, row) in img.rows().enumerate() {
        // SAFETY: the buffer is locked read-write and was just created here,
        // so nothing else aliases it; it has `h` rows of `stride` >= w*4 bytes
        // and y < h, so the row slice stays inside the allocation.
        let dst = unsafe { std::slice::from_raw_parts_mut(base.add(y * stride), w * 4) };
        for (d, p) in dst.chunks_exact_mut(4).zip(row) {
            let [r, g, b, a] = p.0;
            d.copy_from_slice(&[b, g, r, a]);
        }
    }
    drop(guard);
    Ok(buffer)
}
