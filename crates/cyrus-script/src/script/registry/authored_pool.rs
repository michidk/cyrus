//! Authored allocations outlive the presence of a dispatch receiver.
use anyhow::{Context, Result, ensure};
use cyrus_assets::script::Scripts;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Owner {
    name: String,
    instance: usize,
    base: u32,
    count: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct AuthoredPool {
    world: u32,
    content_id: String,
    owners: Vec<Owner>,
    words: Vec<Option<i32>>,
    word_count: usize,
}

impl AuthoredPool {
    fn layout(scripts: &Scripts) -> Result<Vec<Owner>> {
        let mut owners = Vec::new();
        for (name, definition) in &scripts.actors {
            let count = definition.program.variables.len();
            if count == 0 {
                continue;
            }
            let offsets = definition
                .pool_offsets
                .with_context(|| format!("actor {name}: installed pool metadata missing"))?;
            for instance in 0..usize::from(definition.instances) {
                let base = offsets
                    .for_instance(instance, count)?
                    .locals
                    .with_context(|| format!("actor {name}: local offset missing"))?;
                owners.push(Owner {
                    name: name.clone(),
                    instance,
                    base,
                    count,
                });
            }
        }
        owners.sort_by_key(|owner| owner.base);
        let mut end = 0usize;
        for owner in &owners {
            let base = usize::try_from(owner.base)?;
            ensure!(
                base >= end,
                "actor {}: overlapping authored ownership",
                owner.name
            );
            end = base
                .checked_add(owner.count.checked_mul(4).context("pool size overflow")?)
                .context("pool size overflow")?;
        }
        Ok(owners)
    }

    #[cfg(feature = "original-save-import")]
    pub(super) fn validate_layout(scripts: &Scripts) -> Result<()> {
        Self::layout(scripts).map(|_| ())
    }

    pub(super) fn initialized(scripts: &Scripts) -> Result<Self> {
        let owners = Self::layout(scripts)?;
        let mut words = Vec::new();
        for owner in &owners {
            ensure!(
                owner.base as usize == words.len() * 4,
                "actor {}: unowned authored pool words before offset {}",
                owner.name,
                owner.base
            );
            let definition = &scripts.actors[&owner.name];
            let values = definition
                .instance_variables
                .get(owner.instance)
                .with_context(|| {
                    format!(
                        "actor {} instance {}: authored initialization missing",
                        owner.name, owner.instance
                    )
                })?;
            ensure!(
                values.len() == owner.count,
                "actor {}: authored initialization count differs",
                owner.name
            );
            words.extend(values.iter().copied().map(Some));
        }
        Ok(Self {
            world: scripts.world,
            content_id: scripts.content_id.clone(),
            word_count: words.len(),
            owners,
            words,
        })
    }

    pub(super) fn validate(&self, scripts: &Scripts) -> Result<()> {
        ensure!(
            self.world == scripts.world && self.content_id == scripts.content_id,
            "authored pool source identity differs"
        );
        ensure!(
            self.owners == Self::layout(scripts)?,
            "authored pool owner/range identity differs"
        );
        ensure!(
            self.words.len() == self.word_count
                && self.owners.iter().all(|o| (o.base as usize / 4)
                    .checked_add(o.count)
                    .is_some_and(|end| end <= self.word_count)),
            "authored pool retained word count differs"
        );
        Ok(())
    }

    pub(super) fn refresh(&mut self, name: &str, base: u32, variables: &[cyrus_soup::ScriptValue]) {
        let Some(owner) = self.owners.iter().find(|owner| {
            owner.name == name && owner.base == base && owner.count == variables.len()
        }) else {
            return;
        };
        for (index, value) in variables.iter().enumerate() {
            self.words[owner.base as usize / 4 + index] = value.integer().ok();
        }
    }

    #[cfg(feature = "original-save-import")]
    pub(super) fn imported(
        scripts: &Scripts,
        saved: &rgpre::import::savegame::SaveScriptLocals,
    ) -> Result<Self> {
        let owners = Self::layout(scripts)?;
        let count = saved.encode().len() / 4;
        for owner in &owners {
            saved
                .bind_offset(owner.base, owner.count)
                .with_context(|| {
                    format!(
                        "actor {} instance {}: saved authored range missing",
                        owner.name, owner.instance
                    )
                })?;
        }
        let words: Vec<_> = saved
            .bind_offset(0, count)?
            .iter()
            .copied()
            .map(Some)
            .collect();
        Ok(Self {
            world: scripts.world,
            content_id: scripts.content_id.clone(),
            word_count: words.len(),
            owners,
            words,
        })
    }

    #[cfg(feature = "original-save-import")]
    pub(super) fn current_words(
        &self,
        world: &super::ScriptWorld,
        scripts: &Scripts,
    ) -> Result<Vec<i32>> {
        self.validate(scripts)?;
        let mut words = self
            .words
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value.with_context(|| {
                    format!("authored local word {index}: retained reference encoding unsupported")
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for owner in &self.owners {
            let receivers: Vec<_> = world
                .actors
                .iter()
                .filter(|actor| {
                    actor.name() == owner.name
                        && actor
                            .pool_offsets
                            .is_some_and(|p| p.locals == Some(owner.base))
                })
                .collect();
            ensure!(
                receivers.len() <= 1,
                "actor {} instance {}: expected one live pool owner, found {}",
                owner.name,
                owner.instance,
                receivers.len()
            );
            if let Some(actor) = receivers.first() {
                let runtime = actor
                    .runtime
                    .as_ref()
                    .with_context(|| format!("actor {}: live variables unavailable", owner.name))?;
                ensure!(
                    runtime.variables().len() == owner.count,
                    "actor {}: live variable count differs",
                    owner.name
                );
                for (index, value) in runtime.variables().iter().enumerate() {
                    words[owner.base as usize / 4 + index] =
                        value.integer().with_context(|| {
                            format!(
                                "actor {} local {index}: original reference encoding unsupported",
                                owner.name
                            )
                        })?;
                }
            }
        }
        Ok(words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrus_soup::{ActorReference, ScriptValue};

    #[test]
    fn completed_receiver_writes_retain_values_without_creating_owners() {
        let mut pool = AuthoredPool {
            world: 7,
            content_id: "fixture".into(),
            owners: vec![Owner {
                name: "LOGIC".into(),
                instance: 0,
                base: 0,
                count: 1,
            }],
            words: vec![Some(4)],
            word_count: 1,
        };
        pool.refresh("OTHER", 0, &[ScriptValue::Integer(99)]);
        pool.refresh("LOGIC", 4, &[ScriptValue::Integer(99)]);
        pool.refresh("LOGIC", 0, &[]);
        assert_eq!(
            pool.words,
            [Some(4)],
            "name/range/count mismatches must not redirect storage"
        );
        pool.refresh("LOGIC", 0, &[ScriptValue::Integer(-123)]);
        let restored: AuthoredPool =
            serde_json::from_slice(&serde_json::to_vec(&pool).unwrap()).unwrap();
        assert_eq!(restored.words, [Some(-123)]);
        assert_eq!(restored.owners, pool.owners);
        pool.refresh(
            "LOGIC",
            0,
            &[ScriptValue::Actor(ActorReference {
                generation: 1,
                index: 2,
            })],
        );
        assert_eq!(
            pool.words,
            [None],
            "unsupported references cannot retain a stale integer"
        );
    }
}
