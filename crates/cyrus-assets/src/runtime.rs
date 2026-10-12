//! Shared runtime catalog and preview decoding from read-only original files.
use crate::source::{AssetFile, ReadAssets};
use crate::{CATALOG_VERSION, Catalog, Category, Content, Entry, Palette, Preview, Source};
use anyhow::{Context, Result};
use rgpre::import::{FileType, palette, world_ini};
use std::{cell::RefCell, collections::BTreeMap, path::Path, sync::Arc};

mod definitions;
pub mod geometry;
mod prepare;

/// A preview and generated media. These bytes are runtime results, never build inputs.
pub struct Prepared {
    pub preview: Preview,
    pub files: BTreeMap<String, Vec<u8>>,
}

pub struct Library<R> {
    pub source: R,
    pub catalog: Catalog,
    input: RefCell<Option<(String, Arc<prepare::Input>)>>,
    indexed: std::collections::HashSet<String>,
    model_palettes: RefCell<Option<BTreeMap<String, Vec<String>>>>,
    outputs: RefCell<BTreeMap<String, Vec<u8>>>,
}
impl<R: ReadAssets> Library<R> {
    pub fn open(source: R, progress: impl Fn(usize, usize, &str)) -> Result<Self> {
        source.manifest().validate()?;
        let mut hash = blake3::Hasher::new();
        hash.update(&CATALOG_VERSION.to_le_bytes());
        hash.update(&serde_json::to_vec(source.manifest())?);
        let mut catalog = Catalog {
            version: CATALOG_VERSION,
            generation: hash.finalize().to_hex().to_string(),
            entries: Vec::new(),
            palettes: Vec::new(),
        };
        let files = &source.manifest().files;
        for (index, file) in files.iter().enumerate() {
            progress(index, files.len(), &format!("Listing {}", file.path));
            if file.path.starts_with("disc/")
                || file.path.starts_with("splash/")
                || Path::new(&file.path).extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("dll") || extension.eq_ignore_ascii_case("exe")
                })
            {
                continue;
            }
            let mut parent = entry(
                &file.path,
                file.bytes,
                Category::Other,
                Source::File,
                &file.path,
            );
            if let Some(category) = archive_category(&file.path) {
                parent = entry(
                    &file.path,
                    file.bytes,
                    category,
                    Source::Archive,
                    &file.path,
                );
            } else if let Err(error) =
                discover(&source, Path::new(&file.path), &mut parent, &mut catalog)
            {
                parent.error = Some(format!("{error:#}"));
            }
            if parent.category != Category::Other {
                catalog.entries.push(parent);
            }
        }
        if let Some(file) = source.manifest().find("disc/game.ins") {
            let cue = source.text(&file.path)?;
            for line in cue.lines() {
                let words: Vec<_> = line.split_whitespace().collect();
                if words.len() == 3
                    && words[0].eq_ignore_ascii_case("TRACK")
                    && words[2].eq_ignore_ascii_case("AUDIO")
                {
                    let track = words[1].parse()?;
                    catalog.entries.push(entry(
                        &file.path,
                        file.bytes,
                        Category::Audio,
                        Source::CdTrack(track),
                        &format!("CD track {track}"),
                    ));
                }
            }
        }
        if let Some(file) = source.manifest().find("MENU.INI") {
            let text = source.text(&file.path)?;
            for movie in
                rgpre::import::menu_movies::parse_menu_movies(&text).map_err(anyhow::Error::msg)?
            {
                catalog.entries.push(entry(
                    &format!("disc/{}", movie.name),
                    0,
                    Category::Movies,
                    Source::CdMovie(movie.name.clone()),
                    &movie.name,
                ));
            }
        }
        if let Some(file) = source.manifest().find("splash/glide2x_emu.ovl") {
            for index in 0..3 {
                catalog.entries.push(entry(
                    &file.path,
                    file.bytes,
                    Category::Images,
                    Source::Splash(index),
                    &format!("Splash texture {index}"),
                ));
            }
        }
        catalog
            .entries
            .sort_by(|a, b| a.path.cmp(&b.path).then(a.name.cmp(&b.name)));
        catalog.validate()?;
        progress(files.len(), files.len(), "Catalog ready");
        Ok(Self {
            source,
            catalog,
            input: RefCell::new(None),
            indexed: Default::default(),
            model_palettes: RefCell::new(None),
            outputs: RefCell::new(BTreeMap::new()),
        })
    }
    /// Adds an archive's member index without reading its resource payloads.
    pub fn expand(&mut self, path: &str) -> Result<()> {
        if self.indexed.contains(path) {
            return Ok(());
        }
        let parent = self
            .catalog
            .entries
            .iter()
            .find(|entry| entry.path == path && matches!(entry.source, Source::Archive))
            .context("unknown archive")?
            .clone();
        let input = prepare::Input::read(path, &self.source)?;
        let members: Vec<(Category, Source, String)> = match &input {
            prepare::Input::Bsi(bank) => bank
                .images
                .iter()
                .enumerate()
                .map(|(i, image)| {
                    (
                        Category::Images,
                        Source::Image(i),
                        format!("{} · {}", image.image_index, image.name),
                    )
                })
                .collect(),
            prepare::Input::Rob(bank) => bank
                .segments
                .iter()
                .enumerate()
                .map(|(i, segment)| (Category::Models, Source::Model(i), segment.name()))
                .collect(),
            prepare::Input::Sfx(bank) => bank
                .effects
                .iter()
                .enumerate()
                .map(|(i, _)| (Category::Audio, Source::Effect(i), format!("Effect {i}")))
                .collect(),
            prepare::Input::Rtx(bank) => bank
                .entries
                .iter()
                .enumerate()
                .map(|(i, clip)| {
                    (
                        Category::Speech,
                        Source::Speech(i),
                        String::from_utf8_lossy(&clip.tag).into_owned(),
                    )
                })
                .collect(),
            prepare::Input::File(_) => anyhow::bail!("{path}: not an indexed archive"),
        };
        for (category, source, name) in members {
            self.catalog
                .entries
                .push(entry(path, parent.bytes, category, source, &name));
        }
        self.catalog.validate()?;
        *self.input.borrow_mut() = Some((path.into(), Arc::new(input)));
        self.indexed.insert(path.into());
        Ok(())
    }

    /// Prepares one selection. No palette selects the first known world palette;
    /// an explicitly empty string requests embedded colors or raw indices instead.
    pub fn prepare(&mut self, id: &str, palette: Option<&str>) -> Result<Prepared> {
        let entry = self
            .catalog
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .context("unknown catalog item")?;
        {
            let mut input = self.input.borrow_mut();
            if input.as_ref().is_some_and(|(path, _)| path != &entry.path) {
                *input = None;
            }
        }
        self.outputs.borrow_mut().clear();
        let preview = match prepare::prepare(self, entry, palette).and_then(|preview| {
            preview.validate()?;
            Ok(preview)
        }) {
            Ok(preview) => preview,
            Err(error) => {
                self.outputs.borrow_mut().clear();
                Preview {
                    content: Content::Unavailable {
                        reason: format!("{}: {error:#}", entry.path),
                    },
                    details: Vec::new(),
                    frame_interval_ms: None,
                    metadata: None,
                    related: Vec::new(),
                }
            }
        };
        Ok(Prepared {
            preview,
            files: self.outputs.take(),
        })
    }
    fn source_file(&self, path: &str) -> Result<Arc<prepare::Input>> {
        let mut input = self.input.borrow_mut();
        if let Some((loaded, data)) = &*input
            && loaded == path
        {
            return Ok(data.clone());
        }
        let data = Arc::new(prepare::Input::read(path, &self.source)?);
        *input = Some((path.to_owned(), data.clone()));
        Ok(data)
    }
    fn output(&self, id: &str, suffix: &str) -> String {
        format!("viewer/{}/{}.{suffix}", self.catalog.generation, id)
    }
    fn publish(&self, path: &str, bytes: &[u8]) -> Result<()> {
        crate::validate_path(path)?;
        self.outputs
            .borrow_mut()
            .insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }
    fn cached(&self, path: &str) -> bool {
        self.outputs.borrow().contains_key(path)
    }
}

