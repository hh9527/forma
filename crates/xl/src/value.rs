use crate::bytecode::BytecodeFunction;
use crate::vm::CallContext;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BuiltinAtom {
    None,
    Some,
    Ok,
    Err,
    True,
    False,
}

impl BuiltinAtom {
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Some => "Some",
            Self::Ok => "Ok",
            Self::Err => "Err",
            Self::True => "True",
            Self::False => "False",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Atom {
    Builtin(BuiltinAtom),
    Named(Arc<str>),
}

impl Atom {
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        Self::Named(name.into())
    }

    pub const fn builtin(atom: BuiltinAtom) -> Self {
        Self::Builtin(atom)
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Builtin(atom) => atom.name(),
            Self::Named(name) => name,
        }
    }
}

#[derive(Debug, Eq, Hash, PartialEq)]
pub struct Shape {
    fields: Arc<[String]>,
}

impl Shape {
    pub(crate) fn from_sorted_fields(fields: Vec<String>) -> Self {
        debug_assert!(fields.windows(2).all(|pair| pair[0] < pair[1]));
        Self {
            fields: fields.into(),
        }
    }

    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    pub fn field_index(&self, field: &str) -> Option<usize> {
        self.fields
            .binary_search_by(|candidate| candidate.as_str().cmp(field))
            .ok()
    }
}

#[derive(Clone, Debug)]
pub struct Dict {
    shape: Arc<Shape>,
    values: Arc<[Value]>,
}

#[derive(Clone, Debug)]
pub struct Closure {
    identity: Arc<()>,
    prototype: Prototype,
    upvalues: Arc<[Value]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeError {
    pub message: String,
    limit: Option<NativeLimit>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum NativeLimit {
    Stack,
    Allocation,
}

impl NativeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            limit: None,
        }
    }

    pub(crate) fn stack_limit(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            limit: Some(NativeLimit::Stack),
        }
    }

    pub(crate) fn allocation_limit(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            limit: Some(NativeLimit::Allocation),
        }
    }

    pub(crate) const fn limit(&self) -> Option<NativeLimit> {
        self.limit
    }
}

pub type NativeCallback = fn(&mut CallContext<'_, '_>) -> Result<(), NativeError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreArrayFunction {
    Length,
    Map,
    Filter,
    FlatMap,
    Fold,
}

impl CoreArrayFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Length => "core:array.length",
            Self::Map => "core:array.map",
            Self::Filter => "core:array.filter",
            Self::FlatMap => "core:array.flat_map",
            Self::Fold => "core:array.fold",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Length => 1,
            Self::Map | Self::Filter | Self::FlatMap => 2,
            Self::Fold => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeKind {
    Synchronous,
    CoreArray(CoreArrayFunction),
}

#[derive(Clone, Copy)]
pub struct NativeFunction {
    name: &'static str,
    arity: usize,
    callback: NativeCallback,
    kind: NativeKind,
}

impl NativeFunction {
    pub const fn new(name: &'static str, arity: usize, callback: NativeCallback) -> Self {
        Self {
            name,
            arity,
            callback,
            kind: NativeKind::Synchronous,
        }
    }

    pub(crate) const fn core_array(function: CoreArrayFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreArray(function),
        }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn arity(self) -> usize {
        self.arity
    }

    pub const fn callback(self) -> NativeCallback {
        self.callback
    }

    pub(crate) const fn kind(self) -> NativeKind {
        self.kind
    }
}

