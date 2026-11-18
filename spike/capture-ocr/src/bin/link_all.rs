//! Build check: ScreenCaptureKit, Vision and Translation crates linked into one binary.
fn main() {
    let _ = std::any::type_name::<screencapturekit::prelude::SCStream>();
    let _ = std::any::type_name::<apple_vision::prelude::TextRecognizer>();
    let _ = std::any::type_name::<translation::prelude::TranslationSessionConfiguration>();
    println!("linked: screencapturekit + apple-vision + translation-rs");
}
