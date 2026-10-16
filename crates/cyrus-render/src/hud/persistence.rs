//! Retained HUD presentation continuation, separate from inventory and journal owners.
use super::{HudAsset, HudPresentationAssets, HudState};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

/// Opaque HUD presentation checkpoint. Image handles remain prepared host assets.
#[derive(Clone, Serialize, Deserialize)]
pub struct HudPresentationSnapshot {
    version: u32,
    source_identity: String,
    state: HudState,
    elapsed_bits: u32,
    bindings: [u8; 15],
    text: Vec<(String, [i32; 2])>,
    dialogue_text: Option<(usize, String)>,
}

fn source_identity(asset: &HudAsset) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(asset)?)
        .to_hex()
        .to_string())
}

impl HudState {
    /// Capture after the HUD update, retaining flicker, icon, timers and fractional
    /// frame progress. Asset identity covers installed layout, text and inventory.
    pub fn snapshot(&self, asset: &HudAsset) -> Result<HudPresentationSnapshot> {
        validate(self, asset)?;
        let mut state = self.clone();
        state.frames = HudPresentationAssets::default();
        Ok(HudPresentationSnapshot {
            version: 1,
            source_identity: source_identity(asset)?,
            state,
            elapsed_bits: self.elapsed.to_bits(),
            bindings: self.bindings,
            text: self.text.clone(),
            dialogue_text: self.dialogue_text.clone(),
        })
    }

    /// Reconstruct off-live using an already prepared HUD's image handles.
    /// Does not load assets, configure gameplay HUD, consume notifications or advance.
    pub fn from_snapshot(
        asset: &HudAsset,
        prepared: &HudPresentationAssets,
        snapshot: HudPresentationSnapshot,
    ) -> Result<Self> {
        ensure!(
            snapshot.version == 1,
            "HUD presentation snapshot version unsupported"
        );
        ensure!(
            snapshot.source_identity == source_identity(asset)?,
            "HUD presentation installed content differs"
        );
        ensure!(
            prepared.candle.len() == 81
                && prepared.compass.len() == 32
                && prepared.logbook.len() == 32
                && prepared.powerup.len() == asset.powerup_sizes.len(),
            "HUD presentation replacement image census differs"
        );
        let mut state = snapshot.state;
        state.elapsed = f32::from_bits(snapshot.elapsed_bits);
        state.bindings = snapshot.bindings;
        state.text = snapshot.text;
        state.dialogue_text = snapshot.dialogue_text;
        state.frames = prepared.clone();
        state.redraw_text = true;
        validate(&state, asset)?;
        Ok(state)
    }
}

fn validate(state: &HudState, asset: &HudAsset) -> Result<()> {
    ensure!(
        state.health <= 100 && state.candle_mode <= 2 && state.show_timer <= 50,
        "HUD presentation health/candle state outside range"
    );
    ensure!(
        state.candle.shown.is_none_or(|level| level <= 11)
            && state.candle.step <= 14
            && state.candle.frame < 81,
        "HUD presentation candle continuation outside range"
    );
    ensure!(
        state.heading < 2048
            && state.compass_frame.is_none_or(|frame| frame < 32)
            && state.logbook_frame.is_none_or(|frame| frame < 32),
        "HUD presentation heading/icon frame outside range"
    );
    ensure!(
        state.notices.iter().all(|ticks| *ticks <= 24),
        "HUD presentation notice timer outside range"
    );
    ensure!(
        state.elapsed.is_finite() && (0.0..(1.0 / 60.0)).contains(&state.elapsed),
        "HUD presentation fractional frame progress outside range"
    );
    ensure!(
        state
            .powerups
            .iter()
            .flatten()
            .all(|(_, frame)| usize::from(*frame) < asset.powerup_sizes.len()),
        "HUD presentation power-up frame outside installed atlas"
    );
    Ok(())
}
