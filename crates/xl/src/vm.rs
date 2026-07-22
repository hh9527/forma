use crate::bytecode::{BytecodeFunction, Opcode, Register};
use crate::heap::{Handle, Heap, HeapView, Object, PersistentValue, RuntimeValue, publish_root};
use crate::lir::RegisterId;
use crate::value::{
    BuiltinAtom, CoreArrayFunction, CoreCodecFunction, CoreDebugFunction, CoreDictFunction,
    CoreJsonFunction, CoreResultFunction, Dict, NativeError, NativeKind, NativeLimit, Shape, Value,
};
use crate::{Diagnostic, Origin, SourceDatabase};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fmt::Write;
use std::sync::{Arc, Weak};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugEvent {
    pub label: Option<String>,
    pub value: String,
}

pub trait DebugSink: Send + Sync {
    fn emit(&self, event: DebugEvent);
}

#[derive(Debug, Default)]
pub struct DiscardDebugSink;

impl DebugSink for DiscardDebugSink {
    fn emit(&self, _event: DebugEvent) {}
}

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
            RuntimeValue::UpLink(_) => {
                unreachable!("up-links are private VM values")
            }
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

    pub fn function_arity(self) -> Option<usize> {
        let RuntimeValue::Func(handle) = self.value else {
            return None;
        };
        self.view.function_arity(handle).ok()
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
    UninitializedDefinition,
    DuplicateDefinition,
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

pub struct Vm {
    shapes: ShapeInterner,
    debug_sink: Arc<dyn DebugSink>,
}

impl Default for Vm {
    fn default() -> Self {
        Self {
            shapes: ShapeInterner::default(),
            debug_sink: Arc::new(DiscardDebugSink),
        }
    }
}

struct ExecutionFrame {
    function: Arc<BytecodeFunction>,
    prototype: Handle,
    base: usize,
    pc: usize,
    return_target: ReturnTarget,
}

#[derive(Debug)]
enum ReturnTarget {
    Root,
    Register(Register),
    Native(Box<ArrayContinuation>),
}

#[derive(Debug)]
struct ArrayContinuation {
    function: CoreArrayFunction,
    source: RuntimeValue,
    callback: RuntimeValue,
    next_index: usize,
    accumulator: Option<RuntimeValue>,
    output: Vec<RuntimeValue>,
    return_target: ReturnTarget,
    call_function: Arc<BytecodeFunction>,
    call_pc: usize,
    trace_frame: RuntimeFrame,
}

enum VmAction {
    Call {
        callee: RuntimeValue,
        arguments: Vec<RuntimeValue>,
        return_target: ReturnTarget,
        call_function: Arc<BytecodeFunction>,
        call_pc: usize,
    },
    Return {
        value: RuntimeValue,
        return_target: ReturnTarget,
    },
}

enum DriveOutcome {
    Pending,
    Root(RuntimeValue),
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

    pub fn with_debug_sink(mut self, sink: Arc<dyn DebugSink>) -> Self {
        self.debug_sink = sink;
        self
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
            ReturnTarget::Root,
            &mut stack,
            account.stack_limit(),
        )?];
        let debug_sink = Arc::clone(&self.debug_sink);

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
                    Opcode::MakeUpLink { dst } => {
                        charge_allocation(
                            account,
                            logical_value_bytes(1).map_err(|native_error| {
                                allocation_error(native_error.message, function, pc)
                            })?,
                            function,
                            pc,
                        )?;
                        let link = RuntimeValue::UpLink(
                            current.allocate(crate::heap::Object::UpLink { value: None }),
                        );
                        write_register(&mut registers, *dst, link, function, pc)?;
                    }
                    Opcode::ReadUpLink { dst, link } => {
                        let RuntimeValue::UpLink(handle) =
                            *read_register(&registers, *link, function, pc)?
                        else {
                            return Err(error(
                                RuntimeErrorKind::InvalidBytecode,
                                "up-link read operand is not an up-link",
                                function,
                                pc,
                            ));
                        };
                        let value = view
                            .up_link(handle)
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
                                    RuntimeErrorKind::UninitializedDefinition,
                                    "definition was read before initialization",
                                    function,
                                    pc,
                                )
                            })?;
                        write_register(&mut registers, *dst, value, function, pc)?;
                    }
                    Opcode::InitializeUpLink { link, src } => {
                        let RuntimeValue::UpLink(handle) =
                            *read_register(&registers, *link, function, pc)?
                        else {
                            return Err(error(
                                RuntimeErrorKind::InvalidBytecode,
                                "up-link initialization operand is not an up-link",
                                function,
                                pc,
                            ));
                        };
                        if view
                            .up_link(handle)
                            .map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?
                            .is_some()
                        {
                            return Err(error(
                                RuntimeErrorKind::DuplicateDefinition,
                                "definition was initialized more than once",
                                function,
                                pc,
                            ));
                        }
                        let value = *read_register(&registers, *src, function, pc)?;
                        current
                            .initialize_up_link(handle, value)
                            .map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?;
                    }
                    Opcode::AssertUpLinkReady { link } => {
                        let RuntimeValue::UpLink(handle) =
                            *read_register(&registers, *link, function, pc)?
                        else {
                            return Err(error(
                                RuntimeErrorKind::InvalidBytecode,
                                "up-link assertion operand is not an up-link",
                                function,
                                pc,
                            ));
                        };
                        if view
                            .up_link(handle)
                            .map_err(|heap_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    heap_error.to_string(),
                                    function,
                                    pc,
                                )
                            })?
                            .is_none()
                        {
                            return Err(error(
                                RuntimeErrorKind::UninitializedDefinition,
                                "declaration was not initialized before block completion",
                                function,
                                pc,
                            ));
                        }
                    }
                    Opcode::AssertFunctionArity { value, arity } => {
                        let value = *read_register(&registers, *value, function, pc)?;
                        let RuntimeValue::Func(handle) = value else {
                            return Err(error(
                                RuntimeErrorKind::TypeMismatch,
                                format!(
                                    "definition must be a function accepting {arity} arguments"
                                ),
                                function,
                                pc,
                            ));
                        };
                        let actual = view.function_arity(handle).map_err(|heap_error| {
                            error(
                                RuntimeErrorKind::InvalidBytecode,
                                heap_error.to_string(),
                                function,
                                pc,
                            )
                        })?;
                        if actual != *arity {
                            return Err(error(
                                RuntimeErrorKind::TypeMismatch,
                                format!("definition must accept {arity} arguments, got {actual}"),
                                function,
                                pc,
                            ));
                        }
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
                        let left = *read_register(&registers, *left, function, pc)?;
                        let right = *read_register(&registers, *right, function, pc)?;
                        let equal = view.values_equal(left, right).map_err(|heap_error| {
                            error(
                                RuntimeErrorKind::InvalidBytecode,
                                heap_error.to_string(),
                                function,
                                pc,
                            )
                        })?;
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
                        let values = read_many(&registers, parts, function, pc)?;
                        let mut length = 0usize;
                        for value in &values {
                            length += if let RuntimeValue::Int(value) = value {
                                decimal_length(*value)
                            } else if let Some(value) =
                                view.string_text(*value).map_err(|heap_error| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        heap_error.to_string(),
                                        function,
                                        pc,
                                    )
                                })?
                            {
                                value.len()
                            } else if let Some(value) =
                                view.atom_text(*value).map_err(|heap_error| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        heap_error.to_string(),
                                        function,
                                        pc,
                                    )
                                })?
                            {
                                value.len()
                            } else {
                                return Err(runtime_shallow_type_error(
                                    "String, Int, or Atom interpolation value",
                                    *value,
                                    function,
                                    pc,
                                ));
                            };
                        }
                        let bytes = u64::try_from(length).map_err(|_| {
                            allocation_error("String allocation size overflowed", function, pc)
                        })?;
                        charge_allocation(account, bytes, function, pc)?;
                        let mut output = String::with_capacity(length);
                        for value in &values {
                            if let RuntimeValue::Int(value) = value {
                                write!(output, "{value}").expect("writing to String cannot fail");
                            } else if let Some(value) =
                                view.string_text(*value).map_err(|heap_error| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        heap_error.to_string(),
                                        function,
                                        pc,
                                    )
                                })?
                            {
                                output.push_str(value);
                            } else if let Some(value) =
                                view.atom_text(*value).map_err(|heap_error| {
                                    error(
                                        RuntimeErrorKind::InvalidBytecode,
                                        heap_error.to_string(),
                                        function,
                                        pc,
                                    )
                                })?
                            {
                                output.push_str(value);
                            } else {
                                unreachable!("interpolation values were validated");
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
                                identity: Arc::new(()),
                                prototype: closure_prototype,
                                upvalues: captures.into(),
                            }));
                        write_register(&mut registers, *dst, closure, function, pc)?;
                    }
                    Opcode::Call {
                        base: call_base,
                        argument_count,
                    } => {
                        let callee = *read_register(&registers, *call_base, function, pc)?;
                        let arguments = read_call_arguments(
                            &registers,
                            *call_base,
                            *argument_count,
                            function,
                            pc,
                        )?;
                        frames.last_mut().expect("caller frame").pc += 1;
                        let _ = registers;
                        match drive_vm_action(
                            VmAction::Call {
                                callee,
                                arguments,
                                return_target: ReturnTarget::Register(*call_base),
                                call_function: function_arc,
                                call_pc: pc,
                            },
                            &mut frames,
                            &mut stack,
                            &mut current,
                            background,
                            account,
                            debug_sink.as_ref(),
                        )? {
                            DriveOutcome::Pending => continue,
                            DriveOutcome::Root(value) => return Ok(value),
                        }
                    }
                    Opcode::TailCall {
                        base: call_base,
                        argument_count,
                    } => {
                        let callee = *read_register(&registers, *call_base, function, pc)?;
                        let arguments = read_call_arguments(
                            &registers,
                            *call_base,
                            *argument_count,
                            function,
                            pc,
                        )?;
                        let completed = frames.pop().expect("tail caller frame");
                        let _ = registers;
                        stack.truncate(completed.base);
                        match drive_vm_action(
                            VmAction::Call {
                                callee,
                                arguments,
                                return_target: completed.return_target,
                                call_function: function_arc,
                                call_pc: pc,
                            },
                            &mut frames,
                            &mut stack,
                            &mut current,
                            background,
                            account,
                            debug_sink.as_ref(),
                        )? {
                            DriveOutcome::Pending => continue,
                            DriveOutcome::Root(value) => return Ok(value),
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
                        let completed = frames.pop().expect("execution frame");
                        let _ = registers;
                        stack.truncate(completed.base);
                        match drive_vm_action(
                            VmAction::Return {
                                value,
                                return_target: completed.return_target,
                            },
                            &mut frames,
                            &mut stack,
                            &mut current,
                            background,
                            account,
                            debug_sink.as_ref(),
                        )? {
                            DriveOutcome::Pending => continue,
                            DriveOutcome::Root(value) => return Ok(value),
                        }
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
            for (index, frame) in frames.iter().rev().enumerate() {
                if index != 0 || !runtime_error.trace_includes_active_frame {
                    let instruction = frame.pc.saturating_sub(1);
                    runtime_error.trace.push(RuntimeFrame {
                        function: frame.function.name().to_owned(),
                        instruction,
                        origin: frame.function.origin_at(instruction),
                    });
                }
                frame
                    .return_target
                    .append_native_trace(&mut runtime_error.trace);
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
    return_target: ReturnTarget,
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
        return_target,
    })
}

