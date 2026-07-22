use crate::bytecode::{BytecodeFunction, Opcode, Register};
use crate::heap::{Handle, Heap, HeapView, Object, PersistentValue, RuntimeValue, publish_root};
use crate::lir::RegisterId;
use crate::value::{BuiltinAtom, Dict, NativeError, NativeLimit, Shape, Value};
use crate::{Diagnostic, Origin, SourceDatabase};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write;
use std::sync::{Arc, Weak};

const MAX_CALL_DEPTH: usize = 1_024;
const MAX_STACK_SLOTS: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Quota {
    pub fuel: usize,
    pub stack_slots: usize,
    pub allocation_bytes: u64,
}

impl Quota {
    pub const fn new(fuel: usize, stack_slots: usize, allocation_bytes: u64) -> Self {
        Self {
            fuel,
            stack_slots,
            allocation_bytes,
        }
    }

    pub const fn with_fuel(fuel: usize) -> Self {
        Self::new(fuel, MAX_STACK_SLOTS, u64::MAX)
    }
}

#[derive(Debug)]
pub struct QuotaAccount {
    quota: Quota,
    remaining_fuel: usize,
    requested_allocation_bytes: u64,
}

impl QuotaAccount {
    pub fn new(quota: Quota) -> Self {
        Self {
            remaining_fuel: quota.fuel,
            quota,
            requested_allocation_bytes: 0,
        }
    }

    pub const fn quota(&self) -> Quota {
        self.quota
    }

    pub const fn remaining_fuel(&self) -> usize {
        self.remaining_fuel
    }

    pub const fn requested_allocation_bytes(&self) -> u64 {
        self.requested_allocation_bytes
    }

    fn stack_limit(&self) -> usize {
        self.quota.stack_slots.min(MAX_STACK_SLOTS)
    }

    fn charge_allocation(&mut self, bytes: u64) -> Result<(), ()> {
        let requested = self
            .requested_allocation_bytes
            .checked_add(bytes)
            .ok_or(())?;
        if requested > self.quota.allocation_bytes {
            return Err(());
        }
        self.requested_allocation_bytes = requested;
        Ok(())
    }
}

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
    value: RuntimeValue,
    view: HeapView<'a>,
}

impl<'a> ValueRef<'a> {
    pub fn kind(self) -> ValueKind {
        match self.value {
            RuntimeValue::Int(_) => ValueKind::Int,
            RuntimeValue::Float(_) => ValueKind::Float,
            RuntimeValue::ShortString(_) | RuntimeValue::String(_) => ValueKind::String,
            RuntimeValue::Bytes(_) => ValueKind::Bytes,
            RuntimeValue::Dict(_) => ValueKind::Dict,
            RuntimeValue::Array(_) => ValueKind::Array,
            RuntimeValue::BuiltinAtom(_) | RuntimeValue::Atom(_) => ValueKind::Atom,
            RuntimeValue::Tuple(_) => ValueKind::Tuple,
            RuntimeValue::Func(_) => ValueKind::Func,
        }
    }

