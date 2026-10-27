//! Dialogue replacements retain record continuation without replaying speech.
use cyrus_assets::script::DialogueSettings;
use cyrus_game::script::{
    DialogueEffects, DialoguePlayback, DialogueSnapshot, DialogueState, ScriptActorId,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn settings() -> DialogueSettings {
    DialogueSettings {
        menu_start: [35, 25],
        max_menu_items: 300,
        max_dialog_lines: 16,
        max_text_width: 500,
        menu_traverse_delay: 4,
        print_text: true,
        use_speech: true,
        max_distance: 1600,
        rtx_filename: "ENGLISH.RTX".into(),
    }
}
fn actor(generation: u64, index: usize) -> ScriptActorId {
    ScriptActorId { generation, index }
}
fn empty() -> DialogueState {
    let snapshot = serde_json::from_value(json!({
        "version": 1, "settings": settings(), "skip_latched": false, "presented": null,
        "records": vec![json!({"owner": null, "text": "", "active": false, "sample": 0, "remaining": 0}); 16]
    })).unwrap();
    DialogueState::from_snapshot(&settings(), snapshot, &[], &BTreeMap::from([(0, 0)])).unwrap()
}
fn encode(state: &DialogueState) -> Value {
    serde_json::to_value(state.snapshot()).unwrap()
}
fn restore(
    value: Value,
    actors: &[(ScriptActorId, ScriptActorId)],
    samples: &BTreeMap<u8, u8>,
) -> anyhow::Result<DialogueState> {
    let snapshot: DialogueSnapshot = serde_json::from_value(value)?;
    DialogueState::from_snapshot(&settings(), snapshot, actors, samples)
}
#[derive(Default, Debug)]
struct Effects {
    events: Vec<String>,
    playing: Vec<u8>,
}
impl DialoguePlayback for Effects {
    fn sample_playing(&mut self, sample: u8) -> anyhow::Result<bool> {
        self.events.push(format!("playing {sample}"));
        Ok(self.playing.contains(&sample))
    }
    fn stop_sample(&mut self, sample: u8) -> anyhow::Result<()> {
        self.events.push(format!("stop {sample}"));
        Ok(())
    }
    fn refresh_volume(&mut self, speaker: ScriptActorId, sample: u8) -> anyhow::Result<()> {
        self.events.push(format!(
            "volume {} {} {sample}",
            speaker.generation, speaker.index
        ));
        Ok(())
    }
    fn present_line(&mut self, index: usize, text: &str) -> anyhow::Result<()> {
        self.events.push(format!("present {index} {text}"));
        Ok(())
    }
}
impl DialogueEffects for Effects {
    fn inhibit_speaker(&mut self, speaker: ScriptActorId) -> anyhow::Result<()> {
        self.events
            .push(format!("inhibit {} {}", speaker.generation, speaker.index));
        Ok(())
    }
}

#[test]
fn ordered_records_and_voiced_continuation_use_explicit_rebound_owners() {
    let first = actor(4, 2);
    let second = actor(4, 9);
    let mut state = empty();
    assert_eq!(
        state.begin_line(first, "discarded".into(), Some(3), -70),
        Some(1)
    );
    assert_eq!(
        state.begin_line(second, "second".into(), Some(4), 105),
        Some(2)
    );
    state.clear_line(Some(1));
    state.begin_player_line(first, "player".into(), Some(3), 42);
    let mut effects = Effects {
        playing: vec![3],
        ..Default::default()
    };
    state.advance_records(1, true, &mut effects).unwrap();
    assert_eq!(state.presented_line(), Some((0, "player")));
    let saved = encode(&state);
    assert_eq!(
        saved["records"][0],
        json!({"owner":[4,2],"text":"player","active":true,"sample":3,"remaining":21})
    );
    assert_eq!(
        saved["records"][1],
        json!({"owner":[4,2],"text":"","active":false,"sample":null,"remaining":-70})
    );
    assert_eq!(
        saved["records"][2],
        json!({"owner":[4,9],"text":"second","active":true,"sample":4,"remaining":105})
    );
    let actors = [(first, actor(77, 12)), (second, actor(77, 19))];
    let samples = BTreeMap::from([(0, 8), (3, 9), (4, 10)]);
    let mut resumed = restore(saved, &actors, &samples).unwrap();
    assert_eq!(resumed.presented_line(), Some((0, "player")));
    assert_eq!(
        encode(&resumed)["records"][1],
        json!({"owner":[77,12],"text":"","active":false,"sample":null,"remaining":-70})
    );
    assert_eq!(encode(&resumed)["records"][3]["sample"], 8);
    let mut effects = Effects {
        playing: vec![9],
        ..Default::default()
    };
    resumed.advance_records(2, true, &mut effects).unwrap();
    assert_eq!(
        effects.events,
        [
            "inhibit 77 12",
            "playing 9",
            "volume 77 12 9",
            "present 0 player"
        ]
    );
    assert_eq!(encode(&resumed)["records"][0]["remaining"], -21);
    effects.events.clear();
    effects.playing.clear();
    resumed.advance_records(0, true, &mut effects).unwrap();
    assert_eq!(effects.events, ["inhibit 77 12", "playing 9"]);
    assert!(!resumed.line_active(Some(0)));
    effects.events.clear();
    effects.playing.push(10);
    resumed.advance_records(1, true, &mut effects).unwrap();
    assert_eq!(
        effects.events,
        [
            "inhibit 77 19",
            "playing 10",
            "volume 77 19 10",
            "present 2 second"
        ]
    );
    assert_eq!(encode(&resumed)["records"][2]["remaining"], 84);
    assert_eq!(
        resumed.begin_line(actor(77, 12), "reuse".into(), None, 100),
        Some(1)
    );
}

#[test]
fn skip_latch_signed_lifetime_and_stale_presentation_survive_replacement() {
    let speaker = actor(5, 2);
    let bindings = [(speaker, actor(90, 3))];
    let mut state = empty();
    state.begin_player_line(speaker, "held".into(), None, i32::MIN);
    let mut effects = Effects::default();
    state.advance(0, true, true, Some(2), &mut effects).unwrap();
    let mut lost_latch = encode(&state);
    lost_latch["skip_latched"] = json!(false);
    let mut control = restore(lost_latch, &bindings, &BTreeMap::from([(0, 8)])).unwrap();
    assert!(control.advance(1, true, true, None, &mut effects).is_err());
    let mut resumed = restore(encode(&state), &bindings, &BTreeMap::from([(0, 8)])).unwrap();
    resumed.advance(1, true, true, None, &mut effects).unwrap();
    assert_eq!(encode(&resumed)["records"][0]["remaining"], 2147483627_i32);
    resumed
        .advance(0, false, false, None, &mut effects)
        .unwrap();
    assert_eq!(resumed.presented_line(), Some((0, "held")));
    resumed.clear_line(Some(0));
    let stale = encode(&resumed);
    assert_eq!(stale["presented"], 0);
    let mut restored = restore(
        stale,
        &[(actor(90, 3), actor(91, 4))],
        &BTreeMap::from([(8, 7)]),
    )
    .unwrap();
    assert_eq!(restored.presented_line(), None);
    assert_eq!(encode(&restored)["presented"], 0);
    restored
        .advance(0, true, true, Some(1), &mut effects)
        .unwrap();
    assert!(!restored.waiting());
}

#[test]
fn malformed_snapshots_and_incomplete_or_aliased_bindings_are_rejected() {
    let first = actor(4, 2);
    let second = actor(4, 9);
    let mut state = empty();
    state.begin_player_line(first, "player".into(), Some(3), 42);
    state.begin_line(second, "ordinary".into(), None, 20);
    state.clear_line(Some(1));
    let saved = encode(&state);
    let actors = [(first, actor(90, 2)), (second, actor(90, 9))];
    let samples = BTreeMap::from([(0, 8), (3, 9)]);
    let mutations = [
        ("version", json!(2)),
        ("settings", json!({})),
        ("records", json!([])),
        ("presented", json!(16)),
    ];
    for (field, value) in mutations {
        let mut bad = saved.clone();
        bad[field] = value;
        assert!(restore(bad, &actors, &samples).is_err(), "{field}");
    }
    let mut wrong_settings = saved.clone();
    wrong_settings["settings"]["max_distance"] = json!(1);
    assert!(
        restore(wrong_settings, &actors, &samples)
            .unwrap_err()
            .to_string()
            .contains("installed settings")
    );
    let mut bad = saved.clone();
    bad["records"][0]["owner"] = Value::Null;
    assert!(
        format!("{:#}", restore(bad, &actors, &samples).unwrap_err())
            .contains("record 0: active speaker")
    );
    let mut bad = saved.clone();
    bad["records"][0]["sample"] = json!(16);
    assert!(
        format!("{:#}", restore(bad, &actors, &samples).unwrap_err())
            .contains("record 0: saved sample 16")
    );
    let missing_active = restore(saved.clone(), &actors[1..], &samples).unwrap_err();
    assert!(format!("{missing_active:#}").contains("record 0: speaker (4, 2) binding unavailable"));
    let restored = restore(saved.clone(), &actors[..1], &samples).unwrap();
    assert_eq!(encode(&restored)["records"][1]["owner"], Value::Null);
    assert!(restore(saved.clone(), &actors, &BTreeMap::from([(3, 9)])).is_err());
    assert!(restore(saved.clone(), &actors, &BTreeMap::from([(0, 8), (3, 8)])).is_err());
    assert!(restore(saved.clone(), &actors, &BTreeMap::from([(0, 8), (3, 16)])).is_err());
    assert!(restore(saved.clone(), &[actors[0], actors[0]], &samples).is_err());
    assert!(
        restore(
            saved.clone(),
            &[(first, actor(90, 2)), (second, actor(90, 2))],
            &samples
        )
        .is_err()
    );
    assert_eq!(encode(&state), saved);
    assert!(state.waiting());
}
