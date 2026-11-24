//! Original retained light and flat controls bind without rerunning script natives.
use super::*;
use rgpre::import::savegame::SaveObject;

impl ScriptWorld {
    /// Replace actor light and flat records after resolving every texture lookup.
    /// This preserves playback counters and modulation phases; no native callback
    /// runs, and no first-update maintenance is performed during restoration.
    pub fn restore_original_presentation(
        &mut self,
        bindings: &[(ScriptActorId, &SaveObject)],
    ) -> Result<()> {
        let mut seen = BTreeSet::new();
        let mut replacements = Vec::with_capacity(bindings.len());
        for &(id, object) in bindings {
            let index = self.resolve(id)?;
            anyhow::ensure!(
                seen.insert(index),
                "original presentation: duplicate actor {index}"
            );
            let actor = &self.actors[index];
            let expected = match actor.source {
                ScriptActorSource::Player => -2,
                ScriptActorSource::AlternatePlayer => -3,
                ScriptActorSource::Camera => 2,
                ScriptActorSource::Map(_) => i32::from(
                    actor
                        .runtime_identifier
                        .context("original presentation runtime identity absent")?
                        as u16,
                ),
                _ => anyhow::bail!("original presentation actor {index}: unsupported owner"),
            };
            anyhow::ensure!(
                expected == object.object_id
                    && actor
                        .runtime_identifier
                        .is_some_and(|id| id as u16 == object.runtime_identifier()),
                "original presentation actor {index}: saved identity differs"
            );
            let light = object
                .light()
                .map(|saved| restore_light(saved, object.position_q8()));
            let flat = object
                .flat()
                .map(|saved| -> Result<_> {
                    let bank = saved.texture >> 7;
                    let image = saved.texture & 0x7f;
                    let bitmap = self
                        .texture_frames
                        .get(&bank)
                        .is_some_and(|images| images.contains_key(&image))
                        .then_some((bank, image));
                    Ok(flat::FlatState {
                        enabled: saved.enabled,
                        pending: saved.pending,
                        offset: saved.offset,
                        texture: saved.texture,
                        bitmap,
                        frame: saved.frame,
                        last_frame: saved.last_frame,
                        counter: saved.counter,
                        animate: saved.animate,
                    })
                })
                .transpose()?;
            replacements.push((index, light, flat));
        }
        for (index, light, flat) in replacements {
            self.actors[index].light = light;
            self.actors[index].flat = flat;
        }
        Ok(())
    }
}

fn restore_light(
    saved: rgpre::import::savegame::SaveActorLight,
    position_q8: [i32; 3],
) -> light::LightState {
    light::LightState {
        locked: saved.locked,
        enabled: saved.enabled,
        inactive_ticks: saved.inactive_ticks,
        offset: saved.offset,
        position: position_q8.map(|value| value >> 8),
        starting_intensity: saved.starting_intensity,
        intensity: saved.intensity,
        starting_radius: saved.starting_radius,
        radius: saved.radius,
        rgb: saved.rgb,
        starting_rgb: Some(saved.starting_rgb),
        flicker: saved.flicker,
        size: saved.size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_light_keeps_effect_phase_and_starting_colour_without_reinitialization() {
        let saved = rgpre::import::savegame::SaveActorLight {
            locked: false,
            enabled: false,
            inactive_ticks: 31,
            offset: [-7, 8, 9],
            starting_intensity: 300,
            intensity: 234,
            starting_radius: 120,
            radius: 97,
            flicker: [21, 19, 24],
            size: [17, 13, 18],
            rgb: [-1, 2, 3],
            starting_rgb: [4, 5, 6],
        };
        let restored = restore_light(saved, [-257, 512, 768]);
        assert!(!restored.enabled);
        assert_eq!(restored.inactive_ticks, 31);
        assert_eq!(restored.flicker, [21, 19, 24]);
        assert_eq!(restored.size, [17, 13, 18]);
        assert_eq!(restored.intensity, 234);
        assert_eq!(restored.radius, 97);
        assert_eq!(restored.starting_rgb, Some([4, 5, 6]));
        assert_eq!(restored.position, [-2, 2, 3]);
    }
}