fn movie_bytes(source: &dyn ReadAssets, name: &str) -> Result<Vec<u8>> {
    for path in [name.to_owned(), format!("video/{name}")] {
        if let Some(file) = source.manifest().find(&path) {
            return source.read(&file.path);
        }
    }
    let mut image = rgpre::import::cdrom::CdImage::open(AssetFile::open(source, "disc/game.gog")?)?;
    let mut bytes = Vec::new();
    image.copy_file(name, &mut bytes)?;
    Ok(bytes)
}

fn track_pcm(source: &dyn ReadAssets, track: u32) -> Result<Vec<u8>> {
    let cue = source.text("disc/game.ins")?;
    let (name, start, end) =
        crate::disc::cue_track(&cue, track, true).map_err(anyhow::Error::msg)?;
    crate::validate_path(&name)?;
    let path = format!("disc/{name}");
    let file = source.manifest().find(&path).context("missing CD image")?;
    let end = end.unwrap_or(file.bytes);
    anyhow::ensure!(
        start <= end && end <= file.bytes,
        "CD track {track}: invalid byte range"
    );
    source.read_range(&file.path, start, usize::try_from(end - start)?)
}

pub(super) fn entry(
    path: &str,
    bytes: u64,
    category: Category,
    source: Source,
    name: &str,
) -> Entry {
    let mut hash = blake3::Hasher::new();
    hash.update(path.as_bytes());
    hash.update(format!("{source:?}").as_bytes());
    Entry {
        id: hash.finalize().to_hex().to_string(),
        name: name.into(),
        path: path.into(),
        format: path.rsplit('.').next().unwrap_or("file").to_uppercase(),
        category,
        source,
        bytes,
        error: None,
    }
}
fn archive_category(path: &str) -> Option<Category> {
    match FileType::from_path(Path::new(path))? {
        FileType::Bsi => Some(Category::Images),
        FileType::Rob => Some(Category::Models),
        FileType::Sfx => Some(Category::Audio),
        FileType::Rtx => Some(Category::Speech),
        _ => None,
    }
}

