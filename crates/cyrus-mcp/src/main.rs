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
struct ConsoleParams {
    open: Option<bool>,
    line: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct CheatParams {
    name: String,
    #[schemars(
        description = "Raw state value; omit to toggle, list with oracle, or persist with savecheats"
    )]
    value: Option<u32>,
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
    #[schemars(description = "player_controller, camera, water, fog, sky, lighting or music")]
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
struct TextureFrameParams {
    world: String,
    image: String,
    #[schemars(
        description = "Zero-based measured frame to hold; omit or null to resume automatic playback"
    )]
    frame: Option<usize>,
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
        description = "Optional PNG path; defaults to game-config/screenshots/<timestamp>.png"
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

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ScriptsParams {
    offset: Option<usize>,
    limit: Option<usize>,
}
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ScriptStepParams {
    generation: u64,
    index: usize,
    #[schemars(
        description = "prepare_spatial, snapshot, axis_locks, transform, families, animation, native_hook, ordinary_hook, ordinary_main, entry, main, interrupt, or tasks"
    )]
    stage: String,
    #[schemars(
        description = "Required for tasks; original scale with eight fractional bits. No production clock is implied."
    )]
    frame_scale: Option<u32>,
    #[schemars(
        description = "Required when any actor has animation bound; original whole nominal ticks, including zero."
    )]
    nominal_ticks: Option<u32>,
    #[schemars(
        description = "Explicit PlayerDistance override; omit to use runtime neversaydie state."
    )]
    distance_override: Option<bool>,
    #[schemars(
        description = "Q8 fallback coordinates for PlayerDistance without a selected player."
    )]
    fallback_position: Option<[i32; 3]>,
}
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ScriptAnimationParams {
    generation: u64,
    index: usize,
    default_group: u16,
    #[schemars(
        description = "Exact scene node name for visual/sound binding; omit to capture sound intents without playback."
    )]
    node: Option<String>,
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

    #[tool(
        description = "Set or toggle a cheat, list with oracle, or write REDGUARD.CHT with savecheats. Reports unsupported gameplay effects."
    )]
    fn cheat(&self, Parameters(p): Parameters<CheatParams>) -> Result<CallToolResult, McpError> {
        text(call(Command::Cheat {
            name: p.name,
            value: p.value,
        })?)
    }

    #[tool(
        description = "Inspect, open or close the F12 console and submit a command through its shared editor."
    )]
    fn console(
        &self,
        Parameters(p): Parameters<ConsoleParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::Console {
            open: p.open,
            line: p.line,
        })?)
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
            format!("game-config/screenshots/{ts}.png")
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

    #[tool(
        description = "Hold a measured texture frame in a ready world for comparison, or resume automatic playback."
    )]
    fn hold_texture_frame(
        &self,
        Parameters(p): Parameters<TextureFrameParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::HoldTextureFrame {
            world: p.world,
            image: p.image,
            frame: p.frame,
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

    #[tool(
        description = "Inspect logical script actors and explicit execution state without returning installed bytecode."
    )]
    fn scripts(
        &self,
        Parameters(p): Parameters<ScriptsParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::Scripts {
            offset: p.offset.unwrap_or(0),
            limit: p.limit.unwrap_or(32),
        })?)
    }
    #[tool(
        description = "Prepare one inspected actor's animation with an explicitly supplied default group. Does not start gameplay."
    )]
    fn bind_script_animation(
        &self,
        Parameters(p): Parameters<ScriptAnimationParams>,
    ) -> Result<CallToolResult, McpError> {
        text(call(Command::BindScriptAnimation {
            generation: p.generation,
            index: p.index,
            default_group: p.default_group,
            node: p.node,
        })?)
    }
    #[tool(
        description = "Step one explicit script stage for an inspected actor generation. Automatic gameplay remains disabled."
    )]
    fn step_script(
        &self,
        Parameters(p): Parameters<ScriptStepParams>,
    ) -> Result<CallToolResult, McpError> {
        let stage = match p.stage.as_str() {
            "prepare_spatial" => cyrus_control::ScriptStage::PrepareSpatial,
            "snapshot" => cyrus_control::ScriptStage::Snapshot,
            "axis_locks" => cyrus_control::ScriptStage::AxisLocks,
            "transform" => cyrus_control::ScriptStage::Transform,
            "families" => cyrus_control::ScriptStage::Families,
            "animation" => cyrus_control::ScriptStage::Animation,
            "ordinary_hook" => cyrus_control::ScriptStage::OrdinaryHook,
            "ordinary_main" => cyrus_control::ScriptStage::OrdinaryMain,
            "native_hook" => cyrus_control::ScriptStage::NativeHook,
            "entry" => cyrus_control::ScriptStage::Entry,
            "main" => cyrus_control::ScriptStage::Main,
            "interrupt" => cyrus_control::ScriptStage::Interrupt,
            "tasks" => cyrus_control::ScriptStage::Tasks,
            _ => {
                return Err(McpError::invalid_params(
                    "stage must be prepare_spatial, snapshot, axis_locks, transform, families, animation, native_hook, ordinary_hook, ordinary_main, entry, main, interrupt, or tasks",
                    None,
                ));
            }
        };
        text(call(Command::StepScript {
            generation: p.generation,
            index: p.index,
            stage,
            frame_scale: p.frame_scale,
            nominal_ticks: p.nominal_ticks,
            distance_override: p.distance_override,
            fallback_position: p.fallback_position,
        })?)
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
