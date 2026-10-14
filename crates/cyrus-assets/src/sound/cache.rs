//! Portable RTX payload retention; the caller owns synchronous sample feedback.
use anyhow::{Context, Result};

use crate::sound::DialogueSamples;

struct Slot<T> {
    label: Option<[u8; 4]>,
    payload: Option<T>,
    sample: Option<u8>,
    used: bool,
    age: u32,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            label: None,
            payload: None,
            sample: None,
            used: false,
            age: 0,
        }
    }
}

/// Owned RTX payload operations needed by original cache maintenance.
/// Releasing text must retain voice ownership until a sample stop succeeds.
pub trait DialoguePayload {
    fn has_voice(&self) -> bool;
    fn release_text(&mut self);
}

impl DialoguePayload for super::DialogueEntry {
    fn has_voice(&self) -> bool {
        self.voice.is_some()
    }
    fn release_text(&mut self) {
        self.text = String::new();
    }
}

/// Twenty retained RTX entries, following preservation engine/sound.md.
/// Labels retain authored case for cache identity; the archive folds file lookup.
/// The game host owns this cache and associates returned indices with dialogue.
pub struct DialogueCache<T: DialoguePayload> {
    slots: [Slot<T>; 20],
}

impl<T: DialoguePayload> Default for DialogueCache<T> {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| Slot::default()),
        }
    }
}

impl<T: DialoguePayload> DialogueCache<T> {
    /// Reuse the same authored label or replace the first slot not marked used.
    /// Exhaustion returns a contextual error without replacing a used entry.
    /// `read` executes only on a cache miss and returns an owned payload.
    pub fn load(
        &mut self,
        label: [u8; 4],
        read: impl FnOnce([u8; 4]) -> Result<T>,
    ) -> Result<usize> {
        if let Some(index) = self.reuse(label) {
            return Ok(index);
        }
        let index = self
            .slots
            .iter()
            .position(|slot| !slot.used)
            .with_context(|| {
                format!(
                    "dialogue cache exhausted for label {:?}",
                    String::from_utf8_lossy(&label)
                )
            })?;
        let payload = read(label)?;
        self.slots[index] = Slot {
            label: Some(label),
            payload: Some(payload),
            used: true,
            age: 8,
            ..Slot::default()
        };
        Ok(index)
    }

    /// Mark a retained authored label used without requesting another payload.
    /// A miss leaves occupancy and replacement ages unchanged, allowing the host
    /// to prepare an asynchronous payload before calling `load`.
    pub fn reuse(&mut self, label: [u8; 4]) -> Option<usize> {
        let index = self
            .slots
            .iter()
            .position(|slot| slot.label == Some(label))?;
        self.slots[index].used = true;
        self.slots[index].age = 8;
        Some(index)
    }

    /// Retained payload at a previously returned cache index.
    pub fn payload(&self, index: usize) -> Option<&T> {
        self.slots.get(index)?.payload.as_ref()
    }

    /// Remembered channel slot, independent of the sample's current status.
    pub fn sample(&self, index: usize) -> Option<u8> {
        self.slots.get(index).and_then(|slot| slot.sample)
    }

    /// Retain the allocated channel only for an occupied voiced entry.
    pub fn remember_sample(&mut self, index: usize, sample: Option<u8>) -> Result<()> {
        anyhow::ensure!(
            sample.is_none_or(|sample| sample < 16),
            "dialogue cache slot {index}: sample handle outside sixteen channels"
        );
        let slot = self
            .slots
            .get_mut(index)
            .context("dialogue cache index out of range")?;
        anyhow::ensure!(
            slot.payload
                .as_ref()
                .is_some_and(DialoguePayload::has_voice),
            "dialogue cache slot {index}: no voice payload for sample assignment"
        );
        slot.sample = sample;
        Ok(())
    }

