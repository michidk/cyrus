//! Script edits: statement-level changes to an actor's SOUP program, declared by a mod
//! (`[[script-patch]]`'s `edit` entries) and applied when the world's scripts are prepared.
//!
//! An edit names an existing statement by its offset in the decompiled listing (decimal) and
//! changes it: it sets one argument or the receiver of a call, replaces the conditions of an `if`,
//! deletes or replaces the statement, or inserts new statements before or after it. New statements
//! are written in the notation of the listing:
//!
//! ```text
//! FlagshipStory = 1
//! var6 = var6 + 1
//! Player.SetStrength(250)
//! if HaveItem(13) = 1 { menuAddItem("?ssg", var70, 18) }
//! goto 8107
//! ```
//!
//! Names are the installed `SOUP386.DEF` names (flags, natives; `varN` is a script variable), a
//! quoted four-letter word is the integer it encodes (a dialogue label), and `if` bodies are in
//! braces. Offsets always refer to the original script, however many statements earlier edits
//! inserted. Inserted statements live at fresh offsets after the script's last one and jump back,
//! so every original offset stays valid and a jump into a statement still reaches it.
use anyhow::{Context, Result, bail, ensure};
use cyrus_soup::Native;
use cyrus_soup::program::{
    Call, CallKind, Condition, Formula, Object, Operation, Place, Program, Step, Value,
};
use rgpre::import::soup_def::{SoupDef, SoupFunctionKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One change to the statement at offset `at` of an actor's script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptEdit {
    /// Offset of the statement in the original script (the `#offset` of a listing, in decimal).
    pub at: u32,
    #[serde(flatten)]
    pub action: EditAction,
}

/// What an edit does to its statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditAction {
    /// Runs the statements before the statement; every jump to it runs them too.
    InsertBefore(Vec<String>),
    /// Runs the statements after the statement, which must continue to its successor.
    InsertAfter(Vec<String>),
    /// Runs the statements instead of the statement, then continues where it would have.
    Replace(Vec<String>),
    /// Removes the statement; `true` is the only value.
    Delete(bool),
    /// Sets one argument of a call, counted from zero; `value` is an operand (`1`, `after_dragon`,
    /// `"?sec"`).
    SetArgument { index: usize, value: String },
    /// Sets the receiver of a call to a named object.
    SetTarget(String),
    /// Replaces the conditions of an `if` (`"Password = 0"`, `"a = 1 and var11 = 1"`).
    Condition(String),
    /// Sends an `if` (when its condition fails) or a `goto` to another statement of the original
    /// script, which may be code that nothing in it reached.
    BranchTo(u32),
}

/// Decodes the statements reachable from an offset of the script's code, for a `goto` into code
/// that nothing in the original script reaches.
pub type Decode<'a> = &'a dyn Fn(u32) -> Result<BTreeMap<u32, Step>>;

/// Applies the edits in order. `retarget` maps named receivers to others across the whole script;
/// `code_len` is the length of the script's code, so that fresh offsets lie beyond all of it.
pub fn apply(
    program: &mut Program,
    definition: &SoupDef,
    edits: &[ScriptEdit],
    retarget: &BTreeMap<String, String>,
    code_len: u32,
    decode: Decode,
) -> Result<usize> {
    let mut editor = Editor {
        program,
        definition,
        decode,
        moved: BTreeMap::new(),
        free: 0,
    };
    editor.free = editor
        .program
        .steps
        .iter()
        .flat_map(|(&offset, step)| [offset, step.next])
        .max()
        .map_or(0, |last| last + 1)
        .max(code_len);
    let mut changed = 0;
    for edit in edits {
        editor
            .apply(edit)
            .with_context(|| format!("edit at {}", edit.at))?;
        changed += 1;
    }
    for (from, to) in retarget {
        changed += editor.retarget(from, to);
    }
    Ok(changed)
}

struct Editor<'a> {
    program: &'a mut Program,
    definition: &'a SoupDef,
    decode: Decode<'a>,
    /// Original offset to where the original statement lives now, once an insertion before it
    /// moved it.
    moved: BTreeMap<u32, u32>,
    /// The next offset no statement or continuation uses.
    free: u32,
}

/// Whether execution continues at the step's `next` after the operation.
fn continues(operation: &Operation) -> bool {
    !matches!(
        operation,
        Operation::Goto { .. }
            | Operation::End { .. }
            | Operation::Return
            | Operation::Endint
            | Operation::Unsupported { .. }
    )
}

