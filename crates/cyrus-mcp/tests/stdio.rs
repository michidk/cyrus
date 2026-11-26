//! Exercise the shipped MCP executable over stdio and the public TCP control protocol.

use base64::Engine;
use cyrus_control::{Command, InputAction, Request, Response};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const WATCHDOG: Duration = Duration::from_secs(30);

struct Mcp {
    process: Child,
    input: ChildStdin,
    replies: mpsc::Receiver<Value>,
    reader: Option<JoinHandle<()>>,
    next_id: u64,
}

impl Mcp {
    fn start(address: SocketAddr, protocol: &str) -> Self {
        let mut process = std::process::Command::new(env!("CARGO_BIN_EXE_cyrus-mcp"))
            .env("CYRUS_CONTROL_ADDR", address.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start cyrus-mcp");
        let input = process.stdin.take().unwrap();
        let output = process.stdout.take().unwrap();
        let (sender, replies) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let value = serde_json::from_str(&line.expect("MCP stdout read"))
                    .expect("MCP stdout contains only JSON-RPC");
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            process,
            input,
            replies,
            reader: Some(reader),
            next_id: 1,
        };
        let initialized = client.rpc(
            "initialize",
            json!({
                "protocolVersion": protocol,
                "capabilities": {"fixtureExtension": {"enabled": true}},
                "clientInfo": {"name": "cyrus-stdio-contract", "version": "1"}
            }),
        );
        assert_eq!(initialized["result"]["protocolVersion"], protocol);
        assert!(initialized["result"]["capabilities"]["tools"].is_object());
        writeln!(
            client.input,
            "{}",
            json!({
                "jsonrpc": "2.0", "method": "notifications/initialized"
            })
        )
        .unwrap();
        client.input.flush().unwrap();
        client
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(
            self.input,
            "{}",
            json!({
                "jsonrpc": "2.0", "id": id, "method": method, "params": params
            })
        )
        .unwrap();
        self.input.flush().unwrap();
        loop {
            let response = self.replies.recv_timeout(WATCHDOG).expect("MCP reply");
            if response["id"] == id {
                return response;
            }
            assert!(
                response.get("id").is_none(),
                "unexpected response: {response}"
            );
        }
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.rpc("tools/call", json!({"name": name, "arguments": arguments}))
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        self.reader.take().unwrap().join().unwrap();
    }
}

struct Game {
    address: SocketAddr,
    commands: mpsc::Receiver<Command>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Game {
    fn start(handler: impl Fn(&Command) -> Result<Value, String> + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (sender, commands) = mpsc::channel();
        let worker = thread::spawn(move || {
            for connection in listener.incoming() {
                let mut stream = connection.unwrap();
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                stream.set_read_timeout(Some(WATCHDOG)).unwrap();
                stream.set_write_timeout(Some(WATCHDOG)).unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let request: Request = serde_json::from_str(&line).unwrap();
                let result = handler(&request.cmd);
                sender.send(request.cmd).unwrap();
                let response = match result {
                    Ok(value) => Response::ok(request.id, value),
                    Err(error) => Response::err(request.id, error),
                };
                writeln!(stream, "{}", serde_json::to_string(&response).unwrap()).unwrap();
            }
        });
        Self {
            address,
            commands,
            stop,
            worker: Some(worker),
        }
    }

    fn command(&self) -> Command {
        self.commands
            .recv_timeout(WATCHDOG)
            .expect("control command")
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, WATCHDOG);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn text_result(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn legacy_and_current_peers_discover_typed_tools_without_a_game() {
    for protocol in ["2024-11-05", "2025-06-18"] {
        let mut client = Mcp::start("127.0.0.1:0".parse().unwrap(), protocol);
        let response = client.rpc("tools/list", json!({}));
        let tools = response["result"]["tools"].as_array().unwrap();
        let ping = tools.iter().find(|tool| tool["name"] == "ping").unwrap();
        assert_eq!(
            ping["description"],
            "Check that the game is running and reachable."
        );
        let teleport = tools
            .iter()
            .find(|tool| tool["name"] == "teleport")
            .unwrap();
        assert_eq!(teleport["inputSchema"]["type"], "object");
        for coordinate in ["x", "y", "z"] {
            assert_eq!(
                teleport["inputSchema"]["properties"][coordinate]["type"],
                "number"
            );
            assert!(
                teleport["inputSchema"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(coordinate))
            );
        }
    }
}

#[test]
fn tools_open_fresh_control_connections_and_preserve_typed_commands() {
    let generation = AtomicUsize::new(0);
    let game = Game::start(move |_| {
        Ok(json!({"fixture_generation": generation.fetch_add(1, Ordering::Relaxed)}))
    });
    let mut client = Mcp::start(game.address, "2025-06-18");
    assert_eq!(
        text_result(&client.tool("state", json!({})))["fixture_generation"],
        0
    );
    assert!(matches!(game.command(), Command::State));
    assert_eq!(
        text_result(&client.tool("state", json!({})))["fixture_generation"],
        1
    );
    assert!(matches!(game.command(), Command::State));
    client.tool("input", json!({"key": "ArrowUp", "action": "release"}));
    assert!(
        matches!(game.command(), Command::Input { key, action: InputAction::Release } if key == "ArrowUp")
    );
    client.tool("teleport", json!({"x": 1.25, "y": -2.5, "z": 3.0}));
    let Command::Teleport { x, y, z } = game.command() else {
        panic!("teleport tool did not forward a teleport command");
    };
    assert_eq!((x, y, z), (1.25, -2.5, 3.0));
}

#[test]
fn invalid_parameters_and_control_errors_do_not_poison_stdio() {
    let game = Game::start(|command| match command {
        Command::State => Err("fixture world unavailable".into()),
        _ => Ok(json!({"fixture_reply": true})),
    });
    let mut client = Mcp::start(game.address, "2025-06-18");
    for arguments in [
        json!({"key": "ArrowUp", "action": "invalid"}),
        json!({"key": 12, "action": "press"}),
    ] {
        let response = client.tool("input", arguments);
        assert_eq!(response["error"]["code"], -32602);
        assert!(game.commands.try_recv().is_err());
    }
    let rejected = client.tool("state", json!({}));
    assert_eq!(rejected["error"]["code"], -32603);
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("fixture world unavailable")
    );
    assert!(matches!(game.command(), Command::State));
    assert_eq!(
        text_result(&client.tool("ping", json!({}))),
        json!({"fixture_reply": true})
    );
    assert!(matches!(game.command(), Command::Ping));
}

#[test]
fn screenshots_preserve_text_and_image_content() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("screenshot.png");
    let game = Game::start(|command| {
        if let Command::Screenshot { path } = command {
            std::fs::write(path, b"synthetic screenshot").unwrap();
        }
        Ok(json!({}))
    });
    let mut client = Mcp::start(game.address, "2025-06-18");
    let response = client.tool("screenshot", json!({"path": path}));
    assert!(
        matches!(game.command(), Command::Screenshot { path: requested } if std::path::Path::new(&requested) == path)
    );
    assert_eq!(
        response["result"]["content"][0]["text"],
        path.to_str().unwrap()
    );
    let image = &response["result"]["content"][1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    assert_eq!(
        image["data"],
        base64::engine::general_purpose::STANDARD.encode(b"synthetic screenshot")
    );
    std::fs::remove_file(path).unwrap();
}
