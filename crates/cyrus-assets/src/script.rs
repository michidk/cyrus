//! Portable SOUP programs and validated installed-definition records.
mod validation;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Versioned actor-local instructions and initial state, independent of a renderer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub version: u32,
    pub entry: u32,
    #[serde(default)]
    pub variables: Vec<i32>,
    #[serde(default)]
    pub strings: Vec<String>,
    pub steps: BTreeMap<u32, Step>,
}

/// A single decoded statement, with offsets relative to its actor's script slice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Step {
    pub next: u32,
    pub operation: Operation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    Call {
        call: Call,
    },
    ObjectCall {
        call: Call,
    },
    Assign {
        destination: Place,
        formula: Formula,
    },
    Increment {
        destination: Place,
        delta: i32,
    },
    If {
        conditions: Vec<Condition>,
        target: u32,
    },
    Goto {
        target: u32,
    },
    End {
        target: u32,
    },
    Gosub {
        target: u32,
    },
    Return,
    Endint,
    Anchor {
        value: u8,
    },
    Yield,
    TaskStop {
        label: u32,
    },
    TaskPause {
        label: u32,
    },
    TaskResume {
        label: u32,
    },
    ScriptRv {
        expected: u8,
        target: u32,
        save_return: bool,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Call {
    /// Original index in the installed SOUP386.DEF, including its synthetic null entry.
    pub function_id: u16,
    pub kind: CallKind,
    pub target: Object,
    pub function: String,
    pub arguments: Vec<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    Task,
    Multitask,
    Function,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Object {
    Me,
    Player,
    Camera,
    Named { string: u32 },
    Local { index: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Place {
    Flag { index: u16 },
    Local { index: u8 },
    Property { object: Object, reference: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Value {
    Integer { value: i32 },
    String { index: u32 },
    Read { place: Place },
    Update { place: Place, delta: i32 },
    Call { call: Box<Call> },
}

/// Formula operators retain their byte values; evaluation is owned by the consumer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Formula {
    pub first: Value,
    pub rest: Vec<(u8, Value)>,
}

/// Conjunction joins this comparison to the next: zero ends, one AND, two OR.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Condition {
    pub lhs: Value,
    pub comparison: u8,
    pub rhs: Value,
    pub conjunction: u8,
}

impl Program {
    /// Validates entry points and continuations before any host side effect.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 2,
            "unsupported program version {}; re-export the world",
            self.version
        );
        let target = |offset| -> anyhow::Result<()> {
            anyhow::ensure!(
                self.steps.contains_key(&offset),
                "target {offset} is not an instruction"
            );
            Ok(())
        };
        target(self.entry)?;
        if self.entry != 0 {
            target(0)?;
        }
        for (&offset, step) in &self.steps {
            let result: anyhow::Result<()> = (|| {
                self.validate_operation(&step.operation)?;
                match &step.operation {
                    Operation::Goto { target: value } | Operation::End { target: value } => {
                        target(*value)?
                    }
                    Operation::If { target: value, .. }
                    | Operation::Gosub { target: value }
                    | Operation::ScriptRv { target: value, .. } => {
                        target(*value)?;
                        target(step.next)?;
                    }
                    Operation::Return | Operation::Endint | Operation::Unsupported { .. } => {}
                    _ => target(step.next)?,
                }
                if !matches!(step.operation, Operation::Unsupported { .. }) {
                    anyhow::ensure!(step.next > offset, "invalid continuation {}", step.next);
                }
                Ok(())
            })();
            result.with_context(|| format!("instruction {offset}"))?;
        }
        Ok(())
    }
}

/// Installed function declaration; call-site kind is retained separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub param_count: u8,
    pub kind: CallKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flag {
    pub name: String,
    pub flag_type: String,
    pub initial_value: i32,
}

/// Definitions keep installed order because bytecode refers to numeric indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Definition {
    pub functions: Vec<Function>,
    pub flags: Vec<Flag>,
    pub references: Vec<String>,
    pub attributes: Vec<String>,
}

/// Script metadata remains available even for actors without animation geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub header_index: usize,
    /// Byte offset within RASC data, retained for foreign task operand identity.
    pub code_origin: u32,
    pub instances: u16,
    pub namespace: Option<String>,
    pub instance_variables: Vec<Vec<i32>>,
    pub program: Program,
    pub attributes: BTreeMap<String, u8>,
    pub hooks: Vec<u32>,
}

/// Authored MPOB order identifies logical objects independently of mesh availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub object_id: u32,
    pub actor: String,
    pub object_type: u8,
    pub active: u8,
    /// Packed map coordinates retained in their original field units.
    pub map_position: [i64; 3],
    pub angles: [u32; 3],
}

#[derive(bevy::prelude::Asset, bevy::prelude::TypePath, Debug, Clone, Serialize, Deserialize)]
pub struct Scripts {
    pub version: u32,
    /// Installed reusable actor-pool capacity, including the engine's minimum of ten.
    pub actor_pool_capacity: u32,
    pub content_id: String,
    pub world: u32,
    pub definition: Definition,
    pub actors: BTreeMap<String, Actor>,
    pub instances: Vec<Instance>,
    pub unsupported: BTreeMap<String, String>,
}

impl Scripts {
    /// Stable identity covers all definitions, actor programs and unsupported records.
    pub fn content_identity(&self) -> anyhow::Result<String> {
        let bytes = serde_json::to_vec(&(
            self.version,
            self.actor_pool_capacity,
            self.world,
            &self.definition,
            &self.actors,
            &self.instances,
            &self.unsupported,
        ))?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 3,
            "world {}: script schema {} unsupported; re-export the world",
            self.world,
            self.version
        );
        anyhow::ensure!(
            (10..=i32::MAX as u32).contains(&self.actor_pool_capacity),
            "world {}: invalid script actor pool capacity {}",
            self.world,
            self.actor_pool_capacity
        );
        anyhow::ensure!(
            self.content_id == self.content_identity()?,
            "world {}: script content identity mismatch; re-export the world",
            self.world
        );
        anyhow::ensure!(
            self.definition
                .functions
                .first()
                .is_some_and(
                    |function| function.name == "NullFunction" && function.param_count == 0
                ),
            "world {}: null function metadata absent",
            self.world
        );
        for (name, actor) in &self.actors {
            anyhow::ensure!(
                actor.instance_variables.len() == usize::from(actor.instances)
                    && actor
                        .instance_variables
                        .iter()
                        .all(|values| values.len() == actor.program.variables.len()),
                "world {} actor {name}: incomplete instance locals",
                self.world
            );
            actor
                .program
                .validate_definition(&self.definition)
                .with_context(|| format!("world {} actor {name} script", self.world))?;
        }
        Ok(())
    }
}

#[derive(Default, bevy::prelude::TypePath)]
pub struct ScriptLoader;
impl bevy::asset::AssetLoader for ScriptLoader {
    type Asset = Scripts;
    type Settings = ();
    type Error = anyhow::Error;
    async fn load(
        &self,
        reader: &mut dyn bevy::asset::io::Reader,
        _: &(),
        context: &mut bevy::asset::LoadContext<'_>,
    ) -> anyhow::Result<Scripts> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let scripts: Scripts = serde_json::from_slice(&bytes)
            .with_context(|| format!("script cache {}; re-export the world", context.path()))?;
        scripts.validate()?;
        Ok(scripts)
    }
    fn extensions(&self) -> &[&str] {
        &["scripts"]
    }
}