impl Editor<'_> {
    fn fresh(&mut self) -> u32 {
        let offset = self.free;
        self.free += 1;
        offset
    }

    fn locate(&self, at: u32) -> u32 {
        self.moved.get(&at).copied().unwrap_or(at)
    }

    fn step(&mut self, at: u32) -> Result<&mut Step> {
        let key = self.locate(at);
        self.program
            .steps
            .get_mut(&key)
            .with_context(|| format!("no statement at offset {at}"))
    }

    fn apply(&mut self, edit: &ScriptEdit) -> Result<()> {
        let at = edit.at;
        match &edit.action {
            EditAction::InsertBefore(statements) => {
                ensure!(
                    !self.moved.contains_key(&at),
                    "statements were already inserted before offset {at}"
                );
                let block = self.compile(statements)?;
                let original = self
                    .program
                    .steps
                    .remove(&at)
                    .with_context(|| format!("no statement at offset {at}"))?;
                let relocated = self.fresh();
                self.moved.insert(at, relocated);
                let successor = continues(&original.operation).then_some(original.next);
                let next = self.continuation(relocated, successor);
                self.program.steps.insert(
                    relocated,
                    Step {
                        next,
                        operation: original.operation,
                    },
                );
                self.emit(block, Some(at), relocated);
            }
            EditAction::InsertAfter(statements) => {
                let block = self.compile(statements)?;
                let step = self.step(at)?;
                ensure!(
                    continues(&step.operation),
                    "the statement does not continue to a successor"
                );
                let successor = step.next;
                let first = self.emit(block, None, successor);
                self.step(at)?.next = first;
            }
            EditAction::Replace(statements) => {
                let block = self.compile(statements)?;
                let successor = self.successor(at)?;
                self.emit(block, Some(self.locate(at)), successor);
            }
            EditAction::Delete(remove) => {
                ensure!(*remove, "delete must be true");
                let successor = self.successor(at)?;
                *self.step(at)? = Step {
                    next: successor.max(self.locate(at) + 1),
                    operation: Operation::Goto { target: successor },
                };
            }
            EditAction::SetArgument { index, value } => {
                let value = Parser::new(value, self)?.operand()?;
                let call = self.call_at(at)?;
                let length = call.arguments.len();
                *call
                    .arguments
                    .get_mut(*index)
                    .with_context(|| format!("the call has {length} arguments"))? = value;
            }
            EditAction::SetTarget(name) => {
                let string = self.string(name);
                self.call_at(at)?.target = Object::Named { string };
            }
            EditAction::BranchTo(target) => {
                self.reach(*target)?;
                match &mut self.step(at)?.operation {
                    Operation::If { target: jump, .. } | Operation::Goto { target: jump } => {
                        *jump = *target;
                    }
                    _ => bail!("the statement is neither an if nor a goto"),
                }
            }
            EditAction::Condition(text) => {
                let conditions = Parser::new(text, self)?.conditions()?;
                match &mut self.step(at)?.operation {
                    Operation::If {
                        conditions: current,
                        ..
                    } => *current = conditions,
                    _ => bail!("the statement is not an if"),
                }
            }
        }
        Ok(())
    }

    /// Where execution goes after the statement: its successor, which must be a statement.
    fn successor(&mut self, at: u32) -> Result<u32> {
        let next = self.step(at)?.next;
        ensure!(
            self.program.steps.contains_key(&next),
            "the statement has no successor"
        );
        Ok(next)
    }

    fn call_at(&mut self, at: u32) -> Result<&mut Call> {
        match &mut self.step(at)?.operation {
            Operation::Call { call } | Operation::ObjectCall { call } => Ok(call),
            _ => bail!("the statement is not a call"),
        }
    }

    /// The `next` of a relocated statement: a jump to its old successor, as a statement of its
    /// own because the old successor lies before the relocated one.
    fn continuation(&mut self, from: u32, successor: Option<u32>) -> u32 {
        match successor {
            Some(target) => self.trampoline(target),
            None => from + 1,
        }
    }

    fn trampoline(&mut self, target: u32) -> u32 {
        let at = self.fresh();
        self.program.steps.insert(
            at,
            Step {
                next: at + 1,
                operation: Operation::Goto { target },
            },
        );
        at
    }

    fn string(&mut self, name: &str) -> u32 {
        let index = self
            .program
            .strings
            .iter()
            .position(|known| known.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| {
                self.program.strings.push(name.to_owned());
                self.program.strings.len() - 1
            });
        u32::try_from(index).expect("script string table fits u32")
    }

    fn retarget(&mut self, from: &str, to: &str) -> usize {
        let to = self.string(to);
        let from: Vec<u32> = self
            .program
            .strings
            .iter()
            .enumerate()
            .filter(|(_, name)| name.eq_ignore_ascii_case(from))
            .map(|(index, _)| u32::try_from(index).expect("script string table fits u32"))
            .collect();
        let mut changed = 0;
        for step in self.program.steps.values_mut() {
            visit_objects(&mut step.operation, &mut |object| {
                if matches!(object, Object::Named { string } if from.contains(string)) {
                    *object = Object::Named { string: to };
                    changed += 1;
                }
            });
        }
        changed
    }

    fn compile(&mut self, statements: &[String]) -> Result<Vec<Draft>> {
        ensure!(!statements.is_empty(), "no statements");
        let mut drafts = Vec::new();
        for text in statements {
            let parsed = Parser::new(text, self)?.statements()?;
            flatten(parsed, &mut drafts);
        }
        for draft in &drafts {
            if let Operation::Goto { target } = draft.operation {
                self.reach(target)?;
            }
        }
        Ok(drafts)
    }

    /// Makes sure there is a statement at `target`, decoding the code from there when the
    /// original script never reached it.
    fn reach(&mut self, target: u32) -> Result<()> {
        if self.program.steps.contains_key(&target) {
            return Ok(());
        }
        let steps = (self.decode)(target)
            .with_context(|| format!("goto {target}: statements from that offset"))?;
        ensure!(
            steps.contains_key(&target),
            "goto {target}: no statement at that offset"
        );
        for (at, step) in steps {
            self.program.steps.entry(at).or_insert(step);
        }
        Ok(())
    }

    /// Writes the drafts as consecutive statements and ends them with a jump to `exit`. The first
    /// statement goes to `first` when given, else to a fresh offset. Returns the first's offset.
    fn emit(&mut self, drafts: Vec<Draft>, first: Option<u32>, exit: u32) -> u32 {
        let mut offsets: Vec<u32> = (0..drafts.len()).map(|_| self.fresh()).collect();
        if let Some(first) = first {
            offsets[0] = first;
        }
        let end = self.trampoline(exit);
        offsets.push(end);
        for (index, draft) in drafts.into_iter().enumerate() {
            let mut operation = draft.operation;
            if let (Operation::If { target, .. }, Some(jump)) = (&mut operation, draft.jump) {
                *target = offsets[jump];
            }
            self.program.steps.insert(
                offsets[index],
                Step {
                    next: offsets[index + 1],
                    operation,
                },
            );
        }
        offsets[0]
    }
}

