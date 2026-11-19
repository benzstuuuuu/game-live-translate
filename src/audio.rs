//! Where audio comes from for the audio path: the capture card (`Card`,
//! read by `crate::card_audio`), or a WAV file for dev and tests (`--audio`,
//! like `--frames` for pictures).

use crate::core::card_audio::Permission;
use crate::core::error::AppError;
use crate::core::live::CHUNK_BYTES;
use crate::core::wav;
use futures_util::future::{self, BoxFuture};
use std::path::Path;

pub trait AudioSource: Send {
    /// The next 100 ms of 16 kHz s16le mono audio, None when there is no
    /// more, or why reading stopped.
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, AppError>>;
}

/// The capture card's audio input (FR-AUD-01, 08). `crate::recovery::Audio`
/// opens it again after it drops out; tests use a fake.
pub trait Card: Send + Sync {
    /// Device name, e.g. "UGREEN 15390".
    fn device(&self) -> &str;
    /// Microphone permission as macOS reports it now; asks nothing.
    fn permission(&self) -> Permission;
    /// Whether an audio input of the card's name is on the Mac now.
    fn found(&self) -> bool;
    /// Starts hearing the card; it stops when the source is dropped.
    fn open(&self) -> Result<Box<dyn AudioSource>, AppError>;
}

/// The samples of one 16 kHz 16-bit mono WAV file, once, in 100 ms chunks.
pub struct WavFile {
    pcm: Vec<u8>,
    at: usize,
}

impl WavFile {
    /// Reads the whole file; a Thai reason when it cannot be used.
    pub fn open(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let pcm = wav::pcm(&bytes)?.to_vec();
        Ok(Self { pcm, at: 0 })
    }
}

impl AudioSource for WavFile {
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, AppError>> {
        let end = (self.at + CHUNK_BYTES).min(self.pcm.len());
        let chunk = (end > self.at).then(|| self.pcm[self.at..end].to_vec());
        self.at = end;
        Box::pin(future::ready(Ok(chunk)))
    }
}
