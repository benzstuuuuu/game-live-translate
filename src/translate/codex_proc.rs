//! One `codex app-server` kept open (FR-TRN-15): started in a fresh empty
//! folder with every tool feature and MCP server off, checked through the
//! protocol, then fed one line per turn on a thread. A thread takes
//! `Rotation::every` lines; the next one is asked for ahead of its turn, in
//! the same process, so no line waits for it. Dropping the value kills the
//! process and removes its folder, so a turn cut short by a timeout leaves
//! nothing behind.

use super::claude_proc::{Broke, EmptyDir};
use crate::core::claude_cli::Rotation;
use crate::core::codex_cli::{self as core, Msg};
use crate::core::tokens::Usage;
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// What a thread is opened with; a thread whose spec no longer matches the
/// settings makes way for a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub model: String,
    pub system: String,
}

/// The thread lines go to.
struct Thread {
    id: String,
    spec: Spec,
    /// Turns taken on it.
    lines: u32,
    model: Option<String>,
}

/// The thread after the current one.
enum Next {
    /// `thread/start` sent as request `id`; no reply yet.
    Asked {
        id: u64,
        spec: Spec,
    },
    Open(Thread),
}

pub struct Proc {
    // Fields drop in order: the process is killed before its folder goes.
    conn: Conn,
    dir: EmptyDir,
    rotation: Rotation,
    thread: Thread,
    next: Option<Next>,
}

impl Proc {
    /// Starts `program` with the MCP servers `mcp` off, checks that its tool
    /// features are off, opens the first thread and checks that no MCP
    /// server is on for it.
    pub async fn start(
        program: &Path,
        mcp: &[String],
        spec: Spec,
        rotation: Rotation,
    ) -> Result<Proc, Broke> {
        let dir =
            EmptyDir::new("codex").map_err(|e| Broke::Gone(format!("สร้างโฟลเดอร์ว่างไม่ได้: {e}")))?;
        let mut child = Command::new(program)
            .args(core::argv(mcp))
            .current_dir(&dir.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Broke::Gone(spawn_reason(&e)))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(Broke::Gone("ต่อท่อกับ Codex ไม่ได้".into()));
        };
        let mut conn = Conn {
            _child: child,
            stdin,
            out: BufReader::new(stdout).lines(),
            ids: 0,
            usage: Usage::default(),
        };
        conn.call(core::initialize)
            .await?
            .map_err(|e| Broke::Gone(core::failed(&e)))?;
        conn.send(core::initialized()).await?;
        let features = conn.call(core::list_features).await?.unwrap_or_default();
        if let Some(why) = core::features(&features) {
            return Err(Broke::Refused(why));
        }
        let cwd = dir.0.to_string_lossy().into_owned();
        let asked = conn
            .call(|id| core::start_thread(id, &cwd, &spec.model, &spec.system))
            .await?;
        let thread = opened(asked, &spec)?.map_err(Broke::Gone)?;
        let servers = conn
            .call(|id| core::list_mcp(id, &thread.id))
            .await?
            .unwrap_or_default();
        if let Some(why) = core::mcp(&servers) {
            return Err(Broke::Refused(why));
        }
        Ok(Proc {
            conn,
            dir,
            rotation,
            thread,
            next: None,
        })
    }

    /// The model the current thread runs, as the server named it.
    pub fn model(&self) -> Option<String> {
        self.thread.model.clone()
    }

    /// One line as a turn: on a new thread first when the current one is
    /// due to retire or was opened with other settings. The reply (or
    /// Codex's reason it has none), or why the process is no use any more.
    /// Not cancel-safe: a caller that gives up must drop the process.
    pub async fn turn(&mut self, text: &str, spec: &Spec) -> Result<Result<String, String>, Broke> {
        if self.thread.spec != *spec || self.rotation.retire(self.thread.lines) {
            if let Err(reason) = self.switch(spec).await? {
                return Ok(Err(reason));
            }
        }
        let id = self.conn.id();
        self.conn
            .send(core::start_turn(id, &self.thread.id, text))
            .await?;
        loop {
            match self.conn.read().await? {
                Msg::Reply {
                    id: i,
                    result: Err(e),
                } if i == id => return Ok(Err(core::failed(&e))),
                Msg::Reply { id: i, result } if self.asked() == Some(i) => {
                    let spec = self.thread.spec.clone();
                    self.next = opened(result, &spec)?.ok().map(Next::Open);
                }
                Msg::TurnDone { thread, reply } if thread == self.thread.id => {
                    self.thread.lines += 1;
                    self.ask_ahead().await?;
                    return Ok(reply);
                }
                _ => {}
            }
        }
    }

    /// Tokens used since the last call.
    pub fn take_usage(&mut self) -> Usage {
        std::mem::take(&mut self.conn.usage)
    }

    fn asked(&self) -> Option<u64> {
        match self.next {
            Some(Next::Asked { id, .. }) => Some(id),
            _ => None,
        }
    }

    /// Asks for the next thread when the current one nears its last line.
    async fn ask_ahead(&mut self) -> Result<(), Broke> {
        if self.next.is_some() || !self.rotation.warm_next(self.thread.lines) {
            return Ok(());
        }
        let id = self.conn.id();
        let spec = self.thread.spec.clone();
        self.conn
            .send(core::start_thread(
                id,
                &self.cwd(),
                &spec.model,
                &spec.system,
            ))
            .await?;
        self.next = Some(Next::Asked { id, spec });
        Ok(())
    }

    /// Moves to the next thread: the one asked for ahead when it matches
    /// `spec`, else a new one. The old thread is let go.
    async fn switch(&mut self, spec: &Spec) -> Result<Result<(), String>, Broke> {
        let next = match self.next.take() {
            Some(Next::Open(t)) if t.spec == *spec => Ok(t),
            Some(Next::Asked { id, spec: s }) if s == *spec => {
                let result = self.conn.reply(id).await?;
                opened(result, spec)?
            }
            other => {
                if let Some(Next::Open(t)) = other {
                    self.let_go(&t.id).await?;
                }
                let cwd = self.cwd();
                let result = self
                    .conn
                    .call(|id| core::start_thread(id, &cwd, &spec.model, &spec.system))
                    .await?;
                opened(result, spec)?
            }
        };
        match next {
            Err(reason) => Ok(Err(reason)),
            Ok(t) => {
                let old = std::mem::replace(&mut self.thread, t);
                self.let_go(&old.id).await?;
                Ok(Ok(()))
            }
        }
    }

    /// Unsubscribes from a thread without waiting for the reply.
    async fn let_go(&mut self, thread: &str) -> Result<(), Broke> {
        let id = self.conn.id();
        self.conn.send(core::end_thread(id, thread)).await
    }

    fn cwd(&self) -> String {
        self.dir.0.to_string_lossy().into_owned()
    }
}

