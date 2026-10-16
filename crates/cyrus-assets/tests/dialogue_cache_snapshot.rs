use anyhow::Result;
use cyrus_assets::sound::{DialogueCache, DialoguePayload, DialogueSamples};
use std::collections::BTreeMap;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Payload {
    text: String,
}
impl DialoguePayload for Payload {
    fn has_voice(&self) -> bool {
        true
    }
    fn release_text(&mut self) {
        self.text.clear();
    }
}
struct Samples {
    stopped: Vec<u8>,
    fail: bool,
}
impl DialogueSamples for Samples {
    fn playing(&mut self, _: u8) -> Result<bool> {
        Ok(true)
    }
    fn stop(&mut self, sample: u8) -> Result<()> {
        anyhow::ensure!(!self.fail, "deliberate stop failure");
        self.stopped.push(sample);
        Ok(())
    }
}

#[test]
fn restored_cache_retains_failed_release_and_rebinds_remembered_sample() -> Result<()> {
    let mut cache = DialogueCache::default();
    let slot = cache.load(*b"line", |_| {
        Ok(Payload {
            text: "retained".into(),
        })
    })?;
    cache.remember_sample(slot, Some(3))?;
    let mut samples = Samples {
        stopped: vec![],
        fail: true,
    };
    assert!(cache.reset(&mut samples).is_err());
    assert_eq!(cache.payload(slot).unwrap().text, "");
    let snapshot = cache.snapshot(|payload| Ok(payload.clone()))?;
    let bytes = serde_json::to_vec(&snapshot)?;
    let mut restored = DialogueCache::from_snapshot(
        serde_json::from_slice(&bytes)?,
        &BTreeMap::from([(3, 9)]),
        |label, payload: Payload| {
            assert_eq!(label, *b"line");
            Ok(payload)
        },
    )?;
    assert_eq!(restored.sample(slot), Some(9));
    assert_eq!(restored.payload(slot).unwrap().text, "");
    assert_eq!(
        restored.load(*b"line", |_| anyhow::bail!("must reuse retained payload"))?,
        slot
    );
    samples.fail = false;
    restored.reset(&mut samples)?;
    assert_eq!(samples.stopped, [9]);
    assert!(restored.payload(slot).is_none());
    Ok(())
}

#[test]
fn malformed_cache_candidate_leaves_live_payload_and_sample_untouched() -> Result<()> {
    let mut live = DialogueCache::default();
    let slot = live.load(*b"line", |_| {
        Ok(Payload {
            text: "accepted".into(),
        })
    })?;
    live.remember_sample(slot, Some(3))?;
    let snapshot = live.snapshot(|payload| Ok(payload.clone()))?;
    let mut value = serde_json::to_value(&snapshot)?;
    value["slots"][0]["age"] = 9.into();
    assert!(
        DialogueCache::from_snapshot(
            serde_json::from_value(value)?,
            &BTreeMap::from([(3, 3)]),
            |_, payload: Payload| Ok(payload),
        )
        .is_err()
    );
    assert!(
        DialogueCache::from_snapshot(snapshot, &BTreeMap::new(), |_, payload: Payload| Ok(
            payload
        ))
        .is_err()
    );
    assert_eq!(live.payload(slot).unwrap().text, "accepted");
    assert_eq!(live.sample(slot), Some(3));
    Ok(())
}
