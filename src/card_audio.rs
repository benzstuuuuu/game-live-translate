//! Reading the capture card's audio input with cpal (CoreAudio), FR-AUD-01:
//! found by its device name, turned into 16 kHz s16le mono by
//! `core::card_audio`, and kept in memory only (NFR-16). Dropping the source
//! stops the card at once.
//!
//! Microphone permission (FR-AUD-08) is read from
//! `AVCaptureDevice authorizationStatusForMediaType:`, which only reports and
//! never asks. macOS asks the first time the card is opened.

use crate::audio::{AudioSource, Card};
use crate::core::card_audio::{
    pick_device, pick_format, Chunks, Offer, Permission, Resampler, Sample,
};
use crate::core::error::AppError;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, ErrorKind, SampleFormat, StreamConfig};
use futures_util::future::BoxFuture;
use objc2::runtime::{AnyClass, AnyObject};
use std::time::Duration;
use tokio::sync::mpsc;

/// No audio from an open card this long: it has gone (CoreAudio keeps
/// sending silence while it is there).
const QUIET_LIMIT: Duration = Duration::from_secs(3);

/// The card as CoreAudio sees it, by device name.
pub struct Cpal {
    device: String,
}

impl Cpal {
    pub fn new(device: impl Into<String>) -> Self {
        Self {
            device: device.into(),
        }
    }

    fn find(&self) -> Option<cpal::Device> {
        let devices: Vec<cpal::Device> = cpal::default_host().input_devices().ok()?.collect();
        let names: Vec<String> = devices
            .iter()
            .map(|d| {
                d.description()
                    .map(|x| x.name().to_owned())
                    .unwrap_or_default()
            })
            .collect();
        pick_device(&names, &self.device).map(|i| devices[i].clone())
    }

    fn error(&self, e: &cpal::Error) -> AppError {
        match e.kind() {
            ErrorKind::PermissionDenied => AppError::mic_permission_denied(),
            ErrorKind::DeviceNotAvailable => AppError::card_audio_missing(&self.device),
            _ => AppError::card_audio_unreadable(&self.device, &e.to_string()),
        }
    }
}

impl Card for Cpal {
    fn device(&self) -> &str {
        &self.device
    }

    fn permission(&self) -> Permission {
        permission()
    }

    fn found(&self) -> bool {
        self.find().is_some()
    }

    fn open(&self) -> Result<Box<dyn AudioSource>, AppError> {
        let device = self
            .find()
            .ok_or_else(|| AppError::card_audio_missing(&self.device))?;
        let ranges: Vec<_> = device
            .supported_input_configs()
            .map_err(|e| self.error(&e))?
            .collect();
        let offers: Vec<Offer> = ranges
            .iter()
            .map(|r| Offer {
                sample: match r.sample_format() {
                    SampleFormat::F32 => Sample::F32,
                    SampleFormat::I16 => Sample::I16,
                    _ => Sample::Other,
                },
                channels: r.channels(),
                min_rate: r.min_sample_rate(),
                max_rate: r.max_sample_rate(),
            })
            .collect();
        let (i, rate) = pick_format(&offers)
            .ok_or_else(|| AppError::card_audio_unreadable(&self.device, "ไม่มีเสียงแบบ 48 kHz"))?;
        let offer = offers[i];
        let mut resampler = Resampler::new(rate, offer.channels)
            .ok_or_else(|| AppError::card_audio_unreadable(&self.device, "รูปแบบเสียง"))?;
        let config = StreamConfig {
            channels: offer.channels,
            sample_rate: rate,
            buffer_size: BufferSize::Default,
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let failed = tx.clone();
        let device_name = self.device.clone();
        let on_error = move |e: cpal::Error| {
            let _ = failed.send(Err(Cpal::new(device_name.clone()).error(&e)));
        };
        let stream = match offer.sample {
            Sample::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(Ok(resampler.f32(data)));
                },
                on_error,
                None,
            ),
            _ => device.build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(Ok(resampler.i16(data)));
                },
                on_error,
                None,
            ),
        }
        .map_err(|e| self.error(&e))?;
        stream.play().map_err(|e| self.error(&e))?;
        Ok(Box::new(Listening {
            _stream: stream,
            rx,
            chunks: Chunks::default(),
            device: self.device.clone(),
        }))
    }
}

/// The card while it is open: CoreAudio's buffers, 16 kHz mono already,
/// handed out in 100 ms chunks.
struct Listening {
    /// Stops the card when dropped.
    _stream: cpal::Stream,
    rx: mpsc::UnboundedReceiver<Result<Vec<u8>, AppError>>,
    chunks: Chunks,
    device: String,
}

impl AudioSource for Listening {
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, AppError>> {
        Box::pin(async move {
            loop {
                if let Some(chunk) = self.chunks.pop() {
                    return Ok(Some(chunk));
                }
                match tokio::time::timeout(QUIET_LIMIT, self.rx.recv()).await {
                    Ok(Some(Ok(pcm))) => self.chunks.push(&pcm),
                    Ok(Some(Err(e))) => return Err(e),
                    Ok(None) | Err(_) => return Err(AppError::card_audio_missing(&self.device)),
                }
            }
        })
    }
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {
    /// `AVMediaTypeAudio`, an NSString.
    static AVMediaTypeAudio: &'static AnyObject;
}

/// Microphone permission of the app that opened Yokoten, as macOS reports it
/// now. Reading it never shows a prompt.
pub fn permission() -> Permission {
    let Some(class) = AnyClass::get(c"AVCaptureDevice") else {
        return Permission::NotAsked;
    };
    // SAFETY: a class method taking an AVMediaType (NSString) and returning
    // AVAuthorizationStatus (NSInteger); AVFoundation is linked above.
    let status: isize =
        unsafe { objc2::msg_send![class, authorizationStatusForMediaType: AVMediaTypeAudio] };
    Permission::from_av(status)
}
