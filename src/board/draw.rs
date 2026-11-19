//! Thai drawn by CoreText for the LCD board (#85): the board's font has no
//! Thai and cannot stack vowels and tone marks, so the Mac draws the line.
//! CTFramesetter breaks Thai between words; `core::fit` picks the size.

use crate::core::fit;
use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFRange, CFRetained, CFString, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGPath,
};
use objc2_core_text::{kCTFontAttributeName, CTFont, CTFramesetter};
use std::ptr;

/// A system font with Thai; no font files ship with Yokoten.
const FONT: &str = "Thonburi";
/// Smallest size tried before the text is cut with `…`.
const MIN_SIZE: u32 = 14;
/// White space kept around the text, in pixels.
const MARGIN: u32 = 2;

/// Text and font size that fit the image.
#[derive(Debug, PartialEq)]
pub struct Fitted {
    pub text: String,
    pub size: u32,
}

/// The largest size at which all of `text` fits a `w`×`h` image; at the
/// smallest size, as much as fits followed by `…`.
pub fn fit(text: &str, w: u32, h: u32) -> Fitted {
    let area = area(w, h);
    let min = MIN_SIZE.min(h);
    match fit::largest(min, h, |size| fits(text, size, area.size)) {
        Some(size) => Fitted {
            text: text.to_owned(),
            size,
        },
        None => Fitted {
            text: fit::ellipsize(text, |t| fits(t, min, area.size)),
            size: min,
        },
    }
}

/// `fitted` drawn black on white: `w`×`h` gray bytes, top row first, 0
/// black and 255 white.
pub fn gray(fitted: &Fitted, w: u32, h: u32) -> Vec<u8> {
    let (wu, hu) = (w as usize, h as usize);
    let mut pixels = vec![0u8; wu * hu];
    let space = CGColorSpace::new_device_gray().expect("gray color space");
    // SAFETY: `pixels` holds wu*hu bytes, one per pixel, and outlives
    // `context`, which is dropped below before `pixels` is returned.
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            wu,
            hu,
            8,
            wu,
            Some(&space),
            CGImageAlphaInfo::None.0,
        )
    }
    .expect("gray bitmap context");
    let whole = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w.into(), h.into()));
    CGContext::set_gray_fill_color(Some(&context), 1.0, 1.0);
    CGContext::fill_rect(Some(&context), whole);
    // SAFETY: a plain rect path with no transform; the frame draws into the
    // live context. Text color defaults to black.
    unsafe {
        let path = CGPath::with_rect(area(w, h), ptr::null());
        let frame = framesetter(&fitted.text, fitted.size).frame(CFRange::new(0, 0), &path, None);
        frame.draw(&context);
    }
    drop(context);
    pixels
}

fn area(w: u32, h: u32) -> CGRect {
    let inset = |n: u32| f64::from(n.saturating_sub(2 * MARGIN));
    CGRect::new(
        CGPoint::new(MARGIN.into(), MARGIN.into()),
        CGSize::new(inset(w), inset(h)),
    )
}

/// True when all of `text` at `size` fits in `area`.
fn fits(text: &str, size: u32, area: CGSize) -> bool {
    let mut fit = CFRange::new(0, 0);
    // SAFETY: `fit` is a valid out pointer; no frame attributes.
    unsafe {
        framesetter(text, size).suggest_frame_size_with_constraints(
            CFRange::new(0, 0),
            None,
            area,
            &mut fit,
        );
    }
    fit.length as usize == text.encode_utf16().count()
}

fn framesetter(text: &str, size: u32) -> CFRetained<CTFramesetter> {
    // SAFETY: plain CoreText constructors; the attributes dictionary maps
    // the font key to a CTFont as CoreText expects.
    unsafe {
        let font = CTFont::with_name(&CFString::from_str(FONT), size.into(), ptr::null());
        let attrs =
            CFDictionary::<CFString, CTFont>::from_slices(&[kCTFontAttributeName], &[&*font]);
        let string = CFAttributedString::new(
            None,
            Some(&CFString::from_str(text)),
            Some(attrs.as_opaque()),
        )
        .expect("attributed string");
        CTFramesetter::with_attributed_string(&string)
    }
}
