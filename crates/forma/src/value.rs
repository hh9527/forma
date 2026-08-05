use crate::bytecode::BytecodeFunction;
use crate::vm::CallContext;
use std::any::Any;
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

type OpaquePayload = dyn Any + Send + Sync;

#[derive(Clone)]
pub struct OpaqueValue {
    type_name: Arc<str>,
    payload: Arc<OpaquePayload>,
    equal: fn(&OpaquePayload, &OpaquePayload) -> bool,
}

impl OpaqueValue {
    pub fn new<T>(type_name: impl Into<Arc<str>>, payload: T) -> Self
    where
        T: Any + Eq + Send + Sync,
    {
        Self {
            type_name: type_name.into(),
            payload: Arc::new(payload),
            equal: |left, right| {
                left.downcast_ref::<T>()
                    .zip(right.downcast_ref::<T>())
                    .is_some_and(|(left, right)| left == right)
            },
        }
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    pub fn downcast_ref<T: Any>(&self, expected_type: &str) -> Option<&T> {
        (self.type_name.as_ref() == expected_type)
            .then(|| self.payload.downcast_ref::<T>())
            .flatten()
    }

    pub(crate) fn logical_eq(&self, other: &Self) -> bool {
        self.type_name == other.type_name
            && (self.equal)(self.payload.as_ref(), other.payload.as_ref())
    }
}

impl fmt::Debug for OpaqueValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "<opaque {}>", self.type_name)
    }
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
    Push,
    Concat,
    Zip,
    Map,
    Filter,
    FlatMap,
    Fold,
    FoldControl,
    Any,
    All,
    Find,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreDictFunction {
    Keys,
    Values,
    Pairs,
    FromPairs,
    Merge,
    MapValues,
    Filter,
    Fold,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreStringFunction {
    Length,
    Join,
    JoinLines,
    Split,
    Lines,
    StartsWith,
    EndsWith,
    Contains,
    Replace,
    Indent,
    EnsureTrailingNewline,
    TrimMargin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CorePathFunction {
    Join,
    Normalize,
    Parent,
    FileName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreAttributesFunction {
    Normalize,
    Add,
    Get,
    Has,
    All,
    Strip,
}

impl CoreAttributesFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Normalize => "@bim/std/attributes.normalize",
            Self::Add => "@bim/std/attributes.add",
            Self::Get => "@bim/std/attributes.get",
            Self::Has => "@bim/std/attributes.has",
            Self::All => "@bim/std/attributes.all",
            Self::Strip => "@bim/std/attributes.strip",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Normalize | Self::All | Self::Strip => 1,
            Self::Add | Self::Get | Self::Has => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreModelFunction {
    Struct,
    Enum,
    Union,
}

impl CoreModelFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Union => "union",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        2
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreBuiltinTypeFunction {
    FoldControl,
    Option,
    Result,
}

impl CoreBuiltinTypeFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::FoldControl => "FoldControl",
            Self::Option => "Option",
            Self::Result => "Result",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Option => 1,
            Self::FoldControl | Self::Result => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreDebugFunction {
    Dbg,
    DbgWith,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreEqFunction {
    Equal,
}

impl CoreEqFunction {
    pub(crate) const fn name(self) -> &'static str {
        "@bim/std/eq.equal"
    }

    pub(crate) const fn arity(self) -> usize {
        2
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreHashFunction {
    Sha256,
}

impl CoreHashFunction {
    pub(crate) const fn name(self) -> &'static str {
        "@bim/std/hash.sha256"
    }

    pub(crate) const fn arity(self) -> usize {
        1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreCodecFunction {
    Decode,
    Encode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreTypeDescFunction {
    Kind,
    Children,
    OpaqueName,
    Resolve,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreDynFunction {
    Pack,
    Desc,
    Kind,
    CheckInt,
    CheckFloat,
    CheckString,
    CheckBytes,
    Field,
    Fields,
    ArrayItems,
    TupleItems,
    Tag,
    Payload,
}

impl CoreDynFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Pack => "@bim/std/dyn.pack",
            Self::Desc => "@bim/std/dyn.desc",
            Self::Kind => "@bim/std/dyn.kind",
            Self::CheckInt => "@bim/std/dyn.check_int",
            Self::CheckFloat => "@bim/std/dyn.check_float",
            Self::CheckString => "@bim/std/dyn.check_string",
            Self::CheckBytes => "@bim/std/dyn.check_bytes",
            Self::Field => "@bim/std/dyn.field",
            Self::Fields => "@bim/std/dyn.fields",
            Self::ArrayItems => "@bim/std/dyn.array_items",
            Self::TupleItems => "@bim/std/dyn.tuple_items",
            Self::Tag => "@bim/std/dyn.tag",
            Self::Payload => "@bim/std/dyn.payload",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Pack | Self::Field => 2,
            _ => 1,
        }
    }
}

impl CoreTypeDescFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Kind => "@bim/std/type-desc.kind",
            Self::Children => "@bim/std/type-desc.children",
            Self::OpaqueName => "@bim/std/type-desc.opaque_name",
            Self::Resolve => "@bim/std/type-desc.resolve",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        1
    }
}

impl CoreCodecFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Decode => "@bim/std/codec.decode",
            Self::Encode => "@bim/std/codec.encode",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        2
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreResultFunction {
    Unwrap,
}

impl CoreResultFunction {
    pub(crate) const fn name(self) -> &'static str {
        "@bim/std/result.unwrap"
    }

    pub(crate) const fn arity(self) -> usize {
        1
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreJsonFunction {
    Parse,
    Decode,
    Stringify,
    StringifyPretty,
    StringifyPrettyValue,
    Rename,
    RenameDecorator,
    RenameAll,
    RenameAllDecorator,
    Flatten,
    Untagged,
    Schema,
    Default,
    DefaultDecorator,
    SkipSerializingIf,
    SkipSerializingIfDecorator,
}

impl CoreJsonFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Parse => "@bim/std/json.parse",
            Self::Decode => "@bim/std/json.decode",
            Self::Stringify => "@bim/std/json.stringify",
            Self::StringifyPretty => "@bim/std/json.stringify_pretty",
            Self::StringifyPrettyValue => "@bim/std/json.stringify_pretty.configured",
            Self::Rename => "@bim/std/json.rename",
            Self::RenameDecorator => "@bim/std/json.rename.configured",
            Self::RenameAll => "@bim/std/json.rename_all",
            Self::RenameAllDecorator => "@bim/std/json.rename_all.configured",
            Self::Flatten => "@bim/std/json.flatten",
            Self::Untagged => "@bim/std/json.untagged",
            Self::Schema => "@bim/std/json.schema",
            Self::Default => "@bim/std/json.default",
            Self::DefaultDecorator => "@bim/std/json.default.configured",
            Self::SkipSerializingIf => "@bim/std/json.skip_serializing_if",
            Self::SkipSerializingIfDecorator => "@bim/std/json.skip_serializing_if.configured",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Decode => 2,
            Self::Flatten
            | Self::Untagged
            | Self::RenameDecorator
            | Self::RenameAllDecorator
            | Self::DefaultDecorator
            | Self::SkipSerializingIfDecorator => 2,
            _ => 1,
        }
    }
}

impl CoreDebugFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Dbg => "@bim/std/debug.dbg",
            Self::DbgWith => "@bim/std/debug.dbg_with",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Dbg => 1,
            Self::DbgWith => 2,
        }
    }
}

