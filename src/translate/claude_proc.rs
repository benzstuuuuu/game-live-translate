//! One `claude -p` process kept open (FR-TRN-15): started in a fresh empty
//! folder with every tool off, fed one line per turn over stream-json. The
//! process dies with this value: dropping it kills the process and removes
//! its folder, so a turn cut short by a timeout leaves nothing behind.

use crate::core::claude_cli::{self as core, Event};
use crate::core::tokens::Usage;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// What a process was started with; a process whose spec no longer matches
/// the settings makes way for a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub model: String,
    pub system: String,
}

/// Why a process can take no more lines.
#[derive(Debug, Clone, PartialEq)]
pub enum Broke {
    /// It may do more than translate (tools or MCP servers on): never use it.
    Refused(String),
    /// It could not start, exited, or its pipes broke; the Thai reason.
    Gone(String),
}

pub struct Proc {
    // Fields drop in order: the process is killed before its folder goes.
    _child: Child,
    stdin: ChildStdin,
    out: Lines<BufReader<ChildStdout>>,
    _dir: EmptyDir,
    pub spec: Spec,
    /// Lines translated, not counting the warm-up line.
    pub lines: u32,
    /// The model the CLI says it runs, from its init event.
    pub model: Option<String>,
    /// An init event without tools has been seen.
    trusted: bool,
    usage: Usage,
}

impl Proc {
    /// Starts `program` and has it translate `core::WARM_LINE`, so its init
    /// event is checked before a real line goes in.
    pub async fn start(program: &Path, spec: Spec) -> Result<Proc, Broke> {
        let dir = EmptyDir::new().map_err(|e| Broke::Gone(format!("สร้างโฟลเดอร์ว่างไม่ได้: {e}")))?;
        let mut cmd = Command::new(program);
        cmd.args(core::argv(&spec.model, &spec.system))
            .current_dir(&dir.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for var in core::NESTED_ENV {
            cmd.env_remove(var);
        }
        let mut child = cmd.spawn().map_err(|e| Broke::Gone(spawn_reason(&e)))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(Broke::Gone("ต่อท่อกับ Claude Code ไม่ได้".into()));
        };
        let mut proc = Proc {
            _child: child,
            stdin,
            out: BufReader::new(stdout).lines(),
            _dir: dir,
            spec,
            lines: 0,
            model: None,
            trusted: false,
            usage: Usage::default(),
        };
        match proc.turn(core::WARM_LINE).await? {
            Ok(_) => Ok(proc),
            Err(reason) => Err(Broke::Gone(reason)),
        }
    }

    /// Sends one user turn and reads until its result: the reply (or the
    /// CLI's reason it has none), or why the process is no use any more.
    /// Not cancel-safe: a caller that gives up must drop the process.
    pub async fn turn(&mut self, text: &str) -> Result<Result<String, String>, Broke> {
        let gone = |_| Broke::Gone("Claude Code ปิดไปแล้ว".into());
        self.stdin
            .write_all(core::user_message(text).as_bytes())
            .await
            .map_err(gone)?;
        self.stdin.flush().await.map_err(gone)?;
        loop {
            let Some(line) = self.out.next_line().await.map_err(gone)? else {
                return Err(Broke::Gone("Claude Code ปิดไปเองระหว่างแปล".into()));
            };
            match core::decode(&line) {
                Event::Init {
                    refusal: Some(why), ..
                } => return Err(Broke::Refused(why)),
                Event::Init { model, .. } => {
                    self.trusted = true;
                    self.model = model.or(self.model.take());
                }
                Event::Result { .. } if !self.trusted => {
                    return Err(Broke::Refused(
                        "Claude Code ไม่บอกว่าเปิดเครื่องมืออะไรไว้ Yokoten จึงไม่ใช้".into(),
                    ))
                }
                Event::Result { reply, usage } => {
                    self.usage = self.usage + usage;
                    return Ok(reply);
                }
                Event::Other => {}
            }
        }
    }

    /// Tokens used since the last call, warm-up included.
    pub fn take_usage(&mut self) -> Usage {
        std::mem::take(&mut self.usage)
    }
}

fn spawn_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => core::not_found().0.into(),
        _ => format!("เปิด Claude Code ไม่ได้: {e}"),
    }
}

/// A new empty folder under the temp dir, removed on drop.
struct EmptyDir(PathBuf);

impl EmptyDir {
    fn new() -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("yokoten-claude-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for EmptyDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