    /// Playing entries do not age. Used entries reset to eight nominal ticks;
    /// unused entries release only when their age is strictly below this update.
    pub fn advance(&mut self, nominal_ticks: u32, samples: &mut dyn DialogueSamples) -> Result<()> {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if !slot
                .payload
                .as_ref()
                .is_some_and(DialoguePayload::has_voice)
            {
                slot.sample = None;
            }
            if let Some(sample) = slot.sample {
                if samples.playing(sample).with_context(|| {
                    format!("dialogue cache slot {index}: sample {sample} status")
                })? {
                    continue;
                }
                slot.sample = None;
            }
            if slot.used {
                slot.used = false;
                slot.age = 8;
            } else if slot.age < nominal_ticks {
                *slot = Slot::default();
            } else {
                slot.age -= nominal_ticks;
            }
        }
        Ok(())
    }

    /// Release text before checking/stopping each remembered playing channel,
    /// then release audio and clear occupancy. Errors retain remaining ownership.
    pub fn reset(&mut self, samples: &mut dyn DialogueSamples) -> Result<()> {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Some(payload) = &mut slot.payload {
                payload.release_text();
            }
            if let Some(sample) = slot.sample
                && samples.playing(sample).with_context(|| {
                    format!("dialogue cache slot {index}: sample {sample} reset status")
                })?
            {
                samples.stop(sample).with_context(|| {
                    format!("dialogue cache slot {index}: sample {sample} reset stop")
                })?;
            }
            *slot = Slot::default();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::{DialogueEntry, DialogueVoice};

    struct Samples {
        playing: bool,
        fail_stop: bool,
        events: Vec<&'static str>,
    }
    impl DialogueSamples for Samples {
        fn playing(&mut self, sample: u8) -> Result<bool> {
            assert_eq!(sample, 3);
            self.events.push("status");
            Ok(self.playing)
        }
        fn stop(&mut self, sample: u8) -> Result<()> {
            assert_eq!(sample, 3);
            self.events.push("stop");
            anyhow::ensure!(!self.fail_stop, "backend stop failed");
            Ok(())
        }
    }

    fn voiced_cache() -> DialogueCache<DialogueEntry> {
        let mut cache = DialogueCache::default();
        cache.slots[0] = Slot {
            label: Some(*b"voic"),
            payload: Some(DialogueEntry {
                version: 1,
                label: *b"voic",
                text: "Hello".into(),
                text_lifetime: 75,
                voice: Some(DialogueVoice {
                    audio: "dialogue/766f6963.wav".into(),
                    sample_rate: 11025,
                    channels: 1,
                    bits_per_sample: 16,
                    base_volume: 100,
                    loop_flag: 0,
                    loop_offset: 0,
                    loop_end: u32::MAX,
                    lifetime: 1,
                }),
            }),
            sample: Some(3),
            used: true,
            age: 0,
        };
        cache
    }

    #[test]
    fn dialogue_cache_playing_retains_used_flag_until_finished_then_starts_aging() {
        let mut cache = voiced_cache();
        let mut samples = Samples {
            playing: true,
            fail_stop: false,
            events: vec![],
        };
        cache.advance(999, &mut samples).unwrap();
        assert_eq!(cache.sample(0), Some(3));
        samples.playing = false;
        cache.advance(999, &mut samples).unwrap();
        assert_eq!(cache.sample(0), None);
        cache.advance(8, &mut samples).unwrap();
        assert!(cache.payload(0).is_some());
        cache.advance(1, &mut samples).unwrap();
        assert!(cache.payload(0).is_none());
        assert_eq!(samples.events, ["status", "status"]);
    }

    #[test]
    fn dialogue_cache_reset_releases_text_before_stop_and_keeps_audio_on_failure() {
        let mut cache = voiced_cache();
        let mut samples = Samples {
            playing: true,
            fail_stop: true,
            events: vec![],
        };
        assert!(
            cache
                .reset(&mut samples)
                .unwrap_err()
                .to_string()
                .contains("slot 0")
        );
        let payload = cache
            .payload(0)
            .expect("audio ownership must survive failed stop");
        assert!(payload.text.is_empty());
        assert_eq!(
            payload.voice.as_ref().unwrap().audio,
            "dialogue/766f6963.wav"
        );
        assert_eq!(cache.sample(0), Some(3));
        samples.fail_stop = false;
        cache.reset(&mut samples).unwrap();
        assert!(cache.payload(0).is_none());
        assert_eq!(samples.events, ["status", "stop", "status", "stop"]);
    }
}