fn unavailable_core_callback(_: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    Err(NativeError::new(
        "VM-managed core function cannot use the synchronous native ABI",
    ))
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeFunction")
            .field("name", &self.name)
            .field("arity", &self.arity)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub enum Prototype {
    Bytecode(Arc<BytecodeFunction>),
    Native(NativeFunction),
}

pub type Callable = Prototype;

impl Closure {
    pub(crate) fn from_parts(prototype: Prototype, upvalues: Vec<Value>) -> Self {
        Self {
            identity: Arc::new(()),
            prototype,
            upvalues: upvalues.into(),
        }
    }

    pub(crate) fn from_parts_with_identity(
        identity: Arc<()>,
        prototype: Prototype,
        upvalues: Vec<Value>,
    ) -> Self {
        Self {
            identity,
            prototype,
            upvalues: upvalues.into(),
        }
    }

    pub fn new(function: Arc<BytecodeFunction>, captures: Vec<Value>) -> Self {
        Self::from_parts(Prototype::Bytecode(function), captures)
    }

    pub fn native(function: NativeFunction) -> Self {
        Self::native_with_upvalues(function, Vec::new())
    }

    pub fn native_with_upvalues(function: NativeFunction, upvalues: Vec<Value>) -> Self {
        Self::from_parts(Prototype::Native(function), upvalues)
    }

    pub fn prototype(&self) -> &Prototype {
        &self.prototype
    }

    pub fn upvalues(&self) -> &[Value] {
        &self.upvalues
    }

    pub(crate) fn identity(&self) -> &Arc<()> {
        &self.identity
    }
}

impl Dict {
    pub(crate) fn new(shape: Arc<Shape>, values: Vec<Value>) -> Self {
        debug_assert_eq!(shape.fields().len(), values.len());
        Self {
            shape,
            values: values.into(),
        }
    }

    pub fn shape(&self) -> &Arc<Shape> {
        &self.shape
    }

    pub fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn get(&self, field: &str) -> Option<&Value> {
        self.shape
            .field_index(field)
            .map(|index| &self.values[index])
    }

    pub fn shares_shape_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shape, &other.shape)
    }
}

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(Arc<str>),
    Bytes(Arc<[u8]>),
    Dict(Dict),
    Array(Arc<[Value]>),
    Atom(Atom),
    Tuple(Arc<[Value]>),
    Func(Arc<Closure>),
}

impl Value {
    pub const fn bool(value: bool) -> Self {
        Self::Atom(Atom::Builtin(if value {
            BuiltinAtom::True
        } else {
            BuiltinAtom::False
        }))
    }

    pub const fn none() -> Self {
        Self::Atom(Atom::Builtin(BuiltinAtom::None))
    }

    pub fn string(value: impl Into<Arc<str>>) -> Self {
        Self::String(value.into())
    }

    pub fn atom(name: impl Into<Arc<str>>) -> Self {
        Self::Atom(Atom::named(name))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::String(_) => "String",
            Self::Bytes(_) => "Bytes",
            Self::Dict(_) => "Dict",
            Self::Array(_) => "Array",
            Self::Atom(_) => "Atom",
            Self::Tuple(_) => "Tuple",
            Self::Func(_) => "Func",
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(value) => write!(formatter, "{value}"),
            Self::Float(value) => write!(formatter, "{value:?}"),
            Self::String(value) => write!(formatter, "{value:?}"),
            Self::Bytes(value) => {
                write!(formatter, "b\"")?;
                for byte in value.iter() {
                    write!(formatter, "\\x{byte:02x}")?;
                }
                write!(formatter, "\"")
            }
            Self::Dict(dict) => {
                write!(formatter, "{{")?;
                for (index, (field, value)) in
                    dict.shape().fields().iter().zip(dict.values()).enumerate()
                {
                    if index > 0 {
                        write!(formatter, ", ")?;
                    }
                    write!(formatter, "{field}: {value}")?;
                }
                write!(formatter, "}}")
            }
            Self::Array(values) => format_sequence(formatter, "[", "]", values),
            Self::Atom(atom) => write!(formatter, "'{}", atom.name()),
            Self::Tuple(values) => format_sequence(formatter, "(", ")", values),
            Self::Func(closure) => match closure.prototype() {
                Prototype::Bytecode(function) => {
                    write!(formatter, "<fn {}>", function.name())
                }
                Prototype::Native(function) => write!(formatter, "<native fn {}>", function.name()),
            },
        }
    }
}

fn format_sequence(
    formatter: &mut fmt::Formatter<'_>,
    start: &str,
    end: &str,
    values: &[Value],
) -> fmt::Result {
    write!(formatter, "{start}")?;
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            write!(formatter, ", ")?;
        }
        write!(formatter, "{value}")?;
    }
    write!(formatter, "{end}")
}