/// A compiled statement before it has an offset. An `if` jumps over its body to draft `jump`.
struct Draft {
    operation: Operation,
    jump: Option<usize>,
}

enum Statement {
    If(Vec<Condition>, Vec<Statement>),
    Plain(Operation),
}

fn flatten(statements: Vec<Statement>, drafts: &mut Vec<Draft>) {
    for statement in statements {
        match statement {
            Statement::Plain(operation) => drafts.push(Draft {
                operation,
                jump: None,
            }),
            Statement::If(conditions, body) => {
                let at = drafts.len();
                drafts.push(Draft {
                    operation: Operation::If {
                        conditions,
                        target: 0,
                    },
                    jump: None,
                });
                flatten(body, drafts);
                drafts[at].jump = Some(drafts.len());
            }
        }
    }
}

fn visit_objects(operation: &mut Operation, visit: &mut dyn FnMut(&mut Object)) {
    fn call(call: &mut Call, visit: &mut dyn FnMut(&mut Object)) {
        visit(&mut call.target);
        call.arguments.iter_mut().for_each(|v| value(v, visit));
    }
    fn place(place: &mut Place, visit: &mut dyn FnMut(&mut Object)) {
        if let Place::Property { object, .. } = place {
            visit(object);
        }
    }
    fn value(item: &mut Value, visit: &mut dyn FnMut(&mut Object)) {
        match item {
            Value::Read { place: p } | Value::Update { place: p, .. } => place(p, visit),
            Value::Call { call: c } | Value::ObjectCall { call: c } => call(c, visit),
            Value::Integer { .. } | Value::String { .. } => {}
        }
    }
    match operation {
        Operation::Call { call: c } | Operation::ObjectCall { call: c } => call(c, visit),
        Operation::Assign {
            destination,
            formula,
        } => {
            place(destination, visit);
            value(&mut formula.first, visit);
            formula.rest.iter_mut().for_each(|(_, v)| value(v, visit));
        }
        Operation::Increment { destination, .. } => place(destination, visit),
        Operation::If { conditions, .. } => {
            for condition in conditions {
                value(&mut condition.lhs, visit);
                value(&mut condition.rhs, visit);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Number(i32),
    Text(String),
    Symbol(&'static str),
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let characters: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < characters.len() {
        let c = characters[at];
        if c.is_whitespace() {
            at += 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = at;
            while at < characters.len()
                && (characters[at].is_ascii_alphanumeric() || characters[at] == '_')
            {
                at += 1;
            }
            tokens.push(Token::Word(characters[start..at].iter().collect()));
        } else if c.is_ascii_digit() {
            let start = at;
            while at < characters.len() && characters[at].is_ascii_digit() {
                at += 1;
            }
            let digits: String = characters[start..at].iter().collect();
            tokens.push(Token::Number(
                digits
                    .parse()
                    .with_context(|| format!("number {digits} out of range"))?,
            ));
        } else if c == '"' {
            let start = at + 1;
            at = start;
            while at < characters.len() && characters[at] != '"' {
                at += 1;
            }
            ensure!(at < characters.len(), "unterminated string in {text:?}");
            tokens.push(Token::Text(characters[start..at].iter().collect()));
            at += 1;
        } else {
            let pair: String = characters[at..(at + 2).min(characters.len())]
                .iter()
                .collect();
            let symbol = match pair.as_str() {
                "!=" => Some("!="),
                "<=" => Some("<="),
                ">=" => Some(">="),
                _ => None,
            };
            if let Some(symbol) = symbol {
                tokens.push(Token::Symbol(symbol));
                at += 2;
                continue;
            }
            let symbol = match c {
                '(' => "(",
                ')' => ")",
                '{' => "{",
                '}' => "}",
                '.' => ".",
                ',' => ",",
                '=' => "=",
                '+' => "+",
                '-' => "-",
                '<' => "<",
                '>' => ">",
                _ => bail!("unexpected {c:?} in {text:?}"),
            };
            tokens.push(Token::Symbol(symbol));
            at += 1;
        }
    }
    Ok(tokens)
}

struct Parser<'a, 'b> {
    tokens: Vec<Token>,
    at: usize,
    editor: &'a mut Editor<'b>,
}

impl<'a, 'b> Parser<'a, 'b> {
    fn new(text: &str, editor: &'a mut Editor<'b>) -> Result<Self> {
        Ok(Self {
            tokens: tokenize(text).with_context(|| format!("statement {text:?}"))?,
            at: 0,
            editor,
        })
    }

    fn peek(&self, ahead: usize) -> Option<&Token> {
        self.tokens.get(self.at + ahead)
    }

    fn symbol(&mut self, symbol: &str) -> bool {
        if matches!(self.peek(0), Some(Token::Symbol(found)) if *found == symbol) {
            self.at += 1;
            return true;
        }
        false
    }

    fn expect(&mut self, symbol: &str) -> Result<()> {
        ensure!(
            self.symbol(symbol),
            "expected {symbol:?}, found {:?}",
            self.peek(0)
        );
        Ok(())
    }

    fn word(&mut self) -> Result<String> {
        match self.tokens.get(self.at) {
            Some(Token::Word(word)) => {
                self.at += 1;
                Ok(word.clone())
            }
            other => bail!("expected a name, found {other:?}"),
        }
    }

    fn finish<T>(&self, parsed: T) -> Result<T> {
        ensure!(
            self.at == self.tokens.len(),
            "unexpected {:?} after the end",
            self.peek(0)
        );
        Ok(parsed)
    }

    fn statements(mut self) -> Result<Vec<Statement>> {
        let mut statements = Vec::new();
        while self.at < self.tokens.len() {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }

    fn conditions(mut self) -> Result<Vec<Condition>> {
        let conditions = self.condition_list()?;
        self.finish(conditions)
    }

    fn operand(mut self) -> Result<Value> {
        let value = self.value()?;
        self.finish(value)
    }

    fn statement(&mut self) -> Result<Statement> {
        let is_word = |token: Option<&Token>, word: &str| matches!(token, Some(Token::Word(found)) if found.eq_ignore_ascii_case(word));
        if is_word(self.peek(0), "if") {
            self.at += 1;
            let conditions = self.condition_list()?;
            self.expect("{")?;
            let mut body = Vec::new();
            while !self.symbol("}") {
                ensure!(self.at < self.tokens.len(), "if body is not closed");
                body.push(self.statement()?);
            }
            return Ok(Statement::If(conditions, body));
        }
        if is_word(self.peek(0), "goto") {
            self.at += 1;
            let Some(&Token::Number(target)) = self.peek(0) else {
                bail!("goto needs an offset");
            };
            self.at += 1;
            return Ok(Statement::Plain(Operation::Goto {
                target: u32::try_from(target)?,
            }));
        }
        let call_ahead = matches!(self.peek(1), Some(Token::Symbol("(" | ".")));
        if call_ahead {
            let (target, call) = self.call()?;
            return Ok(Statement::Plain(match target {
                Object::Me => Operation::Call { call },
                _ => Operation::ObjectCall { call },
            }));
        }
        let name = self.word()?;
        let destination = self.place(&name)?;
        self.expect("=")?;
        let first = self.value()?;
        let mut rest = Vec::new();
        loop {
            let operator = if self.symbol("+") {
                1
            } else if self.symbol("-") {
                2
            } else {
                break;
            };
            rest.push((operator, self.value()?));
        }
        Ok(Statement::Plain(Operation::Assign {
            destination,
            formula: Formula { first, rest },
        }))
    }

    fn condition_list(&mut self) -> Result<Vec<Condition>> {
        let mut conditions = Vec::new();
        loop {
            let lhs = self.value()?;
            let comparison = ["=", "!=", "<", ">", "<=", ">="]
                .iter()
                .position(|symbol| self.symbol(symbol))
                .context("expected a comparison")?;
            let rhs = self.value()?;
            let conjunction = match self.peek(0) {
                Some(Token::Word(word)) if word.eq_ignore_ascii_case("and") => 1,
                Some(Token::Word(word)) if word.eq_ignore_ascii_case("or") => 2,
                _ => 0,
            };
            conditions.push(Condition {
                lhs,
                comparison: u8::try_from(comparison)?,
                rhs,
                conjunction,
            });
            if conjunction == 0 {
                return Ok(conditions);
            }
            self.at += 1;
        }
    }

    fn value(&mut self) -> Result<Value> {
        let negative = self.symbol("-");
        match self.tokens.get(self.at).cloned() {
            Some(Token::Number(number)) => {
                self.at += 1;
                Ok(Value::Integer {
                    value: if negative { -number } else { number },
                })
            }
            Some(Token::Text(text)) if !negative => {
                self.at += 1;
                let bytes: [u8; 4] = text
                    .as_bytes()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("{text:?} is not four characters"))?;
                Ok(Value::Integer {
                    value: i32::from_le_bytes(bytes),
                })
            }
            Some(Token::Word(name)) if !negative => {
                if matches!(self.peek(1), Some(Token::Symbol("(" | "."))) {
                    let (target, call) = self.call()?;
                    let call = Box::new(call);
                    return Ok(match target {
                        Object::Me => Value::Call { call },
                        _ => Value::ObjectCall { call },
                    });
                }
                self.at += 1;
                Ok(Value::Read {
                    place: self.place(&name)?,
                })
            }
            other => bail!("expected a value, found {other:?}"),
        }
    }

    fn place(&mut self, name: &str) -> Result<Place> {
        if let Some(index) = name
            .strip_prefix("var")
            .and_then(|digits| digits.parse::<u8>().ok())
        {
            return Ok(Place::Local { index });
        }
        let index = self
            .editor
            .definition
            .flags
            .iter()
            .position(|flag| flag.name.eq_ignore_ascii_case(name))
            .with_context(|| format!("{name} is neither a script variable nor a flag"))?;
        Ok(Place::Flag {
            index: u16::try_from(index)?,
        })
    }

    /// `[Receiver.]Native(arguments)`; the receiver is `Me` when absent.
    fn call(&mut self) -> Result<(Object, Call)> {
        let mut name = self.word()?;
        let mut target = Object::Me;
        if self.symbol(".") {
            target = match name.to_ascii_lowercase().as_str() {
                "me" => Object::Me,
                "player" => Object::Player,
                "camera" => Object::Camera,
                _ => Object::Named {
                    string: self.editor.string(&name),
                },
            };
            name = self.word()?;
        }
        self.expect("(")?;
        let mut arguments = Vec::new();
        if !self.symbol(")") {
            loop {
                arguments.push(self.value()?);
                if self.symbol(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        let (function_id, function) = self
            .editor
            .definition
            .functions
            .iter()
            .enumerate()
            .find(|(_, function)| function.name.eq_ignore_ascii_case(&name))
            .with_context(|| format!("{name} is not a function of the definitions"))?;
        ensure!(
            usize::from(function.param_count) == arguments.len(),
            "{} takes {} arguments, not {}",
            function.name,
            function.param_count,
            arguments.len()
        );
        let native: Native =
            serde_json::from_value(serde_json::Value::String(function.name.clone()))
                .with_context(|| format!("{} is not a known native", function.name))?;
        let kind = match function.kind {
            SoupFunctionKind::Task => CallKind::Task,
            SoupFunctionKind::Multitask => CallKind::Multitask,
            SoupFunctionKind::Function => CallKind::Function,
        };
        Ok((
            target.clone(),
            Call {
                function_id: u16::try_from(function_id)?,
                kind,
                target,
                function: native,
                arguments,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> SoupDef {
        rgpre::import::soup_def::parse_soup_def(
            "[functions]\n\
             function\tNothing\tparms 0\n\
             function\tAddLog\tparms 1\n\
             function\tSetStrength\tparms 1\n\
             function\tHaveItem\tparms 1\n\
             function\tMoveMarker\tparms 2\n\
             [refs]\n[equates]\n\
             [flags]\n\
             bool\tafter_iszara\t0\n\
             bool\tFlagshipStory\t0\n",
        )
        .expect("definition")
    }

    fn step(operation: Operation, next: u32) -> Step {
        Step { next, operation }
    }

    fn log(label: i32) -> Operation {
        Operation::Call {
            call: Call {
                function_id: 1,
                kind: CallKind::Function,
                target: Object::Me,
                function: Native::AddLog,
                arguments: vec![Value::Integer { value: label }],
            },
        }
    }

    /// `0: AddLog(1)  10: AddLog(2)  20: End`
    fn program() -> Program {
        Program {
            version: 4,
            entry: 0,
            variables: vec![0; 8],
            strings: vec!["vile".into()],
            steps: BTreeMap::from([
                (0, step(log(1), 10)),
                (10, step(log(2), 20)),
                (20, step(Operation::End { target: 0 }, 21)),
            ]),
        }
    }

    fn edit(at: u32, action: EditAction) -> ScriptEdit {
        ScriptEdit { at, action }
    }

    fn never_decodes(_: u32) -> Result<BTreeMap<u32, Step>> {
        bail!("nothing to decode")
    }

    fn run(program: &mut Program, edits: &[ScriptEdit]) -> Result<usize> {
        apply(
            program,
            &definition(),
            edits,
            &BTreeMap::new(),
            22,
            &never_decodes,
        )
    }

    /// The operations executed from `from` in order, following jumps, until an `End`.
    fn trace(program: &Program, from: u32, flags: &[(usize, i32)]) -> Vec<String> {
        let mut flags: BTreeMap<usize, i32> = flags.iter().copied().collect();
        let mut out = Vec::new();
        let mut at = from;
        for _ in 0..64 {
            let step = &program.steps[&at];
            match &step.operation {
                Operation::Goto { target } => at = *target,
                Operation::End { .. } => return out,
                Operation::If { conditions, target } => {
                    let [condition] = &conditions[..] else {
                        panic!("one condition expected");
                    };
                    let read = |value: &Value| match value {
                        Value::Integer { value } => *value,
                        Value::Read {
                            place: Place::Flag { index },
                        } => flags.get(&usize::from(*index)).copied().unwrap_or(0),
                        other => panic!("{other:?}"),
                    };
                    at = if read(&condition.lhs) == read(&condition.rhs) {
                        step.next
                    } else {
                        *target
                    };
                }
                Operation::Assign {
                    destination: Place::Flag { index },
                    formula,
                } => {
                    let Value::Integer { value } = formula.first else {
                        panic!("literal expected");
                    };
                    flags.insert(usize::from(*index), value);
                    out.push(format!("flag{index} = {value}"));
                    at = step.next;
                }
                Operation::Call { call } => {
                    out.push(format!("{}({:?})", call.function, call.arguments));
                    at = step.next;
                }
                other => panic!("{other:?}"),
            }
        }
        panic!("no End reached");
    }

    #[test]
    fn insert_after_runs_between_the_statement_and_its_successor() {
        let mut program = program();
        let edits = [edit(
            0,
            EditAction::InsertAfter(vec!["FlagshipStory = 1".into()]),
        )];
        run(&mut program, &edits).unwrap();
        program.validate().unwrap();
        assert_eq!(
            trace(&program, 0, &[]),
            [
                "AddLog([Integer { value: 1 }])",
                "flag1 = 1",
                "AddLog([Integer { value: 2 }])"
            ]
        );
    }

    #[test]
    fn insert_before_runs_for_every_jump_to_the_statement() {
        let mut program = program();
        let edits = [edit(
            10,
            EditAction::InsertBefore(vec!["after_iszara = 1".into()]),
        )];
        run(&mut program, &edits).unwrap();
        program.validate().unwrap();
        assert_eq!(trace(&program, 0, &[]).len(), 3);
        assert_eq!(
            trace(&program, 10, &[]),
            ["flag0 = 1", "AddLog([Integer { value: 2 }])"],
            "a jump to the offset reaches the inserted statement first"
        );
    }

    #[test]
    fn an_inserted_if_runs_its_body_only_when_the_condition_holds() {
        let mut program = program();
        let edits = [edit(
            0,
            EditAction::InsertAfter(vec![
                "if after_iszara = 1 { AddLog(\"$105\") FlagshipStory = 1 }".into(),
            ]),
        )];
        run(&mut program, &edits).unwrap();
        program.validate().unwrap();
        let label = i32::from_le_bytes(*b"$105");
        assert_eq!(
            trace(&program, 0, &[(0, 1)]),
            [
                "AddLog([Integer { value: 1 }])".to_owned(),
                format!("AddLog([Integer {{ value: {label} }}])"),
                "flag1 = 1".to_owned(),
                "AddLog([Integer { value: 2 }])".to_owned()
            ]
        );
        assert_eq!(trace(&program, 0, &[(0, 0)]).len(), 2);
    }

    #[test]
    fn a_goto_into_code_nothing_reached_decodes_it_and_fresh_offsets_stay_clear_of_it() {
        let mut program = program();
        let decode = |at: u32| -> Result<BTreeMap<u32, Step>> {
            assert_eq!(at, 30);
            Ok(BTreeMap::from([
                (30, step(log(9), 40)),
                (40, step(Operation::End { target: 30 }, 41)),
            ]))
        };
        apply(
            &mut program,
            &definition(),
            &[edit(0, EditAction::InsertAfter(vec!["goto 30".into()]))],
            &BTreeMap::new(),
            45,
            &decode,
        )
        .unwrap();
        program.validate().unwrap();
        assert_eq!(
            trace(&program, 0, &[]),
            [
                "AddLog([Integer { value: 1 }])",
                "AddLog([Integer { value: 9 }])"
            ]
        );
        let inserted: Vec<u32> = program.steps.keys().copied().filter(|&k| k > 40).collect();
        assert!(inserted.iter().all(|&k| k >= 45), "{inserted:?}");
    }

    #[test]
    fn replace_and_delete_keep_the_flow_through_the_offset() {
        let mut program = program();
        run(
            &mut program,
            &[
                edit(0, EditAction::Replace(vec!["FlagshipStory = 1".into()])),
                edit(10, EditAction::Delete(true)),
            ],
        )
        .unwrap();
        program.validate().unwrap();
        assert_eq!(trace(&program, 0, &[]), ["flag1 = 1"]);
    }

    #[test]
    fn several_edits_name_original_offsets() {
        let mut program = program();
        run(
            &mut program,
            &[
                edit(
                    10,
                    EditAction::InsertBefore(vec!["after_iszara = 1".into()]),
                ),
                edit(
                    10,
                    EditAction::SetArgument {
                        index: 0,
                        value: "7".into(),
                    },
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            trace(&program, 0, &[]),
            [
                "AddLog([Integer { value: 1 }])",
                "flag0 = 1",
                "AddLog([Integer { value: 7 }])"
            ],
            "the argument edit reaches the original statement, not the inserted one"
        );
    }

    #[test]
    fn a_receiver_is_set_and_retargeted_by_name() {
        let mut program = program();
        program.steps.insert(
            0,
            step(
                Operation::ObjectCall {
                    call: Call {
                        function_id: 4,
                        kind: CallKind::Task,
                        target: Object::Named { string: 0 },
                        function: Native::MoveMarker,
                        arguments: vec![Value::Integer { value: 1 }, Value::Integer { value: 2 }],
                    },
                },
                10,
            ),
        );
        let mut again = program.clone();
        apply(
            &mut program,
            &definition(),
            &[],
            &BTreeMap::from([("VILE".to_owned(), "viledog".to_owned())]),
            22,
            &never_decodes,
        )
        .unwrap();
        assert_eq!(program.strings, ["vile", "viledog"]);
        let Operation::ObjectCall { call } = &program.steps[&0].operation else {
            panic!("an object call");
        };
        assert_eq!(call.target, Object::Named { string: 1 });
        run(
            &mut again,
            &[edit(0, EditAction::SetTarget("Dramball".into()))],
        )
        .unwrap();
        let Operation::ObjectCall { call } = &again.steps[&0].operation else {
            panic!("an object call");
        };
        assert_eq!(call.target, Object::Named { string: 1 });
        assert_eq!(again.strings[1], "Dramball");
    }

    #[test]
    fn branch_to_redirects_an_if_into_code_nothing_reached() {
        let mut program = program();
        program.steps.insert(
            0,
            step(
                Operation::If {
                    conditions: vec![Condition {
                        lhs: Value::Read {
                            place: Place::Flag { index: 0 },
                        },
                        comparison: 0,
                        rhs: Value::Integer { value: 1 },
                        conjunction: 0,
                    }],
                    target: 20,
                },
                10,
            ),
        );
        let decode = |at: u32| -> Result<BTreeMap<u32, Step>> {
            assert_eq!(at, 30);
            Ok(BTreeMap::from([
                (30, step(log(9), 40)),
                (40, step(Operation::End { target: 0 }, 41)),
            ]))
        };
        let edits = [edit(0, EditAction::BranchTo(30))];
        apply(
            &mut program,
            &definition(),
            &edits,
            &BTreeMap::new(),
            45,
            &decode,
        )
        .unwrap();
        program.validate().unwrap();
        assert_eq!(
            trace(&program, 0, &[(0, 1)]).len(),
            1,
            "the true branch is unchanged"
        );
        assert_eq!(
            trace(&program, 0, &[(0, 0)]),
            ["AddLog([Integer { value: 9 }])"],
            "the false branch runs the decoded code"
        );
        let error = format!(
            "{:#}",
            run(&mut self::program(), &[edit(10, EditAction::BranchTo(0))]).unwrap_err()
        );
        assert!(error.contains("neither an if nor a goto"), "{error}");
    }

    #[test]
    fn the_conditions_of_an_if_are_replaced() {
        let mut program = program();
        program.steps.insert(
            0,
            step(
                Operation::If {
                    conditions: vec![Condition {
                        lhs: Value::Read {
                            place: Place::Flag { index: 0 },
                        },
                        comparison: 2,
                        rhs: Value::Integer { value: 1 },
                        conjunction: 0,
                    }],
                    target: 20,
                },
                10,
            ),
        );
        run(
            &mut program,
            &[edit(
                0,
                EditAction::Condition("after_iszara = 1 and var3 >= 2".into()),
            )],
        )
        .unwrap();
        let Operation::If { conditions, target } = &program.steps[&0].operation else {
            panic!("an if");
        };
        assert_eq!(*target, 20);
        let shape: Vec<_> = conditions
            .iter()
            .map(|c| (c.comparison, c.conjunction))
            .collect();
        assert_eq!(shape, [(0, 1), (5, 0)]);
    }

    #[test]
    fn mistakes_name_what_is_wrong() {
        let cases: [(EditAction, &str); 6] = [
            (
                EditAction::InsertAfter(vec!["NoSuchFlag = 1".into()]),
                "neither a script variable",
            ),
            (
                EditAction::InsertAfter(vec!["Nothing(1)".into()]),
                "takes 0 arguments",
            ),
            (
                EditAction::InsertAfter(vec!["AddLog(\"abc\")".into()]),
                "four characters",
            ),
            (
                EditAction::InsertAfter(vec!["after_iszara = ".into()]),
                "expected a value",
            ),
            (EditAction::SetTarget("x".into()), "not a call"),
            (
                EditAction::Condition("after_iszara = 1".into()),
                "not an if",
            ),
        ];
        for (action, expected) in cases {
            // Statement 20 is the End: calls and ifs are not there, and nothing follows it.
            let at = if expected.contains("not a call") || expected.contains("not an if") {
                20
            } else {
                0
            };
            let error = format!(
                "{:#}",
                run(&mut program(), &[edit(at, action)]).unwrap_err()
            );
            assert!(error.contains(expected), "{expected}: {error}");
            assert!(error.contains(&format!("edit at {at}")), "{error}");
        }
        let error = format!(
            "{:#}",
            run(
                &mut program(),
                &[edit(
                    20,
                    EditAction::InsertAfter(vec!["FlagshipStory = 1".into()])
                )]
            )
            .unwrap_err()
        );
        assert!(error.contains("does not continue"), "{error}");
    }

    #[test]
    fn a_manifest_writes_edits_as_toml() {
        #[derive(Deserialize)]
        struct Manifest {
            edit: Vec<ScriptEdit>,
        }
        let manifest: Manifest = toml::from_str(
            r#"
            [[edit]]
            at = 16
            insert-after = ['Player.SetStrength(250)']
            [[edit]]
            at = 90
            set-argument = { index = 0, value = '"?sec"' }
            [[edit]]
            at = 40
            delete = true
            "#,
        )
        .unwrap();
        assert_eq!(manifest.edit.len(), 3);
        assert_eq!(
            manifest.edit[1].action,
            EditAction::SetArgument {
                index: 0,
                value: "\"?sec\"".into()
            }
        );
    }
}
