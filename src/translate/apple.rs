//! Apple Translation (on-device Translation.framework) via translation-rs.
//!
//! The framework session is not `Send`, so one thread owns it and takes jobs
//! from a channel; callers await a oneshot reply. The framework finishes its
//! work on the main dispatch queue, so the main thread must be running its run
//! loop (see `crate::runloop`), or every call fails after about 10 seconds.

use super::{Translator, Unavailable};
use futures_util::future::BoxFuture;
use std::sync::mpsc;
use tokio::sync::oneshot;
use translation::prelude::*;

/// English packs we accept, best first. The session's source language must
/// match the installed pack (FR-TRN-11).
const SOURCES: &[&str] = &["en-GB", "en-US"];
const TARGET: &str = "th";

enum Job {
    Check(oneshot::Sender<Result<(), Unavailable>>),
    Translate(String, oneshot::Sender<Result<String, String>>),
}

pub struct Apple {
    jobs: mpsc::Sender<Job>,
}

impl Apple {
    /// Starts the worker thread. Nothing touches the framework until the first
    /// check or translation.
    pub fn start() -> std::io::Result<Self> {
        let (jobs, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("apple-translation".into())
            .spawn(move || Worker::default().run(rx))?;
        Ok(Self { jobs })
    }

    async fn ask<T>(&self, job: Job, reply: oneshot::Receiver<T>) -> Option<T> {
        self.jobs.send(job).ok()?;
        reply.await.ok()
    }
}

impl Translator for Apple {
    fn engine(&self) -> &'static str {
        "apple"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.ask(Job::Translate(text.to_owned(), tx), rx)
                .await
                .unwrap_or_else(|| Err("Apple Translation worker stopped".into()))
        })
    }

    fn check(&self) -> BoxFuture<'_, Result<(), Unavailable>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            self.ask(Job::Check(tx), rx)
                .await
                .unwrap_or_else(|| Err(failed("worker stopped")))
        })
    }
}

#[derive(Default)]
struct Worker {
    session: Option<TranslationSession>,
}

impl Worker {
    fn run(mut self, jobs: mpsc::Receiver<Job>) {
        for job in jobs {
            match job {
                Job::Check(reply) => {
                    let _ = reply.send(self.ready());
                }
                Job::Translate(text, reply) => {
                    let _ = reply.send(self.translate(&text));
                }
            }
        }
    }

    /// Opens a session on the first installed English → Thai pack.
    fn ready(&mut self) -> Result<(), Unavailable> {
        if self.session.is_some() {
            return Ok(());
        }
        let availability = LanguageAvailability::new().map_err(|e| failed(&e.to_string()))?;
        let mut downloadable = false;
        for source in SOURCES {
            let status = availability
                .status_for_pair(source, TARGET)
                .map_err(|e| failed(&e.to_string()))?;
            match status {
                LanguageAvailabilityStatus::Installed => {
                    let config = TranslationSessionConfiguration::new(*source, TARGET);
                    let session =
                        TranslationSession::new(config).map_err(|e| failed(&e.to_string()))?;
                    // Loads the model now so the first line is not slow.
                    if let Err(e) = session.prepare_translation() {
                        eprintln!("apple translation prepare: {e}");
                    }
                    self.session = Some(session);
                    return Ok(());
                }
                LanguageAvailabilityStatus::Supported => downloadable = true,
                _ => {}
            }
        }
        Err(if downloadable {
            missing_pack()
        } else {
            unsupported()
        })
    }

    fn translate(&mut self, text: &str) -> Result<String, String> {
        self.ready().map_err(|e| e.message)?;
        let session = self.session.as_ref().ok_or("no session")?;
        session
            .translate(text)
            .map(|r| r.target_text().to_owned())
            .map_err(|e| e.to_string())
    }
}

fn missing_pack() -> Unavailable {
    Unavailable {
        message: "ยังไม่มีชุดภาษาอังกฤษ → ไทย ของ Apple Translation".into(),
        fix: "เปิด System Settings › General › Language & Region › Translation Languages \
              แล้วดาวน์โหลดภาษาอังกฤษ (สหราชอาณาจักร) และภาษาไทย จากนั้นกดเริ่มแปลอีกครั้ง"
            .into(),
    }
}

fn unsupported() -> Unavailable {
    Unavailable {
        message: "macOS เครื่องนี้แปลอังกฤษ → ไทย ด้วย Apple Translation ไม่ได้".into(),
        fix: "ใช้ macOS 15 ขึ้นไป หรือเลือกตัวแปลอื่น".into(),
    }
}

fn failed(reason: &str) -> Unavailable {
    Unavailable {
        message: format!("Apple Translation ไม่พร้อม: {reason}"),
        fix: "ลองเริ่มแปลอีกครั้ง หรือเลือกตัวแปลอื่น".into(),
    }
}