    pub fn as_atom(self) -> Option<&'a str> {
        match self.value {
            RuntimeValue::BuiltinAtom(atom) => Some(atom.name()),
            RuntimeValue::Atom(id) => self.view.text(id).ok(),
            _ => None,
        }
    }

    pub fn as_int(self) -> Option<i64> {
        match self.value {
            RuntimeValue::Int(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(self) -> Option<&'a str> {
        match self.value {
            RuntimeValue::ShortString(id) => self.view.text(id).ok(),
            RuntimeValue::String(handle) => match self.view.object(handle).ok()? {
                Object::String(value) => Some(value),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn sequence_len(self) -> Option<usize> {
        match self.value {
            RuntimeValue::Array(handle) => self.view.sequence(handle, false).ok().map(<[_]>::len),
            RuntimeValue::Tuple(handle) => self.view.sequence(handle, true).ok().map(<[_]>::len),
            _ => None,
        }
    }

    pub fn sequence_get(self, index: usize) -> Option<ValueRef<'a>> {
        let values = match self.value {
            RuntimeValue::Array(handle) => self.view.sequence(handle, false).ok()?,
            RuntimeValue::Tuple(handle) => self.view.sequence(handle, true).ok()?,
            _ => return None,
        };
        values.get(index).copied().map(|value| ValueRef {
            value,
            view: self.view,
        })
    }

    pub fn dict_fields(self) -> Option<Vec<&'a str>> {
        match self.value {
            RuntimeValue::Dict(handle) => self.view.dict_fields(handle).ok(),
            _ => None,
        }
    }

    pub fn dict_get(self, field: &str) -> Option<ValueRef<'a>> {
        let RuntimeValue::Dict(handle) = self.value else {
            return None;
        };
        self.view
            .dict_get_text(handle, field)
            .ok()
            .flatten()
            .map(|value| ValueRef {
                value,
                view: self.view,
            })
    }
}

pub struct CallContext<'vm, 'stack> {
    current: &'vm mut Heap,
    background: Option<&'vm Heap>,
    stack: &'stack mut Vec<Option<RuntimeValue>>,
    account: &'stack mut QuotaAccount,
    base: usize,
    argument_count: usize,
    upvalue_base: usize,
    upvalue_count: usize,
    result: RegisterId,
}

impl<'vm, 'stack> CallContext<'vm, 'stack> {
    fn new(
        current: &'vm mut Heap,
        background: Option<&'vm Heap>,
        stack: &'stack mut Vec<Option<RuntimeValue>>,
        account: &'stack mut QuotaAccount,
        arguments: Vec<RuntimeValue>,
        upvalues: &[RuntimeValue],
    ) -> Result<Self, NativeError> {
        let base = stack.len();
        let argument_count = arguments.len();
        let window_size = argument_count
            .checked_add(upvalues.len())
            .and_then(|size| size.checked_add(1))
            .ok_or_else(|| NativeError::stack_limit("native stack window is too large"))?;
        let end = base
            .checked_add(window_size)
            .ok_or_else(|| NativeError::stack_limit("XL stack size overflowed"))?;
        if end > account.stack_limit() || window_size > u32::MAX as usize {
            return Err(NativeError::stack_limit(
                "native call exceeds the XL stack-slot limit",
            ));
        }
        stack.extend(arguments.into_iter().map(Some));
        let upvalue_base = argument_count;
        stack.extend(upvalues.iter().cloned().map(Some));
        let upvalue_count = upvalues.len();
        let result_index = argument_count + upvalue_count;
        stack.push(None);
        Ok(Self {
            current,
            background,
            stack,
            account,
            base,
            argument_count,
            upvalue_base,
            upvalue_count,
            result: RegisterId(
                u32::try_from(result_index)
                    .map_err(|_| NativeError::stack_limit("native register count exceeds u32"))?,
            ),
        })
    }

    pub fn argument(&self, index: usize) -> Result<RegisterId, NativeError> {
        if index >= self.argument_count {
            return Err(NativeError::new(format!(
                "argument {index} is out of bounds"
            )));
        }
        Ok(RegisterId(u32::try_from(index).map_err(|_| {
            NativeError::stack_limit("argument register exceeds u32")
        })?))
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
            u32::try_from(self.upvalue_base + index)
                .map_err(|_| NativeError::stack_limit("upvalue register exceeds u32"))?,
        ))
    }

    pub fn value(&self, register: RegisterId) -> Result<ValueRef<'_>, NativeError> {
        let index = usize::try_from(register.0)
            .map_err(|_| NativeError::new("register does not fit this platform"))?;
        self.stack
            .get(self.base + index)
            .and_then(Option::as_ref)
            .copied()
            .map(|value| ValueRef {
                value,
                view: HeapView {
                    current: self.current,
                    background: self.background,
                },
            })
            .ok_or_else(|| NativeError::new(format!("register {} is not initialized", register.0)))
    }

    pub fn scratch(&mut self) -> Result<RegisterId, NativeError> {
        if self.stack.len() >= self.account.stack_limit() {
            return Err(NativeError::stack_limit(
                "native scratch register exceeds the XL stack-slot limit",
            ));
        }
        let register = RegisterId(
            u32::try_from(self.stack.len() - self.base)
                .map_err(|_| NativeError::stack_limit("native scratch register exceeds u32"))?,
        );
        self.stack.push(None);
        Ok(register)
    }

    pub fn set_atom(&mut self, destination: RegisterId, name: &str) -> Result<(), NativeError> {
        let value = self.current.atom(self.background, name);
        self.set(destination, value)
    }

    pub fn set_int(&mut self, destination: RegisterId, value: i64) -> Result<(), NativeError> {
        self.set(destination, RuntimeValue::Int(value))
    }

    pub fn set_float(&mut self, destination: RegisterId, value: f64) -> Result<(), NativeError> {
        self.set(destination, RuntimeValue::Float(value))
    }

    pub fn set_none(&mut self, destination: RegisterId) -> Result<(), NativeError> {
        self.set(destination, RuntimeValue::BuiltinAtom(BuiltinAtom::None))
    }

    pub fn set_string(
        &mut self,
        destination: RegisterId,
        value: impl Into<String>,
    ) -> Result<(), NativeError> {
        let value = value.into();
        self.charge_allocation(value.len())?;
        let value = self.current.string(self.background, &value);
        self.set(destination, value)
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
        self.charge_sequence(values.len())?;
        let value = RuntimeValue::Array(self.current.allocate(Object::Array(values.into())));
        self.set(destination, value)
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
        self.charge_sequence(values.len())?;
        let value = RuntimeValue::Tuple(self.current.allocate(Object::Tuple(values.into())));
        self.set(destination, value)
    }

    pub fn make_dict(
        &mut self,
        destination: RegisterId,
        fields: &[(String, RegisterId)],
    ) -> Result<(), NativeError> {
        let mut entries = fields
            .iter()
            .map(|(name, register)| Ok((name.as_str(), self.owned(*register)?)))
            .collect::<Result<Vec<_>, NativeError>>()?;
        self.charge_dict(&entries)?;
        entries.sort_by(|left, right| left.0.cmp(right.0));
        if entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(NativeError::new("Dict contains a duplicate field"));
        }
        let (fields, values): (Vec<_>, Vec<_>) = entries
            .into_iter()
            .map(|(field, value)| (self.current.intern(field), value))
            .unzip();
        let shape = self.current.intern_shape(fields);
        let value = RuntimeValue::Dict(self.current.allocate(Object::Dict {
            shape,
            values: values.into(),
        }));
        self.set(destination, value)
    }

    fn charge_sequence(&mut self, count: usize) -> Result<(), NativeError> {
        let bytes = logical_value_bytes(count)?;
        self.account
            .charge_allocation(bytes)
            .map_err(|()| NativeError::allocation_limit("native allocation quota exceeded"))
    }

    fn charge_dict(&mut self, entries: &[(&str, RuntimeValue)]) -> Result<(), NativeError> {
        let field_bytes = entries.iter().try_fold(0u64, |total, (field, _)| {
            total.checked_add(field.len() as u64).ok_or_else(|| {
                NativeError::allocation_limit("native Dict allocation size overflowed")
            })
        })?;
        let value_bytes = logical_value_bytes(entries.len())?;
        let bytes = field_bytes.checked_add(value_bytes).ok_or_else(|| {
            NativeError::allocation_limit("native Dict allocation size overflowed")
        })?;
        self.account
            .charge_allocation(bytes)
            .map_err(|()| NativeError::allocation_limit("native allocation quota exceeded"))
    }

    fn charge_allocation(&mut self, bytes: usize) -> Result<(), NativeError> {
        let bytes = u64::try_from(bytes)
            .map_err(|_| NativeError::allocation_limit("native allocation size overflowed"))?;
        self.account
            .charge_allocation(bytes)
            .map_err(|()| NativeError::allocation_limit("native allocation quota exceeded"))
    }

    fn owned(&self, register: RegisterId) -> Result<RuntimeValue, NativeError> {
        let index = usize::try_from(register.0)
            .map_err(|_| NativeError::new("register does not fit this platform"))?;
        self.stack
            .get(self.base + index)
            .and_then(Option::as_ref)
            .copied()
            .ok_or_else(|| NativeError::new(format!("register {} is not initialized", register.0)))
    }

    fn set(&mut self, register: RegisterId, value: RuntimeValue) -> Result<(), NativeError> {
        let index = usize::try_from(register.0)
            .map_err(|_| NativeError::new("register does not fit this platform"))?;
        let slot = self
            .stack
            .get_mut(self.base + index)
            .ok_or_else(|| NativeError::new(format!("register {} is out of bounds", register.0)))?;
        *slot = Some(value);
        Ok(())
    }

    fn take_result(self) -> Result<RuntimeValue, NativeError> {
        let index = usize::try_from(self.result.0)
            .map_err(|_| NativeError::stack_limit("result register does not fit usize"))?;
        let slot = self
            .base
            .checked_add(index)
            .and_then(|slot| self.stack.get_mut(slot))
            .ok_or_else(|| NativeError::new("native result register is out of bounds"))?;
        let result = slot
            .take()
            .ok_or_else(|| NativeError::new("native function did not write its result register"));
        self.stack.truncate(self.base);
        result
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeErrorKind {
    FuelExhausted,
    AllocationQuotaExceeded,
    CallDepthExceeded,
    DivisionByZero,
    IntegerOverflow,
    InvalidBytecode,
    MissingField,
    NoPatternMatched,
    StackLimitExceeded,
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
    trace_includes_active_frame: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeFrame {
    pub function: String,
    pub instruction: usize,
    pub origin: Option<Origin>,
}

impl RuntimeError {
    pub(crate) fn from_heap_error(
        function: &BytecodeFunction,
        heap_error: crate::heap::HeapError,
    ) -> Self {
        error(
            RuntimeErrorKind::InvalidBytecode,
            heap_error.to_string(),
            function,
            0,
        )
    }

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
    prototype: Handle,
    base: usize,
    pc: usize,
    return_destination: Option<Register>,
}

pub(crate) struct ExecutionArena {
    heap: Heap,
    root: RuntimeValue,
}

impl ExecutionArena {
    pub(crate) fn export(&self, world: &Heap) -> Result<Value, crate::heap::HeapError> {
        HeapView {
            current: &self.heap,
            background: Some(world),
        }
        .export_value(self.root)
    }

    pub(crate) fn publish(
        self,
        world: &mut Heap,
    ) -> Result<PersistentValue, crate::heap::HeapError> {
        publish_root(world, &self.heap, self.root)
    }
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
        evaluation_fuel: usize,
    ) -> Result<Value, RuntimeError> {
        self.execute_with_args(function, &[], evaluation_fuel)
    }

    pub fn execute_with_args(
        &mut self,
        function: &BytecodeFunction,
        arguments: &[Value],
        evaluation_fuel: usize,
    ) -> Result<Value, RuntimeError> {
        self.execute_with_quota_and_args(function, arguments, Quota::with_fuel(evaluation_fuel))
    }

    pub fn execute_with_quota(
        &mut self,
        function: &BytecodeFunction,
        quota: Quota,
    ) -> Result<Value, RuntimeError> {
        self.execute_with_quota_and_args(function, &[], quota)
    }

    pub fn execute_with_quota_and_args(
        &mut self,
        function: &BytecodeFunction,
        arguments: &[Value],
        quota: Quota,
    ) -> Result<Value, RuntimeError> {
        let mut account = QuotaAccount::new(quota);
        self.execute_with_account(function, arguments, &mut account)
    }

    pub(crate) fn execute_with_account(
        &mut self,
        function: &BytecodeFunction,
        arguments: &[Value],
        account: &mut QuotaAccount,
    ) -> Result<Value, RuntimeError> {
        let background = Heap::persistent();
        let arena = self.execute_frame(
            &background,
            &HashMap::new(),
            function,
            arguments,
            &[],
            account,
        )?;
        arena.export(&background).map_err(|heap_error| {
            error(
                RuntimeErrorKind::InvalidBytecode,
                heap_error.to_string(),
                function,
                0,
            )
        })
    }

    pub(crate) fn execute_in_background(
        &mut self,
        background: &Heap,
        externals: &HashMap<String, PersistentValue>,
        function: &BytecodeFunction,
        arguments: &[Value],
        account: &mut QuotaAccount,
    ) -> Result<ExecutionArena, RuntimeError> {
        self.execute_frame(background, externals, function, arguments, &[], account)
    }

    #[allow(clippy::needless_borrow)]
    fn execute_frame(
        &mut self,
        background: &Heap,
        externals: &HashMap<String, PersistentValue>,
        function: &BytecodeFunction,
        arguments: &[Value],
        captures: &[Value],
        account: &mut QuotaAccount,
    ) -> Result<ExecutionArena, RuntimeError> {
        // Linking recursively walks the immutable prototype graph. Keep that host
        // recursion off callers' often-small test or embedding threads; VM calls
        // themselves use the explicit frame stack below.
        let (mut current, prototype) = std::thread::scope(|scope| {
            std::thread::Builder::new()
                .name("xl-bytecode-linker".into())
                .stack_size(16 * 1024 * 1024)
                .spawn_scoped(scope, || {
                    let mut current = Heap::local();
                    let prototype =
                        current.link_bytecode_resolved(Some(background), function, externals)?;
                    Ok::<_, crate::heap::HeapError>((current, prototype))
                })
                .map_err(|_| crate::heap::HeapError::new("failed to start bytecode linker"))?
                .join()
                .map_err(|_| crate::heap::HeapError::new("bytecode linker panicked"))?
        })
        .map_err(|heap_error| {
            error(
                RuntimeErrorKind::InvalidBytecode,
                heap_error.to_string(),
                function,
                0,
            )
        })?;
        let arguments = arguments
            .iter()
            .map(|value| current.import_value(Some(background), value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|heap_error| {
                error(
                    RuntimeErrorKind::InvalidBytecode,
                    heap_error.to_string(),
                    function,
                    0,
                )
            })?;
        let captures = captures
            .iter()
            .map(|value| current.import_value(Some(background), value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|heap_error| {
                error(
                    RuntimeErrorKind::InvalidBytecode,
                    heap_error.to_string(),
                    function,
                    0,
                )
            })?;
        let mut stack: Vec<Option<RuntimeValue>> = Vec::new();
        let mut frames = vec![make_execution_frame(
            Arc::new(function.clone()),
            prototype,
            &arguments,
            &captures,
            None,
            &mut stack,
            account.stack_limit(),
        )?];

        let mut result = (|| -> Result<RuntimeValue, RuntimeError> {
            loop {
                let function_arc = frames
                    .last()
                    .expect("execution has at least one frame")
                    .function
                    .clone();
                let function = function_arc.as_ref();
                let pc = frames.last().expect("execution frame").pc;
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
                let view = HeapView {
                    current: &current,
                    background: Some(background),
                };

                match instruction {
                    Opcode::LoadConst { dst, value } => {
                        let (_, values, _, _) =
                            view.bytecode(frame.prototype).map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?;
                        let value = values.get(value.0).copied().ok_or_else(|| {
                            error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("value link {} is out of bounds", value.0),
                                function,
                                pc,
                            )
                        })?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::Move { dst, src } => {
                        let value = *read_register(&registers, *src, function, pc)?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::Add { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Add,
                            &view,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::Subtract { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Subtract,
                            &view,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::Multiply { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Multiply,
                            &view,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::Divide { dst, left, right } => {
                        let value = numeric_binary(
                            read_register(&registers, *left, function, pc)?,
                            read_register(&registers, *right, function, pc)?,
                            NumericOperation::Divide,
                            &view,
                            function,
                            pc,
                        )?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::Negate { dst, src } => {
                        let value = match read_register(&registers, *src, function, pc)? {
                            RuntimeValue::Int(value) => {
                                RuntimeValue::Int(value.checked_neg().ok_or_else(|| {
                                    error(
                                        RuntimeErrorKind::IntegerOverflow,
                                        "integer negation overflowed",
                                        function,
                                        pc,
                                    )
                                })?)
                            }
                            RuntimeValue::Float(value) => RuntimeValue::Float(-value),
                            value => {
                                return Err(runtime_type_error(
                                    "numeric value",
                                    value,
                                    &view,
                                    function,
                                    pc,
                                ));
                            }
                        };
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::Equal { dst, left, right } => {
                        let left = export_runtime(
                            &view,
                            *read_register(&registers, *left, function, pc)?,
                            function,
                            pc,
                        )?;
                        let right = export_runtime(
                            &view,
                            *read_register(&registers, *right, function, pc)?,
                            function,
                            pc,
                        )?;
                        let equal = values_equal(&left, &right, function, pc)?;
                        write_register(&mut registers, *dst, runtime_bool(equal), function, pc)?;
                    }
                    Opcode::LessThan { dst, left, right } => {
                        let left = read_register(&registers, *left, function, pc)?;
                        let right = read_register(&registers, *right, function, pc)?;
                        let less = match (left, right) {
                            (RuntimeValue::Int(left), RuntimeValue::Int(right)) => left < right,
                            (RuntimeValue::Float(left), RuntimeValue::Float(right)) => left < right,
                            _ => {
                                return Err(runtime_numeric_type_error(
                                    left, right, &view, function, pc,
                                ));
                            }
                        };
                        write_register(&mut registers, *dst, runtime_bool(less), function, pc)?;
                    }
                    Opcode::MakeArray { dst, items } => {
                        let values = read_many(&registers, items, function, pc)?;
                        let bytes = logical_value_bytes(values.len()).map_err(|native_error| {
                            allocation_error(native_error.message, function, pc)
                        })?;
                        charge_allocation(account, bytes, function, pc)?;
                        write_register(
                            &mut registers,
                            *dst,
                            RuntimeValue::Array(
                                current.allocate(crate::heap::Object::Array(values.into())),
                            ),
                            function,
                            pc,
                        )?;
                    }
                    Opcode::MakeTuple { dst, items } => {
                        let values = read_many(&registers, items, function, pc)?;
                        let bytes = logical_value_bytes(values.len()).map_err(|native_error| {
                            allocation_error(native_error.message, function, pc)
                        })?;
                        charge_allocation(account, bytes, function, pc)?;
                        write_register(
                            &mut registers,
                            *dst,
                            RuntimeValue::Tuple(
                                current.allocate(crate::heap::Object::Tuple(values.into())),
                            ),
                            function,
                            pc,
                        )?;
                    }
                    Opcode::InterpolateString { dst, parts } => {
                        let values = read_many(&registers, parts, function, pc)?
                            .into_iter()
                            .map(|value| export_runtime(&view, value, function, pc))
                            .collect::<Result<Vec<_>, _>>()?;
                        let mut length = 0usize;
                        for value in &values {
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
                        let bytes = u64::try_from(length).map_err(|_| {
                            allocation_error("String allocation size overflowed", function, pc)
                        })?;
                        charge_allocation(account, bytes, function, pc)?;
                        let mut output = String::with_capacity(length);
                        for value in &values {
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
                        let value = current.string(Some(background), &output);
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::MakeDict { dst, fields } => {
                        let (_, _, text_links, _) =
                            view.bytecode(frame.prototype).map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?;
                        let mut entries = fields
                            .iter()
                            .map(|(field, register)| {
                                let field = text_links.get(field.0).copied().ok_or_else(|| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        format!("text link {} is out of bounds", field.0),
                                        function,
                                        pc,
                                    )
                                })?;
                                Ok((field, *read_register(&registers, *register, function, pc)?))
                            })
                            .collect::<Result<Vec<_>, RuntimeError>>()?;
                        entries.sort_by(|left, right| {
                            view.text(left.0)
                                .unwrap_or("")
                                .cmp(view.text(right.0).unwrap_or(""))
                        });
                        if entries
                            .windows(2)
                            .any(|pair| view.text(pair[0].0).ok() == view.text(pair[1].0).ok())
                        {
                            return Err(error(
                                RuntimeErrorKind::InvalidBytecode,
                                "Dict contains a duplicate field",
                                function,
                                pc,
                            ));
                        }
                        let (fields, values): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
                        let field_bytes = fields.iter().try_fold(0u64, |total, field| {
                            let length = view
                                .text(*field)
                                .map_err(|heap_error| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        heap_error.to_string(),
                                        function,
                                        pc,
                                    )
                                })?
                                .len();
                            total.checked_add(length as u64).ok_or_else(|| {
                                allocation_error("Dict allocation size overflowed", function, pc)
                            })
                        })?;
                        let value_bytes =
                            logical_value_bytes(values.len()).map_err(|native_error| {
                                allocation_error(native_error.message, function, pc)
                            })?;
                        let bytes = field_bytes.checked_add(value_bytes).ok_or_else(|| {
                            allocation_error("Dict allocation size overflowed", function, pc)
                        })?;
                        charge_allocation(account, bytes, function, pc)?;
                        let shape = current.intern_shape(fields);
                        let dict =
                            RuntimeValue::Dict(current.allocate(crate::heap::Object::Dict {
                                shape,
                                values: values.into(),
                            }));
                        write_register(&mut registers, *dst, dict, function, pc)?;
                    }
                    Opcode::GetField { dst, dict, field } => {
                        let (_, _, text_links, _) =
                            view.bytecode(frame.prototype).map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?;
                        let field = text_links.get(field.0).copied().ok_or_else(|| {
                            error(
                                RuntimeErrorKind::InvalidBytecode,
                                format!("text link {} is out of bounds", field.0),
                                function,
                                pc,
                            )
                        })?;
                        let dict = read_register(&registers, *dict, function, pc)?;
                        let RuntimeValue::Dict(dict) = dict else {
                            return Err(runtime_type_error("Dict", dict, &view, function, pc));
                        };
                        let value = view
                            .dict_get(*dict, field)
                            .map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?
                            .ok_or_else(|| {
                                error(
                                    RuntimeErrorKind::MissingField,
                                    format!(
                                        "Dict has no field {:?}",
                                        view.text(field).unwrap_or("<invalid>")
                                    ),
                                    function,
                                    pc,
                                )
                            })?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::TupleLengthEquals { dst, value, length } => {
                        let matches = matches!(
                            read_register(&registers, *value, function, pc)?,
                            RuntimeValue::Tuple(handle) if view.sequence(*handle, true).is_ok_and(|items| items.len() == *length)
                        );
                        write_register(&mut registers, *dst, runtime_bool(matches), function, pc)?;
                    }
                    Opcode::GetTuple { dst, tuple, index } => {
                        let tuple = read_register(&registers, *tuple, function, pc)?;
                        let RuntimeValue::Tuple(handle) = tuple else {
                            return Err(runtime_type_error("Tuple", tuple, &view, function, pc));
                        };
                        let value = view
                            .sequence(*handle, true)
                            .map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?
                            .get(*index)
                            .copied()
                            .ok_or_else(|| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    format!("tuple index {index} is out of bounds"),
                                    function,
                                    pc,
                                )
                            })?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::MakeClosure {
                        dst,
                        prototype,
                        captures,
                    } => {
                        let (_, _, _, prototypes) =
                            view.bytecode(frame.prototype).map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?;
                        let closure_prototype =
                            prototypes.get(prototype.0).copied().ok_or_else(|| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    format!("prototype link {} is out of bounds", prototype.0),
                                    function,
                                    pc,
                                )
                            })?;
                        let captures = read_many(&registers, captures, function, pc)?;
                        let bytes =
                            logical_value_bytes(captures.len()).map_err(|native_error| {
                                allocation_error(native_error.message, function, pc)
                            })?;
                        charge_allocation(account, bytes, function, pc)?;
                        let closure =
                            RuntimeValue::Func(current.allocate(crate::heap::Object::Closure {
                                prototype: closure_prototype,
                                upvalues: captures.into(),
                            }));
                        write_register(&mut registers, *dst, closure, function, pc)?;
                    }
                    Opcode::Call {
                        dst,
                        callee,
                        arguments,
                    } => {
                        let callee = *read_register(&registers, *callee, function, pc)?;
                        let RuntimeValue::Func(closure_handle) = callee else {
                            return Err(runtime_type_error("Func", &callee, &view, function, pc));
                        };
                        let (runtime_prototype, upvalues) =
                            view.closure(closure_handle).map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?;
                        let upvalues = upvalues.to_vec();
                        consume_fuel(account, function, pc)?;
                        let arguments = read_many(&registers, arguments, function, pc)?;
                        match runtime_prototype {
                            crate::heap::RuntimePrototype::Bytecode(prototype) => {
                                if frames.len() >= MAX_CALL_DEPTH {
                                    return Err(error(
                                        RuntimeErrorKind::CallDepthExceeded,
                                        format!(
                                            "call depth exceeds the limit of {MAX_CALL_DEPTH} frames"
                                        ),
                                        function,
                                        pc,
                                    ));
                                }
                                let (code, _, _, _) =
                                    view.bytecode(prototype).map_err(|heap_error| {
                                        error(
                                            RuntimeErrorKind::InvalidBytecode,
                                            heap_error.to_string(),
                                            function,
                                            pc,
                                        )
                                    })?;
                                let callee_function =
                                    Arc::new(BytecodeFunction::from_linked_code(Arc::clone(code)));
                                frames.last_mut().expect("caller frame").pc += 1;
                                let next = make_execution_frame(
                                    callee_function.clone(),
                                    prototype,
                                    &arguments,
                                    &upvalues,
                                    Some(*dst),
                                    &mut stack,
                                    account.stack_limit(),
                                )
                                .map_err(|mut runtime_error| {
                                    runtime_error.trace_includes_active_frame = false;
                                    runtime_error
                                })?;
                                frames.push(next);
                                continue;
                            }
                            crate::heap::RuntimePrototype::Native(native) => {
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
                                // Native scratch slots extend the same stack, so end the
                                // current frame-window borrow before creating its context.
                                let _ = registers;
                                let mut context = CallContext::new(
                                    &mut current,
                                    Some(background),
                                    &mut stack,
                                    account,
                                    arguments,
                                    &upvalues,
                                )
                                .map_err(|native_error| {
                                    error(
                                        RuntimeErrorKind::StackLimitExceeded,
                                        format!("{}: {}", native.name(), native_error.message),
                                        function,
                                        pc,
                                    )
                                })?;
                                (native.callback())(&mut context).map_err(|native_error| {
                                    error(
                                        match native_error.limit() {
                                            Some(NativeLimit::Stack) => {
                                                RuntimeErrorKind::StackLimitExceeded
                                            }
                                            Some(NativeLimit::Allocation) => {
                                                RuntimeErrorKind::AllocationQuotaExceeded
                                            }
                                            None => RuntimeErrorKind::TypeMismatch,
                                        },
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
                    Opcode::Jump { target } => {
                        validate_jump(*target, function, pc)?;
                        if *target <= pc {
                            consume_fuel(account, function, pc)?;
                        }
                        frames.last_mut().expect("execution frame").pc = *target;
                        continue;
                    }
                    Opcode::JumpIfFalse { condition, target } => {
                        match read_register(&registers, *condition, function, pc)? {
                            RuntimeValue::BuiltinAtom(BuiltinAtom::True) => {}
                            RuntimeValue::BuiltinAtom(BuiltinAtom::False) => {
                                validate_jump(*target, function, pc)?;
                                if *target <= pc {
                                    consume_fuel(account, function, pc)?;
                                }
                                frames.last_mut().expect("execution frame").pc = *target;
                                continue;
                            }
                            value => {
                                return Err(runtime_type_error(
                                    "'True or 'False",
                                    value,
                                    &view,
                                    function,
                                    pc,
                                ));
                            }
                        }
                    }
                    Opcode::Return { src } => {
                        let value = *read_register(&registers, *src, function, pc)?;
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
                    Opcode::Fail { message } => {
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
            for frame in frames
                .iter()
                .rev()
                .skip(usize::from(runtime_error.trace_includes_active_frame))
            {
                let instruction = frame.pc.saturating_sub(1);
                runtime_error.trace.push(RuntimeFrame {
                    function: frame.function.name().to_owned(),
                    instruction,
                    origin: frame.function.origin_at(instruction),
                });
            }
            runtime_error.trace_includes_active_frame = false;
        }
        result.map(|root| ExecutionArena {
            heap: current,
            root,
        })
    }
}

fn make_execution_frame(
    function: Arc<BytecodeFunction>,
    prototype: Handle,
    arguments: &[RuntimeValue],
    captures: &[RuntimeValue],
    return_destination: Option<Register>,
    stack: &mut Vec<Option<RuntimeValue>>,
    stack_limit: usize,
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
    let end = base.checked_add(function.register_count()).ok_or_else(|| {
        error(
            RuntimeErrorKind::StackLimitExceeded,
            "XL stack size overflowed",
            &function,
            0,
        )
    })?;
    if end > stack_limit {
        return Err(error(
            RuntimeErrorKind::StackLimitExceeded,
            format!("XL stack exceeds the limit of {stack_limit} slots"),
            &function,
            0,
        ));
    }
    stack.resize(end, None);
    for (index, value) in arguments.iter().chain(captures).enumerate() {
        let Some(register) = stack.get_mut(base + index) else {
            return Err(error(
                RuntimeErrorKind::InvalidBytecode,
                "function signature exceeds its register count",
                &function,
                0,
            ));
        };
        *register = Some(*value);
    }
    Ok(ExecutionFrame {
        function,
        prototype,
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
    registers: &'a [Option<RuntimeValue>],
    register: Register,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<&'a RuntimeValue, RuntimeError> {
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
    registers: &mut [Option<RuntimeValue>],
    register: Register,
    value: RuntimeValue,
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
    registers: &[Option<RuntimeValue>],
    items: &[Register],
    function: &BytecodeFunction,
    pc: usize,
) -> Result<Vec<RuntimeValue>, RuntimeError> {
    items
        .iter()
        .map(|register| read_register(registers, *register, function, pc).copied())
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
    left: &RuntimeValue,
    right: &RuntimeValue,
    operation: NumericOperation,
    view: &HeapView<'_>,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<RuntimeValue, RuntimeError> {
    match (left, right) {
        (RuntimeValue::Int(left), RuntimeValue::Int(right)) => {
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
            Ok(RuntimeValue::Int(value))
        }
        (RuntimeValue::Float(left), RuntimeValue::Float(right)) => {
            Ok(RuntimeValue::Float(match operation {
                NumericOperation::Add => left + right,
                NumericOperation::Subtract => left - right,
                NumericOperation::Multiply => left * right,
                NumericOperation::Divide => left / right,
            }))
        }
        _ => Err(runtime_numeric_type_error(left, right, view, function, pc)),
    }
}

fn runtime_bool(value: bool) -> RuntimeValue {
    RuntimeValue::BuiltinAtom(if value {
        BuiltinAtom::True
    } else {
        BuiltinAtom::False
    })
}

fn export_runtime(
    view: &HeapView<'_>,
    value: RuntimeValue,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<Value, RuntimeError> {
    view.export_value(value).map_err(|heap_error| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            heap_error.to_string(),
            function,
            pc,
        )
    })
}

fn runtime_type_error(
    expected: &str,
    actual: &RuntimeValue,
    view: &HeapView<'_>,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    match view.export_value(*actual) {
        Ok(actual) => type_error(expected, &actual, function, pc),
        Err(heap_error) => error(
            RuntimeErrorKind::InvalidBytecode,
            heap_error.to_string(),
            function,
            pc,
        ),
    }
}

fn runtime_numeric_type_error(
    left: &RuntimeValue,
    right: &RuntimeValue,
    view: &HeapView<'_>,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    match (view.export_value(*left), view.export_value(*right)) {
        (Ok(left), Ok(right)) => numeric_type_error(&left, &right, function, pc),
        (Err(heap_error), _) | (_, Err(heap_error)) => error(
            RuntimeErrorKind::InvalidBytecode,
            heap_error.to_string(),
            function,
            pc,
        ),
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

fn logical_value_bytes(count: usize) -> Result<u64, NativeError> {
    let count = u64::try_from(count)
        .map_err(|_| NativeError::allocation_limit("allocation item count overflowed"))?;
    let value_size = u64::try_from(std::mem::size_of::<Value>())
        .map_err(|_| NativeError::allocation_limit("Value size overflowed"))?;
    count
        .checked_mul(value_size)
        .ok_or_else(|| NativeError::allocation_limit("allocation size overflowed"))
}

fn allocation_error(
    message: impl Into<String>,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    error(
        RuntimeErrorKind::AllocationQuotaExceeded,
        message,
        function,
        pc,
    )
}

fn charge_allocation(
    account: &mut QuotaAccount,
    bytes: u64,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<(), RuntimeError> {
    account.charge_allocation(bytes).map_err(|()| {
        allocation_error(
            format!(
                "allocation quota of {} bytes exceeded",
                account.quota.allocation_bytes
            ),
            function,
            pc,
        )
    })
}

fn consume_fuel(
    account: &mut QuotaAccount,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<(), RuntimeError> {
    if account.remaining_fuel == 0 {
        return Err(error(
            RuntimeErrorKind::FuelExhausted,
            "evaluation fuel exhausted",
            function,
            pc,
        ));
    }
    account.remaining_fuel -= 1;
    Ok(())
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
        trace_includes_active_frame: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Atom, BytecodeFunction, Closure, Instruction, NativeFunction, Register};

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
    fn enforces_fuel_and_rejects_malformed_bytecode() {
        let loop_function =
            BytecodeFunction::new("loop", 0, vec![], vec![Instruction::Jump { target: 0 }]);
        let error = Vm::new().execute(&loop_function, 5).unwrap_err();
        assert_eq!(error.kind, RuntimeErrorKind::FuelExhausted);

        let invalid = BytecodeFunction::new(
            "invalid",
            0,
            vec![],
            vec![Instruction::Return { src: Register(9) }],
        );
        let error = Vm::new().execute(&invalid, 5).unwrap_err();
        assert_eq!(error.kind, RuntimeErrorKind::InvalidBytecode);
    }

    #[test]
    fn straight_line_and_forward_control_flow_need_no_fuel() {
        let straight = BytecodeFunction::new(
            "straight",
            1,
            vec![Value::Int(42)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        );
        assert!(matches!(
            Vm::new().execute(&straight, 0).unwrap(),
            Value::Int(42)
        ));

        let forward = BytecodeFunction::new(
            "forward",
            1,
            vec![Value::Int(42)],
            vec![
                Instruction::Jump { target: 2 },
                Instruction::Fail {
                    message: "skipped".into(),
                },
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        );
        assert!(matches!(
            Vm::new().execute(&forward, 0).unwrap(),
            Value::Int(42)
        ));
    }

    #[test]
    fn only_taken_back_edges_consume_fuel() {
        let untaken = BytecodeFunction::new(
            "untaken",
            1,
            vec![Value::bool(true)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::JumpIfFalse {
                    condition: Register(0),
                    target: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        );
        assert!(Vm::new().execute(&untaken, 0).is_ok());

        let one_back_edge = BytecodeFunction::new(
            "one-back-edge",
            1,
            vec![Value::bool(false), Value::bool(true)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Jump { target: 3 },
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 1,
                },
                Instruction::JumpIfFalse {
                    condition: Register(0),
                    target: 2,
                },
                Instruction::Return { src: Register(0) },
            ],
        );
        let exhausted = Vm::new().execute(&one_back_edge, 0).unwrap_err();
        assert_eq!(exhausted.kind, RuntimeErrorKind::FuelExhausted);
        assert!(Vm::new().execute(&one_back_edge, 1).is_ok());
    }

    #[test]
    fn bytecode_and_native_calls_each_consume_one_fuel() {
        let callee = Arc::new(BytecodeFunction::new(
            "callee",
            1,
            vec![Value::Int(42)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        ));
        let bytecode = BytecodeFunction::new(
            "bytecode-call",
            2,
            vec![Value::Func(Arc::new(Closure::new(callee, Vec::new())))],
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
        );
        assert_eq!(
            Vm::new().execute(&bytecode, 0).unwrap_err().kind,
            RuntimeErrorKind::FuelExhausted
        );
        assert!(Vm::new().execute(&bytecode, 1).is_ok());

        let nested = BytecodeFunction::new(
            "nested-call",
            2,
            vec![Value::Func(Arc::new(Closure::new(
                Arc::new(bytecode),
                Vec::new(),
            )))],
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
        );
        assert_eq!(
            Vm::new().execute(&nested, 1).unwrap_err().kind,
            RuntimeErrorKind::FuelExhausted
        );
        assert!(Vm::new().execute(&nested, 2).is_ok());

        let native = NativeFunction::new("add_upvalue", 1, native_add_upvalue);
        let native = BytecodeFunction::new(
            "native-call",
            3,
            vec![
                Value::Func(Arc::new(Closure::native_with_upvalues(
                    native,
                    vec![Value::Int(40)],
                ))),
                Value::Int(2),
            ],
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
        assert_eq!(
            Vm::new().execute(&native, 0).unwrap_err().kind,
            RuntimeErrorKind::FuelExhausted
        );
        assert!(Vm::new().execute(&native, 1).is_ok());
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

    #[test]
    fn enforces_independent_call_depth_and_stack_slot_limits() {
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
        for _ in 0..MAX_CALL_DEPTH {
            let closure = Value::Func(Arc::new(Closure::new(function, Vec::new())));
            function = Arc::new(BytecodeFunction::new(
                "recursive-shape",
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
        let depth = Vm::new().execute(&function, usize::MAX).unwrap_err();
        assert_eq!(depth.kind, RuntimeErrorKind::CallDepthExceeded);

        let oversized = BytecodeFunction::new(
            "oversized",
            MAX_STACK_SLOTS + 1,
            vec![],
            vec![Instruction::Return { src: Register(0) }],
        );
        let stack = Vm::new().execute(&oversized, usize::MAX).unwrap_err();
        assert_eq!(stack.kind, RuntimeErrorKind::StackLimitExceeded);
    }

    #[test]
    fn trace_does_not_deduplicate_equal_function_names_and_pcs() {
        let leaf = Arc::new(BytecodeFunction::new(
            "same",
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
        ));
        let mut function = leaf;
        for _ in 0..2 {
            let closure = Value::Func(Arc::new(Closure::new(function, Vec::new())));
            function = Arc::new(BytecodeFunction::new(
                "same",
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
        let error = Vm::new().execute(&function, 100).unwrap_err();
        assert_eq!(error.trace.len(), 3);
        assert!(error.trace.iter().all(|frame| frame.function == "same"));
    }

    #[test]
    fn dict_allocation_charge_does_not_depend_on_shape_cache_hits() {
        let function = crate::compile_source("test", "{answer: 42}").unwrap();
        let mut vm = Vm::new();
        let mut account = QuotaAccount::new(Quota::new(0, 100, u64::MAX));
        vm.execute_with_account(&function, &[], &mut account)
            .unwrap();
        let first = account.requested_allocation_bytes();
        vm.execute_with_account(&function, &[], &mut account)
            .unwrap();
        let second = account.requested_allocation_bytes() - first;
        assert_eq!(first, second);
        assert!(first > 0);
    }
}
