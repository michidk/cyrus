//! Native preload scan and parser coverage results.
use bevy::prelude::*;
use rgpre::import::{FileType, registry};
use std::sync::mpsc::Sender;
#[derive(Debug, Clone)]
pub enum Progress {
    File {
        done: usize,
        total: usize,
        name: String,
    },
    Done(Box<Preloaded>),
    /// Required menu and HUD preparation completed, without a parser-coverage scan.
    MenuReady,
    /// Required startup export failed; parser-coverage failures remain in `Preloaded`.
    Failed(String),
}

/// Result of the preload, kept for the rest of the session and written to
/// `cyrus-config/preload-report.json`: the format-coverage report.
#[derive(Resource, Debug, Clone, Default, serde::Serialize)]
pub struct Preloaded {
    pub parsed: usize,
    pub failed: Vec<(String, String)>,
    /// Per file type (display name): parsed and failed counts.
    pub by_type: std::collections::BTreeMap<String, TypeCount>,
    /// Extensions in the install that no parser claims, with file counts.
    pub unsupported: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct TypeCount {
    pub parsed: usize,
    pub failed: usize,
}

pub fn preload(root: &std::path::Path, tx: &Sender<Progress>) {
    let registry = match registry::scan_dir(root) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(Progress::Done(Box::new(Preloaded {
                failed: vec![("scan".into(), e.to_string())],
                ..Default::default()
            })));
            return;
        }
    };
    let mut entries: Vec<_> = registry
        .files
        .values()
        .filter(|entry| !crate::excluded_file_type(entry.file_type))
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let total = entries.len();
    let mut failed = Vec::new();
    let mut by_type = std::collections::BTreeMap::<String, TypeCount>::new();
    for (i, entry) in entries.iter().enumerate() {
        let _ = tx.send(Progress::File {
            done: i,
            total,
            name: entry.name.clone(),
        });
        let count = by_type
            .entry(entry.file_type.display_name().to_string())
            .or_default();
        match parse_one(&entry.path, entry.file_type) {
            Ok(()) => count.parsed += 1,
            Err(e) => {
                count.failed += 1;
                failed.push((entry.name.clone(), e));
            }
        }
    }
    let _ = tx.send(Progress::File {
        done: total,
        total,
        name: "done".into(),
    });
    let _ = tx.send(Progress::Done(Box::new(Preloaded {
        parsed: total - failed.len(),
        failed,
        by_type,
        unsupported: unsupported_extensions(root),
    })));
}

/// Files under the install whose extension no parser claims, counted per extension.
pub fn unsupported_extensions(root: &std::path::Path) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() || FileType::from_path(entry.path()).is_some() {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        *counts.entry(ext).or_insert(0) += 1;
    }
    counts
}

/// One parse per file type. Returns the error text so coverage gaps are visible in the log.
pub fn parse_one(path: &std::path::Path, file_type: FileType) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    use rgpre::import::*;
    let r: Result<(), String> = match file_type {
        FileType::Bsi => bsi::parse_bsi_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Cht => cht::parse_cht_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Col => palette::Palette::parse(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Model3d | FileType::Model3dc => model3d::parse_3d_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Rob => rob::parse_rob_with_models(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Rgm => rgm::parse_rgm_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Pvo => pvo::parse_pvo_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Wld => wld::parse_wld_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Fnt => fnt::parse_fnt(&data).map(drop).map_err(|e| e.to_string()),
        FileType::Pcx => pcx::parse_pcx_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Lbm => lbm::parse_lbm_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Anm => anm::parse_anm_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Gxa => gxa::parse_gxa_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Sfx => sfx::parse_sfx_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Rtx => rtx::parse_rtx_file(&data)
            .map(drop)
            .map_err(|e| e.to_string()),
        FileType::Dig
        | FileType::Mdi
        | FileType::Pif
        | FileType::Opl
        | FileType::Ad
        | FileType::Rst => Err("Support file excluded from parsing".into()),
    };
    r
}
