//! Native installed Smacker playback. Rules live in preservation's engine/movies.md.
use super::{
    StartupContent,
    menu::{MenuState, MovieRequest, MovieStatus},
};
use crate::{AppState, install::GameInstall};
use anyhow::{Context, ensure};
use bevy::{
    audio::{AudioPlayer, AudioSource, PlaybackSettings},
    prelude::{
        App, Assets, ButtonInput, Commands, Component, Entity, Handle, Image, IntoScheduleConfigs,
        KeyCode, MessageReader, OnExit, Query, Real, Res, ResMut, Resource, Sprite, Time, Update,
        With, in_state,
    },
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use rgpre::import::{
    cdrom::CdImage,
    fnt::{FntFile, parse_fnt},
    menu_movies::{MovieDefinition, parse_menu_movies},
    sfx::AudioType,
    smacker::SmackerHeader,
};
use std::{
    fs::File,
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

#[derive(Resource, Default)]
struct MoviePlayback {
    load: Option<Task<anyhow::Result<Prepared>>>,
    playing: Option<Playing>,
}
struct Prepared {
    /// The whole Smacker file, decoded in-process.
    data: Arc<[u8]>,
    definition: MovieDefinition,
    header: SmackerHeader,
    font: FntFile,
    line_height: u16,
    font_color: [u8; 3],
    audio: Option<AudioSource>,
}
struct Playing {
    movie: Prepared,
    decoder: Decoder,
    image: Handle<Image>,
    audio: Option<Handle<AudioSource>>,
    elapsed: f64,
    next_frame: u32,
    started: bool,
    audio_started: bool,
}
/// The audio of the playing movie; CD music pauses while it exists.
#[derive(Component)]
pub struct MovieAudio;
/// Dropping it disconnects the channel, which stops the decoding thread.
struct Decoder {
    frames: Mutex<mpsc::Receiver<anyhow::Result<Vec<u8>>>>,
}

pub(super) fn register(app: &mut App) {
    app.add_message::<MovieRequest>()
        .init_resource::<MoviePlayback>()
        .add_systems(
            Update,
            drive
                .after(super::menu::drive)
                .run_if(in_state(AppState::Menu)),
        )
        .add_systems(OnExit(AppState::Menu), cleanup);
}

fn prepare(root: &Path, index: usize) -> anyhow::Result<Prepared> {
    let ini = std::fs::read_to_string(root.join("MENU.INI")).context("movie: reading MENU.INI")?;
    let definition = parse_menu_movies(&ini)
        .map_err(anyhow::Error::msg)?
        .into_iter()
        .nth(index)
        .with_context(|| format!("movie: MENU.INI definition {index} missing"))?;
    let system =
        std::fs::read_to_string(root.join("SYSTEM.INI")).context("movie: reading SYSTEM.INI")?;
    let name = &definition.name;
    let drive = crate::sound::ini_value(&system, "system", "animation_drive").unwrap_or("");
    let data = if drive.contains(':') {
        let prefix = drive.as_bytes();
        ensure!(
            prefix.len() >= 3
                && prefix[0].is_ascii_alphabetic()
                && prefix[1] == b':'
                && prefix[2..].iter().all(|b| matches!(*b, b'/' | b'\\')),
            "movie {name}: animation_drive {drive:?}: only the CD root is supported"
        );
        let disc = root
            .parent()
            .context("movie: install has no parent")?
            .join("game.gog");
        let mut image = CdImage::open(std::io::BufReader::new(
            File::open(&disc).with_context(|| format!("movie {name}: {}", disc.display()))?,
        ))?;
        let mut data = Vec::new();
        image
            .copy_file(name, &mut data)
            .with_context(|| format!("movie {name}: CD extraction"))?;
        data
    } else {
        let mut directory = root.to_path_buf();
        for part in drive.split(['/', '\\']).filter(|p| !p.is_empty()) {
            ensure!(
                part != "." && part != "..",
                "movie {name}: invalid animation_drive"
            );
            directory = crate::scene::find_file(&directory, part)
                .with_context(|| format!("movie {name}: animation_drive {drive:?} missing"))?;
        }
        let path = crate::scene::find_file(&directory, name)
            .with_context(|| format!("movie {name}: not found in {}", directory.display()))?;
        std::fs::read(&path).with_context(|| format!("movie {name}: reading {}", path.display()))?
    };
    let header = SmackerHeader::parse(&data)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("movie {name}: Smacker header"))?;
    ensure!(
        header.frames < u32::MAX,
        "movie {name}: frame count exceeds playback counter capacity"
    );
    ensure!(
        header.width == 640 && header.height == 240 && header.double_height,
        "movie {name}: unsupported viewport {}x{}",
        header.width,
        header.height
    );
    ensure!(
        definition.keys.iter().all(|key| *key <= header.frames),
        "movie {name}: skip point exceeds frame count"
    );
    let font_name = crate::sound::ini_value(&system, "system", "gui_low_font")
        .context("movie: SYSTEM.INI gui_low_font missing")?;
    let mut font_path = root.to_path_buf();
    for part in font_name.split(['/', '\\']) {
        ensure!(
            !part.is_empty() && part != "." && part != "..",
            "movie: invalid gui_low_font path"
        );
        font_path = crate::scene::find_file(&font_path, part)
            .with_context(|| format!("movie: gui_low_font {font_name} missing"))?;
    }
    let font = parse_fnt(&std::fs::read(&font_path)?).context("movie: gui_low_font")?;
    let line_height = usize::from(b'W')
        .checked_sub(font.header.character_start.into())
        .and_then(|i| font.glyphs.get(i))
        .filter(|g| g.enabled != 0 && g.width > 0)
        .context("movie: gui_low_font has no W glyph for line spacing")?
        .width;
    let palette =
        rgpre::import::palette::Palette::parse(&std::fs::read(root.join("fxart/REDGUARD.COL"))?)
            .context("movie: REDGUARD.COL")?;
    let font_color = palette.colors[161];
    let audio = if header.has_audio {
        Some(decode_audio(&data).with_context(|| format!("movie {name}: audio decode"))?)
    } else {
        None
    };
    Ok(Prepared {
        data: data.into(),
        definition,
        header,
        font,
        line_height,
        font_color,
        audio,
    })
}

/// The first audio track as WAV.
fn decode_audio(data: &[u8]) -> anyhow::Result<AudioSource> {
    let mut smk = smk::Smk::open_memory(data)?;
    let info = smk.info_audio();
    ensure!(info.track_mask & 1 != 0, "no first audio track");
    let audio_type = match (info.channels[0], info.bitdepth[0]) {
        (1, 8) => AudioType::Mono8,
        (1, 16) => AudioType::Mono16,
        (2, 8) => AudioType::Stereo8,
        (2, 16) => AudioType::Stereo16,
        other => anyhow::bail!("unsupported audio layout {other:?}"),
    };
    smk.enable_all(1);
    let mut pcm = Vec::new();
    let mut status = smk.first_frame()?;
    loop {
        pcm.extend_from_slice(smk.audio_data(0).unwrap_or_default());
        if status != smk::FrameStatus::More {
            break;
        }
        status = smk.next_frame()?;
    }
    let bytes =
        crate::sound::pcm_wav(audio_type, info.rate[0], &pcm).map_err(anyhow::Error::msg)?;
    Ok(AudioSource {
        bytes: bytes.into(),
    })
}

/// Decodes on a thread from one-based `frame` on; Smacker frames are deltas, so
/// earlier frames are decoded but not sent.
fn decode(movie: &Prepared, frame: u32) -> anyhow::Result<Decoder> {
    let mut smk = smk::Smk::open_memory(&movie.data)?;
    smk.enable_all(0x80);
    let (sender, receiver) = mpsc::sync_channel(2);
    let frames = movie.header.frames;
    let name = movie.definition.name.clone();
    std::thread::spawn(move || {
        let mut status = smk.first_frame();
        for index in 1..=frames {
            let failed = status.is_err();
            if failed || index >= frame {
                let result = status
                    .as_ref()
                    .map_err(|e| anyhow::anyhow!("movie {name}: frame {index}: {e}"))
                    .map(|_| stretch(smk.video_data(), smk.palette()));
                if sender.send(result).is_err() || failed {
                    break;
                }
            }
            if failed || index == frames {
                break;
            }
            status = smk.next_frame();
        }
    });
    Ok(Decoder {
        frames: Mutex::new(receiver),
    })
}

/// A 640×240 indexed frame as 640×480 RGBA, filtered vertically like a
/// bilinear texture stretch (sample centres at `(y + 0.5) / 2 − 0.5`).
fn stretch(indices: &[u8], palette: &[[u8; 3]; 256]) -> Vec<u8> {
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

fn drive(
    runtime: (Commands, Query<Entity, With<MovieAudio>>),
    install: Res<GameInstall>,
    mut requests: MessageReader<MovieRequest>,
    state: (ResMut<MoviePlayback>, ResMut<MenuState>),
    input: (Res<Time<Real>>, Res<ButtonInput<KeyCode>>),
    rendering: (
        ResMut<Assets<Image>>,
        Query<&mut Sprite, With<StartupContent>>,
    ),
    mut sources: Option<ResMut<Assets<AudioSource>>>,
) {
    let (mut commands, audio_players) = runtime;
    let (mut playback, mut menu) = state;
    let (mut images, mut sprite) = rendering;
    for request in requests.read() {
        if playback.load.is_some() || playback.playing.is_some() {
            continue;
        }
        let root = install.root.clone();
        let index = request.0;
        menu.movie = Some(MovieStatus {
            name: String::new(),
            frame: 0,
            frames: 0,
            loading: true,
        });
        menu.ready = false;
        playback.load = Some(IoTaskPool::get().spawn(async move { prepare(&root, index) }));
    }
    if let Some(load) = playback.load.as_mut()
        && let Some(result) = check_ready(load)
    {
        playback.load = None;
        match result.and_then(|movie| {
            let decoder = decode(&movie, 1)?;
            Ok(Playing {
                movie,
                decoder,
                image: Handle::default(),
                audio: None,
                elapsed: 0.0,
                next_frame: 1,
                started: false,
                audio_started: false,
            })
        }) {
            Ok(mut playing) => {
                if let Some(source) = playing.movie.audio.take()
                    && let Some(sources) = sources.as_mut()
                {
                    playing.audio = Some(sources.add(source));
                }
                menu.movie = Some(MovieStatus {
                    name: playing.movie.definition.name.clone(),
                    frame: 0,
                    frames: playing.movie.header.frames,
                    loading: true,
                });
                playback.playing = Some(playing);
            }
            Err(error) => {
                menu.movie = None;
                menu.unavailable = Some(format!("Movie unavailable: {error:#}"));
                menu.dirty = true;
            }
        }
    }
    let Some(playing) = playback.playing.as_mut() else {
        return;
    };
    let mut finished = false;
    let mut failure = None;
    let frame_seconds = f64::from(playing.movie.header.frame_micros) / 1_000_000.0;
    if playing.started {
        playing.elapsed += input.0.delta_secs_f64();
    }
    // The activation key from this update starts playback; it must not also skip it.
    if playing.started && input.1.get_just_pressed().next().is_some() {
        let current = menu.movie.as_ref().map_or(0, |m| m.frame);
        if let Some(frame) = skip_point(&playing.movie.definition, current) {
            match decode(&playing.movie, frame) {
                Ok(decoder) => {
                    playing.decoder = decoder;
                    playing.next_frame = frame;
                    playing.elapsed = f64::from(frame - 1) * frame_seconds;
                    playing.started = false;
                    playing.audio_started = false;
                    if let Some(status) = menu.movie.as_mut() {
                        status.loading = true;
                    }
                    for entity in &audio_players {
                        commands.entity(entity).despawn();
                    }
                }
                Err(error) => failure = Some(error.to_string()),
            }
        } else {
            finished = true;
        }
    }
    let due = if playing.started {
        ((playing.elapsed / frame_seconds).floor() as u32).saturating_add(1)
    } else {
        playing.next_frame
    };
    let mut pixels = None;
    while !finished && playing.next_frame <= due.min(playing.movie.header.frames) {
        let result = playing.decoder.frames.lock().unwrap().try_recv();
        match result {
            Ok(Ok(frame)) => {
                let index = playing.next_frame;
                playing.next_frame += 1;
                if let Some(status) = menu.movie.as_mut() {
                    status.frame = index;
                    status.loading = false;
                }
                pixels = Some(frame);
                playing.started = true;
            }
            Ok(Err(error)) => {
                failure = Some(error.to_string());
                break;
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                failure = Some("Movie decoder disconnected before completion".into());
                break;
            }
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
            &playing.movie.font,
            playing.movie.line_height,
            playing.movie.font_color,
            &playing.movie.definition,
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
        if let Some(mut previous) = images.get_mut(&playing.image) {
            *previous = image;
        } else {
            playing.image = images.add(image);
        }
        if let Ok(mut sprite) = sprite.single_mut() {
            sprite.image = playing.image.clone();
        }
        if !playing.audio_started {
            if let Some(audio) = &playing.audio {
                commands.spawn((
                    MovieAudio,
                    AudioPlayer(audio.clone()),
                    PlaybackSettings::DESPAWN
                        .with_start_position(Duration::from_secs_f64(playing.elapsed)),
                ));
            }
            playing.audio_started = true;
        }
    }
    if playing.next_frame > playing.movie.header.frames
        && playing.elapsed >= f64::from(playing.movie.header.frames) * frame_seconds
    {
        finished = true;
    }
    if finished || failure.is_some() {
        for entity in &audio_players {
            commands.entity(entity).despawn();
        }
        playback.playing = None;
        menu.movie = None;
        menu.dirty = true;
        if let Some(error) = failure {
            menu.unavailable = Some(error);
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

fn paint_subtitles(
    pixels: &mut [u8],
    font: &FntFile,
    line_height: u16,
    color: [u8; 3],
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
    let glyph = |ch: u8| {
        usize::from(ch)
            .checked_sub(font.header.character_start.into())
            .and_then(|i| font.glyphs.get(i))
            .filter(|g| g.enabled != 0)
    };
    for (row, line) in lines.iter().enumerate() {
        let advance = |ch| {
            glyph(ch).map_or(0, |g| {
                i32::from(g.width)
                    + 1
                    + if ch == b' ' {
                        font.glyphs
                            .get(usize::from(ch) - usize::from(font.header.character_start) + 1)
                            .map_or(0, |g| i32::from(g.width))
                    } else {
                        0
                    }
            })
        };
        let width: i32 = line.bytes().map(advance).sum();
        let mut x = 320 - width / 2;
        let y = 470 - (lines.len() - row) as i32 * i32::from(line_height);
        for ch in line.bytes() {
            let Some(g) = glyph(ch) else {
                continue;
            };
            for gy in 0..g.height {
                for gx in 0..g.width {
                    let value = g.pixels[usize::from(gy) * usize::from(g.width) + usize::from(gx)];
                    let px = x + i32::from(g.offset_left) + i32::from(gx);
                    let py = y + i32::from(g.offset_top) + i32::from(gy);
                    if value != 0 && (0..640).contains(&px) && (0..480).contains(&py) {
                        let offset = (py as usize * 640 + px as usize) * 4;
                        pixels[offset..offset + 4]
                            .copy_from_slice(&[color[0], color[1], color[2], 255]);
                    }
                }
            }
            x += advance(ch);
        }
    }
}

fn cleanup(
    mut commands: Commands,
    mut playback: ResMut<MoviePlayback>,
    audio: Query<Entity, With<MovieAudio>>,
) {
    *playback = MoviePlayback::default();
    for entity in &audio {
        commands.entity(entity).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_movies_decode_in_process() {
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
            let last = movie.header.frames;
            let started = std::time::Instant::now();
            let frame = decode(&movie, last)
                .unwrap()
                .frames
                .into_inner()
                .unwrap()
                .recv();
            eprintln!(
                "{}: {last} frames, last frame decoded in {:?}",
                movie.definition.name,
                started.elapsed()
            );
            assert_eq!(frame.unwrap().unwrap().len(), 640 * 480 * 4);
            assert_eq!(movie.audio.is_some(), movie.header.has_audio);
        }
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

    #[test]
    fn subtitles_obey_frame_bounds_and_always_flag() {
        let font = FntFile {
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
                colors: vec![],
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
            subtitles: vec![rgpre::import::menu_movies::MovieSubtitle {
                always: false,
                color: [255, 0, 0],
                start: 10,
                stop: 20,
                text: "A".into(),
            }],
        };
        let draw = |movie: &MovieDefinition, frame, enabled| {
            let mut pixels = vec![0; 640 * 480 * 4];
            paint_subtitles(
                &mut pixels,
                &font,
                2,
                [240, 240, 240],
                movie,
                frame,
                enabled,
            );
            pixels
        };
        let offset = (468 * 640 + 319) * 4;
        assert_eq!(
            &draw(&movie, 10, true)[offset..offset + 4],
            &[240, 240, 240, 255]
        );
        assert!(draw(&movie, 9, true).iter().all(|v| *v == 0));
        assert!(draw(&movie, 20, true).iter().all(|v| *v == 0));
        assert!(draw(&movie, 10, false).iter().all(|v| *v == 0));
        movie.subtitles[0].always = true;
        assert_eq!(
            &draw(&movie, 10, false)[offset..offset + 4],
            &[240, 240, 240, 255]
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
        paint_subtitles(&mut pixels, &font, 2, [240; 3], &movie, 10, true);
        let offset = (468 * 640 + 322) * 4;
        assert_eq!(&pixels[offset..offset + 4], &[240, 240, 240, 255]);
    }
}
