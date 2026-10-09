//! Control protocol: one JSON object per line over TCP.
//!
//! Request: `{"id":1,"cmd":{"type":"teleport","x":0,"y":0,"z":0}}`
//! Response: `{"id":1,"ok":true,"result":{...}}` or `{"id":1,"ok":false,"error":"..."}`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};

pub const DEFAULT_ADDR: &str = "127.0.0.1:7117";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Ping,
    /// Scene name, player transform, fidelity settings, frame counter.
    State,
    /// Capture the primary window to `path` (PNG). The file appears a frame or two later.
    Screenshot {
        path: String,
    },
    Teleport {
        x: f32,
        y: f32,
        z: f32,
    },
    /// `key` is a Bevy `KeyCode` name, e.g. `KeyW`, `ArrowUp`, `Space`.
    Input {
        key: String,
        action: InputAction,
    },
    SetFidelity {
        area: String,
        mode: String,
    },
    LoadScene {
        name: String,
    },
    Quit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InputAction {
    Press,
    Release,
    /// Press now, release on the next frame.
    Tap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub cmd: Command,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }
    pub fn err(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }
}

/// Blocking client; one request at a time.
pub struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    next_id: u64,
}

impl Client {
    pub fn connect(addr: impl ToSocketAddrs) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        Ok(Self {
            reader: BufReader::new(stream.try_clone()?),
            writer: stream,
            next_id: 1,
        })
    }

    pub fn send(&mut self, cmd: Command) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = serde_json::to_string(&Request { id, cmd }).map_err(|e| e.to_string())?;
        writeln!(self.writer, "{line}").map_err(|e| e.to_string())?;
        let mut buf = String::new();
        self.reader.read_line(&mut buf).map_err(|e| e.to_string())?;
        if buf.is_empty() {
            return Err("connection closed".into());
        }
        let resp: Response = serde_json::from_str(buf.trim()).map_err(|e| e.to_string())?;
        if resp.id != id {
            return Err(format!(
                "response id {} does not match request {id}",
                resp.id
            ));
        }
        if resp.ok {
            Ok(resp.result.unwrap_or(Value::Null))
        } else {
            Err(resp.error.unwrap_or_default())
        }
    }
}
