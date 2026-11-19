//! Where audio comes from for the audio path: a WAV file for dev and tests
//! (`--audio`, like `--frames` for pictures). Reading the capture card is
//! #70; it only needs to be another `AudioSource`.

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
