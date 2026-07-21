use crate::bytecode::{BytecodeFunction, Instruction, Register};
use crate::lir::RegisterId;
use crate::value::{Atom, BuiltinAtom, Closure, Dict, NativeError, Prototype, Shape, Value};
use crate::{Diagnostic, Origin, SourceDatabase};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write;
use std::sync::{Arc, Weak};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueKind {
    Int,
    Float,
    String,
    Bytes,
    Dict,
    Array,
    Atom,
    Tuple,
    Func,
}

#[derive(Clone, Copy)]
pub struct ValueRef<'a> {
    value: &'a Value,
}

impl<'a> ValueRef<'a> {
    pub fn kind(self) -> ValueKind {
        match self.value {
            Value::Int(_) => ValueKind::Int,
            Value::Float(_) => ValueKind::Float,
            Value::String(_) => ValueKind::String,
            Value::Bytes(_) => ValueKind::Bytes,
            Value::Dict(_) => ValueKind::Dict,
            Value::Array(_) => ValueKind::Array,
            Value::Atom(_) => ValueKind::Atom,
            Value::Tuple(_) => ValueKind::Tuple,
            Value::Func(_) => ValueKind::Func,
        }
    }

    pub fn as_atom(self) -> Option<&'a str> {
        match self.value {
            Value::Atom(atom) => Some(atom.name()),
            _ => None,
        }
    }

    pub fn as_int(self) -> Option<i64> {
        match self.value {
            Value::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(self) -> Option<&'a str> {
        match self.value {
            Value::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn sequence_len(self) -> Option<usize> {
        match self.value {
            Value::Array(values) | Value::Tuple(values) => Some(values.len()),
            _ => None,
        }
    }

    pub fn sequence_get(self, index: usize) -> Option<ValueRef<'a>> {
        match self.value {
            Value::Array(values) | Value::Tuple(values) => {
                values.get(index).map(|value| ValueRef { value })
            }
            _ => None,
        }
    }

    pub fn dict_fields(self) -> Option<&'a [String]> {
        match self.value {
            Value::Dict(dict) => Some(dict.shape().fields()),
            _ => None,
        }
    }

    pub fn dict_get(self, field: &str) -> Option<ValueRef<'a>> {
        match self.value {
            Value::Dict(dict) => dict.get(field).map(|value| ValueRef { value }),
            _ => None,
        }
    }
}

pub struct CallContext<'vm, 'stack> {
    vm: &'vm mut Vm,
    stack: &'stack mut Vec<Option<Value>>,
    base: usize,
    argument_count: usize,
    upvalue_base: usize,
    upvalue_count: usize,
    result: RegisterId,
}

impl<'vm, 'stack> CallContext<'vm, 'stack> {
    fn new(
        vm: &'vm mut Vm,
        stack: &'stack mut Vec<Option<Value>>,
        arguments: Vec<Value>,
        upvalues: &[Value],
    ) -> Self {
        let base = stack.len();
        let argument_count = arguments.len();
        stack.extend(arguments.into_iter().map(Some));
        let upvalue_base = argument_count;
        stack.extend(upvalues.iter().cloned().map(Some));
        let upvalue_count = upvalues.len();
        let result_index = argument_count + upvalue_count;
        stack.push(None);
        Self {
            vm,
            stack,
            base,
            argument_count,
            upvalue_base,
            upvalue_count,
            result: RegisterId(
                u32::try_from(result_index).expect("native register count exceeds u32"),
            ),
        }
    }

    pub fn argument(&self, index: usize) -> Result<RegisterId, NativeError> {
        if index >= self.argument_count {
            return Err(NativeError::new(format!(
                "argument {index} is out of bounds"
            )));
        }
        Ok(RegisterId(
            u32::try_from(index).expect("argument count exceeds u32"),
        ))
    }

    pub const fn result(&self) -> RegisterId {
        self.result
    }

    pub fn upvalue(&self, index: usize) -> Result<RegisterId, NativeError> {
        if index >= self.upvalue_count {
            return Err(NativeError::new(format!(
                "upvalue {index} is out of bounds"
            )));
        }
        Ok(RegisterId(
            u32::try_from(self.upvalue_base + index).expect("native register count exceeds u32"),
        ))
    }

