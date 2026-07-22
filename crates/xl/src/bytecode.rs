use crate::{Origin, Value};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Register(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ValueLinkId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TextLinkId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProtoLinkId(pub usize);

#[derive(Clone, Debug)]
pub enum Instruction {
    LoadConst {
        dst: Register,
        constant: usize,
    },
    Move {
        dst: Register,
        src: Register,
    },
    Add {
        dst: Register,
        left: Register,
        right: Register,
    },
    Subtract {
        dst: Register,
        left: Register,
        right: Register,
    },
    Multiply {
        dst: Register,
        left: Register,
        right: Register,
    },
    Divide {
        dst: Register,
        left: Register,
        right: Register,
    },
    Negate {
        dst: Register,
        src: Register,
    },
    Equal {
        dst: Register,
        left: Register,
        right: Register,
    },
    LessThan {
        dst: Register,
        left: Register,
        right: Register,
    },
    MakeArray {
        dst: Register,
        items: Vec<Register>,
    },
    MakeTuple {
        dst: Register,
        items: Vec<Register>,
    },
    InterpolateString {
        dst: Register,
        parts: Vec<Register>,
    },
    MakeDict {
        dst: Register,
        fields: Vec<(String, Register)>,
    },
    GetField {
        dst: Register,
        dict: Register,
        field: String,
    },
    TupleLengthEquals {
        dst: Register,
        value: Register,
        length: usize,
    },
    GetTuple {
        dst: Register,
        tuple: Register,
        index: usize,
    },
    MakeClosure {
        dst: Register,
        function: Arc<BytecodeFunction>,
        captures: Vec<Register>,
    },
    Call {
        dst: Register,
        callee: Register,
        arguments: Vec<Register>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        condition: Register,
        target: usize,
    },
    Return {
        src: Register,
    },
    Fail {
        message: String,
    },
}

#[derive(Clone, Debug)]
pub enum Opcode {
    LoadConst {
        dst: Register,
        value: ValueLinkId,
    },
    Move {
        dst: Register,
        src: Register,
    },
    Add {
        dst: Register,
        left: Register,
        right: Register,
    },
    Subtract {
        dst: Register,
        left: Register,
        right: Register,
    },
    Multiply {
        dst: Register,
        left: Register,
        right: Register,
    },
    Divide {
        dst: Register,
        left: Register,
        right: Register,
    },
    Negate {
        dst: Register,
        src: Register,
    },
    Equal {
        dst: Register,
        left: Register,
        right: Register,
    },
    LessThan {
        dst: Register,
        left: Register,
        right: Register,
    },
    MakeArray {
        dst: Register,
        items: Vec<Register>,
    },
    MakeTuple {
        dst: Register,
        items: Vec<Register>,
    },
    InterpolateString {
        dst: Register,
        parts: Vec<Register>,
    },
    MakeDict {
        dst: Register,
        fields: Vec<(TextLinkId, Register)>,
    },
    GetField {
        dst: Register,
        dict: Register,
        field: TextLinkId,
    },
    TupleLengthEquals {
        dst: Register,
        value: Register,
        length: usize,
    },
    GetTuple {
        dst: Register,
        tuple: Register,
        index: usize,
    },
    MakeClosure {
        dst: Register,
        prototype: ProtoLinkId,
        captures: Vec<Register>,
    },
    Call {
        dst: Register,
        callee: Register,
        arguments: Vec<Register>,
    },
    Jump {
        target: usize,
    },
    JumpIfFalse {
        condition: Register,
        target: usize,
    },
    Return {
        src: Register,
    },
    Fail {
        message: String,
    },
}

#[derive(Clone, Debug)]
pub struct FuncByteCode {
    name: Arc<str>,
    parameter_count: usize,
    capture_count: usize,
    register_count: usize,
    instructions: Vec<Opcode>,
    debug_origins: Vec<DebugOriginRange>,
}

#[derive(Clone, Debug, Default)]
pub struct LinkingTable {
    values: Vec<Value>,
    text: Vec<Arc<str>>,
    prototypes: Vec<Arc<BytecodeFunction>>,
}

impl LinkingTable {
    pub(crate) fn values(&self) -> &[Value] {
        &self.values
    }

    pub(crate) fn text(&self) -> &[Arc<str>] {
        &self.text
    }

    pub(crate) fn prototypes(&self) -> &[Arc<BytecodeFunction>] {
        &self.prototypes
    }
}

#[derive(Clone, Debug)]
pub struct BytecodeFunction {
    code: Arc<FuncByteCode>,
    links: LinkingTable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebugOriginRange {
    pub start: usize,
    pub end: usize,
    pub origin: Origin,
}

impl BytecodeFunction {
    pub(crate) fn from_linked_code(code: Arc<FuncByteCode>) -> Self {
        Self {
            code,
            links: LinkingTable::default(),
        }
    }

    pub(crate) fn from_linked_parts(
        code: Arc<FuncByteCode>,
        values: Vec<Value>,
        text: Vec<Arc<str>>,
        prototypes: Vec<Arc<BytecodeFunction>>,
    ) -> Self {
        Self {
            code,
            links: LinkingTable {
                values,
                text,
                prototypes,
            },
        }
    }

    pub fn new(
        name: impl Into<Arc<str>>,
        register_count: usize,
        constants: Vec<Value>,
        instructions: Vec<Instruction>,
    ) -> Self {
        Self::with_signature(name, 0, 0, register_count, constants, instructions)
    }

    pub fn with_signature(
        name: impl Into<Arc<str>>,
        parameter_count: usize,
        capture_count: usize,
        register_count: usize,
        constants: Vec<Value>,
        instructions: Vec<Instruction>,
    ) -> Self {
        Self::assembled(
            name,
            parameter_count,
            capture_count,
            register_count,
            constants,
            instructions,
            Vec::new(),
        )
    }

    pub(crate) fn assembled(
        name: impl Into<Arc<str>>,
        parameter_count: usize,
        capture_count: usize,
        register_count: usize,
        constants: Vec<Value>,
        instructions: Vec<Instruction>,
        debug_origins: Vec<DebugOriginRange>,
    ) -> Self {
        let mut links = LinkingTable {
            values: constants,
            ..LinkingTable::default()
        };
        let instructions = instructions
            .into_iter()
            .map(|instruction| link_instruction(instruction, &mut links))
            .collect();
        Self {
            code: Arc::new(FuncByteCode {
                name: name.into(),
                parameter_count,
                capture_count,
                register_count,
                instructions,
                debug_origins,
            }),
            links,
        }
    }

    pub fn code(&self) -> &Arc<FuncByteCode> {
        &self.code
    }

    pub fn links(&self) -> &LinkingTable {
        &self.links
    }

    pub fn value_link(&self, id: ValueLinkId) -> Option<&Value> {
        self.links.values.get(id.0)
    }

    pub fn text_link(&self, id: TextLinkId) -> Option<&str> {
        self.links.text.get(id.0).map(AsRef::as_ref)
    }

    pub fn prototype_link(&self, id: ProtoLinkId) -> Option<&Arc<BytecodeFunction>> {
        self.links.prototypes.get(id.0)
    }

    pub fn shares_code_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.code, &other.code)
    }

    pub fn relink(&self) -> Self {
        self.relink_with(Clone::clone, |text| text.into(), Arc::clone)
    }

    pub fn relink_with(
        &self,
        mut value: impl FnMut(&Value) -> Value,
        mut text: impl FnMut(&str) -> Arc<str>,
        mut prototype: impl FnMut(&Arc<BytecodeFunction>) -> Arc<BytecodeFunction>,
    ) -> Self {
        Self {
            code: Arc::clone(&self.code),
            links: LinkingTable {
                values: self.links.values.iter().map(&mut value).collect(),
                text: self.links.text.iter().map(|item| text(item)).collect(),
                prototypes: self.links.prototypes.iter().map(&mut prototype).collect(),
            },
        }
    }

    pub fn name(&self) -> &str {
        &self.code.name
    }

    pub fn register_count(&self) -> usize {
        self.code.register_count
    }

    pub fn parameter_count(&self) -> usize {
        self.code.parameter_count
    }

    pub fn capture_count(&self) -> usize {
        self.code.capture_count
    }

    pub fn constants(&self) -> &[Value] {
        &self.links.values
    }

    pub fn instructions(&self) -> &[Opcode] {
        &self.code.instructions
    }

    pub fn origin_at(&self, instruction: usize) -> Option<Origin> {
        self.code
            .debug_origins
            .iter()
            .find(|range| range.start <= instruction && instruction < range.end)
            .map(|range| range.origin)
    }

    pub fn debug_origins(&self) -> &[DebugOriginRange] {
        &self.code.debug_origins
    }
}

