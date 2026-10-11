//! Serialized explicit animation program descriptors.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Deserialize, Serialize)]
pub struct Program {
    pub version: u32,
    pub entry: u32,
    pub steps: BTreeMap<u32, Step>,
}

#[derive(Deserialize, Serialize)]
pub struct Step {
    pub next: u32,
    pub operation: Operation,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Operation {
    Task {
        function: String,
        arguments: Vec<i32>,
    },
    End {
        target: u32,
    },
    Unsupported {
        reason: String,
    },
}