fn drive_vm_action(
    mut action: VmAction,
    frames: &mut Vec<ExecutionFrame>,
    stack: &mut Vec<Option<RuntimeValue>>,
    current: &mut Heap,
    background: &Heap,
    account: &mut QuotaAccount,
    debug_sink: &dyn DebugSink,
) -> Result<DriveOutcome, RuntimeError> {
    loop {
        action = match action {
            VmAction::Return {
                value,
                return_target,
            } => match return_target {
                ReturnTarget::Root => return Ok(DriveOutcome::Root(value)),
                ReturnTarget::Register(destination) => {
                    let caller = frames.last().ok_or_else(|| RuntimeError {
                        kind: RuntimeErrorKind::InvalidBytecode,
                        message: "return register has no caller".into(),
                        function: "<vm>".into(),
                        instruction: 0,
                        trace: Vec::new(),
                        rendered: None,
                        trace_includes_active_frame: false,
                    })?;
                    let caller_function = caller.function.clone();
                    let caller_end = caller.base + caller.function.register_count();
                    write_register(
                        &mut stack[caller.base..caller_end],
                        destination,
                        value,
                        &caller_function,
                        caller.pc.saturating_sub(1),
                    )?;
                    return Ok(DriveOutcome::Pending);
                }
                ReturnTarget::Native(continuation) => {
                    let trace_frame = continuation.trace_frame.clone();
                    resume_array_continuation(*continuation, value, current, background, account)
                        .map_err(|mut runtime_error| {
                            runtime_error.trace.push(trace_frame);
                            runtime_error
                        })?
                }
            },
            VmAction::Call {
                callee,
                arguments,
                return_target,
                call_function,
                call_pc,
            } => {
                consume_fuel(account, &call_function, call_pc).map_err(|mut runtime_error| {
                    return_target.append_native_trace(&mut runtime_error.trace);
                    runtime_error
                })?;
                let logical_depth = frames.len()
                    + frames
                        .iter()
                        .map(|frame| frame.return_target.native_depth())
                        .sum::<usize>()
                    + return_target.native_depth();
                if logical_depth >= MAX_CALL_DEPTH {
                    return Err(error(
                        RuntimeErrorKind::CallDepthExceeded,
                        format!("call depth exceeds the limit of {MAX_CALL_DEPTH} frames"),
                        &call_function,
                        call_pc,
                    ));
                }
                let RuntimeValue::Func(closure_handle) = callee else {
                    let view = HeapView {
                        current,
                        background: Some(background),
                    };
                    return Err(runtime_type_error(
                        "Func",
                        &callee,
                        &view,
                        &call_function,
                        call_pc,
                    ));
                };
                let view = HeapView {
                    current,
                    background: Some(background),
                };
                let (runtime_prototype, upvalues) =
                    view.closure(closure_handle).map_err(|heap_error| {
                        error(
                            RuntimeErrorKind::InvalidBytecode,
                            heap_error.to_string(),
                            &call_function,
                            call_pc,
                        )
                    })?;
                let upvalues = upvalues.to_vec();
                let expected_arity = match runtime_prototype {
                    crate::heap::RuntimePrototype::Bytecode(prototype) => view
                        .bytecode(prototype)
                        .map_err(|heap_error| {
                            error(
                                RuntimeErrorKind::InvalidBytecode,
                                heap_error.to_string(),
                                &call_function,
                                call_pc,
                            )
                        })?
                        .0
                        .parameter_count(),
                    crate::heap::RuntimePrototype::Native(native) => native.arity(),
                };
                if arguments.len() != expected_arity {
                    return Err(error(
                        RuntimeErrorKind::TypeMismatch,
                        format!(
                            "expected {expected_arity} arguments, got {}",
                            arguments.len()
                        ),
                        &call_function,
                        call_pc,
                    ));
                }
                match runtime_prototype {
                    crate::heap::RuntimePrototype::Bytecode(prototype) => {
                        let (code, _, _, _) = view.bytecode(prototype).map_err(|heap_error| {
                            error(
                                RuntimeErrorKind::InvalidBytecode,
                                heap_error.to_string(),
                                &call_function,
                                call_pc,
                            )
                        })?;
                        let callee_function =
                            Arc::new(BytecodeFunction::from_linked_code(Arc::clone(code)));
                        let next = make_execution_frame(
                            callee_function,
                            prototype,
                            &arguments,
                            &upvalues,
                            return_target,
                            stack,
                            account.stack_limit(),
                        )
                        .map_err(|runtime_error| {
                            error(
                                runtime_error.kind,
                                runtime_error.message,
                                &call_function,
                                call_pc,
                            )
                        })?;
                        frames.push(next);
                        return Ok(DriveOutcome::Pending);
                    }
                    crate::heap::RuntimePrototype::Native(native) => match native.kind() {
                        NativeKind::Synchronous => {
                            let mut context = CallContext::new(
                                current,
                                Some(background),
                                stack,
                                account,
                                arguments,
                                &upvalues,
                            )
                            .map_err(|native_error| {
                                native_runtime_error(native, native_error, &call_function, call_pc)
                            })?;
                            (native.callback())(&mut context).map_err(|native_error| {
                                native_runtime_error(native, native_error, &call_function, call_pc)
                            })?;
                            let value = context.take_result().map_err(|native_error| {
                                error(
                                    RuntimeErrorKind::InvalidBytecode,
                                    format!("{}: {}", native.name(), native_error.message),
                                    &call_function,
                                    call_pc,
                                )
                            })?;
                            VmAction::Return {
                                value,
                                return_target,
                            }
                        }
                        NativeKind::CoreArray(function) => start_array_continuation(
                            function,
                            arguments,
                            return_target,
                            call_function,
                            call_pc,
                            current,
                            background,
                        )?,
                        NativeKind::CoreDict(function) => run_core_dict(
                            function,
                            &arguments,
                            return_target,
                            &call_function,
                            call_pc,
                            current,
                            background,
                            account,
                        )?,
                        NativeKind::CoreDebug(function) => run_core_debug(
                            function,
                            &arguments,
                            return_target,
                            &call_function,
                            call_pc,
                            current,
                            background,
                            debug_sink,
                        )?,
                        NativeKind::CoreCodec(operation) => run_core_codec(
                            operation,
                            &arguments,
                            return_target,
                            &call_function,
                            call_pc,
                            current,
                            background,
                            account,
                        )?,
                        NativeKind::CoreResult(operation) => run_core_result(
                            operation,
                            &arguments,
                            return_target,
                            &call_function,
                            call_pc,
                            current,
                            background,
                        )?,
                        NativeKind::CoreJson(operation) => run_core_json(
                            operation,
                            &arguments,
                            &upvalues,
                            return_target,
                            &call_function,
                            call_pc,
                            current,
                            background,
                            account,
                        )?,
                    },
                }
            }
        };
    }
}

impl ReturnTarget {
    fn native_depth(&self) -> usize {
        match self {
            Self::Root | Self::Register(_) => 0,
            Self::Native(continuation) => 1 + continuation.return_target.native_depth(),
        }
    }