impl CoreDictFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Keys => "@bim/std/dict.keys",
            Self::Values => "@bim/std/dict.values",
            Self::Pairs => "@bim/std/dict.pairs",
            Self::FromPairs => "@bim/std/dict.from_pairs",
            Self::Merge => "@bim/std/dict.merge",
            Self::MapValues => "@bim/std/dict.map_values",
            Self::Filter => "@bim/std/dict.filter",
            Self::Fold => "@bim/std/dict.fold",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Keys | Self::Values | Self::Pairs | Self::FromPairs => 1,
            Self::Merge | Self::MapValues | Self::Filter => 2,
            Self::Fold => 3,
        }
    }
}

impl CoreStringFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Length => "@bim/std/string.length",
            Self::Join => "@bim/std/string.join",
            Self::JoinLines => "@bim/std/string.join_lines",
            Self::Split => "@bim/std/string.split",
            Self::Lines => "@bim/std/string.lines",
            Self::StartsWith => "@bim/std/string.starts_with",
            Self::EndsWith => "@bim/std/string.ends_with",
            Self::Contains => "@bim/std/string.contains",
            Self::Replace => "@bim/std/string.replace",
            Self::Indent => "@bim/std/string.indent",
            Self::EnsureTrailingNewline => "@bim/std/string.ensure_trailing_newline",
            Self::TrimMargin => "@bim/std/string.trim_margin",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Length | Self::JoinLines | Self::Lines | Self::EnsureTrailingNewline => 1,
            Self::Join
            | Self::Split
            | Self::StartsWith
            | Self::EndsWith
            | Self::Contains
            | Self::Indent
            | Self::TrimMargin => 2,
            Self::Replace => 3,
        }
    }
}

impl CorePathFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Join => "@bim/std/path.join",
            Self::Normalize => "@bim/std/path.normalize",
            Self::Parent => "@bim/std/path.parent",
            Self::FileName => "@bim/std/path.file_name",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        1
    }
}