fn discover(
    source: &dyn ReadAssets,
    path: &Path,
    parent: &mut Entry,
    catalog: &mut Catalog,
) -> Result<()> {
    let kind = FileType::from_path(path);
    parent.category = match kind {
        Some(FileType::Bsi | FileType::Gxa | FileType::Pcx | FileType::Lbm | FileType::Anm) => {
            Category::Images
        }
        Some(FileType::Col) => Category::Palettes,
        Some(FileType::Fnt) => Category::Fonts,
        Some(FileType::Model3d | FileType::Model3dc) => Category::Models,
        Some(FileType::Rgm | FileType::Wld) => Category::Worlds,
        Some(_) => Category::Data,
        None => match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "ini" | "def" | "txt" | "lst" => Category::Data,
            "smk" => Category::Movies,
            "bmp" | "png" => Category::Images,
            "wav" => Category::Audio,
            _ => Category::Other,
        },
    };
    if kind == Some(FileType::Col) {
        let palette = palette::Palette::parse(&source.read(&parent.path)?)?;
        catalog.palettes.push(Palette {
            name: parent.path.clone(),
            colors: palette.colors.to_vec(),
        });
    }
    if path
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("WORLD.INI"))
    {
        let worlds = world_ini::WorldIni::parse(&source.text(&parent.path)?);
        for world in worlds.entries {
            catalog.entries.push(entry(
                &parent.path,
                parent.bytes,
                Category::Worlds,
                Source::World(world.index),
                &format!("{} · {} · {}", world.index, world.map, world.palette),
            ));
        }
    }
    Ok(())
}
