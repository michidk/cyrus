//! Shared installed Smacker playback. Rules live in preservation's engine/movies.md.
use super::StartupContent;
use super::menu::MenuState;
use super::menu::MovieRequest;
use super::menu::MovieStatus;
#[cfg(not(target_arch = "wasm32"))]
use super::movie_native::StreamingAudio;
#[cfg(not(target_arch = "wasm32"))]
use bevy::audio::{AddAudioSource, AudioPlayer, AudioSink, PlaybackSettings};
use bevy::prelude::*;
use cyrus_assets::movie::{MovieDefinition, Prepared};
use cyrus_game::AppState;
#[cfg(not(target_arch = "wasm32"))]
use cyrus_media::Packets;
use std::{sync::Mutex, time::Duration};
#[cfg(target_arch = "wasm32")]
pub(super) const BROWSER_DECODE_BUDGET: Duration = Duration::from_millis(8);
#[cfg(not(target_arch = "wasm32"))]
use {
    bevy::tasks::{IoTaskPool, Task, futures::check_ready},
    cyrus_assets::install::GameInstall,
    cyrus_assets::startup::movie_native::prepare,
    std::sync::mpsc,
};

#[derive(Resource, Default)]
pub(super) struct MoviePlayback {
    new_game: bool,
    /// A gameplay script's movie is loading or playing; the world it paused resumes afterwards.
    /// Holds the `LoadWorld` destination that was waiting behind it.
    script: Option<Option<[i32; 3]>>,
    #[cfg(not(target_arch = "wasm32"))]
    load: Option<Task<anyhow::Result<Prepared>>>,
    #[cfg(target_arch = "wasm32")]
    load: Option<Handle<Prepared>>,
    playing: Option<Playing>,
}
impl MoviePlayback {
    /// No movie is loading or playing.
    pub(super) fn idle(&self) -> bool {
        self.load.is_none() && self.playing.is_none()
    }
}
struct Playing {
    movie: Prepared,
    decoder: Decoder,
    /// One decoder per skip point, already decoded up to it and waiting on its bounded
    /// channel, so a skip swaps decoders instead of decoding every frame before it.
    #[cfg(not(target_arch = "wasm32"))]
    prefetched: Vec<(u32, Decoder)>,
    image: Handle<Image>,
    #[cfg(not(target_arch = "wasm32"))]
    audio: Option<Handle<StreamingAudio>>,
    elapsed: f64,
    next_frame: u32,
    started: bool,
    audio_started: bool,
}
use cyrus_audio::CinematicAudio;
#[cfg(not(target_arch = "wasm32"))]
type NativeFrame = (Vec<u8>, Option<StreamingAudio>);

