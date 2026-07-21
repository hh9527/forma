use crate::bytecode::BytecodeFunction;
use crate::vm::Vm;
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
    function: Arc<BytecodeFunction>,
    captures: Arc<[Value]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeError {
    pub message: String,
}

impl NativeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub type NativeCallback = fn(&mut Vm, &[Value]) -> Result<Value, NativeError>;

#[derive(Clone, Copy)]
pub struct NativeFunction {
    name: &'static str,
    arity: usize,
    callback: NativeCallback,
}

impl NativeFunction {
    pub const fn new(name: &'static str, arity: usize, callback: NativeCallback) -> Self {
        Self {
            name,
            arity,
            callback,
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
}

impl fmt::Debug for NativeFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeFunction")
            .field("name", &self.name)
            .field("arity", &self.arity)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub enum Callable {
    Bytecode(Closure),
    Native(NativeFunction),
}

impl Closure {
    pub fn new(function: Arc<BytecodeFunction>, captures: Vec<Value>) -> Self {
        Self {
            function,
            captures: captures.into(),
        }
    }

    pub fn function(&self) -> &Arc<BytecodeFunction> {
        &self.function
    }

    pub fn captures(&self) -> &[Value] {
        &self.captures
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
    Func(Arc<Callable>),
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
            Self::Func(callable) => match callable.as_ref() {
                Callable::Bytecode(closure) => {
                    write!(formatter, "<fn {}>", closure.function().name())
                }
                Callable::Native(function) => write!(formatter, "<native fn {}>", function.name()),
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