    fn append_native_trace(&self, trace: &mut Vec<RuntimeFrame>) {
        if let Self::Native(continuation) = self {
            trace.push(continuation.trace_frame.clone());
            continuation.return_target.append_native_trace(trace);
        }
    }
}

fn native_runtime_error(
    native: crate::NativeFunction,
    native_error: NativeError,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    error(
        match native_error.limit() {
            Some(NativeLimit::Stack) => RuntimeErrorKind::StackLimitExceeded,
            Some(NativeLimit::Allocation) => RuntimeErrorKind::AllocationQuotaExceeded,
            None => RuntimeErrorKind::TypeMismatch,
        },
        format!("{}: {}", native.name(), native_error.message),
        function,
        pc,
    )
}

fn start_array_continuation(
    function: CoreArrayFunction,
    arguments: Vec<RuntimeValue>,
    return_target: ReturnTarget,
    call_function: Arc<BytecodeFunction>,
    call_pc: usize,
    current: &mut Heap,
    background: &Heap,
) -> Result<VmAction, RuntimeError> {
    let source = arguments[0];
    let RuntimeValue::Array(source_handle) = source else {
        let view = HeapView {
            current,
            background: Some(background),
        };
        return Err(runtime_type_error(
            "Array",
            &source,
            &view,
            &call_function,
            call_pc,
        ));
    };
    let view = HeapView {
        current,
        background: Some(background),
    };
    let length = view
        .sequence(source_handle, false)
        .map_err(|heap_error| {
            error(
                RuntimeErrorKind::InvalidBytecode,
                heap_error.to_string(),
                &call_function,
                call_pc,
            )
        })?
        .len();
    if function == CoreArrayFunction::Length {
        let length = i64::try_from(length).map_err(|_| {
            error(
                RuntimeErrorKind::IntegerOverflow,
                "Array length does not fit Int",
                &call_function,
                call_pc,
            )
        })?;
        return Ok(VmAction::Return {
            value: RuntimeValue::Int(length),
            return_target,
        });
    }

    let callback_index = if function == CoreArrayFunction::Fold {
        2
    } else {
        1
    };
    let callback = arguments[callback_index];
    let RuntimeValue::Func(callback_handle) = callback else {
        return Err(runtime_type_error(
            "Func",
            &callback,
            &view,
            &call_function,
            call_pc,
        ));
    };
    let expected_callback_arity = if function == CoreArrayFunction::Fold {
        2
    } else {
        1
    };
    let actual_callback_arity = view.function_arity(callback_handle).map_err(|heap_error| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            heap_error.to_string(),
            &call_function,
            call_pc,
        )
    })?;
    if actual_callback_arity != expected_callback_arity {
        return Err(error(
            RuntimeErrorKind::TypeMismatch,
            format!(
                "{} callback must accept {expected_callback_arity} arguments, got {actual_callback_arity}",
                core_array_name(function)
            ),
            &call_function,
            call_pc,
        ));
    }

    let accumulator = (function == CoreArrayFunction::Fold).then_some(arguments[1]);
    let continuation = ArrayContinuation {
        function,
        source,
        callback,
        next_index: 0,
        accumulator,
        output: Vec::new(),
        return_target,
        trace_frame: RuntimeFrame {
            function: core_array_name(function).into(),
            instruction: 0,
            origin: call_function.origin_at(call_pc),
        },
        call_function,
        call_pc,
    };
    next_array_action(continuation, current, background)
}

fn resume_array_continuation(
    mut continuation: ArrayContinuation,
    value: RuntimeValue,
    current: &mut Heap,
    background: &Heap,
    account: &mut QuotaAccount,
) -> Result<VmAction, RuntimeError> {
    match continuation.function {
        CoreArrayFunction::Length => unreachable!("length does not suspend"),
        CoreArrayFunction::Map => {
            charge_array_output(&continuation, account, 1)?;
            continuation.output.push(value);
        }
        CoreArrayFunction::Filter => match value {
            RuntimeValue::BuiltinAtom(BuiltinAtom::True) => {
                let item = array_item(
                    continuation.source,
                    continuation.next_index - 1,
                    current,
                    background,
                    &continuation.call_function,
                    continuation.call_pc,
                )?;
                charge_array_output(&continuation, account, 1)?;
                continuation.output.push(item);
            }
            RuntimeValue::BuiltinAtom(BuiltinAtom::False) => {}
            _ => {
                return Err(error(
                    RuntimeErrorKind::TypeMismatch,
                    "core:array.filter predicate must return 'True or 'False",
                    &continuation.call_function,
                    continuation.call_pc,
                ));
            }
        },
        CoreArrayFunction::FlatMap => {
            let RuntimeValue::Array(handle) = value else {
                return Err(error(
                    RuntimeErrorKind::TypeMismatch,
                    "core:array.flat_map callback must return an Array",
                    &continuation.call_function,
                    continuation.call_pc,
                ));
            };
            let view = HeapView {
                current,
                background: Some(background),
            };
            let values = view
                .sequence(handle, false)
                .map_err(|heap_error| {
                    error(
                        RuntimeErrorKind::InvalidBytecode,
                        heap_error.to_string(),
                        &continuation.call_function,
                        continuation.call_pc,
                    )
                })?
                .to_vec();
            charge_array_output(&continuation, account, values.len())?;
            continuation.output.extend(values);
        }
        CoreArrayFunction::Fold => continuation.accumulator = Some(value),
    }
    next_array_action(continuation, current, background)
}

fn next_array_action(
    mut continuation: ArrayContinuation,
    current: &mut Heap,
    background: &Heap,
) -> Result<VmAction, RuntimeError> {
    let RuntimeValue::Array(handle) = continuation.source else {
        unreachable!("validated Array continuation source")
    };
    let view = HeapView {
        current,
        background: Some(background),
    };
    let length = view
        .sequence(handle, false)
        .map_err(|heap_error| {
            error(
                RuntimeErrorKind::InvalidBytecode,
                heap_error.to_string(),
                &continuation.call_function,
                continuation.call_pc,
            )
        })?
        .len();
    if continuation.next_index >= length {
        let value = if continuation.function == CoreArrayFunction::Fold {
            continuation
                .accumulator
                .expect("fold continuation has an accumulator")
        } else {
            RuntimeValue::Array(current.allocate(Object::Array(continuation.output.into())))
        };
        return Ok(VmAction::Return {
            value,
            return_target: continuation.return_target,
        });
    }

    let item = array_item(
        continuation.source,
        continuation.next_index,
        current,
        background,
        &continuation.call_function,
        continuation.call_pc,
    )?;
    continuation.next_index += 1;
    let arguments = if continuation.function == CoreArrayFunction::Fold {
        vec![
            continuation
                .accumulator
                .expect("fold continuation has an accumulator"),
            item,
        ]
    } else {
        vec![item]
    };
    let callee = continuation.callback;
    let call_function = Arc::clone(&continuation.call_function);
    let call_pc = continuation.call_pc;
    Ok(VmAction::Call {
        callee,
        arguments,
        return_target: ReturnTarget::Native(Box::new(continuation)),
        call_function,
        call_pc,
    })
}

fn array_item(
    source: RuntimeValue,
    index: usize,
    current: &Heap,
    background: &Heap,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<RuntimeValue, RuntimeError> {
    let RuntimeValue::Array(handle) = source else {
        unreachable!("validated Array source")
    };
    HeapView {
        current,
        background: Some(background),
    }
    .sequence(handle, false)
    .map_err(|heap_error| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            heap_error.to_string(),
            function,
            pc,
        )
    })?
    .get(index)
    .copied()
    .ok_or_else(|| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            "Array continuation index is out of bounds",
            function,
            pc,
        )
    })
}

fn charge_array_output(
    continuation: &ArrayContinuation,
    account: &mut QuotaAccount,
    count: usize,
) -> Result<(), RuntimeError> {
    let bytes = logical_value_bytes(count).map_err(|native_error| {
        allocation_error(
            native_error.message,
            &continuation.call_function,
            continuation.call_pc,
        )
    })?;
    charge_allocation(
        account,
        bytes,
        &continuation.call_function,
        continuation.call_pc,
    )
}

const fn core_array_name(function: CoreArrayFunction) -> &'static str {
    function.name()
}

