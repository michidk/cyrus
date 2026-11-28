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
struct Receiver {
    index: usize,
    name: String,
    base: u32,
    runtime_identifier: Option<i16>,
}

#[derive(Clone, Serialize, Deserialize)]
enum RuntimeAuthority {
    Initialized,
    Restored(Vec<Receiver>),
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct AuthoredPool {
    world: u32,
    content_id: String,
    owners: Vec<Owner>,
    words: Vec<Option<i32>>,
    word_count: usize,
    authority: RuntimeAuthority,
    receiver_count: usize,
    unbound_writer: Option<String>,
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
            authority: RuntimeAuthority::Initialized,
            unbound_writer: None,
            receiver_count: 0,
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
        if let RuntimeAuthority::Restored(receivers) = &self.authority {
            ensure!(
                receivers.len() == self.receiver_count,
                "restored authored receiver census differs"
            );
            let mut indices = std::collections::BTreeSet::new();
            let mut ranges = std::collections::BTreeSet::new();
            for receiver in receivers {
                ensure!(
                    indices.insert(receiver.index)
                        && ranges.insert((&receiver.name, receiver.base)),
                    "duplicate restored authored pool owner"
                );
                ensure!(
                    self.owners
                        .iter()
                        .any(|o| o.name == receiver.name && o.base == receiver.base),
                    "restored authored receiver range differs"
                );
            }
        }
        Ok(())
    }

    pub(super) fn refresh(
        &mut self,
        index: usize,
        name: &str,
        base: u32,
        variables: &[cyrus_soup::ScriptValue],
    ) {
        let base = match &self.authority {
            RuntimeAuthority::Initialized => base,
            RuntimeAuthority::Restored(receivers) => {
                let Some(receiver) = receivers
                    .iter()
                    .find(|r| r.index == index && r.name == name)
                else {
                    if !variables.is_empty() {
                        self.unbound_writer = Some(format!(
                            "actor {name} receiver {index}: execution lacks restored authored storage binding"
                        ));
                    }
                    return;
                };
                receiver.base
            }
        };
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
            authority: RuntimeAuthority::Restored(Vec::new()),
            unbound_writer: None,
            receiver_count: 0,
            owners,
            words,
        })
    }

    #[cfg(feature = "original-save-import")]
    pub(super) fn bind_restored_receivers(
        &mut self,
        bindings: Vec<(usize, String, Option<u32>)>,
        scripts: &Scripts,
        actors: &[super::ScriptActor],
    ) -> Result<()> {
        let mut receivers = Vec::new();
        let mut ranges = std::collections::BTreeSet::new();
        for (index, name, base) in bindings {
            if scripts.actors[&name].program.variables.is_empty() {
                continue;
            }
            let base =
                base.with_context(|| format!("actor {name}: restored local allocation missing"))?;
            ensure!(
                self.owners.iter().any(|o| o.name == name && o.base == base),
                "actor {name}: restored authored range differs"
            );
            ensure!(
                ranges.insert((name.clone(), base)),
                "actor {name}: duplicate restored authored pool owner"
            );
            receivers.push(Receiver {
                index,
                name,
                base,
                runtime_identifier: actors[index].runtime_identifier,
            });
        }
        self.receiver_count = receivers.len();
        self.authority = RuntimeAuthority::Restored(receivers);
        Ok(())
    }

    #[cfg(feature = "original-save-import")]
    pub(super) fn current_words(
        &self,
        world: &super::ScriptWorld,
        scripts: &Scripts,
    ) -> Result<Vec<i32>> {
        self.validate(scripts)?;
        ensure!(
            self.unbound_writer.is_none(),
            "{}",
            self.unbound_writer.as_deref().unwrap_or_default()
        );
        if let RuntimeAuthority::Restored(receivers) = &self.authority {
            for receiver in receivers {
                ensure!(
                    world.actors.get(receiver.index).is_some_and(|actor| {
                        actor.name() == receiver.name
                            && actor.runtime_identifier == receiver.runtime_identifier
                            && match receiver.runtime_identifier {
                                Some(_) => !actor
                                    .placement()
                                    .is_some_and(|p| p.object_type.has_separate_record()),
                                None => actor.placement().is_some_and(|p| {
                                    p.object_type
                                        == cyrus_assets::script::PlacementType::SCRIPT_ONLY
                                }),
                            }
                    }),
                    "restored authored receiver identity differs"
                );
            }
        }
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
                .enumerate()
                .filter(|(index, actor)| {
                    let bound = match &self.authority {
                        RuntimeAuthority::Initialized => {
                            actor.pool_offsets.and_then(|p| p.locals) == Some(owner.base)
                        }
                        RuntimeAuthority::Restored(receivers) => receivers.iter().any(|r| {
                            r.index == *index && r.name == owner.name && r.base == owner.base
                        }),
                    };
                    bound && actor.name() == owner.name
                })
                .map(|(_, actor)| actor)
                .collect();
            ensure!(
                receivers.len() <= 1,
                "actor {} instance {}: expected one live pool owner, found {}",
                owner.name,
                owner.instance,
                receivers.len()
            );
            if let Some(actor) = receivers.first() {
                if actor.runtime.is_none()
                    && matches!(self.authority, RuntimeAuthority::Restored(_))
                {
                    continue;
                }
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
    fn restored_contexts_cannot_redirect_retained_word_writes() {
        let mut pool = AuthoredPool {
            world: 7,
            content_id: "fixture".into(),
            owners: vec![Owner {
                name: "LOGIC".into(),
                instance: 0,
                base: 0,
                count: 1,
            }],
            words: vec![Some(-17)],
            word_count: 1,
            authority: RuntimeAuthority::Restored(vec![Receiver {
                index: 4,
                name: "LOGIC".into(),
                base: 0,
                runtime_identifier: Some(9),
            }]),
            receiver_count: 1,
            unbound_writer: None,
        };
        let mut unknown = pool.clone();
        unknown.refresh(0, "LOGIC", 0, &[ScriptValue::Integer(999)]);
        assert_eq!(
            unknown.words,
            [Some(-17)],
            "an initialization alias must not replace retained saved values"
        );
        assert!(
            unknown.unbound_writer.is_some(),
            "unbound execution must make export unsupported"
        );
        let restored: AuthoredPool =
            serde_json::from_slice(&serde_json::to_vec(&unknown).unwrap()).unwrap();
        assert!(
            restored.unbound_writer.is_some(),
            "snapshot restore must not clear unsupported writes"
        );
        pool.refresh(4, "LOGIC", 16, &[ScriptValue::Integer(23)]);
        assert_eq!(
            pool.words,
            [Some(23)],
            "restored receiver uses its retained binding rather than a fresh allocation offset"
        );
        assert!(pool.unbound_writer.is_none());
    }

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
            authority: RuntimeAuthority::Initialized,
            unbound_writer: None,
            receiver_count: 0,
        };
        pool.refresh(0, "OTHER", 0, &[ScriptValue::Integer(99)]);
        pool.refresh(0, "LOGIC", 4, &[ScriptValue::Integer(99)]);
        pool.refresh(0, "LOGIC", 0, &[]);
        assert_eq!(
            pool.words,
            [Some(4)],
            "name/range/count mismatches must not redirect storage"
        );
        pool.refresh(0, "LOGIC", 0, &[ScriptValue::Integer(-123)]);
        let restored: AuthoredPool =
            serde_json::from_slice(&serde_json::to_vec(&pool).unwrap()).unwrap();
        assert_eq!(restored.words, [Some(-123)]);
        assert_eq!(restored.owners, pool.owners);
        pool.refresh(
            0,
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
