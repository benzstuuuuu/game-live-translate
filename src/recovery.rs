//! A session's frames through OBS dropping out, the card going missing or
//! black (FR-ERR-04 to 06): show the cause card, try again on the schedule of
//! `core::recovery`, and go back to translating once a frame comes. The
//! card's audio gets the same (`Audio`).

use crate::app::{App, Opener};
use crate::audio::{AudioSource, Card};
use crate::core::card_audio::{silence, Permission};
use crate::core::error::AppError;
use crate::core::recovery::{retry_for, Outage, Retry};
use crate::frames::{Frame, FrameSource};
use futures_util::future::BoxFuture;
use std::sync::Arc;
use tokio::time::Instant;

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

/// The card's audio for one session (FR-AUD-01, 08). While the card is not
/// found or was unplugged it shows the cause card, sends silence so the Live
/// Translate session stays open, opens the card again on the schedule of
/// `core::recovery`, and goes back to translating once the card is heard.
/// Microphone permission turned off ends the session.
pub struct Audio {
    app: Arc<App>,
    card: Arc<dyn Card>,
    source: Option<Box<dyn AudioSource>>,
    outage: Outage,
    /// While the card is away: when to open it again.
    retry_at: Option<Instant>,
}

impl Audio {
    pub fn new(app: Arc<App>, card: Arc<dyn Card>) -> Self {
        Self {
            app,
            card,
            source: None,
            outage: Outage::default(),
            retry_at: None,
        }
    }

    async fn next(&mut self) -> Result<Option<Vec<u8>>, AppError> {
        // Turned off in System Settings, or "Don't Allow" on the first ask.
        if self.card.permission() == Permission::Denied {
            return Err(AppError::mic_permission_denied());
        }
        let source = match &mut self.source {
            Some(source) => source,
            None if self.retry_at.is_some_and(|at| Instant::now() < at) => {
                return Ok(Some(silence()));
            }
            None => match self.card.open() {
                Ok(source) => self.source.insert(source),
                Err(e) => return self.wait_out(e),
            },
        };
        match source.next_chunk().await {
            Ok(Some(chunk)) => {
                if self.outage.frame() {
                    self.app.resume();
                }
                Ok(Some(chunk))
            }
            Ok(None) => Ok(None),
            Err(e) => self.wait_out(e),
        }
    }

    /// Shows `error` and sends silence until the next try, or gives up when
    /// only the player can fix it.
    fn wait_out(&mut self, error: AppError) -> Result<Option<Vec<u8>>, AppError> {
        let Some(delay) = retry_for(error.code).delay() else {
            return Err(error);
        };
        self.source = None;
        self.retry_at = Some(Instant::now() + delay);
        if let Some(shown) = self.outage.fail(error.with_retry(delay)) {
            self.app.fail(shown);
        }
        Ok(Some(silence()))
    }
}

impl AudioSource for Audio {
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, AppError>> {
        Box::pin(self.next())
    }
}
