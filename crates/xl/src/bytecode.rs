use crate::Value;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Register(pub usize);

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
pub struct BytecodeFunction {
    name: Arc<str>,
    parameter_count: usize,
    capture_count: usize,
    register_count: usize,
    constants: Vec<Value>,
    instructions: Vec<Instruction>,
}

impl BytecodeFunction {
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
        Self {
            name: name.into(),
            parameter_count,
            capture_count,
            register_count,
            constants,
            instructions,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn register_count(&self) -> usize {
        self.register_count
    }

    pub fn parameter_count(&self) -> usize {
        self.parameter_count
    }

    pub fn capture_count(&self) -> usize {
        self.capture_count
    }

    pub fn constants(&self) -> &[Value] {
        &self.constants
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }
}
