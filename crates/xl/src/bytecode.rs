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
}

#[derive(Clone, Debug)]
pub struct BytecodeFunction {
    name: Arc<str>,
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
        Self {
            name: name.into(),
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

    pub fn constants(&self) -> &[Value] {
        &self.constants
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }
}