    pub fn value(&self, register: RegisterId) -> Result<ValueRef<'_>, NativeError> {
        let index = usize::try_from(register.0)
            .map_err(|_| NativeError::new("register does not fit this platform"))?;
        self.stack
            .get(self.base + index)
            .and_then(Option::as_ref)
            .map(|value| ValueRef { value })
            .ok_or_else(|| NativeError::new(format!("register {} is not initialized", register.0)))
    }

    pub fn scratch(&mut self) -> RegisterId {
        let register = RegisterId(
            u32::try_from(self.stack.len() - self.base).expect("native register count exceeds u32"),
        );
        self.stack.push(None);
        register
    }

    pub fn set_atom(&mut self, destination: RegisterId, name: &str) -> Result<(), NativeError> {
        self.set(destination, Value::Atom(atom_from_name(name)))
    }

    pub fn set_int(&mut self, destination: RegisterId, value: i64) -> Result<(), NativeError> {
        self.set(destination, Value::Int(value))
    }

    pub fn set_float(&mut self, destination: RegisterId, value: f64) -> Result<(), NativeError> {
        self.set(destination, Value::Float(value))
    }

    pub fn set_none(&mut self, destination: RegisterId) -> Result<(), NativeError> {
        self.set(destination, Value::none())
    }

    pub fn set_string(
        &mut self,
        destination: RegisterId,
        value: impl Into<String>,
    ) -> Result<(), NativeError> {
        self.set(destination, Value::string(value.into()))
    }

    pub fn copy(&mut self, destination: RegisterId, source: RegisterId) -> Result<(), NativeError> {
        let value = self.owned(source)?;
        self.set(destination, value)
    }

    pub fn make_array(
        &mut self,
        destination: RegisterId,
        items: &[RegisterId],
    ) -> Result<(), NativeError> {
        let values = items
            .iter()
            .map(|item| self.owned(*item))
            .collect::<Result<Vec<_>, _>>()?;
        self.set(destination, Value::Array(values.into()))
    }

    pub fn make_tuple(
        &mut self,
        destination: RegisterId,
        items: &[RegisterId],
    ) -> Result<(), NativeError> {
        let values = items
            .iter()
            .map(|item| self.owned(*item))
            .collect::<Result<Vec<_>, _>>()?;
        self.set(destination, Value::Tuple(values.into()))
    }

    pub fn make_dict(
        &mut self,
        destination: RegisterId,
        fields: &[(String, RegisterId)],
    ) -> Result<(), NativeError> {
        let entries = fields
            .iter()
            .map(|(name, register)| Ok((name.clone(), self.owned(*register)?)))
            .collect::<Result<Vec<_>, NativeError>>()?;
        let value = self.vm.make_dict(entries).map_err(NativeError::new)?;
        self.set(destination, value)
    }

    fn owned(&self, register: RegisterId) -> Result<Value, NativeError> {
        let index = usize::try_from(register.0)
            .map_err(|_| NativeError::new("register does not fit this platform"))?;
        self.stack
            .get(self.base + index)
            .and_then(Option::as_ref)
            .cloned()
            .ok_or_else(|| NativeError::new(format!("register {} is not initialized", register.0)))
    }

    fn set(&mut self, register: RegisterId, value: Value) -> Result<(), NativeError> {
        let index = usize::try_from(register.0)
            .map_err(|_| NativeError::new("register does not fit this platform"))?;
        let slot = self
            .stack
            .get_mut(self.base + index)
            .ok_or_else(|| NativeError::new(format!("register {} is out of bounds", register.0)))?;
        *slot = Some(value);
        Ok(())
    }

    fn take_result(self) -> Result<Value, NativeError> {
        let index = usize::try_from(self.result.0).expect("result register fits usize");
        let result = self.stack[self.base + index]
            .take()
            .ok_or_else(|| NativeError::new("native function did not write its result register"));
        self.stack.truncate(self.base);
        result
    }
}

