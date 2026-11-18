//! Where frames come from: OBS (`crate::obs`) or a folder of images for dev
//! and tests (FR-CAP-03).

use crate::core::error::AppError;
use futures_util::future::{self, BoxFuture};
use std::io;
use std::path::{Path, PathBuf};

/// One picture to OCR.
pub enum Frame {
    /// An image file on disk (folder source only).
    File(PathBuf),
    /// Encoded JPG/PNG bytes; never written to disk (NFR-05, FR-CAP-08).
    Image(Vec<u8>),
}

pub trait FrameSource: Send {
    /// The next frame, None when there are no more, or why reading stopped.
    fn next_frame(&mut self) -> BoxFuture<'_, Result<Option<Frame>, AppError>>;
}

/// Every png/jpg in a folder, once, in file-name order. The default has none.
#[derive(Default)]
pub struct Folder {
    files: std::vec::IntoIter<PathBuf>,
}

impl Folder {
    pub fn open(dir: &Path) -> io::Result<Self> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                    matches!(x.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg")
                })
            })
            .collect();
        files.sort();
        Ok(Self {
            files: files.into_iter(),
        })
    }
}

impl FrameSource for Folder {
    fn next_frame(&mut self) -> BoxFuture<'_, Result<Option<Frame>, AppError>> {
        Box::pin(future::ready(Ok(self.files.next().map(Frame::File))))
    }
}