/// Dropping it disconnects the channel, which stops the decoding thread.
struct Decoder {
    #[cfg(not(target_arch = "wasm32"))]
    frames: Mutex<mpsc::Receiver<anyhow::Result<NativeFrame>>>,
    #[cfg(not(target_arch = "wasm32"))]
    audio: Option<StreamingAudio>,
    #[cfg(target_arch = "wasm32")]
    frames: Mutex<super::movie_stream::BrowserMovie>,
}
impl Decoder {
    fn next(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            match self.frames.lock().unwrap().try_recv() {
                Ok(frame) => frame.map(|(pixels, audio)| {
                    self.audio = audio;
                    Some(pixels)
                }),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => {
                    anyhow::bail!("Movie decoder disconnected before completion")
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(self.frames.get_mut().unwrap().next())
        }
    }
}

pub(super) fn register(app: &mut App) {
    #[cfg(not(target_arch = "wasm32"))]
    app.add_audio_source::<StreamingAudio>();
    #[cfg(target_arch = "wasm32")]
    cyrus_assets::movie::register(app);
    app.add_message::<MovieRequest>()
        .init_resource::<MoviePlayback>()
        .add_systems(
            Update,
            drive
                .after(super::menu::drive)
                .run_if(in_state(AppState::Menu)),
        )
        .add_systems(
            Update,
            ending.before(drive).run_if(in_state(AppState::Menu)),
        )
        .add_systems(
            Update,
            script_movie.before(drive).run_if(in_state(AppState::Menu)),
        )
        .add_systems(OnExit(AppState::Menu), cleanup);
}

/// `WonGame` index of `WIN.SMK` among the installed MENU.INI movies.
const ENDING_MOVIE: usize = 4;

/// Runs the ending's movie once the book menu exists: effects stop, the CD pauses with the movie
/// (the music owner pauses while a cinematic plays) and `WIN.SMK` plays; the credits scroll follows
/// it and the book follows the credits.
fn ending(
    mut commands: Commands,
    entry: Option<Res<cyrus_game::script::EndingEntry>>,
    menu: Option<Res<MenuState>>,
    mut movies: MessageWriter<MovieRequest>,
    mut sounds: Option<MessageWriter<cyrus_audio::sound::SoundRequest>>,
) {
    if entry.is_none() || menu.is_none() {
        return;
    }
    commands.remove_resource::<cyrus_game::script::EndingEntry>();
    if let Some(sounds) = sounds.as_mut() {
        sounds.write(cyrus_audio::sound::SoundRequest::StopAll);
    }
    commands.insert_resource(super::credits::CreditsPending);
    movies.write(MovieRequest::Menu(ENDING_MOVIE));
}

/// Plays a gameplay script's movie once the book menu exists; the world resumes when it ends.
fn script_movie(
    mut commands: Commands,
    entry: Option<Res<cyrus_game::script::ScriptMovieEntry>>,
    menu: Option<Res<MenuState>>,
    mut movies: MessageWriter<MovieRequest>,
) {
    let (Some(entry), Some(_)) = (entry, menu) else {
        return;
    };
    movies.write(MovieRequest::Script(entry.index, entry.then_load));
    commands.remove_resource::<cyrus_game::script::ScriptMovieEntry>();
}

/// A script movie is over, played or not: the paused world resumes, after requesting the world load
/// that was waiting behind it.
fn resume_script_world(commands: &mut Commands, menu: &mut MenuState, then_load: Option<[i32; 3]>) {
    if let Some(destination) = then_load
        && let Err(error) = cyrus_game::script::request_world(commands, destination)
    {
        menu.unavailable = Some(format!("{error:#}"));
    }
    menu.resume_game = true;
}

/// Decodes on a thread from one-based `frame` on; Smacker frames are deltas, so
/// earlier frames are decoded but not sent.
#[cfg(not(target_arch = "wasm32"))]
fn decode(movie: &Prepared, frame: u32) -> anyhow::Result<Decoder> {
    let source = movie.source.clone();
    let (sender, receiver) = mpsc::sync_channel(2);
    let frames = movie.metadata.frames;
    let name = movie.metadata.definition.name.clone();
    let start =
        Duration::from_micros(u64::from(frame - 1) * u64::from(movie.metadata.frame_micros));
    let has_audio = movie.metadata.has_audio;
    let frame_micros = movie.metadata.frame_micros;
    let format = movie.metadata.codec_format();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<()> {
            let mut packets = Packets::new(source.open()?, 0x80)?;
            format.validate(&packets.codec)?;
            let info = packets.codec.info();
            anyhow::ensure!(
                info.frame_count == frames
                    && (info.microseconds_per_frame - f64::from(frame_micros)).abs() < 0.5,
                "movie changed during preparation"
            );
            let mut audio = if has_audio {
                Some(StreamingAudio::prepare(&source, start, name.clone())?)
            } else {
                None
            };
            for index in 1..=frames {
                anyhow::ensure!(
                    packets
                        .advance()
                        .map_err(|error| anyhow::anyhow!("frame {index}: {error:#}"))?,
                    "missing frame {index}"
                );
                if index >= frame {
                    let pixels = stretch(packets.codec.video_data(), packets.codec.palette());
                    if sender.send(Ok((pixels, audio.take()))).is_err() {
                        break;
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = sender.send(Err(anyhow::anyhow!("movie {name}: {error:#}")));
        }
    });
    Ok(Decoder {
        frames: Mutex::new(receiver),
        audio: None,
    })
}

#[cfg(target_arch = "wasm32")]
fn decode(movie: &Prepared, _: u32) -> anyhow::Result<Decoder> {
    Ok(Decoder {
        frames: Mutex::new(super::movie_stream::BrowserMovie::new(movie)?),
    })
}

/// A 640×240 indexed frame as 640×480 RGBA, filtered vertically like a
/// bilinear texture stretch (sample centres at `(y + 0.5) / 2 − 0.5`).
pub(super) fn stretch(indices: &[u8], palette: &[[u8; 3]; 256]) -> Vec<u8> {
    let row = |y: usize| &indices[y * 640..(y + 1) * 640];
    let mut pixels = Vec::with_capacity(640 * 480 * 4);
    for y in 0..480 {
        let source = ((y as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, 239.0);
        let (top, weight) = (source.floor() as usize, source.fract());
        let (upper, lower) = (row(top), row((top + 1).min(239)));
        for x in 0..640 {
            let (a, b) = (
                palette[usize::from(upper[x])],
                palette[usize::from(lower[x])],
            );
            for c in 0..3 {
                let mixed = f32::from(a[c]) * (1.0 - weight) + f32::from(b[c]) * weight;
                pixels.push(mixed.round() as u8);
            }
            pixels.push(255);
        }
    }
    pixels
}

fn skip_point(movie: &MovieDefinition, frame: u32) -> Option<u32> {
    movie.keys.iter().copied().find(|key| *key > frame)
}

pub(super) fn drive(
    runtime: (Commands, Query<Entity, With<CinematicAudio>>),
    #[cfg(not(target_arch = "wasm32"))] install: Res<GameInstall>,
    #[cfg(target_arch = "wasm32")] assets: Res<AssetServer>,
    #[cfg(target_arch = "wasm32")] mut prepared: ResMut<Assets<Prepared>>,
    mut requests: MessageReader<MovieRequest>,
    state: (
        ResMut<MoviePlayback>,
        ResMut<MenuState>,
        Res<super::LaunchRequest>,
        Option<ResMut<cyrus_game::MovieUnlocks>>,
    ),
    input: (Res<Time<Real>>, Res<ButtonInput<KeyCode>>),
    rendering: (
        ResMut<Assets<Image>>,
        Query<&mut Sprite, With<StartupContent>>,
    ),
    #[cfg(not(target_arch = "wasm32"))] audio: (
        ResMut<Assets<StreamingAudio>>,
        Query<&AudioSink, With<CinematicAudio>>,
    ),
) {
    let (mut commands, audio_players) = runtime;
    #[cfg(not(target_arch = "wasm32"))]
    let (mut sources, sinks) = audio;
    let (mut playback, mut menu, launch, mut unlocks) = state;
    let (mut images, mut sprite) = rendering;
    for request in requests.read() {
        if playback.load.is_some() || playback.playing.is_some() {
            continue;
        }
        if launch.skip_movies && matches!(request, MovieRequest::NewGame) {
            menu.enter_world = true;
            menu.ready = false;
            continue;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let root = install.root.clone();
        let index = match request {
            MovieRequest::Menu(index) | MovieRequest::Script(index, _) => *index,
            MovieRequest::NewGame => 0,
        };
        playback.script = match request {
            MovieRequest::Script(_, then_load) => Some(*then_load),
            _ => None,
        };
        // Playing a movie sets its unlock flag before playback.
        if let Some(unlocks) = unlocks.as_mut() {
            unlocks.unlock(index);
            menu.movie_unlocks = **unlocks;
        }
        playback.new_game = matches!(request, MovieRequest::NewGame);
        // The CD pauses from the request, not the first frame, and resumes when the movie ends.
        commands.spawn(CinematicAudio);
        menu.movie = Some(MovieStatus {
            name: String::new(),
            frame: 0,
            frames: 0,
            loading: true,
        });
        menu.ready = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            playback.load = Some(IoTaskPool::get().spawn(async move { prepare(&root, index) }));
        }
        #[cfg(target_arch = "wasm32")]
        {
            playback.load = Some(assets.load(format!("cache://movies/{index}.movie")));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let loaded = playback.load.as_mut().and_then(check_ready);
    #[cfg(target_arch = "wasm32")]
    let loaded = playback.load.as_ref().and_then(|handle| {
        if let Some(bevy::asset::LoadState::Failed(error)) = assets.get_load_state(handle.id()) {
            Some(Err(anyhow::anyhow!("{error}")))
        } else {
            prepared.remove(handle.id()).map(Ok)
        }
    });
    if let Some(result) = loaded {
        playback.load = None;
        match result.and_then(|movie| {
            let decoder = decode(&movie, 1)?;
            // A failed prefetch only loses the shortcut: the skip decodes it again and reports.
            #[cfg(not(target_arch = "wasm32"))]
            let prefetched = movie
                .metadata
                .definition
                .keys
                .iter()
                .filter_map(|&key| Some((key, decode(&movie, key).ok()?)))
                .collect();
            Ok(Playing {
                movie,
                decoder,
                #[cfg(not(target_arch = "wasm32"))]
                prefetched,
                image: Handle::default(),
                #[cfg(not(target_arch = "wasm32"))]
                audio: None,
                elapsed: 0.0,
                next_frame: 1,
                started: false,
                audio_started: false,
            })
        }) {
            Ok(playing) => {
                menu.movie = Some(MovieStatus {
                    name: playing.movie.metadata.definition.name.clone(),
                    frame: 0,
                    frames: playing.movie.metadata.frames,
                    loading: true,
                });
                playback.playing = Some(playing);
            }
            Err(error) => {
                for entity in &audio_players {
                    commands.entity(entity).despawn();
                }
                playback.new_game = false;
                menu.movie = None;
                menu.unavailable = Some(format!("Movie unavailable: {error:#}"));
                menu.dirty = true;
                if let Some(then_load) = playback.script.take() {
                    resume_script_world(&mut commands, &mut menu, then_load);
                }
            }
        }
    }
    let Some(playing) = playback.playing.as_mut() else {
        return;
    };
    let mut finished = false;
    let mut failure = None;
    let frame_seconds = f64::from(playing.movie.metadata.frame_micros) / 1_000_000.0;
    if playing.started {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(source) = playing
                .audio
                .as_ref()
                .and_then(|handle| sources.get(handle))
            {
                failure = source.error();
                if !sinks.is_empty() {
                    playing.elapsed = source
                        .elapsed()
                        .unwrap_or(playing.elapsed + input.0.delta_secs_f64());
                } else {
                    playing.elapsed += input.0.delta_secs_f64();
                }
            } else {
                playing.elapsed += input.0.delta_secs_f64();
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            playing.elapsed = playing.decoder.frames.get_mut().unwrap().elapsed();
        }
    }
    // The activation key from this update starts playback; it must not also skip it.
    if playing.started && input.1.get_just_pressed().next().is_some() {
        let current = menu.movie.as_ref().map_or(0, |m| m.frame);
        if let Some(frame) = skip_point(&playing.movie.metadata.definition, current) {
            #[cfg(not(target_arch = "wasm32"))]
            let sought = match playing.prefetched.iter().position(|(key, _)| *key == frame) {
                Some(index) => Ok(playing.prefetched.remove(index).1),
                None => decode(&playing.movie, frame),
            }
            .map(|decoder| {
                playing.decoder = decoder;
                playing.audio = None;
            });
            #[cfg(target_arch = "wasm32")]
            let sought = playing.decoder.frames.get_mut().unwrap().seek(frame);
            match sought {
                Ok(()) => {
                    playing.next_frame = frame;
                    playing.elapsed = f64::from(frame - 1) * frame_seconds;
                    playing.started = false;
                    playing.audio_started = false;
                    if let Some(status) = menu.movie.as_mut() {
                        status.loading = true;
                    }
                    // The old stream goes, but the movie stays active so the CD does not resume mid-skip.
                    for entity in &audio_players {
                        commands.entity(entity).despawn();
                    }
                    commands.spawn(CinematicAudio);
                }
                Err(error) => failure = Some(error.to_string()),
            }
        } else {
            finished = true;
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Err(error) = playing
        .decoder
        .frames
        .get_mut()
        .unwrap()
        .pump(playing.elapsed)
    {
        failure = Some(format!("{error:#}"));
    }
    let due = if playing.started {
        ((playing.elapsed / frame_seconds).floor() as u32).saturating_add(1)
    } else {
        playing.next_frame
    };
    let mut pixels = None;
    #[cfg(target_arch = "wasm32")]
    let decode_started = bevy::platform::time::Instant::now();
    while !finished
        && failure.is_none()
        && playing.next_frame <= due.min(playing.movie.metadata.frames)
    {
        #[cfg(target_arch = "wasm32")]
        if decode_started.elapsed() >= BROWSER_DECODE_BUDGET {
            break;
        }
        let result = playing.decoder.next();
        match result {
            Ok(Some(frame)) => {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(audio) = playing.decoder.audio.take() {
                    playing.audio = Some(sources.add(audio));
                }
                let index = playing.next_frame;
                playing.next_frame += 1;
                if let Some(status) = menu.movie.as_mut() {
                    status.frame = index;
                    status.loading = false;
                }
                pixels = Some(frame);
                playing.started = true;
            }
            Err(error) => {
                failure = Some(error.to_string());
                break;
            }
            Ok(None) => break,
        }
    }
    if let Some(mut pixels) = pixels.filter(|_| failure.is_none() && !finished) {
        if menu
            .preferences
            .as_ref()
            .is_some_and(|p| p.values[&505] != 0)
        {
            for y in (1..480).step_by(2) {
                for rgba in pixels[y * 640 * 4..(y + 1) * 640 * 4]
                    .as_chunks_mut::<4>()
                    .0
                {
                    *rgba = [0, 0, 0, 255];
                }
            }
        }
        let frame = menu.movie.as_ref().unwrap().frame;
        let subtitles = menu
            .preferences
            .as_ref()
            .is_some_and(|p| p.values[&504] != 0);
        paint_subtitles(
            &mut pixels,
            &playing.movie.metadata.font,
            &playing.movie.metadata.definition,
            frame,
            subtitles,
        );
        let image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: 640,
                height: 480,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            pixels,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::default(),
        );
        // The default handle names the engine's shared fallback texture.
        if playing.image == Handle::default() {
            playing.image = images.add(image);
        } else if let Some(mut previous) = images.get_mut(&playing.image) {
            *previous = image;
        } else {
            playing.image = images.add(image);
        }
        if let Ok(mut sprite) = sprite.single_mut() {
            sprite.image = playing.image.clone();
        }
        if !playing.audio_started {
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(audio) = &playing.audio {
                commands.spawn((
                    CinematicAudio,
                    AudioPlayer(audio.clone()),
                    PlaybackSettings::DESPAWN,
                ));
            }
            #[cfg(target_arch = "wasm32")]
            playing.decoder.frames.get_mut().unwrap().start();
            playing.audio_started = true;
        }
    }
    if playing.next_frame > playing.movie.metadata.frames
        && playing.elapsed >= f64::from(playing.movie.metadata.frames) * frame_seconds
    {
        finished = true;
    }
    if finished || failure.is_some() {
        for entity in &audio_players {
            commands.entity(entity).despawn();
        }
        menu.enter_world = failure.is_none() && playback.new_game;
        playback.new_game = false;
        playback.playing = None;
        menu.movie = None;
        menu.dirty = true;
        if let Some(error) = failure {
            menu.unavailable = Some(error);
        }
        if let Some(then_load) = playback.script.take() {
            resume_script_world(&mut commands, &mut menu, then_load);
        }
    }
}

fn subtitle_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut search = 35;
    while search + 1 < text.len() {
        if text.as_bytes()[search] == b' ' {
            lines.push(&text[start..search]);
            start = search + 1;
            search += 35;
        }
        search += 1;
    }
    lines.push(&text[start..]);
    lines
}

/// The player's REDSEL copy (value pair 255,180): main alpha 180, shadow alpha 180 × 200 / 256 = 140, offset (2, 2).
const SUBTITLE_ALPHA: [u8; 2] = [180, 140];
const SUBTITLE_SHADOW: i32 = 2;

fn paint_subtitles(
    pixels: &mut [u8],
    font: &cyrus_text::Font,
    movie: &MovieDefinition,
    frame: u32,
    enabled: bool,
) {
    let Some(subtitle) = movie
        .subtitles
        .iter()
        .find(|s| s.start <= frame && frame < s.stop && (enabled || s.always))
    else {
        return;
    };
    let lines = subtitle_lines(&subtitle.text);
    for (row, line) in lines.iter().enumerate() {
        let x = 320 - font.width(line) / 2;
        let y = 470 - (lines.len() - row) as i32 * i32::from(font.line_height);
        font.draw_shadowed_by(pixels, 640, line, [x, y], SUBTITLE_ALPHA, SUBTITLE_SHADOW);
    }
}

fn cleanup(
    mut commands: Commands,
    mut playback: ResMut<MoviePlayback>,
    audio: Query<Entity, With<CinematicAudio>>,
) {
    *playback = MoviePlayback::default();
    for entity in &audio {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use cyrus_assets::movie::MovieSubtitle;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn movie_frames_publish_to_one_owned_image_without_changing_the_fallback() {
        let mut world = World::new();
        world.insert_resource(GameInstall {
            root: Default::default(),
        });
        world.init_resource::<Messages<MovieRequest>>();
        world.init_resource::<crate::LaunchRequest>();
        world.init_resource::<Time<Real>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StreamingAudio>>();
        let fallback = Handle::<Image>::default();
        let sentinel = Image::default();
        world
            .resource_mut::<Assets<Image>>()
            .insert(&fallback, sentinel.clone())
            .unwrap();
        let sprite = world.spawn((StartupContent, Sprite::default())).id();
        let mut menu = MenuState::default();
        menu.movie = Some(MovieStatus {
            name: "synthetic.smk".into(),
            frame: 0,
            frames: 3,
            loading: true,
        });
        world.insert_resource(menu);
        let (sender, receiver) = mpsc::channel();
        world.insert_resource(MoviePlayback {
            playing: Some(Playing {
                movie: Prepared {
                    source: cyrus_assets::movie::MovieInput::File(Default::default()),
                    metadata: cyrus_assets::movie::MovieMetadata {
                        definition: MovieDefinition {
                            name: "synthetic.smk".into(),
                            keys: Vec::new(),
                            subtitles: Vec::new(),
                        },
                        frames: 3,
                        frame_micros: 1000,
                        has_audio: false,
                        font: default(),
                    },
                },
                decoder: Decoder {
                    frames: Mutex::new(receiver),
                    audio: None,
                },
                prefetched: Vec::new(),
                image: default(),
                audio: None,
                elapsed: 0.0,
                next_frame: 1,
                started: false,
                audio_started: false,
            }),
            ..default()
        });
        let mut owned = None;
        for (index, rgba) in [[12, 34, 56, 255], [78, 90, 123, 255]]
            .into_iter()
            .enumerate()
        {
            let pixels = rgba.repeat(640 * 480);
            sender.send(Ok((pixels.clone(), None))).unwrap();
            world
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_millis(index as u64));
            world.run_system_cached(drive).unwrap();
            let playing = world.resource::<MoviePlayback>().playing.as_ref().unwrap();
            assert_eq!(playing.next_frame, index as u32 + 2);
            let images = world.resource::<Assets<Image>>();
            let unchanged = images.get(&fallback).unwrap();
            assert!(
                unchanged.data == sentinel.data,
                "shared fallback pixels changed"
            );
            assert_eq!(unchanged.texture_descriptor, sentinel.texture_descriptor);
            assert_ne!(playing.image, fallback);
            assert!(playing.image.is_strong());
            assert_eq!(world.get::<Sprite>(sprite).unwrap().image, playing.image);
            if let Some(owned) = &owned {
                assert_eq!(&playing.image, owned, "later frames must retain the image");
            } else {
                owned = Some(playing.image.clone());
            }
            let published = images.get(&playing.image).unwrap();
            assert_eq!(published.data.as_ref().unwrap(), &pixels);
            assert_eq!(published.width(), 640);
            assert_eq!(published.height(), 480);
            assert_eq!(images.len(), 2, "each movie owns only one image");
        }
    }

    #[test]
    fn ending_plays_movie_four_once_after_the_book_exists_and_stops_effects() {
        use bevy::ecs::system::RunSystemOnce;
        use cyrus_audio::sound::SoundRequest;
        let mut world = World::new();
        world.init_resource::<Messages<MovieRequest>>();
        world.init_resource::<Messages<SoundRequest>>();
        world.insert_resource(cyrus_game::script::EndingEntry);
        world.run_system_once(ending).unwrap();
        assert!(
            world.contains_resource::<cyrus_game::script::EndingEntry>(),
            "the ending waits for the book menu"
        );
        assert!(
            world
                .resource_mut::<Messages<MovieRequest>>()
                .drain()
                .next()
                .is_none()
        );
        world.init_resource::<MenuState>();
        world.run_system_once(ending).unwrap();
        world.run_system_once(ending).unwrap();
        assert!(!world.contains_resource::<cyrus_game::script::EndingEntry>());
        let movies: Vec<_> = world
            .resource_mut::<Messages<MovieRequest>>()
            .drain()
            .collect();
        assert!(matches!(movies.as_slice(), [MovieRequest::Menu(4)]));
        let sounds: Vec<_> = world
            .resource_mut::<Messages<SoundRequest>>()
            .drain()
            .collect();
        assert_eq!(sounds, [SoundRequest::StopAll]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn skip_movies_enters_the_world_without_loading_the_introduction() {
        let mut world = World::new();
        world.insert_resource(GameInstall {
            root: Default::default(),
        });
        world.init_resource::<Messages<MovieRequest>>();
        world.insert_resource(crate::LaunchRequest::new(false, None, true));
        world.init_resource::<Time<Real>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StreamingAudio>>();
        world.init_resource::<MenuState>();
        world.init_resource::<MoviePlayback>();
        world
            .resource_mut::<Messages<MovieRequest>>()
            .write(MovieRequest::NewGame);
        world.run_system_cached(drive).unwrap();
        let menu = world.resource::<MenuState>();
        assert!(menu.enter_world && menu.movie.is_none());
        assert!(world.resource::<MoviePlayback>().load.is_none());
    }

    #[test]
    fn script_movie_entry_is_taken_once_the_book_exists() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Messages<MovieRequest>>();
        world.insert_resource(cyrus_game::script::ScriptMovieEntry {
            index: 7,
            then_load: Some([3, 0, 0]),
        });
        world.run_system_once(script_movie).unwrap();
        assert!(world.contains_resource::<cyrus_game::script::ScriptMovieEntry>());
        world.init_resource::<MenuState>();
        world.run_system_once(script_movie).unwrap();
        assert!(!world.contains_resource::<cyrus_game::script::ScriptMovieEntry>());
        let requests: Vec<_> = world
            .resource_mut::<Messages<MovieRequest>>()
            .drain()
            .collect();
        assert!(matches!(
            requests.as_slice(),
            [MovieRequest::Script(7, Some([3, 0, 0]))]
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn requesting_a_movie_sets_its_unlock_flag_before_playback() {
        bevy::tasks::IoTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let mut world = World::new();
        world.insert_resource(GameInstall {
            root: Default::default(),
        });
        world.init_resource::<Messages<MovieRequest>>();
        world.init_resource::<crate::LaunchRequest>();
        world.init_resource::<Time<Real>>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<Assets<Image>>();
        world.init_resource::<Assets<StreamingAudio>>();
        world.init_resource::<MenuState>();
        world.init_resource::<MoviePlayback>();
        world.init_resource::<cyrus_game::MovieUnlocks>();
        world
            .resource_mut::<Messages<MovieRequest>>()
            .write(MovieRequest::Menu(3));
        world.run_system_cached(drive).unwrap();
        assert!(world.resource::<cyrus_game::MovieUnlocks>().is_unlocked(3));
        assert!(world.resource::<MenuState>().movie_unlocks.is_unlocked(3));
        assert!(!world.resource::<cyrus_game::MovieUnlocks>().is_unlocked(2));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn movie_completion_enters_new_game_only_after_success() {
        for (new_game, complete, fail, script) in [
            (true, false, false, None),
            (true, true, false, None),
            (false, true, false, None),
            (true, false, true, None),
            (false, true, false, Some(Some([7, 1, 0]))),
            (false, false, false, Some(None)),
            (false, false, true, Some(None)),
        ] {
            let mut world = World::new();
            world.insert_resource(GameInstall {
                root: Default::default(),
            });
            world.init_resource::<Messages<MovieRequest>>();
            world.init_resource::<crate::LaunchRequest>();
            world.init_resource::<Time<Real>>();
            world.init_resource::<ButtonInput<KeyCode>>();
            world.init_resource::<Assets<Image>>();
            world.init_resource::<Assets<StreamingAudio>>();
            let mut menu = MenuState::default();
            menu.movie = Some(MovieStatus {
                name: "synthetic.smk".into(),
                frame: u32::from(complete),
                frames: 1,
                loading: !complete,
            });
            world.insert_resource(menu);
            let (sender, receiver) = mpsc::channel();
            let _sender = (!fail).then_some(sender);
            world.insert_resource(MoviePlayback {
                new_game,
                script,
                load: None,
                playing: Some(Playing {
                    movie: Prepared {
                        source: cyrus_assets::movie::MovieInput::File(Default::default()),
                        metadata: cyrus_assets::movie::MovieMetadata {
                            definition: MovieDefinition {
                                name: "synthetic.smk".into(),
                                keys: Vec::new(),
                                subtitles: Vec::new(),
                            },
                            frames: 1,
                            frame_micros: 1000,
                            has_audio: false,
                            font: default(),
                        },
                    },
                    decoder: Decoder {
                        frames: Mutex::new(receiver),
                        audio: None,
                    },
                    prefetched: Vec::new(),
                    image: default(),
                    audio: None,
                    elapsed: if complete { 0.001 } else { 0.0 },
                    next_frame: if complete { 2 } else { 1 },
                    started: complete,
                    audio_started: false,
                }),
            });
            world.run_system_cached(drive).unwrap();
            let menu = world.resource::<MenuState>();
            assert_eq!(menu.enter_world, new_game && complete && !fail);
            // A script movie resumes its world whether it ended or failed, never before.
            assert_eq!(menu.resume_game, script.is_some() && (complete || fail));
            assert_eq!(
                world.contains_resource::<cyrus_world::lifecycle::SceneRequest>(),
                script == Some(Some([7, 1, 0])) && complete
            );
            assert_eq!(menu.movie.is_some(), !complete && !fail);
            assert_eq!(menu.unavailable.is_some(), fail);
            if fail {
                assert!(menu.unavailable.as_ref().unwrap().contains("disconnected"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn installed_movies_decode_in_process() {
        use {rgpre::import::menu_movies::parse_menu_movies, std::path::Path};
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard"));
        let install = match GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => return eprintln!("skipped: no install ({error})"),
        };
        let ini = std::fs::read_to_string(install.root.join("MENU.INI")).unwrap();
        let definitions = parse_menu_movies(&ini).unwrap();
        assert!(!definitions.is_empty(), "no installed movie definitions");
        for index in 0..definitions.len() {
            let movie = prepare(&install.root, index)
                .unwrap_or_else(|error| panic!("movie {index}: {error:#}"));
            let last = movie.metadata.frames;
            let started = std::time::Instant::now();
            let frame = decode(&movie, last)
                .unwrap()
                .frames
                .into_inner()
                .unwrap()
                .recv();
            eprintln!(
                "{}: {last} frames, last frame decoded in {:?}",
                movie.metadata.definition.name,
                started.elapsed()
            );
            let (pixels, audio) = frame.unwrap().unwrap();
            assert_eq!(pixels.len(), 640 * 480 * 4);
            assert_eq!(audio.is_some(), movie.metadata.has_audio);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn installed_movie_starts_from_only_its_first_packet() {
        use std::io::Read;
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        let install = match GameInstall::open(&root) {
            Ok(install) => install,
            Err(error) => return eprintln!("skipped: no install ({error})"),
        };
        let started = std::time::Instant::now();
        let mut movie = prepare(&install.root, 0).unwrap();
        let (pixels, audio) = decode(&movie, 1)
            .unwrap()
            .frames
            .into_inner()
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        eprintln!(
            "installed INTRO request to first picture and audio block: {:?}",
            started.elapsed()
        );
        assert_eq!(pixels.len(), 640 * 480 * 4);
        assert!(audio.is_some());
        drop(audio);
        let mut reader = movie.source.open().unwrap();
        let mut bytes = vec![0; 104];
        reader.read_exact(&mut bytes).unwrap();
        let size = cyrus_media::codec::Smk::header_size(&bytes)
            .unwrap()
            .unwrap();
        bytes.resize(size, 0);
        reader.read_exact(&mut bytes[104..]).unwrap();
        let codec = cyrus_media::codec::Smk::open_stream(&bytes).unwrap();
        bytes.resize(size + codec.packet_size().unwrap(), 0);
        reader.read_exact(&mut bytes[size..]).unwrap();
        drop(reader);
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../game-config/movie-tests");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("first-packet-{}.smk", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        movie.source = cyrus_assets::movie::MovieInput::File(path.clone());
        let (first, audio) = decode(&movie, 1)
            .unwrap()
            .frames
            .into_inner()
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();
        assert_eq!(first, pixels, "first picture requires later packets");
        assert!(audio.is_some());
        drop(audio);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn subtitle_breaks_at_first_space_after_column_35() {
        assert_eq!(
            subtitle_lines("01234567890123456789012345678901234 end, Cyrus."),
            ["01234567890123456789012345678901234", "end, Cyrus."]
        );
        assert_eq!(
            subtitle_lines("0123456789012345678901234567890123456789 end"),
            ["0123456789012345678901234567890123456789", "end"]
        );
    }
    #[test]
    fn skip_points_are_forward_only_and_end_after_the_last() {
        let movie = MovieDefinition {
            name: "test".into(),
            keys: vec![10, 20],
            subtitles: vec![],
        };
        assert_eq!(skip_point(&movie, 1), Some(10));
        assert_eq!(skip_point(&movie, 10), Some(20));
        assert_eq!(skip_point(&movie, 20), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn subtitles_obey_frame_bounds_and_always_flag() {
        let font = rgpre::import::fnt::FntFile {
            chunk_order: vec![],
            header: rgpre::import::fnt::FntHeader {
                description_raw: [0; 32],
                description_text: String::new(),
                unknown_24: 0,
                has_rdat: 0,
                unknown_28: 0,
                unknown_2a: 0,
                unknown_2c: 0,
                max_width: 1,
                line_height: 2,
                character_start: 65,
                character_count: 1,
                unknown_36: 0,
                unknown_38: 0,
                has_palette: 0,
            },
            palette: rgpre::import::fnt::FntPalette {
                tag: *b"FPAL",
                colors: vec![[63, 62, 10]; 8],
            },
            glyphs: vec![rgpre::import::fnt::FntGlyph {
                enabled: 1,
                offset_left: 0,
                offset_top: 0,
                width: 1,
                height: 1,
                pixels: vec![7],
            }],
            rdat: None,
            trailing_padding: vec![],
        };
        let mut movie = MovieDefinition {
            name: "test".into(),
            keys: vec![],
            subtitles: vec![MovieSubtitle {
                always: false,
                start: 10,
                stop: 20,
                text: "A".into(),
            }],
        };
        let draw = |movie: &MovieDefinition, frame, enabled| {
            let mut pixels = vec![0; 640 * 480 * 4];
            paint_subtitles(
                &mut pixels,
                &cyrus_text::Font::from(&font),
                movie,
                frame,
                enabled,
            );
            pixels
        };
        let offset = (468 * 640 + 319) * 4;
        assert_eq!(
            &draw(&movie, 10, true)[offset..offset + 4],
            &[252, 248, 40, 180]
        );
        assert!(draw(&movie, 9, true).iter().all(|v| *v == 0));
        assert!(draw(&movie, 20, true).iter().all(|v| *v == 0));
        assert!(draw(&movie, 10, false).iter().all(|v| *v == 0));
        movie.subtitles[0].always = true;
        assert_eq!(
            &draw(&movie, 10, false)[offset..offset + 4],
            &[252, 248, 40, 180]
        );

        let mut font = font;
        font.header.character_start = 31;
        font.header.character_count = 35;
        font.header.max_width = 3;
        font.glyphs.resize(35, font.glyphs[0].clone());
        font.glyphs[1].pixels[0] = 0;
        font.glyphs[2].width = 3;
        font.glyphs[2].pixels = vec![0; 3];
        movie.subtitles[0].text = " A".into();
        let mut pixels = vec![0; 640 * 480 * 4];
        paint_subtitles(
            &mut pixels,
            &cyrus_text::Font::from(&font),
            &movie,
            10,
            true,
        );
        let offset = (468 * 640 + 322) * 4;
        assert_eq!(&pixels[offset..offset + 4], &[252, 248, 40, 180]);
    }
}