fn atom_from_name(name: &str) -> Atom {
    match name {
        "None" => Atom::builtin(BuiltinAtom::None),
        "Some" => Atom::builtin(BuiltinAtom::Some),
        "Ok" => Atom::builtin(BuiltinAtom::Ok),
        "Err" => Atom::builtin(BuiltinAtom::Err),
        "True" => Atom::builtin(BuiltinAtom::True),
        "False" => Atom::builtin(BuiltinAtom::False),
        _ => Atom::named(name),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeErrorKind {
    BudgetExceeded,
    DivisionByZero,
    IntegerOverflow,
    InvalidBytecode,
    MissingField,
    NoPatternMatched,
    TypeMismatch,
    UnsupportedEquality,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
    pub function: String,
    pub instruction: usize,
    pub trace: Vec<RuntimeFrame>,
    rendered: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeFrame {
    pub function: String,
    pub instruction: usize,
    pub origin: Option<Origin>,
}

impl RuntimeError {
    pub fn origin(&self) -> Option<Origin> {
        self.trace.first().and_then(|frame| frame.origin)
    }

    pub fn with_sources(mut self, sources: &SourceDatabase) -> Self {
        let location = self.origin().and_then(|origin| match origin {
            Origin::Source(location) => Some(location),
            Origin::Synthetic { derived_from } => derived_from,
        });
        if let Some(location) = location {
            self.rendered =
                Some(sources.render(&Diagnostic::error(self.message.clone(), location)));
        }
        self
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(rendered) = &self.rendered {
            return formatter.write_str(rendered);
        }
        write!(
            formatter,
            "{} at {}:{}",
            self.message, self.function, self.instruction
        )
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Default)]
struct ShapeInterner {
    shapes: HashMap<Vec<String>, Weak<Shape>>,
}

impl ShapeInterner {
    fn intern(&mut self, fields: Vec<String>) -> Arc<Shape> {
        if let Some(shape) = self.shapes.get(&fields).and_then(Weak::upgrade) {
            return shape;
        }
        let shape = Arc::new(Shape::from_sorted_fields(fields.clone()));
        self.shapes.insert(fields, Arc::downgrade(&shape));
        shape
    }
}

#[derive(Default)]
pub struct Vm {
    shapes: ShapeInterner,
}

struct ExecutionFrame {
    function: Arc<BytecodeFunction>,
    base: usize,
    pc: usize,
    return_destination: Option<Register>,
}

impl Vm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn make_dict(
        &mut self,
        entries: impl IntoIterator<Item = (String, Value)>,
    ) -> Result<Value, String> {
        let mut entries = entries.into_iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        if entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err("Dict contains a duplicate field".into());
        }
        let (fields, values): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
        let shape = self.shapes.intern(fields);
        Ok(Value::Dict(Dict::new(shape, values)))
    }

    pub fn execute(
        &mut self,
        function: &BytecodeFunction,
        instruction_budget: usize,
    ) -> Result<Value, RuntimeError> {
        self.execute_with_args(function, &[], instruction_budget)
    }

    pub fn execute_with_args(
        &mut self,
        function: &BytecodeFunction,
        arguments: &[Value],
        instruction_budget: usize,
    ) -> Result<Value, RuntimeError> {
        let mut remaining = instruction_budget;
        self.execute_frame(function, arguments, &[], &mut remaining)
    }

    #[allow(clippy::needless_borrow)]
    fn execute_frame(
        &mut self,
        function: &BytecodeFunction,
        arguments: &[Value],
        captures: &[Value],
        remaining: &mut usize,
    ) -> Result<Value, RuntimeError> {
        let mut stack = Vec::new();
        let mut frames = vec![make_execution_frame(
            Arc::new(function.clone()),
            arguments,
            captures,
            None,
            &mut stack,
        )?];

        let mut result = (|| -> Result<Value, RuntimeError> {
            loop {
                let function_arc = frames
                    .last()
                    .expect("execution has at least one frame")
                    .function
                    .clone();
                let function = function_arc.as_ref();
                let pc = frames.last().expect("execution frame").pc;
                if *remaining == 0 {
                    return Err(error(
                        RuntimeErrorKind::BudgetExceeded,
                        "instruction budget exhausted",
                        function,
                        pc,
                    ));
                }
                *remaining -= 1;

                let instruction = function.instructions().get(pc).ok_or_else(|| {
                    error(
                        RuntimeErrorKind::InvalidBytecode,
                        "instruction pointer is out of bounds",
                        function,
                        pc,
                    )
                })?;
                let frame = frames.last().expect("execution frame");
                let base = frame.base;
                let end = base + frame.function.register_count();
                let mut registers = &mut stack[base..end];

                match instruction {
                    Instruction::LoadConst { dst, constant } => {
                        let value =
                            function
                                .constants()
                                .get(*constant)
                                .cloned()
                                .ok_or_else(|| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        format!("constant index {constant} is out of bounds"),
                                        function,
                                        pc,
                                    )
                                })?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::Move { dst, src } => {
                        let value = read_register(&registers, *src, function, pc)?.clone();
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::Add { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Add,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::Subtract { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Subtract,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::Multiply { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Multiply,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::Divide { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Divide,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::Negate { dst, src } => {
                        let value = match read_register(&registers, *src, function, pc)? {
                            Value::Int(value) => {
                                Value::Int(value.checked_neg().ok_or_else(|| {
                                    error(
                                        RuntimeErrorKind::IntegerOverflow,
                                        "integer negation overflowed",
                                        function,
                                        pc,
                                    )
                                })?)
                            }
                            Value::Float(value) => Value::Float(-value),
                            value => {
                                return Err(type_error("numeric value", value, function, pc));
                            }
                        };
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::Equal { dst, left, right } => {
                        let equal = values_equal(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, Value::bool(equal), function, pc)?;
                    }
                    Instruction::LessThan { dst, left, right } => {
                        let left = read_register(&registers, *left, function, pc)?;
                        let right = read_register(&registers, *right, function, pc)?;
                        let less = match (left, right) {
                            (Value::Int(left), Value::Int(right)) => left < right,
                            (Value::Float(left), Value::Float(right)) => left < right,
                            _ => return Err(numeric_type_error(left, right, function, pc)),
                        };
                        write_register(&mut registers, *dst, Value::bool(less), function, pc)?;
                    }
                    Instruction::MakeArray { dst, items } => {
                        let values = read_many(&registers, items, function, pc)?;
                        write_register(
                            &mut registers,
                            *dst,
                            Value::Array(values.into()),
                            function,
                            pc,
                        )?;
                    }
                    Instruction::MakeTuple { dst, items } => {
                        let values = read_many(&registers, items, function, pc)?;
                        write_register(
                            &mut registers,
                            *dst,
                            Value::Tuple(values.into()),
                            function,
                            pc,
                        )?;
                    }
                    Instruction::InterpolateString { dst, parts } => {
                        let mut length = 0usize;
                        for part in parts {
                            let value = read_register(&registers, *part, function, pc)?;
                            length += match value {
                                Value::String(value) => value.len(),
                                Value::Int(value) => decimal_length(*value),
                                Value::Atom(value) => value.name().len(),
                                value => {
                                    return Err(type_error(
                                        "String, Int, or Atom interpolation value",
                                        value,
                                        function,
                                        pc,
                                    ));
                                }
                            };
                        }
                        let mut output = String::with_capacity(length);
                        for part in parts {
                            let value = read_register(&registers, *part, function, pc)?;
                            match value {
                                Value::String(value) => output.push_str(value),
                                Value::Int(value) => {
                                    write!(output, "{value}")
                                        .expect("writing to String cannot fail");
                                }
                                Value::Atom(value) => output.push_str(value.name()),
                                _ => unreachable!("interpolation values were validated"),
                            }
                        }
                        write_register(&mut registers, *dst, Value::string(output), function, pc)?;
                    }
                    Instruction::MakeDict { dst, fields } => {
                        let mut entries = fields
                            .iter()
                            .map(|(field, register)| {
                                Ok((
                                    field.clone(),
                                    read_register(&registers, *register, function, pc)?.clone(),
                                ))
                            })
                            .collect::<Result<Vec<_>, RuntimeError>>()?;
                        entries.sort_by(|left, right| left.0.cmp(&right.0));
                        if entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                            return Err(error(
                                RuntimeErrorKind::InvalidBytecode,
                                "Dict contains a duplicate field",
                                function,
                                pc,
                            ));
                        }
                        let (fields, values): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
                        let shape = self.shapes.intern(fields);
                        write_register(
                            &mut registers,
                            *dst,
                            Value::Dict(Dict::new(shape, values)),
                            function,
                            pc,
                        )?;
                    }
                    Instruction::GetField { dst, dict, field } => {
                        let dict = read_register(&registers, *dict, function, pc)?;
                        let Value::Dict(dict) = dict else {
                            return Err(type_error("Dict", dict, function, pc));
                        };
                        let value = dict.get(field).cloned().ok_or_else(|| {
                            error(
                                RuntimeErrorKind::MissingField,
                                format!("Dict has no field {field:?}"),
                                function,
                                pc,
                            )
                        })?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::TupleLengthEquals { dst, value, length } => {
                        let matches = matches!(
                            read_register(&registers, *value, function, pc)?,
                            Value::Tuple(items) if items.len() == *length
                        );
                        write_register(&mut registers, *dst, Value::bool(matches), function, pc)?;
                    }
                    Instruction::GetTuple { dst, tuple, index } => {
                        let tuple = read_register(&registers, *tuple, function, pc)?;
                        let Value::Tuple(items) = tuple else {
                            return Err(type_error("Tuple", tuple, function, pc));
                        };
                        let value = items.get(*index).cloned().ok_or_else(|| {
                            error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("tuple index {index} is out of bounds"),
                                function,
                                pc,
                            )
                        })?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Instruction::MakeClosure {
                        dst,
                        function: closure_function,
                        captures,
                    } => {
                        let captures = read_many(&registers, captures, function, pc)?;
                        let closure = Closure::new(closure_function.clone(), captures);
                        write_register(
                            &mut registers,
                            *dst,
                            Value::Func(Arc::new(closure)),
                            function,
                            pc,
                        )?;
                    }
                    Instruction::Call {
                        dst,
                        callee,
                        arguments,
                    } => {
                        let callee = read_register(&registers, *callee, function, pc)?.clone();
                        let Value::Func(callable) = callee else {
                            return Err(type_error("Func", &callee, function, pc));
                        };
                        let arguments = read_many(&registers, arguments, function, pc)?;
                        match callable.prototype() {
                            Prototype::Bytecode(callee_function) => {
                                frames.last_mut().expect("caller frame").pc += 1;
                                let next = make_execution_frame(
                                    callee_function.clone(),
                                    &arguments,
                                    callable.upvalues(),
                                    Some(*dst),
                                    &mut stack,
                                )?;
                                frames.push(next);
                                continue;
                            }
                            Prototype::Native(native) => {
                                if arguments.len() != native.arity() {
                                    return Err(error(
                                        RuntimeErrorKind::TypeMismatch,
                                        format!(
                                            "expected {} arguments, got {}",
                                            native.arity(),
                                            arguments.len()
                                        ),
                                        function,
                                        pc,
                                    ));
                                }
                                if *remaining == 0 {
                                    return Err(error(
                                        RuntimeErrorKind::BudgetExceeded,
                                        "instruction budget exhausted",
                                        function,
                                        pc,
                                    ));
                                }
                                *remaining -= 1;
                                let _ = registers;
                                let mut context = CallContext::new(
                                    self,
                                    &mut stack,
                                    arguments,
                                    callable.upvalues(),
                                );
                                (native.callback())(&mut context).map_err(|native_error| {
                                    error(
                                        RuntimeErrorKind::TypeMismatch,
                                        format!("{}: {}", native.name(), native_error.message),
                                        function,
                                        pc,
                                    )
                                })?;
                                let value = context.take_result().map_err(|native_error| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        format!("{}: {}", native.name(), native_error.message),
                                        function,
                                        pc,
                                    )
                                })?;
                                write_register(&mut stack[base..end], *dst, value, function, pc)?;
                                frames.last_mut().expect("execution frame").pc += 1;
                                continue;
                            }
                        }
                    }
                    Instruction::Jump { target } => {
                        validate_jump(*target, function, pc)?;
                        frames.last_mut().expect("execution frame").pc = *target;
                        continue;
                    }
                    Instruction::JumpIfFalse { condition, target } => {
                        match read_register(&registers, *condition, function, pc)? {
                            Value::Atom(Atom::Builtin(BuiltinAtom::True)) => {}
                            Value::Atom(Atom::Builtin(BuiltinAtom::False)) => {
                                validate_jump(*target, function, pc)?;
                                frames.last_mut().expect("execution frame").pc = *target;
                                continue;
                            }
                            value => {
                                return Err(type_error("'True or 'False", value, function, pc));
                            }
                        }
                    }
                    Instruction::Return { src } => {
                        let value = read_register(&registers, *src, function, pc)?.clone();
                        let destination =
                            frames.last().expect("execution frame").return_destination;
                        let completed = frames.pop().expect("execution frame");
                        stack.truncate(completed.base);
                        let Some(destination) = destination else {
                            return Ok(value);
                        };
                        let caller = frames.last_mut().expect("return has a caller");
                        let caller_function = caller.function.clone();
                        let caller_end = caller.base + caller.function.register_count();
                        write_register(
                            &mut stack[caller.base..caller_end],
                            destination,
                            value,
                            &caller_function,
                            caller.pc.saturating_sub(1),
                        )?;
                        continue;
                    }
                    Instruction::Fail { message } => {
                        return Err(error(
                            RuntimeErrorKind::NoPatternMatched,
                            message,
                            function,
                            pc,
                        ));
                    }
                }
                frames.last_mut().expect("execution frame").pc += 1;
            }
        })();
        if let Err(runtime_error) = &mut result {
            let skip_active = frames.last().is_some_and(|frame| {
                runtime_error.trace.first().is_some_and(|trace| {
                    trace.function == frame.function.name() && trace.instruction == frame.pc
                })
            });
            for frame in frames.iter().rev().skip(usize::from(skip_active)) {
                let instruction = frame.pc.saturating_sub(1);
                runtime_error.trace.push(RuntimeFrame {
                    function: frame.function.name().to_owned(),
                    instruction,
                    origin: frame.function.origin_at(instruction),
                });
            }
        }
        result
    }
}

fn make_execution_frame(
    function: Arc<BytecodeFunction>,
    arguments: &[Value],
    captures: &[Value],
    return_destination: Option<Register>,
    stack: &mut Vec<Option<Value>>,
) -> Result<ExecutionFrame, RuntimeError> {
    if arguments.len() != function.parameter_count() {
        return Err(error(
            RuntimeErrorKind::TypeMismatch,
            format!(
                "expected {} arguments, got {}",
                function.parameter_count(),
                arguments.len()
            ),
            &function,
            0,
        ));
    }
    if captures.len() != function.capture_count() {
        return Err(error(
            RuntimeErrorKind::InvalidBytecode,
            "closure capture count does not match function signature",
            &function,
            0,
        ));
    }
    let base = stack.len();
    stack.resize(base + function.register_count(), None);
    for (index, value) in arguments.iter().chain(captures).enumerate() {
        let Some(register) = stack.get_mut(base + index) else {
            return Err(error(
                RuntimeErrorKind::InvalidBytecode,
                "function signature exceeds its register count",
                &function,
                0,
            ));
        };
        *register = Some(value.clone());
    }
    Ok(ExecutionFrame {
        function,
        base,
        pc: 0,
        return_destination,
    })
}

fn decimal_length(value: i64) -> usize {
    let magnitude = value.unsigned_abs();
    let digits = if magnitude == 0 {
        1
    } else {
        magnitude.ilog10() as usize + 1
    };
    digits + usize::from(value.is_negative())
}

fn read_register<'a>(
    registers: &'a [Option<Value>],
    register: Register,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<&'a Value, RuntimeError> {
    registers
        .get(register.0)
        .ok_or_else(|| {
            error(
                RuntimeErrorKind::InvalidBytecode,
                format!("register {} is out of bounds", register.0),
                function,
                pc,
            )
        })?
        .as_ref()
        .ok_or_else(|| {
            error(
                RuntimeErrorKind::InvalidBytecode,
                format!("register {} is uninitialized", register.0),
                function,
                pc,
            )
        })
}

fn write_register(
    registers: &mut [Option<Value>],
    register: Register,
    value: Value,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<(), RuntimeError> {
    let slot = registers.get_mut(register.0).ok_or_else(|| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            format!("register {} is out of bounds", register.0),
            function,
            pc,
        )
    })?;
    *slot = Some(value);
    Ok(())
}

fn read_many(
    registers: &[Option<Value>],
    items: &[Register],
    function: &BytecodeFunction,
    pc: usize,
) -> Result<Vec<Value>, RuntimeError> {
    items
        .iter()
        .map(|register| read_register(registers, *register, function, pc).cloned())
        .collect()
}

#[derive(Clone, Copy)]
enum NumericOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

fn numeric_binary(
    left: &Value,
    right: &Value,
    operation: NumericOperation,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<Value, RuntimeError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => {
            if matches!(operation, NumericOperation::Divide) && *right == 0 {
                return Err(error(
                    RuntimeErrorKind::DivisionByZero,
                    "integer division by zero",
                    function,
                    pc,
                ));
            }
            let value = match operation {
                NumericOperation::Add => left.checked_add(*right),
                NumericOperation::Subtract => left.checked_sub(*right),
                NumericOperation::Multiply => left.checked_mul(*right),
                NumericOperation::Divide => left.checked_div(*right),
            }
            .ok_or_else(|| {
                error(
                    RuntimeErrorKind::IntegerOverflow,
                    "integer arithmetic overflowed",
                    function,
                    pc,
                )
            })?;
            Ok(Value::Int(value))
        }
        (Value::Float(left), Value::Float(right)) => Ok(Value::Float(match operation {
            NumericOperation::Add => left + right,
            NumericOperation::Subtract => left - right,
            NumericOperation::Multiply => left * right,
            NumericOperation::Divide => left / right,
        })),
        _ => Err(numeric_type_error(left, right, function, pc)),
    }
}

fn numeric_type_error(
    left: &Value,
    right: &Value,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    error(
        RuntimeErrorKind::TypeMismatch,
        format!(
            "numeric operands must have the same type, got {} and {}",
            left.type_name(),
            right.type_name()
        ),
        function,
        pc,
    )
}

fn values_equal(
    left: &Value,
    right: &Value,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<bool, RuntimeError> {
    match (left, right) {
        (Value::Func(_), _) | (_, Value::Func(_)) => Err(error(
            RuntimeErrorKind::UnsupportedEquality,
            "functions cannot be compared for equality",
            function,
            pc,
        )),
        (Value::Int(left), Value::Int(right)) => Ok(left == right),
        (Value::Float(left), Value::Float(right)) => Ok(left == right),
        (Value::String(left), Value::String(right)) => Ok(left == right),
        (Value::Bytes(left), Value::Bytes(right)) => Ok(left == right),
        (Value::Atom(left), Value::Atom(right)) => Ok(left == right),
        (Value::Array(left), Value::Array(right)) | (Value::Tuple(left), Value::Tuple(right)) => {
            sequences_equal(left, right, function, pc)
        }
        (Value::Dict(left), Value::Dict(right)) => {
            if left.shape().fields() != right.shape().fields() {
                return Ok(false);
            }
            sequences_equal(left.values(), right.values(), function, pc)
        }
        _ => Ok(false),
    }
}

fn sequences_equal(
    left: &[Value],
    right: &[Value],
    function: &BytecodeFunction,
    pc: usize,
) -> Result<bool, RuntimeError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left.iter().zip(right) {
        if !values_equal(left, right, function, pc)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_jump(
    target: usize,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<(), RuntimeError> {
    if target >= function.instructions().len() {
        return Err(error(
            RuntimeErrorKind::InvalidBytecode,
            format!("jump target {target} is out of bounds"),
            function,
            pc,
        ));
    }
    Ok(())
}

fn type_error(
    expected: &str,
    actual: &Value,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    error(
        RuntimeErrorKind::TypeMismatch,
        format!("expected {expected}, got {}", actual.type_name()),
        function,
        pc,
    )
}

fn error(
    kind: RuntimeErrorKind,
    message: impl Into<String>,
    function: &BytecodeFunction,
    instruction: usize,
) -> RuntimeError {
    RuntimeError {
        kind,
        message: message.into(),
        function: function.name().to_owned(),
        instruction,
        trace: vec![RuntimeFrame {
            function: function.name().to_owned(),
            instruction,
            origin: function.origin_at(instruction),
        }],
        rendered: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BytecodeFunction, Instruction, NativeFunction, Register};

    fn run(
        vm: &mut Vm,
        registers: usize,
        constants: Vec<Value>,
        instructions: Vec<Instruction>,
    ) -> Result<Value, RuntimeError> {
        vm.execute(
            &BytecodeFunction::new("test", registers, constants, instructions),
            1_000,
        )
    }

    #[test]
    fn executes_arithmetic_and_branching() {
        let result = run(
            &mut Vm::new(),
            4,
            vec![Value::Int(20), Value::Int(22), Value::Int(0)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::LoadConst {
                    dst: Register(1),
                    constant: 1,
                },
                Instruction::Add {
                    dst: Register(2),
                    left: Register(0),
                    right: Register(1),
                },
                Instruction::LoadConst {
                    dst: Register(3),
                    constant: 2,
                },
                Instruction::LessThan {
                    dst: Register(3),
                    left: Register(3),
                    right: Register(2),
                },
                Instruction::JumpIfFalse {
                    condition: Register(3),
                    target: 7,
                },
                Instruction::Return { src: Register(2) },
                Instruction::Return { src: Register(0) },
            ],
        )
        .unwrap();
        assert!(matches!(result, Value::Int(42)));
    }

    #[test]
    fn canonicalizes_and_interns_dict_shapes() {
        let result = run(
            &mut Vm::new(),
            4,
            vec![Value::Int(1), Value::Int(2)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::LoadConst {
                    dst: Register(1),
                    constant: 1,
                },
                Instruction::MakeDict {
                    dst: Register(2),
                    fields: vec![("b".into(), Register(1)), ("a".into(), Register(0))],
                },
                Instruction::MakeDict {
                    dst: Register(3),
                    fields: vec![("a".into(), Register(1)), ("b".into(), Register(0))],
                },
                Instruction::MakeTuple {
                    dst: Register(0),
                    items: vec![Register(2), Register(3)],
                },
                Instruction::Return { src: Register(0) },
            ],
        )
        .unwrap();
        let Value::Tuple(dicts) = result else {
            panic!("expected tuple");
        };
        let (Value::Dict(left), Value::Dict(right)) = (&dicts[0], &dicts[1]) else {
            panic!("expected Dict values");
        };
        assert_eq!(left.shape().fields(), &["a".to_owned(), "b".to_owned()]);
        assert!(left.shares_shape_with(right));
        assert!(matches!(left.get("a"), Some(Value::Int(1))));
    }

    #[test]
    fn constructs_and_reads_structured_values() {
        let result = run(
            &mut Vm::new(),
            5,
            vec![Value::Atom(Atom::builtin(BuiltinAtom::Ok)), Value::Int(42)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::LoadConst {
                    dst: Register(1),
                    constant: 1,
                },
                Instruction::MakeTuple {
                    dst: Register(2),
                    items: vec![Register(0), Register(1)],
                },
                Instruction::MakeArray {
                    dst: Register(3),
                    items: vec![Register(1), Register(2)],
                },
                Instruction::MakeDict {
                    dst: Register(4),
                    fields: vec![("result".into(), Register(3))],
                },
                Instruction::GetField {
                    dst: Register(0),
                    dict: Register(4),
                    field: "result".into(),
                },
                Instruction::Return { src: Register(0) },
            ],
        )
        .unwrap();
        assert_eq!(result.to_string(), "[42, ('Ok, 42)]");
    }

    #[test]
    fn reports_integer_errors_consistently() {
        let overflow = run(
            &mut Vm::new(),
            3,
            vec![Value::Int(i64::MAX), Value::Int(1)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::LoadConst {
                    dst: Register(1),
                    constant: 1,
                },
                Instruction::Add {
                    dst: Register(2),
                    left: Register(0),
                    right: Register(1),
                },
                Instruction::Return { src: Register(2) },
            ],
        )
        .unwrap_err();
        assert_eq!(overflow.kind, RuntimeErrorKind::IntegerOverflow);

        let division = run(
            &mut Vm::new(),
            3,
            vec![Value::Int(1), Value::Int(0)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::LoadConst {
                    dst: Register(1),
                    constant: 1,
                },
                Instruction::Divide {
                    dst: Register(2),
                    left: Register(0),
                    right: Register(1),
                },
                Instruction::Return { src: Register(2) },
            ],
        )
        .unwrap_err();
        assert_eq!(division.kind, RuntimeErrorKind::DivisionByZero);
    }

    #[test]
    fn rejects_non_boolean_conditions() {
        let error = run(
            &mut Vm::new(),
            1,
            vec![Value::Int(1)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::JumpIfFalse {
                    condition: Register(0),
                    target: 2,
                },
                Instruction::Return { src: Register(0) },
            ],
        )
        .unwrap_err();
        assert_eq!(error.kind, RuntimeErrorKind::TypeMismatch);
    }

    #[test]
    fn enforces_budget_and_rejects_malformed_bytecode() {
        let loop_function =
            BytecodeFunction::new("loop", 0, vec![], vec![Instruction::Jump { target: 0 }]);
        let error = Vm::new().execute(&loop_function, 5).unwrap_err();
        assert_eq!(error.kind, RuntimeErrorKind::BudgetExceeded);

        let invalid = BytecodeFunction::new(
            "invalid",
            0,
            vec![],
            vec![Instruction::Return { src: Register(9) }],
        );
        let error = Vm::new().execute(&invalid, 5).unwrap_err();
        assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    }

    fn native_add_upvalue(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
        let argument = context
            .value(context.argument(0)?)?
            .as_int()
            .ok_or_else(|| NativeError::new("expected Int argument"))?;
        let upvalue = context
            .value(context.upvalue(0)?)?
            .as_int()
            .ok_or_else(|| NativeError::new("expected Int upvalue"))?;
        context.set_int(context.result(), argument + upvalue)
    }

    #[test]
    fn native_closures_use_register_context_and_upvalues() {
        let native = NativeFunction::new("add_upvalue", 1, native_add_upvalue);
        let closure = Closure::native_with_upvalues(native, vec![Value::Int(40)]);
        let function = BytecodeFunction::new(
            "test",
            3,
            vec![Value::Func(Arc::new(closure)), Value::Int(2)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::LoadConst {
                    dst: Register(1),
                    constant: 1,
                },
                Instruction::Call {
                    dst: Register(2),
                    callee: Register(0),
                    arguments: vec![Register(1)],
                },
                Instruction::Return { src: Register(2) },
            ],
        );
        assert!(matches!(
            Vm::new().execute(&function, 20).unwrap(),
            Value::Int(42)
        ));
    }

    #[test]
    fn nested_calls_use_explicit_vm_frames() {
        let mut function = Arc::new(BytecodeFunction::new(
            "leaf",
            1,
            vec![Value::Int(7)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        ));
        for depth in 0..512 {
            let closure = Value::Func(Arc::new(Closure::new(function, Vec::new())));
            function = Arc::new(BytecodeFunction::new(
                format!("frame{depth}"),
                2,
                vec![closure],
                vec![
                    Instruction::LoadConst {
                        dst: Register(0),
                        constant: 0,
                    },
                    Instruction::Call {
                        dst: Register(1),
                        callee: Register(0),
                        arguments: vec![],
                    },
                    Instruction::Return { src: Register(1) },
                ],
            ));
        }
        assert!(matches!(
            Vm::new().execute(&function, 2_000).unwrap(),
            Value::Int(7)
        ));
    }
}