#[allow(clippy::too_many_arguments)]
fn run_core_dict(
    operation: CoreDictFunction,
    arguments: &[RuntimeValue],
    return_target: ReturnTarget,
    function: &BytecodeFunction,
    pc: usize,
    current: &mut Heap,
    background: &Heap,
    account: &mut QuotaAccount,
) -> Result<VmAction, RuntimeError> {
    let value = match operation {
        CoreDictFunction::Keys => {
            let entries =
                core_dict_entries(arguments[0], "Dict", function, pc, current, background)?;
            charge_core_dict_output(
                entries.len(),
                entries.iter().map(|(field, _)| field.len()),
                function,
                pc,
                account,
            )?;
            let values = entries
                .into_iter()
                .map(|(field, _)| current.string(Some(background), &field))
                .collect::<Box<[_]>>();
            RuntimeValue::Array(current.allocate(Object::Array(values)))
        }
        CoreDictFunction::Values => {
            let entries =
                core_dict_entries(arguments[0], "Dict", function, pc, current, background)?;
            charge_core_dict_output(entries.len(), std::iter::empty(), function, pc, account)?;
            let values = entries
                .into_iter()
                .map(|(_, value)| value)
                .collect::<Box<[_]>>();
            RuntimeValue::Array(current.allocate(Object::Array(values)))
        }
        CoreDictFunction::Pairs => {
            let entries =
                core_dict_entries(arguments[0], "Dict", function, pc, current, background)?;
            let slot_count = entries.len().checked_mul(3).ok_or_else(|| {
                allocation_error("core:dict.pairs allocation size overflowed", function, pc)
            })?;
            charge_core_dict_output(
                slot_count,
                entries.iter().map(|(field, _)| field.len()),
                function,
                pc,
                account,
            )?;
            let pairs = entries
                .into_iter()
                .map(|(field, value)| {
                    let field = current.string(Some(background), &field);
                    RuntimeValue::Tuple(current.allocate(Object::Tuple(vec![field, value].into())))
                })
                .collect::<Box<[_]>>();
            RuntimeValue::Array(current.allocate(Object::Array(pairs)))
        }
        CoreDictFunction::FromPairs => {
            let RuntimeValue::Array(handle) = arguments[0] else {
                let view = HeapView {
                    current,
                    background: Some(background),
                };
                return Err(runtime_type_error(
                    "Array",
                    &arguments[0],
                    &view,
                    function,
                    pc,
                ));
            };
            let view = HeapView {
                current,
                background: Some(background),
            };
            let items = view
                .sequence(handle, false)
                .map_err(|heap_error| core_dict_heap_error(heap_error, function, pc))?;
            let mut entries = Vec::with_capacity(items.len());
            for (index, item) in items.iter().copied().enumerate() {
                let RuntimeValue::Tuple(pair) = item else {
                    return Err(error(
                        RuntimeErrorKind::TypeMismatch,
                        format!("core:dict.from_pairs item {index} must be a two-element Tuple"),
                        function,
                        pc,
                    ));
                };
                let pair = view
                    .sequence(pair, true)
                    .map_err(|heap_error| core_dict_heap_error(heap_error, function, pc))?;
                if pair.len() != 2 {
                    return Err(error(
                        RuntimeErrorKind::TypeMismatch,
                        format!("core:dict.from_pairs item {index} must be a two-element Tuple"),
                        function,
                        pc,
                    ));
                }
                let Some(field) = view
                    .string_text(pair[0])
                    .map_err(|heap_error| core_dict_heap_error(heap_error, function, pc))?
                else {
                    return Err(error(
                        RuntimeErrorKind::TypeMismatch,
                        format!("core:dict.from_pairs item {index} key must be a String"),
                        function,
                        pc,
                    ));
                };
                entries.push((field.to_owned(), pair[1]));
            }
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            if let Some(duplicate) = entries
                .windows(2)
                .find(|pair| pair[0].0 == pair[1].0)
                .map(|pair| pair[0].0.as_str())
            {
                return Err(error(
                    RuntimeErrorKind::TypeMismatch,
                    format!("core:dict.from_pairs contains duplicate field {duplicate:?}"),
                    function,
                    pc,
                ));
            }
            allocate_core_dict(entries, function, pc, current, account)?
        }
        CoreDictFunction::Merge => {
            let left =
                core_dict_entries(arguments[0], "left Dict", function, pc, current, background)?;
            let right = core_dict_entries(
                arguments[1],
                "right Dict",
                function,
                pc,
                current,
                background,
            )?;
            let mut merged = Vec::with_capacity(left.len().saturating_add(right.len()));
            let (mut left_index, mut right_index) = (0, 0);
            while left_index < left.len() && right_index < right.len() {
                match left[left_index].0.cmp(&right[right_index].0) {
                    std::cmp::Ordering::Less => {
                        merged.push(left[left_index].clone());
                        left_index += 1;
                    }
                    std::cmp::Ordering::Equal => {
                        merged.push(right[right_index].clone());
                        left_index += 1;
                        right_index += 1;
                    }
                    std::cmp::Ordering::Greater => {
                        merged.push(right[right_index].clone());
                        right_index += 1;
                    }
                }
            }
            merged.extend_from_slice(&left[left_index..]);
            merged.extend_from_slice(&right[right_index..]);
            allocate_core_dict(merged, function, pc, current, account)?
        }
    };
    Ok(VmAction::Return {
        value,
        return_target,
    })
}

fn core_dict_entries(
    value: RuntimeValue,
    expected: &str,
    function: &BytecodeFunction,
    pc: usize,
    current: &Heap,
    background: &Heap,
) -> Result<Vec<(String, RuntimeValue)>, RuntimeError> {
    let view = HeapView {
        current,
        background: Some(background),
    };
    let RuntimeValue::Dict(handle) = value else {
        return Err(runtime_type_error(expected, &value, &view, function, pc));
    };
    let (fields, values) = view
        .dict_parts(handle)
        .map_err(|heap_error| core_dict_heap_error(heap_error, function, pc))?;
    fields
        .iter()
        .zip(values)
        .map(|(field, value)| {
            Ok((
                view.text(*field)
                    .map_err(|heap_error| core_dict_heap_error(heap_error, function, pc))?
                    .to_owned(),
                *value,
            ))
        })
        .collect()
}

fn allocate_core_dict(
    entries: Vec<(String, RuntimeValue)>,
    function: &BytecodeFunction,
    pc: usize,
    current: &mut Heap,
    account: &mut QuotaAccount,
) -> Result<RuntimeValue, RuntimeError> {
    charge_core_dict_output(
        entries.len(),
        entries.iter().map(|(field, _)| field.len()),
        function,
        pc,
        account,
    )?;
    let (fields, values): (Vec<_>, Vec<_>) = entries
        .into_iter()
        .map(|(field, value)| (current.intern(&field), value))
        .unzip();
    let shape = current.intern_shape(fields);
    Ok(RuntimeValue::Dict(current.allocate(Object::Dict {
        shape,
        values: values.into(),
    })))
}

fn charge_core_dict_output(
    value_slots: usize,
    mut text_lengths: impl Iterator<Item = usize>,
    function: &BytecodeFunction,
    pc: usize,
    account: &mut QuotaAccount,
) -> Result<(), RuntimeError> {
    let text_bytes = text_lengths.try_fold(0u64, |total, length| {
        total
            .checked_add(length as u64)
            .ok_or_else(|| allocation_error("core:dict allocation size overflowed", function, pc))
    })?;
    let value_bytes = logical_value_bytes(value_slots)
        .map_err(|native_error| allocation_error(native_error.message, function, pc))?;
    let bytes = text_bytes
        .checked_add(value_bytes)
        .ok_or_else(|| allocation_error("core:dict allocation size overflowed", function, pc))?;
    charge_allocation(account, bytes, function, pc)
}

fn core_dict_heap_error(
    heap_error: crate::heap::HeapError,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    error(
        RuntimeErrorKind::InvalidBytecode,
        heap_error.to_string(),
        function,
        pc,
    )
}

#[derive(Clone, Debug)]
enum CodecType {
    Any,
    Int,
    Float,
    String,
    Bytes,
    Atom(String),
    Array(Box<Self>),
    Tuple(Vec<Self>),
    Struct(BTreeMap<String, Self>),
    Union(Vec<Self>),
    Function,
}

#[derive(Clone, Debug)]
enum CodecNode {
    Existing(RuntimeValue),
    Atom(BuiltinAtom),
    Array(Vec<Self>),
    Tuple(Vec<Self>),
    Dict(Vec<(String, Self)>),
    String(String),
}

#[derive(Clone, Copy)]
enum CodecDirection {
    Decode,
    Encode,
}

#[allow(clippy::too_many_arguments)]
fn run_core_codec(
    operation: CoreCodecFunction,
    arguments: &[RuntimeValue],
    return_target: ReturnTarget,
    function: &BytecodeFunction,
    pc: usize,
    current: &mut Heap,
    background: &Heap,
    account: &mut QuotaAccount,
) -> Result<VmAction, RuntimeError> {
    let schema = decode_runtime_type(arguments[0], current, background)
        .map_err(|message| error(RuntimeErrorKind::TypeMismatch, message, function, pc))?;
    let direction = match operation {
        CoreCodecFunction::Decode => CodecDirection::Decode,
        CoreCodecFunction::Encode => CodecDirection::Encode,
    };
    let result = transform_codec(&schema, arguments[1], direction, "$", current, background);
    let (tag, payload) = match result {
        Ok(node) => (BuiltinAtom::Ok, node),
        Err(message) => (BuiltinAtom::Err, CodecNode::String(message)),
    };
    let bytes = codec_node_bytes(&payload)
        .and_then(|bytes| {
            bytes
                .checked_add(logical_value_bytes(2)?)
                .ok_or_else(|| NativeError::allocation_limit("codec Result size overflowed"))
        })
        .map_err(|native_error| allocation_error(native_error.message, function, pc))?;
    charge_allocation(account, bytes, function, pc)?;
    let payload = materialize_codec_node(payload, current, background);
    let value = RuntimeValue::Tuple(current.allocate(Object::Tuple(
        vec![RuntimeValue::BuiltinAtom(tag), payload].into(),
    )));
    Ok(VmAction::Return {
        value,
        return_target,
    })
}

