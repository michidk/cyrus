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
    /// App state, scene name, player transform, fidelity settings, frame counter.
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
    /// Match a captured original sky direction and texture offsets for comparison.
    SetSkyScroll {
        direction: u16,
        offset: [f32; 2],
    },
    LoadScene {
        name: String,
    },
    /// Skip the splash, retain required menu preparation behind the title, and stop at the menu.
    SkipStartup,
    /// Play effect `effect` (0-based index in `MAIN.SFX`); looping effects play until stopped.
    /// With `position` (world space, as in `state`) it attenuates with the camera's distance.
    PlaySound {
        effect: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<[f32; 3]>,
    },
    /// Play the voice clip with this RTX label, e.g. `&005`.
    PlaySpeech {
        label: String,
    },
    /// Stop every effect and voice clip; music keeps playing.
    StopSounds,
    /// Inspect at most 64 logical script actors in the prepared world.
    Scripts {
        #[serde(default)]
        offset: usize,
        #[serde(default = "script_page_size")]
        limit: usize,
    },
    /// Prepare animation for an inspected actor, optionally binding a named scene node.
    BindScriptAnimation {
        generation: u64,
        index: usize,
        default_group: u16,
        #[serde(default)]
        node: Option<String>,
    },
    /// Invoke one explicit stage; does not enable automatic gameplay.
    StepScript {
        generation: u64,
        index: usize,
        stage: ScriptStage,
        /// Required for tasks; eight fractional bits, supplied by the inspection caller.
        #[serde(default)]
        frame_scale: Option<u32>,
        /// Required for animation-bound actors; whole original nominal ticks, including zero.
        #[serde(default)]
        nominal_ticks: Option<u32>,
        /// Explicit inspection override for PlayerDistance; absence remains unsupported.
        #[serde(default)]
        distance_override: Option<bool>,
        /// Original Q8 fallback coordinates used only without a selected player.
        #[serde(default)]
        fallback_position: Option<[i32; 3]>,
    },
    Quit,
}

fn script_page_size() -> usize {
    32
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStage {
    OrdinaryMain,
    NativeHook,
    PrepareSpatial,
    Snapshot,
    AxisLocks,
    Transform,
    Families,
    Animation,
    Entry,
    Main,
    Interrupt,
    Tasks,
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

#[cfg(not(target_arch = "wasm32"))]
pub mod server;
