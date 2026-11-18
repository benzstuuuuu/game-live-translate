//! A session's frames through OBS dropping out, the card going missing or
//! black (FR-ERR-04 to 06): show the cause card, try again on the schedule of
//! `core::recovery`, and go back to translating once a frame comes.

use crate::app::{App, Opener};
use crate::core::error::AppError;
use crate::core::recovery::{retry_for, Outage, Retry};
use crate::frames::{Frame, FrameSource};
use std::sync::Arc;

pub struct Frames {
    app: Arc<App>,
    open: Opener,
    source: Option<Box<dyn FrameSource>>,
    outage: Outage,
}

impl Frames {
    pub fn new(app: Arc<App>, open: Opener) -> Self {
        Self {
            app,
            open,
            source: None,
            outage: Outage::default(),
        }
    }

    /// The next frame, waiting through problems that can fix themselves.
    /// None when the source ran out or only the player can fix the problem.
    pub async fn next(&mut self) -> Option<Frame> {
        loop {
            let result = match self.source.as_mut() {
                Some(source) => source.next_frame().await,
                None => match (self.open)().await {
                    Ok(source) => {
                        self.source = Some(source);
                        continue;
                    }
                    Err(e) => Err(e),
                },
            };
            match result {
                Ok(Some(frame)) => {
                    if self.outage.frame() {
                        self.app.resume();
                    }
                    return Some(frame);
                }
                Ok(None) => return None,
                Err(e) => {
                    if !self.wait_out(e).await {
                        return None;
                    }
                }
            }
        }
    }

    /// Shows `error` and waits before the next try; false if there is none.
    async fn wait_out(&mut self, error: AppError) -> bool {
        let retry = retry_for(error.code);
        let Some(delay) = retry.delay() else {
            self.app.fail(error);
            return false;
        };
        if let Retry::Reopen(_) = retry {
            self.source = None;
        }
        if let Some(shown) = self.outage.fail(error.with_retry(delay)) {
            self.app.fail(shown);
        }
        tokio::time::sleep(delay).await;
        true
    }
}