fn decode_runtime_type(
    value: RuntimeValue,
    current: &Heap,
    background: &Heap,
) -> Result<CodecType, String> {
    decode_runtime_type_at(value, "Type", current, background)
}

fn decode_runtime_type_at(
    value: RuntimeValue,
    path: &str,
    current: &Heap,
    background: &Heap,
) -> Result<CodecType, String> {
    let view = HeapView {
        current,
        background: Some(background),
    };
    let RuntimeValue::Dict(handle) = value else {
        return Err(format!("{path} must be Type metadata"));
    };
    let kind = view
        .dict_get_text(handle, "kind")
        .map_err(|error| error.to_string())?
        .and_then(|kind| view.atom_text(kind).ok().flatten())
        .ok_or_else(|| format!("{path}.kind must be an Atom"))?;
    Ok(match kind {
        "Any" => CodecType::Any,
        "Int" => CodecType::Int,
        "Float" => CodecType::Float,
        "String" => CodecType::String,
        "Bytes" => CodecType::Bytes,
        "Atom" => {
            let tag = view
                .dict_get_text(handle, "tag")
                .map_err(|error| error.to_string())?
                .and_then(|tag| view.atom_text(tag).ok().flatten())
                .ok_or_else(|| format!("{path}.tag must be an Atom"))?;
            CodecType::Atom(tag.to_owned())
        }
        "Array" => {
            let item = view
                .dict_get_text(handle, "item")
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{path}.item is missing"))?;
            CodecType::Array(Box::new(decode_runtime_type_at(
                item,
                &format!("{path}.item"),
                current,
                background,
            )?))
        }
        "Tuple" | "Union" => {
            let field = if kind == "Tuple" { "items" } else { "variants" };
            let items = view
                .dict_get_text(handle, field)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{path}.{field} is missing"))?;
            let RuntimeValue::Array(items) = items else {
                return Err(format!("{path}.{field} must be an Array"));
            };
            let items = view
                .sequence(items, false)
                .map_err(|error| error.to_string())?
                .to_vec();
            let decoded = items
                .into_iter()
                .enumerate()
                .map(|(index, item)| {
                    decode_runtime_type_at(
                        item,
                        &format!("{path}.{field}[{index}]"),
                        current,
                        background,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if kind == "Tuple" {
                CodecType::Tuple(decoded)
            } else {
                CodecType::Union(decoded)
            }
        }
        "Struct" => {
            let fields = view
                .dict_get_text(handle, "fields")
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("{path}.fields is missing"))?;
            let RuntimeValue::Dict(fields) = fields else {
                return Err(format!("{path}.fields must be a Dict"));
            };
            let (names, values) = view.dict_parts(fields).map_err(|error| error.to_string())?;
            let entries = names
                .iter()
                .zip(values)
                .map(|(name, value)| Ok((view.text(*name)?.to_owned(), *value)))
                .collect::<Result<Vec<_>, crate::heap::HeapError>>()
                .map_err(|error| error.to_string())?;
            CodecType::Struct(
                entries
                    .into_iter()
                    .map(|(name, value)| {
                        let field = decode_runtime_type_at(
                            value,
                            &format!("{path}.fields.{name}"),
                            current,
                            background,
                        )?;
                        Ok((name, field))
                    })
                    .collect::<Result<_, String>>()?,
            )
        }
        "Function" => CodecType::Function,
        other => return Err(format!("{path}.kind has unsupported value '{other}")),
    })
}

fn option_item(schema: &CodecType) -> Option<&CodecType> {
    let CodecType::Union(variants) = schema else {
        return None;
    };
    if variants.len() != 2 {
        return None;
    }
    let none = variants
        .iter()
        .any(|variant| matches!(variant, CodecType::Atom(tag) if tag == "None"));
    let some = variants.iter().find_map(|variant| {
        let CodecType::Tuple(items) = variant else {
            return None;
        };
        match items.as_slice() {
            [CodecType::Atom(tag), item] if tag == "Some" => Some(item),
            _ => None,
        }
    });
    none.then_some(some).flatten()
}

fn transform_codec(
    schema: &CodecType,
    value: RuntimeValue,
    direction: CodecDirection,
    path: &str,
    current: &Heap,
    background: &Heap,
) -> Result<CodecNode, String> {
    let view = HeapView {
        current,
        background: Some(background),
    };
    match schema {
        CodecType::Any => Ok(CodecNode::Existing(value)),
        CodecType::Int if matches!(value, RuntimeValue::Int(_)) => Ok(CodecNode::Existing(value)),
        CodecType::Float if matches!(value, RuntimeValue::Float(_)) => {
            Ok(CodecNode::Existing(value))
        }
        CodecType::String
            if view
                .string_text(value)
                .map_err(|e| e.to_string())?
                .is_some() =>
        {
            Ok(CodecNode::Existing(value))
        }
        CodecType::Atom(expected) => {
            let actual = view.atom_text(value).map_err(|e| e.to_string())?;
            if actual == Some(expected) {
                Ok(CodecNode::Existing(value))
            } else {
                Err(format!("{path}: expected '{expected}"))
            }
        }
        CodecType::Array(item) => {
            let RuntimeValue::Array(handle) = value else {
                return Err(format!("{path}: expected Array"));
            };
            let values = view
                .sequence(handle, false)
                .map_err(|error| error.to_string())?
                .to_vec();
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    transform_codec(
                        item,
                        value,
                        direction,
                        &format!("{path}[{index}]"),
                        current,
                        background,
                    )
                })
                .collect::<Result<Vec<_>, _>>()
                .map(CodecNode::Array)
        }
        CodecType::Tuple(items) => {
            let (handle, input_is_tuple) = match (direction, value) {
                (CodecDirection::Decode, RuntimeValue::Array(handle)) => (handle, false),
                (CodecDirection::Encode, RuntimeValue::Tuple(handle)) => (handle, true),
                (CodecDirection::Decode, _) => return Err(format!("{path}: expected Array")),
                (CodecDirection::Encode, _) => return Err(format!("{path}: expected Tuple")),
            };
            let values = view
                .sequence(handle, input_is_tuple)
                .map_err(|error| error.to_string())?
                .to_vec();
            if values.len() != items.len() {
                return Err(format!("{path}: expected {} items", items.len()));
            }
            let nodes = items
                .iter()
                .zip(values)
                .enumerate()
                .map(|(index, (item, value))| {
                    transform_codec(
                        item,
                        value,
                        direction,
                        &format!("{path}[{index}]"),
                        current,
                        background,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(match direction {
                CodecDirection::Decode => CodecNode::Tuple(nodes),
                CodecDirection::Encode => CodecNode::Array(nodes),
            })
        }
        CodecType::Struct(fields) => {
            transform_codec_struct(fields, value, direction, path, current, background)
        }
        CodecType::Union(variants) => {
            let mut errors = Vec::new();
            for variant in variants {
                match transform_codec(variant, value, direction, path, current, background) {
                    Ok(node) => return Ok(node),
                    Err(message) => errors.push(message),
                }
            }
            Err(format!(
                "{path}: value matches no Union variant ({})",
                errors.join("; ")
            ))
        }
        CodecType::Bytes => Err(format!("{path}: Bytes has no JSON codec")),
        CodecType::Function => Err(format!("{path}: Function has no JSON codec")),
        expected => Err(format!("{path}: expected {}", codec_type_name(expected))),
    }
}

fn transform_codec_struct(
    fields: &BTreeMap<String, CodecType>,
    value: RuntimeValue,
    direction: CodecDirection,
    path: &str,
    current: &Heap,
    background: &Heap,
) -> Result<CodecNode, String> {
    let RuntimeValue::Dict(handle) = value else {
        return Err(format!("{path}: expected Dict"));
    };
    let view = HeapView {
        current,
        background: Some(background),
    };
    let (names, values) = view.dict_parts(handle).map_err(|error| error.to_string())?;
    let input = names
        .iter()
        .zip(values)
        .map(|(name, value)| Ok((view.text(*name)?.to_owned(), *value)))
        .collect::<Result<BTreeMap<_, _>, crate::heap::HeapError>>()
        .map_err(|error| error.to_string())?;
    if let Some(unknown) = input.keys().find(|name| !fields.contains_key(*name)) {
        return Err(format!("{path}.{unknown}: unknown field"));
    }
    let mut output = Vec::with_capacity(fields.len());
    for (name, field) in fields {
        let field_path = format!("{path}.{name}");
        let node = match (direction, input.get(name).copied(), option_item(field)) {
            (CodecDirection::Decode, None, Some(_)) => CodecNode::Atom(BuiltinAtom::None),
            (_, None, _) => return Err(format!("{field_path}: missing required field")),
            (
                CodecDirection::Decode,
                Some(RuntimeValue::BuiltinAtom(BuiltinAtom::None)),
                Some(_),
            ) => CodecNode::Atom(BuiltinAtom::None),
            (CodecDirection::Decode, Some(value), Some(item)) => CodecNode::Tuple(vec![
                CodecNode::Atom(BuiltinAtom::Some),
                transform_codec(item, value, direction, &field_path, current, background)?,
            ]),
            (
                CodecDirection::Encode,
                Some(RuntimeValue::BuiltinAtom(BuiltinAtom::None)),
                Some(_),
            ) => CodecNode::Atom(BuiltinAtom::None),
            (CodecDirection::Encode, Some(RuntimeValue::Tuple(handle)), Some(item)) => {
                let tuple = view
                    .sequence(handle, true)
                    .map_err(|error| error.to_string())?;
                if tuple.len() != 2
                    || view.atom_text(tuple[0]).map_err(|e| e.to_string())? != Some("Some")
                {
                    return Err(format!("{field_path}: expected Option"));
                }
                transform_codec(item, tuple[1], direction, &field_path, current, background)?
            }
            (CodecDirection::Encode, Some(_), Some(_)) => {
                return Err(format!("{field_path}: expected Option"));
            }
            (_, Some(value), None) => {
                transform_codec(field, value, direction, &field_path, current, background)?
            }
        };
        output.push((name.clone(), node));
    }
    Ok(CodecNode::Dict(output))
}

fn codec_type_name(schema: &CodecType) -> &'static str {
    match schema {
        CodecType::Any => "Any",
        CodecType::Int => "Int",
        CodecType::Float => "Float",
        CodecType::String => "String",
        CodecType::Bytes => "Bytes",
        CodecType::Atom(_) => "Atom",
        CodecType::Array(_) => "Array",
        CodecType::Tuple(_) => "Tuple",
        CodecType::Struct(_) => "Struct",
        CodecType::Union(_) => "Union",
        CodecType::Function => "Function",
    }
}

fn codec_node_bytes(node: &CodecNode) -> Result<u64, NativeError> {
    match node {
        CodecNode::Existing(_) | CodecNode::Atom(_) => Ok(0),
        CodecNode::String(value) => Ok(value.len() as u64),
        CodecNode::Array(items) | CodecNode::Tuple(items) => {
            let own = logical_value_bytes(items.len())?;
            items.iter().try_fold(own, |total, item| {
                total
                    .checked_add(codec_node_bytes(item)?)
                    .ok_or_else(|| NativeError::allocation_limit("codec output size overflowed"))
            })
        }
        CodecNode::Dict(fields) => {
            let own = logical_value_bytes(fields.len())?;
            fields.iter().try_fold(own, |total, (name, value)| {
                let value_bytes = codec_node_bytes(value)?;
                total
                    .checked_add(name.len() as u64)
                    .and_then(|total| total.checked_add(value_bytes))
                    .ok_or_else(|| NativeError::allocation_limit("codec output size overflowed"))
            })
        }
    }
}

fn materialize_codec_node(node: CodecNode, current: &mut Heap, background: &Heap) -> RuntimeValue {
    match node {
        CodecNode::Existing(value) => value,
        CodecNode::Atom(atom) => RuntimeValue::BuiltinAtom(atom),
        CodecNode::String(value) => current.string(Some(background), &value),
        CodecNode::Array(items) => {
            let items = items
                .into_iter()
                .map(|item| materialize_codec_node(item, current, background))
                .collect::<Box<_>>();
            RuntimeValue::Array(current.allocate(Object::Array(items)))
        }
        CodecNode::Tuple(items) => {
            let items = items
                .into_iter()
                .map(|item| materialize_codec_node(item, current, background))
                .collect::<Box<_>>();
            RuntimeValue::Tuple(current.allocate(Object::Tuple(items)))
        }
        CodecNode::Dict(fields) => {
            let (fields, values): (Vec<_>, Vec<_>) = fields
                .into_iter()
                .map(|(name, value)| {
                    (
                        current.intern(&name),
                        materialize_codec_node(value, current, background),
                    )
                })
                .unzip();
            let shape = current.intern_shape(fields);
            RuntimeValue::Dict(current.allocate(Object::Dict {
                shape,
                values: values.into(),
            }))
        }
    }
}

fn run_core_result(
    _operation: CoreResultFunction,
    arguments: &[RuntimeValue],
    return_target: ReturnTarget,
    function: &BytecodeFunction,
    pc: usize,
    current: &Heap,
    background: &Heap,
) -> Result<VmAction, RuntimeError> {
    let view = HeapView {
        current,
        background: Some(background),
    };
    let RuntimeValue::Tuple(handle) = arguments[0] else {
        return Err(error(
            RuntimeErrorKind::TypeMismatch,
            "core:result.unwrap expects ('Ok, value) or ('Err, message)",
            function,
            pc,
        ));
    };
    let tuple = view
        .sequence(handle, true)
        .map_err(|heap_error| core_dict_heap_error(heap_error, function, pc))?;
    if tuple.len() != 2 {
        return Err(error(
            RuntimeErrorKind::TypeMismatch,
            "core:result.unwrap expects a two-element Result",
            function,
            pc,
        ));
    }
    match view.atom_text(tuple[0]).map_err(|heap_error| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            heap_error.to_string(),
            function,
            pc,
        )
    })? {
        Some("Ok") => Ok(VmAction::Return {
            value: tuple[1],
            return_target,
        }),
        Some("Err") => {
            let message = view
                .string_text(tuple[1])
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
                        RuntimeErrorKind::TypeMismatch,
                        "core:result.unwrap Err payload must be a String",
                        function,
                        pc,
                    )
                })?;
            Err(error(RuntimeErrorKind::TypeMismatch, message, function, pc))
        }
        _ => Err(error(
            RuntimeErrorKind::TypeMismatch,
            "core:result.unwrap expects ('Ok, value) or ('Err, message)",
            function,
            pc,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_core_json(
    operation: CoreJsonFunction,
    arguments: &[RuntimeValue],
    upvalues: &[RuntimeValue],
    return_target: ReturnTarget,
    function: &BytecodeFunction,
    pc: usize,
    current: &mut Heap,
    background: &Heap,
    account: &mut QuotaAccount,
) -> Result<VmAction, RuntimeError> {
    if operation == CoreJsonFunction::StringifyPretty {
        let RuntimeValue::Int(indent) = arguments[0] else {
            let view = HeapView {
                current,
                background: Some(background),
            };
            return Err(runtime_type_error(
                "Int",
                &arguments[0],
                &view,
                function,
                pc,
            ));
        };
        if !(0..=16).contains(&indent) {
            return Err(error(
                RuntimeErrorKind::TypeMismatch,
                "core:json.stringify_pretty indent must be between 0 and 16",
                function,
                pc,
            ));
        }
        charge_allocation(
            account,
            logical_value_bytes(1).map_err(|e| allocation_error(e.message, function, pc))?,
            function,
            pc,
        )?;
        let closure = RuntimeValue::Func(current.allocate(Object::Closure {
            identity: Arc::new(()),
            prototype: crate::heap::RuntimePrototype::Native(crate::NativeFunction::core_json(
                CoreJsonFunction::StringifyPrettyValue,
            )),
            upvalues: vec![RuntimeValue::Int(indent)].into(),
        }));
        return Ok(VmAction::Return {
            value: closure,
            return_target,
        });
    }
    let indent = match operation {
        CoreJsonFunction::Stringify => None,
        CoreJsonFunction::StringifyPrettyValue => match upvalues {
            [RuntimeValue::Int(indent)] => Some(*indent as usize),
            _ => {
                return Err(error(
                    RuntimeErrorKind::InvalidBytecode,
                    "configured JSON formatter has invalid upvalues",
                    function,
                    pc,
                ));
            }
        },
        CoreJsonFunction::StringifyPretty => unreachable!(),
    };
    let view = HeapView {
        current,
        background: Some(background),
    };
    let mut writer = JsonWriter::new(view, indent);
    writer
        .value(arguments[0], 0)
        .map_err(|message| error(RuntimeErrorKind::TypeMismatch, message, function, pc))?;
    let output = writer.output;
    charge_allocation(account, output.len() as u64, function, pc)?;
    let value = current.string(Some(background), &output);
    Ok(VmAction::Return {
        value,
        return_target,
    })
}

struct JsonWriter<'a> {
    view: HeapView<'a>,
    indent: Option<usize>,
    output: String,
    active: HashSet<Handle>,
}

impl<'a> JsonWriter<'a> {
    fn new(view: HeapView<'a>, indent: Option<usize>) -> Self {
        Self {
            view,
            indent,
            output: String::new(),
            active: HashSet::new(),
        }
    }

    fn value(&mut self, value: RuntimeValue, depth: usize) -> Result<(), String> {
        match value {
            RuntimeValue::Int(value) => self.output.push_str(&value.to_string()),
            RuntimeValue::Float(value) if value.is_finite() => {
                self.output.push_str(&value.to_string())
            }
            RuntimeValue::Float(_) => return Err("JSON cannot encode a non-finite Float".into()),
            RuntimeValue::BuiltinAtom(BuiltinAtom::None) => self.output.push_str("null"),
            RuntimeValue::BuiltinAtom(BuiltinAtom::True) => self.output.push_str("true"),
            RuntimeValue::BuiltinAtom(BuiltinAtom::False) => self.output.push_str("false"),
            RuntimeValue::ShortString(id) => {
                self.string(self.view.text(id).map_err(|e| e.to_string())?)
            }
            RuntimeValue::String(handle) => {
                match self.view.object(handle).map_err(|e| e.to_string())? {
                    Object::String(value) => self.string(value),
                    _ => return Err("invalid String handle".into()),
                }
            }
            RuntimeValue::Array(handle) => self.array(handle, depth)?,
            RuntimeValue::Dict(handle) => self.dict(handle, depth)?,
            RuntimeValue::BuiltinAtom(atom) => {
                return Err(format!("JSON cannot encode '{}", atom.name()));
            }
            RuntimeValue::Atom(id) => {
                return Err(format!(
                    "JSON cannot encode '{}",
                    self.view.text(id).map_err(|e| e.to_string())?
                ));
            }
            RuntimeValue::Bytes(_) => return Err("JSON cannot encode Bytes".into()),
            RuntimeValue::Tuple(_) => {
                return Err("JSON cannot encode Tuple; use a codec first".into());
            }
            RuntimeValue::Func(_) => return Err("JSON cannot encode Func".into()),
            RuntimeValue::UpLink(_) => return Err("JSON cannot encode an internal up-link".into()),
        }
        Ok(())
    }

    fn array(&mut self, handle: Handle, depth: usize) -> Result<(), String> {
        if !self.active.insert(handle) {
            return Err("JSON cannot encode cyclic values".into());
        }
        let values = self
            .view
            .sequence(handle, false)
            .map_err(|e| e.to_string())?
            .to_vec();
        self.output.push('[');
        for (index, value) in values.into_iter().enumerate() {
            self.separator(index, depth + 1);
            self.value(value, depth + 1)?;
        }
        self.close_collection(values_len_hint(handle, &self.view, false)?, depth, ']');
        self.active.remove(&handle);
        Ok(())
    }

    fn dict(&mut self, handle: Handle, depth: usize) -> Result<(), String> {
        if !self.active.insert(handle) {
            return Err("JSON cannot encode cyclic values".into());
        }
        let (fields, values) = self.view.dict_parts(handle).map_err(|e| e.to_string())?;
        let entries = fields
            .iter()
            .zip(values)
            .map(|(field, value)| Ok((self.view.text(*field)?.to_owned(), *value)))
            .collect::<Result<Vec<_>, crate::heap::HeapError>>()
            .map_err(|e| e.to_string())?;
        self.output.push('{');
        for (index, (field, value)) in entries.iter().enumerate() {
            self.separator(index, depth + 1);
            self.string(field);
            self.output.push(':');
            if self.indent.is_some() {
                self.output.push(' ');
            }
            self.value(*value, depth + 1)?;
        }
        self.close_collection(entries.len(), depth, '}');
        self.active.remove(&handle);
        Ok(())
    }

    fn separator(&mut self, index: usize, depth: usize) {
        if index > 0 {
            self.output.push(',');
        }
        if let Some(indent) = self.indent {
            self.output.push('\n');
            self.output
                .extend(std::iter::repeat_n(' ', indent.saturating_mul(depth)));
        }
    }

    fn close_collection(&mut self, len: usize, depth: usize, close: char) {
        if len > 0
            && let Some(indent) = self.indent
        {
            self.output.push('\n');
            self.output
                .extend(std::iter::repeat_n(' ', indent.saturating_mul(depth)));
        }
        self.output.push(close);
    }

    fn string(&mut self, value: &str) {
        self.output.push('"');
        for character in value.chars() {
            match character {
                '"' => self.output.push_str("\\\""),
                '\\' => self.output.push_str("\\\\"),
                '\u{08}' => self.output.push_str("\\b"),
                '\u{0c}' => self.output.push_str("\\f"),
                '\n' => self.output.push_str("\\n"),
                '\r' => self.output.push_str("\\r"),
                '\t' => self.output.push_str("\\t"),
                c if c <= '\u{1f}' => {
                    let _ = write!(self.output, "\\u{:04x}", c as u32);
                }
                c => self.output.push(c),
            }
        }
        self.output.push('"');
    }
}

fn values_len_hint(handle: Handle, view: &HeapView<'_>, tuple: bool) -> Result<usize, String> {
    view.sequence(handle, tuple)
        .map(|values| values.len())
        .map_err(|e| e.to_string())
}

const DEBUG_MAX_DEPTH: usize = 8;
const DEBUG_MAX_ITEMS: usize = 32;
const DEBUG_MAX_BYTES: usize = 4_096;
const DEBUG_MAX_LABEL_BYTES: usize = 256;

#[allow(clippy::too_many_arguments)]
fn run_core_debug(
    operation: CoreDebugFunction,
    arguments: &[RuntimeValue],
    return_target: ReturnTarget,
    function: &BytecodeFunction,
    pc: usize,
    current: &Heap,
    background: &Heap,
    sink: &dyn DebugSink,
) -> Result<VmAction, RuntimeError> {
    let view = HeapView {
        current,
        background: Some(background),
    };
    let (label, value) = match operation {
        CoreDebugFunction::Dbg => (None, arguments[0]),
        CoreDebugFunction::DbgWith => {
            let Some(label) = view
                .string_text(arguments[0])
                .map_err(|heap_error| core_debug_heap_error(heap_error, function, pc))?
            else {
                return Err(runtime_type_error(
                    "String",
                    &arguments[0],
                    &view,
                    function,
                    pc,
                ));
            };
            (Some(truncate_debug_label(label)), arguments[1])
        }
    };
    let value_text = DebugValueFormatter::new(view)
        .format(value)
        .map_err(|heap_error| core_debug_heap_error(heap_error, function, pc))?;
    sink.emit(DebugEvent {
        label,
        value: value_text,
    });
    Ok(VmAction::Return {
        value,
        return_target,
    })
}

fn core_debug_heap_error(
    heap_error: crate::heap::HeapError,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    error(
        RuntimeErrorKind::InvalidBytecode,
        format!("core:debug formatter: {heap_error}"),
        function,
        pc,
    )
}

fn truncate_debug_label(label: &str) -> String {
    if label.len() <= DEBUG_MAX_LABEL_BYTES {
        return label.to_owned();
    }
    let mut end = DEBUG_MAX_LABEL_BYTES.saturating_sub(3);
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &label[..end])
}