impl CoreArrayFunction {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Length => "@bim/std/array.length",
            Self::Push => "@bim/std/array.push",
            Self::Concat => "@bim/std/array.concat",
            Self::Zip => "@bim/std/array.zip",
            Self::Map => "@bim/std/array.map",
            Self::Filter => "@bim/std/array.filter",
            Self::FlatMap => "@bim/std/array.flat_map",
            Self::Fold => "@bim/std/array.fold",
            Self::FoldControl => "@bim/std/array.fold_control",
            Self::Any => "@bim/std/array.any",
            Self::All => "@bim/std/array.all",
            Self::Find => "@bim/std/array.find",
        }
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Length | Self::Concat => 1,
            Self::Push | Self::Zip => 2,
            Self::Map | Self::Filter | Self::FlatMap | Self::Any | Self::All | Self::Find => 2,
            Self::Fold | Self::FoldControl => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeKind {
    Synchronous,
    CoreArray(CoreArrayFunction),
    CoreAttributes(CoreAttributesFunction),
    CoreModel(CoreModelFunction),
    CoreBuiltinType(CoreBuiltinTypeFunction),
    CoreDict(CoreDictFunction),
    CoreString(CoreStringFunction),
    CorePath(CorePathFunction),
    CoreDebug(CoreDebugFunction),
    CoreHash(CoreHashFunction),
    CoreCodec(CoreCodecFunction),
    CoreTypeDesc(CoreTypeDescFunction),
    CoreDyn(CoreDynFunction),
    CoreEq(CoreEqFunction),
    CoreResult(CoreResultFunction),
    CoreJson(CoreJsonFunction),
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

    pub(crate) const fn core_attributes(function: CoreAttributesFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreAttributes(function),
        }
    }

    pub(crate) const fn core_model(function: CoreModelFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreModel(function),
        }
    }

    pub(crate) const fn core_builtin_type(function: CoreBuiltinTypeFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreBuiltinType(function),
        }
    }

    pub(crate) const fn core_dict(function: CoreDictFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreDict(function),
        }
    }

    pub(crate) const fn core_string(function: CoreStringFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreString(function),
        }
    }

    pub(crate) const fn core_path(function: CorePathFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CorePath(function),
        }
    }

    pub(crate) const fn core_debug(function: CoreDebugFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreDebug(function),
        }
    }

    pub(crate) const fn core_hash(function: CoreHashFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreHash(function),
        }
    }

    pub(crate) const fn core_codec(function: CoreCodecFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreCodec(function),
        }
    }

    pub(crate) const fn core_type_desc(function: CoreTypeDescFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreTypeDesc(function),
        }
    }

    pub(crate) const fn core_dyn(function: CoreDynFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreDyn(function),
        }
    }

    pub(crate) const fn core_eq(function: CoreEqFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreEq(function),
        }
    }

    pub(crate) const fn core_result(function: CoreResultFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreResult(function),
        }
    }

    pub(crate) const fn core_json(function: CoreJsonFunction) -> Self {
        Self {
            name: function.name(),
            arity: function.arity(),
            callback: unavailable_core_callback,
            kind: NativeKind::CoreJson(function),
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
pub struct DynValue {
    identity: Arc<()>,
    descriptor: Box<Value>,
    value: Box<Value>,
}

impl DynValue {
    pub(crate) fn from_parts_with_identity(
        identity: Arc<()>,
        descriptor: Value,
        value: Value,
    ) -> Self {
        Self {
            identity,
            descriptor: Box::new(descriptor),
            value: Box::new(value),
        }
    }

    pub(crate) fn identity(&self) -> &Arc<()> {
        &self.identity
    }

    pub(crate) fn descriptor(&self) -> &Value {
        &self.descriptor
    }

    pub(crate) fn value(&self) -> &Value {
        &self.value
    }
}

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(Arc<str>),
    Bytes(Arc<[u8]>),
    Opaque(OpaqueValue),
    Dict(Dict),
    Array(Arc<[Value]>),
    Atom(Atom),
    Tagged { tag: Atom, payload: Box<Value> },
    Tuple(Arc<[Value]>),
    Func(Arc<Closure>),
    Dyn(Arc<DynValue>),
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

    pub fn tagged(tag: Atom, payload: Value) -> Self {
        Self::Tagged {
            tag,
            payload: Box::new(payload),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "Int",
            Self::Float(_) => "Float",
            Self::String(_) => "String",
            Self::Bytes(_) => "Bytes",
            Self::Opaque(_) => "Opaque",
            Self::Dict(_) => "Dict",
            Self::Array(_) => "Array",
            Self::Atom(_) => "Atom",
            Self::Tagged { .. } => "Tagged",
            Self::Tuple(_) => "Tuple",
            Self::Func(_) => "Func",
            Self::Dyn(_) => "Dyn",
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
            Self::Opaque(value) => write!(formatter, "{value:?}"),
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
            Self::Tagged { tag, payload } => write!(formatter, "'{}({payload})", tag.name()),
            Self::Tuple(values) => format_sequence(formatter, "(", ")", values),
            Self::Func(closure) => match closure.prototype() {
                Prototype::Bytecode(function) => {
                    write!(formatter, "<fn {}>", function.name())
                }
                Prototype::Native(function) => write!(formatter, "<native fn {}>", function.name()),
            },
            Self::Dyn(_) => formatter.write_str("<dyn>"),
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
