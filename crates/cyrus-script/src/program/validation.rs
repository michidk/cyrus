//! Validate portable operands before a script can issue host side effects.
use super::{Call, Definition, Formula, Object, Operation, Place, Program, Value};
use anyhow::Result;
impl Program {
    /// Checks an actor-load hook against its program operands and installed declarations.
    pub fn validate_native_hook(&self, hook: &Call, definition: &Definition) -> Result<()> {
        anyhow::ensure!(
            hook.target == Object::Me && hook.kind == super::CallKind::Function,
            "native hook must directly call a function on its actor"
        );
        self.call(hook, 0)?;
        let declaration = definition.functions.get(usize::from(hook.function_id));
        anyhow::ensure!(
            declaration.is_some_and(|function| {
                function.name == hook.function
                    && usize::from(function.param_count) == hook.arguments.len()
            }),
            "native hook function {} declaration mismatch for index {}",
            hook.function,
            hook.function_id
        );
        anyhow::ensure!(
            hook.arguments.len() <= 8,
            "native hook exceeds eight parameters"
        );
        for argument in &hook.arguments {
            anyhow::ensure!(
                !matches!(
                    argument,
                    Value::Call { .. } | Value::ObjectCall { .. } | Value::Update { .. }
                ),
                "native hook parameter cannot call or update"
            );
            if let Value::Read { place } = argument {
                match place {
                    Place::Flag { index } => anyhow::ensure!(
                        usize::from(*index) < definition.flags.len(),
                        "native hook global flag {index} absent"
                    ),
                    Place::Property { reference, .. } => anyhow::ensure!(
                        usize::from(*reference) < definition.references.len(),
                        "native hook reference {reference} absent"
                    ),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Checks every installed index and function signature before execution.
    pub fn validate_definition(&self, definition: &Definition) -> Result<()> {
        self.validate()?;
        for (offset, step) in &self.steps {
            let mut values = Vec::new();
            let mut places = Vec::new();
            let mut calls = Vec::new();
            match &step.operation {
                Operation::Call { call } | Operation::ObjectCall { call } => calls.push(call),
                Operation::Assign {
                    destination,
                    formula,
                } => {
                    places.push(destination);
                    values.push(&formula.first);
                    values.extend(formula.rest.iter().map(|(_, value)| value));
                }
                Operation::Increment { destination, .. } => places.push(destination),
                Operation::If { conditions, .. } => {
                    for condition in conditions {
                        values.extend([&condition.lhs, &condition.rhs]);
                    }
                }
                _ => {}
            }
            while !values.is_empty() || !calls.is_empty() {
                while let Some(value) = values.pop() {
                    match value {
                        Value::Read { place } | Value::Update { place, .. } => places.push(place),
                        Value::Call { call } | Value::ObjectCall { call } => calls.push(call),
                        _ => {}
                    }
                }
                while let Some(call) = calls.pop() {
                    let declaration = definition.functions.get(usize::from(call.function_id));
                    anyhow::ensure!(
                        declaration.is_some_and(|function| {
                            function.name == call.function
                                && usize::from(function.param_count) == call.arguments.len()
                        }),
                        "instruction {offset}: function {} declaration mismatch for index {}",
                        call.function,
                        call.function_id
                    );
                    values.extend(&call.arguments);
                }
            }
            for place in places {
                match place {
                    Place::Flag { index } => anyhow::ensure!(
                        usize::from(*index) < definition.flags.len(),
                        "instruction {offset}: global flag {index} absent"
                    ),
                    Place::Property { reference, .. } => anyhow::ensure!(
                        usize::from(*reference) < definition.references.len(),
                        "instruction {offset}: reference {reference} absent"
                    ),
                    _ => {}
                }
            }
        }
        Ok(())
    }
    pub(super) fn validate_operation(&self, operation: &Operation) -> Result<()> {
        match operation {
            Operation::Call { call } => {
                anyhow::ensure!(
                    call.target == Object::Me,
                    "self call carries an object selector"
                );
                self.call(call, 0)?;
            }
            Operation::ObjectCall { call } => self.call(call, 0)?,
            Operation::Assign {
                destination,
                formula,
            } => {
                self.place(destination)?;
                self.formula(formula)?;
            }
            Operation::Increment { destination, delta } => {
                self.place(destination)?;
                anyhow::ensure!(matches!(delta, -1 | 1), "invalid increment {delta}");
            }
            Operation::If { conditions, .. } => {
                anyhow::ensure!(!conditions.is_empty(), "condition list absent");
                for (index, condition) in conditions.iter().enumerate() {
                    self.value(&condition.lhs, 0)?;
                    self.value(&condition.rhs, 0)?;
                    anyhow::ensure!(
                        condition.comparison <= 5,
                        "invalid comparison {}",
                        condition.comparison
                    );
                    let valid = if index + 1 == conditions.len() {
                        condition.conjunction == 0
                    } else {
                        matches!(condition.conjunction, 1 | 2)
                    };
                    anyhow::ensure!(valid, "invalid conjunction {}", condition.conjunction);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn object(&self, object: &Object) -> Result<()> {
        match object {
            Object::Named { string } => anyhow::ensure!(
                (*string as usize) < self.strings.len(),
                "object string {string} absent"
            ),
            Object::Local { index } => anyhow::ensure!(
                usize::from(*index) < self.variables.len(),
                "object local {index} absent"
            ),
            _ => {}
        }
        Ok(())
    }
    fn place(&self, place: &Place) -> Result<()> {
        match place {
            Place::Local { index } => anyhow::ensure!(
                usize::from(*index) < self.variables.len(),
                "local variable {index} absent"
            ),
            Place::Property { object, .. } => self.object(object)?,
            Place::Flag { .. } => {}
        }
        Ok(())
    }
    fn value(&self, value: &Value, depth: usize) -> Result<()> {
        anyhow::ensure!(depth < 128, "expression nesting limit exceeded");
        match value {
            Value::String { index } => anyhow::ensure!(
                (*index as usize) < self.strings.len(),
                "script string {index} absent"
            ),
            Value::Read { place } => self.place(place)?,
            Value::Update { place, delta } => {
                self.place(place)?;
                anyhow::ensure!(
                    matches!(delta, -1 | 1),
                    "invalid conditional update {delta}"
                );
            }
            Value::Call { call } | Value::ObjectCall { call } => {
                anyhow::ensure!(
                    call.kind == super::CallKind::Function,
                    "value call is not a function"
                );
                self.call(call, depth + 1)?;
            }
            Value::Integer { .. } => {}
        }
        Ok(())
    }
    fn call(&self, call: &Call, depth: usize) -> Result<()> {
        self.object(&call.target)?;
        for argument in &call.arguments {
            anyhow::ensure!(
                !matches!(
                    argument,
                    Value::Call { .. } | Value::ObjectCall { .. } | Value::Update { .. }
                ),
                "native parameter expression unsupported"
            );
            self.value(argument, depth)?;
        }
        Ok(())
    }
    fn formula(&self, formula: &Formula) -> Result<()> {
        self.value(&formula.first, 0)?;
        for (operator, value) in &formula.rest {
            anyhow::ensure!(
                (1..=9).contains(operator),
                "invalid formula operator {operator}"
            );
            self.value(value, 0)?;
        }
        Ok(())
    }
}