struct DebugValueFormatter<'a> {
    view: HeapView<'a>,
    output: String,
    active: HashSet<Handle>,
    truncated: bool,
}

impl<'a> DebugValueFormatter<'a> {
    fn new(view: HeapView<'a>) -> Self {
        Self {
            view,
            output: String::new(),
            active: HashSet::new(),
            truncated: false,
        }
    }

    fn format(mut self, value: RuntimeValue) -> Result<String, crate::heap::HeapError> {
        self.value(value, 0)?;
        if self.truncated {
            self.output.push_str("...");
        }
        Ok(self.output)
    }

    fn value(&mut self, value: RuntimeValue, depth: usize) -> Result<(), crate::heap::HeapError> {
        if self.truncated {
            return Ok(());
        }
        match value {
            RuntimeValue::Int(value) => self.push(&value.to_string()),
            RuntimeValue::Float(value) => self.push(&format!("{value:?}")),
            RuntimeValue::BuiltinAtom(atom) => {
                self.push("'");
                self.push(atom.name());
            }
            RuntimeValue::Atom(id) => {
                self.push("'");
                self.push(self.view.text(id)?);
            }
            RuntimeValue::ShortString(id) => self.quoted(self.view.text(id)?),
            RuntimeValue::String(handle) => match self.view.object(handle)? {
                Object::String(value) => self.quoted(value),
                _ => return Err(crate::heap::HeapError::new("invalid String handle")),
            },
            RuntimeValue::Bytes(handle) => match self.view.object(handle)? {
                Object::Bytes(value) => {
                    self.push("b\"");
                    for byte in value.iter().take(DEBUG_MAX_ITEMS) {
                        self.push(&format!("\\x{byte:02x}"));
                    }
                    if value.len() > DEBUG_MAX_ITEMS {
                        self.push("...");
                    }
                    self.push("\"");
                }
                _ => return Err(crate::heap::HeapError::new("invalid Bytes handle")),
            },
            RuntimeValue::Array(handle) => self.sequence(handle, false, depth, "[", "]")?,
            RuntimeValue::Tuple(handle) => self.sequence(handle, true, depth, "(", ")")?,
            RuntimeValue::Dict(handle) => self.dict(handle, depth)?,
            RuntimeValue::Func(handle) => {
                let (prototype, _) = self.view.closure(handle)?;
                let name = match prototype {
                    crate::heap::RuntimePrototype::Native(function) => function.name(),
                    crate::heap::RuntimePrototype::Bytecode(prototype) => {
                        self.view.bytecode(prototype)?.0.name()
                    }
                };
                self.push("<fn ");
                self.push(name);
                self.push(">");
            }
            RuntimeValue::UpLink(handle) => {
                if !self.enter(handle, depth) {
                    return Ok(());
                }
                match self.view.up_link(handle)? {
                    Some(value) => self.value(value, depth + 1)?,
                    None => self.push("<uninitialized up-link>"),
                }
                self.active.remove(&handle);
            }
        }
        Ok(())
    }