fn link_instruction(instruction: Instruction, links: &mut LinkingTable) -> Opcode {
    let text = |value: String, links: &mut LinkingTable| {
        if let Some(index) = links.text.iter().position(|candidate| **candidate == value) {
            return TextLinkId(index);
        }
        let id = TextLinkId(links.text.len());
        links.text.push(value.into());
        id
    };
    match instruction {
        Instruction::LoadConst { dst, constant } => Opcode::LoadConst {
            dst,
            value: ValueLinkId(constant),
        },
        Instruction::Move { dst, src } => Opcode::Move { dst, src },
        Instruction::Add { dst, left, right } => Opcode::Add { dst, left, right },
        Instruction::Subtract { dst, left, right } => Opcode::Subtract { dst, left, right },
        Instruction::Multiply { dst, left, right } => Opcode::Multiply { dst, left, right },
        Instruction::Divide { dst, left, right } => Opcode::Divide { dst, left, right },
        Instruction::Negate { dst, src } => Opcode::Negate { dst, src },
        Instruction::Equal { dst, left, right } => Opcode::Equal { dst, left, right },
        Instruction::LessThan { dst, left, right } => Opcode::LessThan { dst, left, right },
        Instruction::MakeArray { dst, items } => Opcode::MakeArray { dst, items },
        Instruction::MakeTuple { dst, items } => Opcode::MakeTuple { dst, items },
        Instruction::InterpolateString { dst, parts } => Opcode::InterpolateString { dst, parts },
        Instruction::MakeDict { dst, fields } => Opcode::MakeDict {
            dst,
            fields: fields
                .into_iter()
                .map(|(field, register)| (text(field, links), register))
                .collect(),
        },
        Instruction::GetField { dst, dict, field } => Opcode::GetField {
            dst,
            dict,
            field: text(field, links),
        },
        Instruction::TupleLengthEquals { dst, value, length } => {
            Opcode::TupleLengthEquals { dst, value, length }
        }
        Instruction::GetTuple { dst, tuple, index } => Opcode::GetTuple { dst, tuple, index },
        Instruction::MakeClosure {
            dst,
            function,
            captures,
        } => {
            let prototype = ProtoLinkId(links.prototypes.len());
            links.prototypes.push(function);
            Opcode::MakeClosure {
                dst,
                prototype,
                captures,
            }
        }
        Instruction::Call {
            dst,
            callee,
            arguments,
        } => Opcode::Call {
            dst,
            callee,
            arguments,
        },
        Instruction::Jump { target } => Opcode::Jump { target },
        Instruction::JumpIfFalse { condition, target } => Opcode::JumpIfFalse { condition, target },
        Instruction::Return { src } => Opcode::Return { src },
        Instruction::Fail { message } => Opcode::Fail { message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_heap_dependent_operands_out_of_the_code_blob() {
        let child = Arc::new(BytecodeFunction::new(
            "child",
            1,
            vec![Value::Int(1)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        ));
        let function = BytecodeFunction::new(
            "parent",
            3,
            vec![Value::string("constant")],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::GetField {
                    dst: Register(1),
                    dict: Register(0),
                    field: "name".into(),
                },
                Instruction::MakeClosure {
                    dst: Register(2),
                    function: child,
                    captures: vec![],
                },
                Instruction::Return { src: Register(2) },
            ],
        );

        assert!(matches!(
            function.instructions()[0],
            Opcode::LoadConst {
                value: ValueLinkId(0),
                ..
            }
        ));
        assert!(matches!(
            function.instructions()[1],
            Opcode::GetField {
                field: TextLinkId(0),
                ..
            }
        ));
        assert!(matches!(
            function.instructions()[2],
            Opcode::MakeClosure {
                prototype: ProtoLinkId(0),
                ..
            }
        ));
        assert_eq!(function.text_link(TextLinkId(0)), Some("name"));
        assert_eq!(
            function.prototype_link(ProtoLinkId(0)).unwrap().name(),
            "child"
        );
    }

    #[test]
    fn relinking_shares_heap_independent_code() {
        let function = BytecodeFunction::new(
            "value",
            1,
            vec![Value::string("linked")],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        );
        let relinked = function.relink();
        assert!(function.shares_code_with(&relinked));
        assert_eq!(relinked.constants().len(), 1);
    }
}
