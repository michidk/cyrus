//! Prepare original PVO object admission through the portable byte source.
use crate::collision::{ModelVisibility, VisibilityNode};
use crate::source::ReadAssets;
use anyhow::{Context, Result};
use rgpre::import::pvo::{self, LEAF_REF_NONE};
use std::collections::BTreeMap;

pub(super) fn prepare(source: &dyn ReadAssets, stem: &str) -> Result<ModelVisibility> {
    let path = format!("maps/{stem}.PVO");
    let Some(file) = source.manifest().find(&path) else {
        return Ok(ModelVisibility::default());
    };
    let tree = pvo::parse_pvo_file(&source.read(&file.path)?).with_context(|| file.path.clone())?;
    let indices: BTreeMap<_, _> = tree
        .octr_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.byte_offset, index))
        .collect();
    let nodes = tree
        .octr_nodes
        .iter()
        .map(|node| -> Result<VisibilityNode> {
            let mut children = [None; 8];
            for &(octant, offset) in &node.child_refs {
                children[usize::from(octant)] =
                    Some(*indices.get(&(offset as usize)).with_context(|| {
                        format!(
                            "{} OCTR {}: unsupported child {offset}",
                            file.path, node.byte_offset
                        )
                    })?);
            }
            let mut objects = Vec::new();
            if node.leaf_ref != LEAF_REF_NONE {
                let leaf = tree
                    .plst_leaves
                    .iter()
                    .find(|leaf| leaf.byte_offset == node.leaf_ref as usize)
                    .context("PVO leaf missing after parsing")?;
                for entry in leaf.entries.iter().rev() {
                    let start = entry.mlst_start as usize;
                    let end = start
                        .checked_add(usize::from(entry.count))
                        .context("PVO MLST range overflow")?;
                    objects.extend_from_slice(
                        tree.mlst_indices
                            .get(start..end)
                            .context("PVO MLST range outside parsed data")?,
                    );
                }
            }
            Ok(VisibilityNode { children, objects })
        })
        .collect::<Result<_>>()?;
    let result = ModelVisibility {
        center: [
            tree.header.center_x,
            tree.header.center_y,
            tree.header.center_z,
        ],
        half_extent: tree.header.cell_size,
        nodes,
    };
    result.validate()?;
    Ok(result)
}

#[cfg(all(test, feature = "native", not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn installed_model_visibility_prepares_every_shipped_tree_and_absent_start() {
        let root = std::env::var_os("REDGUARD_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../redguard")
            });
        if !root.join("RGFX.EXE").is_file() {
            eprintln!("skipped: no install at {}", root.display());
            return;
        }
        let install = crate::install::GameInstall::open(&root).unwrap();
        let source = crate::native::Files::scan(&install, false).unwrap();
        assert!(prepare(&source, "START").unwrap().nodes.is_empty());
        for name in ["CATACOMB", "CAVERNS", "DRINT", "ISLAND", "PALACE"] {
            let data = prepare(&source, name).unwrap();
            assert!(!data.nodes.is_empty(), "{name}");
            assert!(
                data.nodes.iter().any(|node| !node.objects.is_empty()),
                "{name}"
            );
            let restored: ModelVisibility =
                serde_json::from_slice(&serde_json::to_vec(&data).unwrap()).unwrap();
            restored.validate().unwrap();
            assert_eq!(restored.nodes.len(), data.nodes.len(), "{name}");
        }
    }
}