    fn sequence(
        &mut self,
        handle: Handle,
        tuple: bool,
        depth: usize,
        open: &str,
        close: &str,
    ) -> Result<(), crate::heap::HeapError> {
        if !self.enter(handle, depth) {
            return Ok(());
        }
        self.push(open);
        let (value_count, values) = {
            let sequence = self.view.sequence(handle, tuple)?;
            (
                sequence.len(),
                sequence
                    .iter()
                    .take(DEBUG_MAX_ITEMS)
                    .copied()
                    .collect::<Vec<_>>(),
            )
        };
        for (index, value) in values.iter().take(DEBUG_MAX_ITEMS).enumerate() {
            if index > 0 {
                self.push(", ");
            }
            self.value(*value, depth + 1)?;
        }
        if value_count > DEBUG_MAX_ITEMS {
            if DEBUG_MAX_ITEMS > 0 {
                self.push(", ");
            }
            self.push("...");
        }
        if tuple && value_count == 1 {
            self.push(",");
        }
        self.push(close);
        self.active.remove(&handle);
        Ok(())
    }

    fn dict(&mut self, handle: Handle, depth: usize) -> Result<(), crate::heap::HeapError> {
        if !self.enter(handle, depth) {
            return Ok(());
        }
        self.push("{");
        let (fields, values) = self.view.dict_parts(handle)?;
        let entries = fields
            .iter()
            .zip(values)
            .take(DEBUG_MAX_ITEMS)
            .map(|(field, value)| Ok((self.view.text(*field)?.to_owned(), *value)))
            .collect::<Result<Vec<_>, crate::heap::HeapError>>()?;
        for (index, (field, value)) in entries.into_iter().enumerate() {
            if index > 0 {
                self.push(", ");
            }
            self.quoted(&field);
            self.push(": ");
            self.value(value, depth + 1)?;
        }
        if values.len() > DEBUG_MAX_ITEMS {
            if DEBUG_MAX_ITEMS > 0 {
                self.push(", ");
            }
            self.push("...");
        }
        self.push("}");
        self.active.remove(&handle);
        Ok(())
    }

