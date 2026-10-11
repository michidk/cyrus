//! Each tool opens a fresh connection to the game's control port, so the server
//! can start before the game and survives game restarts.

use base64::Engine;
use cyrus_control::{Client, Command, InputAction};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock as Content};
use rmcp::{ErrorData as McpError, ServiceExt, schemars, tool, tool_router, transport::stdio};
use serde_json::Value;

fn addr() -> String {
    std::env::var("CYRUS_CONTROL_ADDR").unwrap_or_else(|_| cyrus_control::DEFAULT_ADDR.to_string())
}

fn call(cmd: Command) -> Result<Value, McpError> {
    let mut client = Client::connect(addr()).map_err(|e| {
        McpError::internal_error(format!("game not reachable at {}: {e}", addr()), None)
    })?;
    client
        .send(cmd)
        .map_err(|e| McpError::internal_error(e, None))
}

fn text(v: Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct TeleportParams {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct InputParams {
    #[schemars(description = "Bevy KeyCode name, e.g. KeyW, ArrowUp, Space, Escape")]
    key: String,
    #[schemars(description = "press, release or tap (press for one frame)")]
    action: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct FidelityParams {
    #[schemars(description = "player_controller, camera, water, fog or sky")]
    area: String,
    #[schemars(description = "faithful or modern")]
    mode: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SkyScrollParams {
    #[schemars(description = "Captured original sky direction, 0 through 2047")]
    direction: u16,
    #[schemars(
        description = "Captured U/V texture offsets in texels, each within -256 through 256"
    )]
    offset: [f32; 2],
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SceneParams {
    #[schemars(
        description = "Scene stem from the install's maps/ directory, e.g. ISLAND, START, TAVERN"
    )]
    name: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ScreenshotParams {
    #[schemars(
        description = "Optional PNG path; defaults to cyrus-config/screenshots/<timestamp>.png"
    )]
    path: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SoundParams {
    #[schemars(description = "0-based effect index in the install's sound/MAIN.SFX")]
    effect: usize,
    #[schemars(
        description = "Optional world position [x, y, z]; the volume then falls off with the camera's distance"
    )]
    position: Option<[f32; 3]>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SpeechParams {
    #[schemars(description = "4-character RTX label of a voice clip, e.g. &005")]
    label: String,
}

#[derive(Clone)]
struct Cyrus;

#[tool_router(server_handler)]
impl Cyrus {
    #[tool(description = "Check that the game is running and reachable.")]
    fn ping(&self) -> Result<CallToolResult, McpError> {
        text(call(Command::Ping)?)
    }

    #[tool(
        description = "App state, current scene, player position/rotation, fidelity settings and frame counter."
    )]
    fn state(&self) -> Result<CallToolResult, McpError> {
        text(call(Command::State)?)
    }

    #[tool(description = "Capture the game window as PNG and return the image.")]
    fn screenshot(
        &self,
        Parameters(p): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, McpError> {
        let path = p.path.unwrap_or_else(|| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            format!("cyrus-config/screenshots/{ts}.png")
        });
        call(Command::Screenshot { path: path.clone() })?;
        // The capture lands one or two frames later; poll for the file.
        let mut bytes = None;
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Ok(b) = std::fs::read(&path)
                && !b.is_empty()
            {
                bytes = Some(b);
                break;
            }
        }
        let Some(bytes) = bytes else {
            return Err(McpError::internal_error(
                format!("screenshot {path} did not appear"),
                None,
            ));
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(CallToolResult::success(vec![
            Content::text(path),
            Content::image(b64, "image/png"),
        ]))
    }

    #[tool(description = "Move the player to engine-space coordinates.")]
    fn teleport(
        &self,
        Parameters(p): Parameters<TeleportParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::Teleport {
            x: p.x,
            y: p.y,
            z: p.z,
        })?)
    }

    #[tool(description = "Inject a keyboard event as if the player pressed it.")]
    fn input(&self, Parameters(p): Parameters<InputParams>) -> Result<CallToolResult, McpError> {
        let action = match p.action.as_str() {
            "press" => InputAction::Press,
            "release" => InputAction::Release,
            "tap" => InputAction::Tap,
            other => {
                return Err(McpError::invalid_params(
                    format!("unknown action {other}"),
                    None,
                ));
            }
        };
        text(call(Command::Input { key: p.key, action })?)
    }

    #[tool(description = "Switch one fidelity area between faithful and modern at runtime.")]
    fn set_fidelity(
        &self,
        Parameters(p): Parameters<FidelityParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::SetFidelity {
            area: p.area,
            mode: p.mode,
        })?)
    }

    #[tool(
        description = "Synchronize sky scrolling with a captured original session for comparison."
    )]
    fn set_sky_scroll(
        &self,
        Parameters(p): Parameters<SkyScrollParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::SetSkyScroll {
            direction: p.direction,
            offset: p.offset,
        })?)
    }

    #[tool(description = "Skip startup or leave the menu and load a world from the install.")]
    fn load_scene(
        &self,
        Parameters(p): Parameters<SceneParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::LoadScene { name: p.name })?)
    }

    #[tool(description = "Skip the splash and title and stop at the book menu.")]
    fn skip_startup(&self) -> Result<CallToolResult, McpError> {
        text(call(Command::SkipStartup)?)
    }

    #[tool(
        description = "Play a sound effect from MAIN.SFX; looping effects play until stop_sounds."
    )]
    fn play_sound(
        &self,
        Parameters(p): Parameters<SoundParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::PlaySound {
            effect: p.effect,
            position: p.position,
        })?)
    }

    #[tool(description = "Play a voice clip from the install's RTX file by label.")]
    fn play_speech(
        &self,
        Parameters(p): Parameters<SpeechParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::PlaySpeech { label: p.label })?)
    }

    #[tool(description = "Stop every playing sound effect and voice clip; music keeps playing.")]
    fn stop_sounds(&self) -> Result<CallToolResult, McpError> {
        text(call(Command::StopSounds)?)
    }

    #[tool(description = "Exit the game.")]
    fn quit(&self) -> Result<CallToolResult, McpError> {
        text(call(Command::Quit)?)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = Cyrus.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
