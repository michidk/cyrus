//! Original authored-local record export from complete installed pool ownership.
use super::ScriptWorld;
use anyhow::{Context, Result, ensure};
use cyrus_assets::script::Scripts;
use rgpre::import::savegame::SaveScriptLocals;

impl ScriptWorld {
    /// Encode the complete SVRG payload from live integer variables.
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
        let mut pool = std::collections::BTreeMap::new();
        for (name, definition) in &scripts.actors {
            let count = definition.program.variables.len();
            if count == 0 {
                continue;
            }
            let offsets = definition
                .pool_offsets
                .with_context(|| format!("actor {name}: installed pool metadata missing"))?;
            for instance in 0..usize::from(definition.instances) {
                let offsets = offsets.for_instance(instance, count)?;
                let base = offsets
                    .locals
                    .with_context(|| format!("actor {name}: local offset missing"))?;
                let owners: Vec<_> = self
                    .actors
                    .iter()
                    .filter(|actor| {
                        actor.name() == name
                            && actor.pool_offsets.is_some_and(|p| p.locals == Some(base))
                    })
                    .collect();
                ensure!(
                    owners.len() == 1,
                    "actor {name} instance {instance}: expected one live pool owner, found {}",
                    owners.len()
                );
                let runtime = owners[0]
                    .runtime
                    .as_ref()
                    .with_context(|| format!("actor {name}: live variables unavailable"))?;
                ensure!(
                    runtime.variables().len() == count,
                    "actor {name}: live variable count differs from installed definition"
                );
                for (index, value) in runtime.variables().iter().enumerate() {
                    let offset = base
                        .checked_add(
                            u32::try_from(index)?
                                .checked_mul(4)
                                .context("local offset overflow")?,
                        )
                        .context("pool offset overflow")?;
                    let value = value.integer().with_context(|| {
                        format!(
                            "actor {name} local {index}: original reference encoding unsupported"
                        )
                    })?;
                    ensure!(
                        pool.insert(offset, value).is_none(),
                        "actor {name}: overlapping authored ownership"
                    );
                }
            }
        }
        for (index, offset) in pool.keys().enumerate() {
            ensure!(
                usize::try_from(*offset)?
                    == index.checked_mul(4).context("pool length overflow")?,
                "unowned authored pool words before offset {offset}"
            );
        }
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
        Ok(pool.into_values().collect())
    }
}
