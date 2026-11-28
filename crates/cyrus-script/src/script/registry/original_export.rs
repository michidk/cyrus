//! Original authored-local record export from complete installed pool ownership.
use super::ScriptWorld;
use anyhow::{Context, Result, ensure};
use cyrus_assets::script::Scripts;
use rgpre::import::savegame::SaveScriptLocals;

impl ScriptWorld {
    /// Encode SVRG from retained authored allocations and current live integer variables.
    /// Unsupported ownership rejects the record before any slot filesystem mutation.
    /// Pool addressing follows preservation `docs/formats/SAVEGAME.md`, authored script locals.
    pub fn export_original_authored_locals(&self, scripts: &Scripts) -> Result<Vec<u8>> {
        self.authored_local_words(scripts)
            .map(|words| SaveScriptLocals::from(words).encode())
            .with_context(|| format!("world {} SVRG export unsupported", self.world))
    }

    fn authored_local_words(&self, scripts: &Scripts) -> Result<Vec<i32>> {
        ensure!(
            self.world == scripts.world && self.content_id == scripts.content_id,
            "source identity does not match installed world/content"
        );
        super::authored_pool::AuthoredPool::validate_layout(scripts)?;
        let retained = self
            .authored_pool
            .as_ref()
            .context("retained authored pool missing")?;
        let words = retained.current_words(self, scripts)?;
        let pool: std::collections::BTreeMap<_, _> = words
            .iter()
            .copied()
            .enumerate()
            .map(|(index, word)| ((index * 4) as u32, word))
            .collect();
        if let Some(source) = &self.globals.separate_native_source
            && !source.locals.is_empty()
        {
            let base = source
                .pool_offsets
                .and_then(|p| p.locals)
                .context("retained temporary source local offset missing")?;
            for (index, value) in source.locals.iter().enumerate() {
                let offset = base
                    .checked_add(
                        u32::try_from(index)?
                            .checked_mul(4)
                            .context("source offset overflow")?,
                    )
                    .context("source offset overflow")?;
                ensure!(
                    pool.get(&offset)
                        == Some(
                            &value
                                .integer()
                                .context("retained source reference encoding unsupported")?
                        ),
                    "retained temporary source local {index} disagrees with current pool"
                );
            }
        }
        Ok(words)
    }
}
