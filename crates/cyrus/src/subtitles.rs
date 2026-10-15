//! Connect native speech metadata to subtitle presentation without coupling the backends.
use bevy::prelude::*;
use cyrus_audio::sound::{SoundRequest, Speech};
use cyrus_render::subtitles::{SubtitleFont, SubtitleRequest, UpdateSubtitles};

pub(crate) fn register(app: &mut App) {
    app.add_systems(
        Update,
        forward
            .before(UpdateSubtitles)
            .run_if(resource_exists::<SubtitleFont>),
    );
}

fn forward(
    mut requests: MessageReader<SoundRequest>,
    speech: Res<Speech>,
    mut subtitles: MessageWriter<SubtitleRequest>,
) {
    for request in requests.read() {
        match request {
            SoundRequest::Speech(label) => {
                if let Some(clip) = speech.0.get(label) {
                    subtitles.write(SubtitleRequest::Speech {
                        text: clip.text.clone(),
                        seconds: clip.seconds,
                    });
                }
            }
            SoundRequest::StopAll => {
                subtitles.write(SubtitleRequest::StopAll);
            }
            SoundRequest::Play { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_known_speech_and_stop_in_order_without_effect_requests() {
        let mut app = App::new();
        app.add_message::<SoundRequest>()
            .add_message::<SubtitleRequest>()
            .init_resource::<Speech>()
            .add_systems(Update, forward);
        app.world_mut().resource_mut::<Speech>().0.insert(
            "greeting".into(),
            cyrus_assets::sound::VoiceClip {
                bits_per_sample: 8,
                text: "Hello.".into(),
                seconds: 2.5,
                audio: Handle::default(),
                base_volume: 127,
            },
        );
        for request in [
            SoundRequest::Speech("missing".into()),
            SoundRequest::Speech("greeting".into()),
            SoundRequest::Play {
                effect: 0,
                position: None,
            },
            SoundRequest::StopAll,
        ] {
            app.world_mut().write_message(request);
        }
        app.update();
        let requests: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<SubtitleRequest>>()
            .drain()
            .collect();
        assert_eq!(
            requests,
            vec![
                SubtitleRequest::Speech {
                    text: "Hello.".into(),
                    seconds: 2.5
                },
                SubtitleRequest::StopAll,
            ]
        );
    }
}
