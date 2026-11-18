//! Where frames come from: a folder of images now, OBS later (FR-CAP-03).

use std::io;
use std::path::{Path, PathBuf};

pub trait FrameSource: Send {
    /// Path of the next image to OCR, or None when there are no more.
    fn next_frame(&mut self) -> Option<PathBuf>;
}

/// Every png/jpg in a folder, once, in file-name order.
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
    fn next_frame(&mut self) -> Option<PathBuf> {
        self.files.next()
    }
}