/// The reply to `thread/start` as a thread opened with `spec`: Codex's
/// reason when it failed, a refusal when it is not read-only.
fn opened(result: Result<Value, String>, spec: &Spec) -> Result<Result<Thread, String>, Broke> {
    let v = match result {
        Ok(v) => v,
        Err(e) => return Ok(Err(core::failed(&e))),
    };
    let t = core::thread(&v).map_err(Broke::Refused)?;
    Ok(Ok(Thread {
        id: t.id,
        spec: spec.clone(),
        lines: 0,
        model: t.model,
    }))
}

/// The JSON-RPC link to the process.
struct Conn {
    _child: Child,
    stdin: ChildStdin,
    out: Lines<BufReader<ChildStdout>>,
    /// The last request id used.
    ids: u64,
    usage: Usage,
}

impl Conn {
    fn id(&mut self) -> u64 {
        self.ids += 1;
        self.ids
    }

    async fn send(&mut self, line: String) -> Result<(), Broke> {
        let gone = |_| Broke::Gone("Codex ปิดไปแล้ว".into());
        self.stdin.write_all(line.as_bytes()).await.map_err(gone)?;
        self.stdin.flush().await.map_err(gone)
    }

    /// Sends the request `make` builds and waits for its reply.
    async fn call(
        &mut self,
        make: impl FnOnce(u64) -> String,
    ) -> Result<Result<Value, String>, Broke> {
        let id = self.id();
        self.send(make(id)).await?;
        self.reply(id).await
    }

    /// Reads until the reply to request `id`.
    async fn reply(&mut self, id: u64) -> Result<Result<Value, String>, Broke> {
        loop {
            if let Msg::Reply { id: i, result } = self.read().await? {
                if i == id {
                    return Ok(result);
                }
            }
        }
    }

    /// The next message worth a look. Tokens are counted on the way; a tool
    /// at work, or the server asking to run one, breaks the process.
    async fn read(&mut self) -> Result<Msg, Broke> {
        loop {
            let line = self
                .out
                .next_line()
                .await
                .map_err(|_| Broke::Gone("Codex ปิดไปแล้ว".into()))?;
            let Some(line) = line else {
                return Err(Broke::Gone("Codex ปิดไปเองระหว่างแปล".into()));
            };
            match core::decode(&line) {
                Msg::Asks(what) | Msg::Tool(what) => {
                    return Err(Broke::Refused(format!(
                        "Codex พยายามใช้เครื่องมือ ({what}) Yokoten จึงไม่ใช้"
                    )))
                }
                Msg::Tokens(u) => self.usage = self.usage + u,
                Msg::Other => {}
                msg => return Ok(msg),
            }
        }
    }
}

fn spawn_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => core::not_found().0.into(),
        _ => format!("เปิด Codex ไม่ได้: {e}"),
    }
}
