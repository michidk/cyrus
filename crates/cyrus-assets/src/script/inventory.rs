//! Static SOUP census; occurrence counts do not establish runtime support or reachability.
use super::Scripts;
use cyrus_script::program::{Call, Object, Operation, Place, Value};
use serde::Serialize;
use std::collections::BTreeMap;

/// Counts authored occurrences, including nested expressions and unplaced actors.
/// Execution and original-game evidence must be collected separately.
#[derive(Default, Debug, Serialize)]
pub struct Inventory {
    pub world: u32,
    pub declared_functions: Vec<String>,
    pub declared_references: Vec<String>,
    pub actors: BTreeMap<String, ActorInventory>,
    pub unsupported: BTreeMap<String, String>,
}

#[derive(Default, Debug, Serialize)]
pub struct ActorInventory {
    pub placements: usize,
    pub instances: u16,
    pub native_hook: Option<Call>,
    pub instructions: BTreeMap<String, usize>,
    pub calls: BTreeMap<String, usize>,
    pub operands: BTreeMap<String, usize>,
    pub references: BTreeMap<String, usize>,
    pub unsupported: Vec<String>,
}

/// Inspects every decoded instruction without executing it or claiming host support.
pub fn inspect(scripts: &Scripts) -> Inventory {
    let mut inventory = Inventory {
        world: scripts.world,
        declared_functions: scripts
            .definition
            .functions
            .iter()
            .map(|f| f.name.clone())
            .collect(),
        declared_references: scripts.definition.references.clone(),
        unsupported: scripts.unsupported.clone(),
        ..Default::default()
    };
    for (name, actor) in &scripts.actors {
        let mut entry = ActorInventory {
            placements: scripts
                .instances
                .iter()
                .filter(|i| &i.actor == name)
                .count(),
            instances: actor.instances,
            native_hook: actor.native_hook.clone(),
            ..Default::default()
        };
        if let Some(hook) = &actor.native_hook {
            entry.call(hook);
        }
        for step in actor.program.steps.values() {
            let kind = match &step.operation {
                Operation::Call { call } => {
                    entry.call(call);
                    "call"
                }
                Operation::ObjectCall { call } => {
                    entry.call(call);
                    "object_call"
                }
                Operation::Assign {
                    destination,
                    formula,
                } => {
                    entry.place(destination);
                    entry.value(&formula.first);
                    for (operator, value) in &formula.rest {
                        count(&mut entry.operands, format!("formula_operator:{operator}"));
                        entry.value(value);
                    }
                    "assign"
                }
                Operation::Increment { destination, .. } => {
                    entry.place(destination);
                    "increment"
                }
                Operation::If { conditions, .. } => {
                    for condition in conditions {
                        count(
                            &mut entry.operands,
                            format!("comparison:{}", condition.comparison),
                        );
                        count(
                            &mut entry.operands,
                            format!("conjunction:{}", condition.conjunction),
                        );
                        entry.value(&condition.lhs);
                        entry.value(&condition.rhs);
                    }
                    "if"
                }
                Operation::Goto { .. } => "goto",
                Operation::End { .. } => "end",
                Operation::Gosub { .. } => "gosub",
                Operation::Return => "return",
                Operation::Endint => "endint",
                Operation::Anchor { .. } => "anchor",
                Operation::Yield => "yield",
                Operation::TaskStop { .. } => "task_stop",
                Operation::TaskPause { .. } => "task_pause",
                Operation::TaskResume { .. } => "task_resume",
                Operation::ScriptRv { .. } => "script_rv",
                Operation::Unsupported { reason } => {
                    entry.unsupported.push(reason.clone());
                    "unsupported"
                }
            };
            count(&mut entry.instructions, kind.into());
        }
        inventory.actors.insert(name.clone(), entry);
    }
    inventory
}

fn count(counts: &mut BTreeMap<String, usize>, name: String) {
    *counts.entry(name).or_default() += 1;
}

impl ActorInventory {
    fn object(&mut self, object: &Object) {
        let kind = match object {
            Object::Me => "me",
            Object::Player => "player",
            Object::Camera => "camera",
            Object::Named { .. } => "named",
            Object::Local { .. } => "local",
        };
        count(&mut self.references, format!("object:{kind}"));
    }
    fn place(&mut self, place: &Place) {
        let kind = match place {
            Place::Flag { .. } => "flag",
            Place::Local { .. } => "local",
            Place::Property { object, reference } => {
                self.object(object);
                count(&mut self.references, format!("property:{reference}"));
                "property"
            }
        };
        count(&mut self.operands, format!("place:{kind}"));
    }
    fn call(&mut self, call: &Call) {
        count(
            &mut self.calls,
            format!("{}:{:?}:{}", call.function_id, call.kind, call.function),
        );
        self.object(&call.target);
        for value in &call.arguments {
            self.value(value);
        }
    }
    fn value(&mut self, value: &Value) {
        let kind = match value {
            Value::Integer { .. } => "integer",
            Value::String { .. } => "string",
            Value::Read { place } => {
                self.place(place);
                "read"
            }
            Value::Update { place, .. } => {
                self.place(place);
                "update"
            }
            Value::Call { call } | Value::ObjectCall { call } => {
                self.call(call);
                if matches!(value, Value::ObjectCall { .. }) {
                    "object_call"
                } else {
                    "call"
                }
            }
        };
        count(&mut self.operands, format!("value:{kind}"));
    }
}