    fn enter(&mut self, handle: Handle, depth: usize) -> bool {
        if depth >= DEBUG_MAX_DEPTH {
            self.push("...");
            return false;
        }
        if !self.active.insert(handle) {
            self.push("<cycle>");
            return false;
        }
        true
    }

    fn quoted(&mut self, text: &str) {
        self.push("\"");
        for character in text.chars() {
            for escaped in character.escape_debug() {
                let mut buffer = [0u8; 4];
                self.push(escaped.encode_utf8(&mut buffer));
            }
        }
        self.push("\"");
    }

    fn push(&mut self, text: &str) {
        if self.truncated {
            return;
        }
        let content_limit = DEBUG_MAX_BYTES.saturating_sub(3);
        for character in text.chars() {
            if self.output.len() + character.len_utf8() > content_limit {
                self.truncated = true;
                return;
            }
            self.output.push(character);
        }
    }
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

fn read_call_arguments(
    registers: &[Option<RuntimeValue>],
    base: Register,
    argument_count: usize,
    function: &BytecodeFunction,
    pc: usize,
) -> Result<Vec<RuntimeValue>, RuntimeError> {
    let start = base.0.checked_add(1).ok_or_else(|| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            "call window overflows",
            function,
            pc,
        )
    })?;
    let end = start.checked_add(argument_count).ok_or_else(|| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            "call window overflows",
            function,
            pc,
        )
    })?;
    let arguments = registers.get(start..end).ok_or_else(|| {
        error(
            RuntimeErrorKind::InvalidBytecode,
            "call window is out of bounds",
            function,
            pc,
        )
    })?;
    arguments
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.as_ref().copied().ok_or_else(|| {
                error(
                    RuntimeErrorKind::InvalidBytecode,
                    format!("call argument register {} is uninitialized", start + index),
                    function,
                    pc,
                )
            })
        })
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

fn runtime_shallow_type_error(
    expected: &str,
    actual: RuntimeValue,
    function: &BytecodeFunction,
    pc: usize,
) -> RuntimeError {
    let actual = match actual {
        RuntimeValue::Int(_) => "Int",
        RuntimeValue::Float(_) => "Float",
        RuntimeValue::BuiltinAtom(_) | RuntimeValue::Atom(_) => "Atom",
        RuntimeValue::ShortString(_) | RuntimeValue::String(_) => "String",
        RuntimeValue::Bytes(_) => "Bytes",
        RuntimeValue::Array(_) => "Array",
        RuntimeValue::Tuple(_) => "Tuple",
        RuntimeValue::Dict(_) => "Dict",
        RuntimeValue::Func(_) => "Func",
        RuntimeValue::UpLink(_) => "internal up-link",
    };
    error(
        RuntimeErrorKind::TypeMismatch,
        format!("expected {expected}, got {actual}"),
        function,
        pc,
    )
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

        let invalid_call_window = BytecodeFunction::new(
            "invalid-call-window",
            1,
            vec![Value::Func(Arc::new(Closure::native(NativeFunction::new(
                "identity",
                1,
                native_identity,
            ))))],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::TailCall {
                    base: Register(0),
                    argument_count: 1,
                },
            ],
        );
        let error = Vm::new().execute(&invalid_call_window, 5).unwrap_err();
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
                    base: Register(0),
                    argument_count: 0,
                },
                Instruction::Return { src: Register(0) },
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
                    base: Register(0),
                    argument_count: 0,
                },
                Instruction::Return { src: Register(0) },
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
                    base: Register(0),
                    argument_count: 1,
                },
                Instruction::Return { src: Register(0) },
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

    fn native_identity(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
        let value = context
            .value(context.argument(0)?)?
            .as_int()
            .ok_or_else(|| NativeError::new("expected Int argument"))?;
        context.set_int(context.result(), value)
    }

    #[test]
    fn tail_calls_native_functions_and_replace_bytecode_frames() {
        let native = NativeFunction::new("identity", 1, native_identity);
        let native_tail = BytecodeFunction::new(
            "native-tail",
            2,
            vec![
                Value::Func(Arc::new(Closure::native(native))),
                Value::Int(42),
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
                Instruction::TailCall {
                    base: Register(0),
                    argument_count: 1,
                },
            ],
        );
        assert_eq!(
            Vm::new().execute(&native_tail, 0).unwrap_err().kind,
            RuntimeErrorKind::FuelExhausted
        );
        assert!(matches!(
            Vm::new().execute(&native_tail, 1).unwrap(),
            Value::Int(42)
        ));

        let large = Arc::new(BytecodeFunction::new(
            "large-frame",
            100,
            vec![Value::Int(7)],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::Return { src: Register(0) },
            ],
        ));
        let replace = BytecodeFunction::new(
            "small-frame",
            1,
            vec![Value::Func(Arc::new(Closure::new(large, Vec::new())))],
            vec![
                Instruction::LoadConst {
                    dst: Register(0),
                    constant: 0,
                },
                Instruction::TailCall {
                    base: Register(0),
                    argument_count: 0,
                },
            ],
        );
        assert!(matches!(
            Vm::new()
                .execute_with_quota(&replace, Quota::new(1, 100, u64::MAX))
                .unwrap(),
            Value::Int(7)
        ));
        assert_eq!(
            Vm::new()
                .execute_with_quota(&replace, Quota::new(1, 99, u64::MAX))
                .unwrap_err()
                .kind,
            RuntimeErrorKind::StackLimitExceeded
        );
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
                    base: Register(0),
                    argument_count: 1,
                },
                Instruction::Return { src: Register(0) },
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
                        base: Register(0),
                        argument_count: 0,
                    },
                    Instruction::Return { src: Register(0) },
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
                        base: Register(0),
                        argument_count: 0,
                    },
                    Instruction::Return { src: Register(0) },
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
                        base: Register(0),
                        argument_count: 0,
                    },
                    Instruction::Return { src: Register(0) },
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

    #[test]
    fn debug_formatter_is_cycle_safe_and_bounded() {
        let background = Heap::persistent();
        let mut current = Heap::local();
        let cycle = current.reserve();
        current
            .initialize(
                cycle,
                Object::Array(vec![RuntimeValue::Array(cycle)].into()),
            )
            .unwrap();
        let cycle_text = DebugValueFormatter::new(HeapView {
            current: &current,
            background: Some(&background),
        })
        .format(RuntimeValue::Array(cycle))
        .unwrap();
        assert_eq!(cycle_text, "[<cycle>]");

        let long = current.string(None, &"x".repeat(DEBUG_MAX_BYTES * 2));
        let long_text = DebugValueFormatter::new(HeapView {
            current: &current,
            background: Some(&background),
        })
        .format(long)
        .unwrap();
        assert_eq!(long_text.len(), DEBUG_MAX_BYTES);
        assert!(long_text.ends_with("..."));

        let bytes = RuntimeValue::Bytes(current.allocate(Object::Bytes(
            (0..64).map(|value| value as u8).collect::<Vec<_>>().into(),
        )));
        let bytes_text = DebugValueFormatter::new(HeapView {
            current: &current,
            background: Some(&background),
        })
        .format(bytes)
        .unwrap();
        assert!(bytes_text.starts_with("b\"\\x00\\x01"));
        assert!(bytes_text.contains("..."));
    }

    #[test]
    fn json_writer_rejects_internal_cycles() {
        let background = Heap::persistent();
        let mut current = Heap::local();
        let cycle = current.reserve();
        current
            .initialize(
                cycle,
                Object::Array(vec![RuntimeValue::Array(cycle)].into()),
            )
            .unwrap();
        let mut writer = JsonWriter::new(
            HeapView {
                current: &current,
                background: Some(&background),
            },
            None,
        );
        assert_eq!(
            writer.value(RuntimeValue::Array(cycle), 0).unwrap_err(),
            "JSON cannot encode cyclic values"
        );
    }
}
