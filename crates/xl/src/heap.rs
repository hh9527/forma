#![allow(dead_code)]

use crate::{
    Atom, BuiltinAtom, BytecodeFunction, Closure, Dict, FuncByteCode, NativeFunction, Prototype,
    Shape, Value,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

const SHORT_TEXT_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct HeapId(u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Handle {
    heap: HeapId,
    slot: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct InternId {
    heap: HeapId,
    slot: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ShapeId {
    heap: HeapId,
    slot: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RuntimeValue {
    Int(i64),
    Float(f64),
    BuiltinAtom(BuiltinAtom),
    Atom(InternId),
    ShortString(InternId),
    String(Handle),
    Bytes(Handle),
    Array(Handle),
    Tuple(Handle),
    Dict(Handle),
    Func(Handle),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RuntimePrototype {
    Bytecode(Handle),
    Native(NativeFunction),
}

#[derive(Clone, Debug)]
pub(crate) enum Object {
    Reserved,
    String(Box<str>),
    Bytes(Box<[u8]>),
    Array(Box<[RuntimeValue]>),
    Tuple(Box<[RuntimeValue]>),
    Dict {
        shape: ShapeId,
        values: Box<[RuntimeValue]>,
    },
    Closure {
        prototype: RuntimePrototype,
        upvalues: Box<[RuntimeValue]>,
    },
    ByteCodeProto {
        code: Arc<FuncByteCode>,
        values: Box<[RuntimeValue]>,
        text: Box<[InternId]>,
        prototypes: Box<[RuntimePrototype]>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct HeapError(&'static str);

impl HeapError {
    pub(crate) const fn new(message: &'static str) -> Self {
        Self(message)
    }
}

impl fmt::Display for HeapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Default)]
struct TextTable {
    values: Vec<Arc<str>>,
    slots: HashMap<Arc<str>, u32>,
}

impl TextTable {
    fn find(&self, text: &str) -> Option<u32> {
        self.slots.get(text).copied()
    }

    fn resolve(&self, slot: u32) -> Option<&str> {
        self.values.get(slot as usize).map(AsRef::as_ref)
    }

    fn insert(&mut self, text: &str) -> u32 {
        if let Some(slot) = self.find(text) {
            return slot;
        }
        let slot = self.values.len() as u32;
        let value: Arc<str> = text.into();
        self.values.push(value.clone());
        self.slots.insert(value, slot);
        slot
    }
}

pub(crate) struct Heap {
    id: HeapId,
    objects: Vec<Object>,
    text: TextTable,
    shapes: Vec<Box<[InternId]>>,
    shape_slots: HashMap<Vec<InternId>, u32>,
    exported_shapes: RefCell<HashMap<u32, Arc<Shape>>>,
}

impl Heap {
    pub(crate) fn new(id: u32) -> Self {
        Self {
            id: HeapId(id),
            objects: Vec::new(),
            text: TextTable::default(),
            shapes: Vec::new(),
            shape_slots: HashMap::new(),
            exported_shapes: RefCell::new(HashMap::new()),
        }
    }

    pub(crate) const fn id(&self) -> HeapId {
        self.id
    }

    pub(crate) fn counts(&self) -> (usize, usize, usize) {
        (
            self.objects.len(),
            self.text.values.len(),
            self.shapes.len(),
        )
    }

    pub(crate) fn allocate(&mut self, object: Object) -> Handle {
        let handle = Handle {
            heap: self.id,
            slot: self.objects.len() as u32,
        };
        self.objects.push(object);
        handle
    }

    pub(crate) fn reserve(&mut self) -> Handle {
        self.allocate(Object::Reserved)
    }

    pub(crate) fn initialize(&mut self, handle: Handle, object: Object) -> Result<(), HeapError> {
        let slot = self.object_mut(handle)?;
        if !matches!(slot, Object::Reserved) {
            return Err(HeapError("heap slot is already initialized"));
        }
        *slot = object;
        Ok(())
    }

    pub(crate) fn object(&self, handle: Handle) -> Result<&Object, HeapError> {
        if handle.heap != self.id {
            return Err(HeapError("object handle belongs to another heap"));
        }
        self.objects
            .get(handle.slot as usize)
            .ok_or(HeapError("object handle is out of bounds"))
    }

    fn object_mut(&mut self, handle: Handle) -> Result<&mut Object, HeapError> {
        if handle.heap != self.id {
            return Err(HeapError("object handle belongs to another heap"));
        }
        self.objects
            .get_mut(handle.slot as usize)
            .ok_or(HeapError("object handle is out of bounds"))
    }

    pub(crate) fn intern(&mut self, text: &str) -> InternId {
        InternId {
            heap: self.id,
            slot: self.text.insert(text),
        }
    }

    pub(crate) fn find_text(&self, text: &str) -> Option<InternId> {
        self.text.find(text).map(|slot| InternId {
            heap: self.id,
            slot,
        })
    }

    pub(crate) fn resolve_text(&self, id: InternId) -> Result<&str, HeapError> {
        if id.heap != self.id {
            return Err(HeapError("intern ID belongs to another heap"));
        }
        self.text
            .resolve(id.slot)
            .ok_or(HeapError("intern ID is out of bounds"))
    }

    pub(crate) fn string(&mut self, background: Option<&Heap>, text: &str) -> RuntimeValue {
        if text.len() <= SHORT_TEXT_BYTES {
            if let Some(id) = background.and_then(|heap| heap.find_text(text)) {
                RuntimeValue::ShortString(id)
            } else {
                RuntimeValue::ShortString(self.intern(text))
            }
        } else {
            RuntimeValue::String(self.allocate(Object::String(text.into())))
        }
    }

    pub(crate) fn atom(&mut self, background: Option<&Heap>, text: &str) -> RuntimeValue {
        if let Some(builtin) = builtin_atom(text) {
            RuntimeValue::BuiltinAtom(builtin)
        } else if let Some(id) = background.and_then(|heap| heap.find_text(text)) {
            RuntimeValue::Atom(id)
        } else {
            RuntimeValue::Atom(self.intern(text))
        }
    }

    pub(crate) fn intern_shape(&mut self, fields: Vec<InternId>) -> ShapeId {
        if let Some(slot) = self.shape_slots.get(&fields) {
            return ShapeId {
                heap: self.id,
                slot: *slot,
            };
        }
        let slot = self.shapes.len() as u32;
        self.shapes.push(fields.clone().into());
        self.shape_slots.insert(fields, slot);
        ShapeId {
            heap: self.id,
            slot,
        }
    }

    fn shape(&self, id: ShapeId) -> Result<&[InternId], HeapError> {
        if id.heap != self.id {
            return Err(HeapError("shape ID belongs to another heap"));
        }
        self.shapes
            .get(id.slot as usize)
            .map(AsRef::as_ref)
            .ok_or(HeapError("shape ID is out of bounds"))
    }

    pub(crate) fn import_value(
        &mut self,
        background: Option<&Heap>,
        value: &Value,
    ) -> Result<RuntimeValue, HeapError> {
        let mut prototypes = HashMap::new();
        self.import_value_with(background, value, &mut prototypes)
    }

    fn import_value_with(
        &mut self,
        background: Option<&Heap>,
        value: &Value,
        prototypes: &mut HashMap<*const BytecodeFunction, Handle>,
    ) -> Result<RuntimeValue, HeapError> {
        Ok(match value {
            Value::Int(value) => RuntimeValue::Int(*value),
            Value::Float(value) => RuntimeValue::Float(*value),
            Value::String(value) => self.string(background, value),
            Value::Bytes(value) => {
                RuntimeValue::Bytes(self.allocate(Object::Bytes(value.as_ref().into())))
            }
            Value::Atom(Atom::Builtin(atom)) => RuntimeValue::BuiltinAtom(*atom),
            Value::Atom(Atom::Named(name)) => self.atom(background, name),
            Value::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| self.import_value_with(background, value, prototypes))
                    .collect::<Result<Box<[_]>, _>>()?;
                RuntimeValue::Array(self.allocate(Object::Array(values)))
            }
            Value::Tuple(values) => {
                let values = values
                    .iter()
                    .map(|value| self.import_value_with(background, value, prototypes))
                    .collect::<Result<Box<[_]>, _>>()?;
                RuntimeValue::Tuple(self.allocate(Object::Tuple(values)))
            }
            Value::Dict(dict) => {
                let fields = dict
                    .shape()
                    .fields()
                    .iter()
                    .map(|field| {
                        Ok(background
                            .and_then(|heap| heap.find_text(field))
                            .unwrap_or_else(|| self.intern(field)))
                    })
                    .collect::<Result<Vec<_>, HeapError>>()?;
                let shape = self.intern_shape(fields);
                let values = dict
                    .values()
                    .iter()
                    .map(|value| self.import_value_with(background, value, prototypes))
                    .collect::<Result<Box<[_]>, _>>()?;
                RuntimeValue::Dict(self.allocate(Object::Dict { shape, values }))
            }
            Value::Func(closure) => {
                let prototype = match closure.prototype() {
                    Prototype::Bytecode(function) => RuntimePrototype::Bytecode(
                        self.link_bytecode_with(background, function, prototypes)?,
                    ),
                    Prototype::Native(function) => RuntimePrototype::Native(*function),
                };
                let upvalues = closure
                    .upvalues()
                    .iter()
                    .map(|value| self.import_value_with(background, value, prototypes))
                    .collect::<Result<Box<[_]>, _>>()?;
                RuntimeValue::Func(self.allocate(Object::Closure {
                    prototype,
                    upvalues,
                }))
            }
        })
    }

    pub(crate) fn link_bytecode(
        &mut self,
        background: Option<&Heap>,
        function: &BytecodeFunction,
    ) -> Result<Handle, HeapError> {
        self.link_bytecode_with(background, function, &mut HashMap::new())
    }

    fn link_bytecode_with(
        &mut self,
        background: Option<&Heap>,
        function: &BytecodeFunction,
        forwarded: &mut HashMap<*const BytecodeFunction, Handle>,
    ) -> Result<Handle, HeapError> {
        let identity = std::ptr::from_ref(function);
        if let Some(handle) = forwarded.get(&identity) {
            return Ok(*handle);
        }
        let handle = self.reserve();
        forwarded.insert(identity, handle);
        let values = function
            .links()
            .values()
            .iter()
            .map(|value| self.import_value_with(background, value, forwarded))
            .collect::<Result<Box<[_]>, _>>()?;
        let text = function
            .links()
            .text()
            .iter()
            .map(|text| {
                background
                    .and_then(|heap| heap.find_text(text))
                    .unwrap_or_else(|| self.intern(text))
            })
            .collect::<Box<[_]>>();
        let prototypes = function
            .links()
            .prototypes()
            .iter()
            .map(|prototype| {
                self.link_bytecode_with(background, prototype, forwarded)
                    .map(RuntimePrototype::Bytecode)
            })
            .collect::<Result<Box<[_]>, _>>()?;
        self.initialize(
            handle,
            Object::ByteCodeProto {
                code: Arc::clone(function.code()),
                values,
                text,
                prototypes,
            },
        )?;
        Ok(handle)
    }
}

pub(crate) struct HeapView<'a> {
    pub(crate) current: &'a Heap,
    pub(crate) background: Option<&'a Heap>,
}

type BytecodeLinks<'a> = (
    &'a Arc<FuncByteCode>,
    &'a [RuntimeValue],
    &'a [InternId],
    &'a [RuntimePrototype],
);

impl HeapView<'_> {
    fn heap(&self, id: HeapId) -> Result<&Heap, HeapError> {
        if self.current.id == id {
            return Ok(self.current);
        }
        self.background
            .filter(|heap| heap.id == id)
            .ok_or(HeapError("value refers to a heap outside its view"))
    }

    pub(crate) fn object(&self, handle: Handle) -> Result<&Object, HeapError> {
        self.heap(handle.heap)?.object(handle)
    }

    pub(crate) fn text(&self, id: InternId) -> Result<&str, HeapError> {
        self.heap(id.heap)?.resolve_text(id)
    }

    fn shape(&self, id: ShapeId) -> Result<&[InternId], HeapError> {
        self.heap(id.heap)?.shape(id)
    }

    pub(crate) fn bytecode(&self, handle: Handle) -> Result<BytecodeLinks<'_>, HeapError> {
        let Object::ByteCodeProto {
            code,
            values,
            text,
            prototypes,
        } = self.object(handle)?
        else {
            return Err(HeapError("handle is not a bytecode prototype"));
        };
        Ok((code, values, text, prototypes))
    }

    pub(crate) fn closure(
        &self,
        handle: Handle,
    ) -> Result<(RuntimePrototype, &[RuntimeValue]), HeapError> {
        let Object::Closure {
            prototype,
            upvalues,
        } = self.object(handle)?
        else {
            return Err(HeapError("handle is not a closure"));
        };
        Ok((*prototype, upvalues))
    }

    pub(crate) fn sequence(
        &self,
        handle: Handle,
        tuple: bool,
    ) -> Result<&[RuntimeValue], HeapError> {
        match self.object(handle)? {
            Object::Array(values) if !tuple => Ok(values),
            Object::Tuple(values) if tuple => Ok(values),
            _ => Err(HeapError("handle is not the requested sequence kind")),
        }
    }

    pub(crate) fn dict_get(
        &self,
        handle: Handle,
        field: InternId,
    ) -> Result<Option<RuntimeValue>, HeapError> {
        let Object::Dict { shape, values } = self.object(handle)? else {
            return Err(HeapError("handle is not a Dict"));
        };
        let wanted = self.text(field)?;
        for (index, candidate) in self.shape(*shape)?.iter().enumerate() {
            if self.text(*candidate)? == wanted {
                return Ok(values.get(index).copied());
            }
        }
        Ok(None)
    }

    pub(crate) fn export_value(&self, value: RuntimeValue) -> Result<Value, HeapError> {
        self.export_value_with(value, &mut HashSet::new())
    }

    fn export_value_with(
        &self,
        value: RuntimeValue,
        visiting: &mut HashSet<Handle>,
    ) -> Result<Value, HeapError> {
        Ok(match value {
            RuntimeValue::Int(value) => Value::Int(value),
            RuntimeValue::Float(value) => Value::Float(value),
            RuntimeValue::BuiltinAtom(atom) => Value::Atom(Atom::builtin(atom)),
            RuntimeValue::Atom(id) => Value::atom(self.text(id)?),
            RuntimeValue::ShortString(id) => Value::string(self.text(id)?),
            RuntimeValue::String(handle) => {
                let Object::String(value) = self.enter_object(handle, visiting)? else {
                    return Err(HeapError("String handle refers to another object kind"));
                };
                let value = Value::string(value.as_ref());
                visiting.remove(&handle);
                value
            }
            RuntimeValue::Bytes(handle) => {
                let Object::Bytes(value) = self.enter_object(handle, visiting)? else {
                    return Err(HeapError("Bytes handle refers to another object kind"));
                };
                let value = Value::Bytes(value.clone().into());
                visiting.remove(&handle);
                value
            }
            RuntimeValue::Array(handle) | RuntimeValue::Tuple(handle) => {
                let tuple = matches!(value, RuntimeValue::Tuple(_));
                let object = self.enter_object(handle, visiting)?;
                let values = match object {
                    Object::Array(values) if !tuple => values,
                    Object::Tuple(values) if tuple => values,
                    _ => return Err(HeapError("sequence handle refers to another object kind")),
                };
                let values = values
                    .iter()
                    .map(|value| self.export_value_with(*value, visiting))
                    .collect::<Result<Vec<_>, _>>()?;
                visiting.remove(&handle);
                if tuple {
                    Value::Tuple(values.into())
                } else {
                    Value::Array(values.into())
                }
            }
            RuntimeValue::Dict(handle) => {
                let Object::Dict { shape, values } = self.enter_object(handle, visiting)? else {
                    return Err(HeapError("Dict handle refers to another object kind"));
                };
                let fields = self
                    .shape(*shape)?
                    .iter()
                    .map(|field| self.text(*field).map(str::to_owned))
                    .collect::<Result<Vec<_>, _>>()?;
                let values = values
                    .iter()
                    .map(|value| self.export_value_with(*value, visiting))
                    .collect::<Result<Vec<_>, _>>()?;
                visiting.remove(&handle);
                let owner = self.heap(shape.heap)?;
                let shape = if let Some(shape) = owner.exported_shapes.borrow().get(&shape.slot) {
                    Arc::clone(shape)
                } else {
                    let shape_value = Arc::new(Shape::from_sorted_fields(fields));
                    owner
                        .exported_shapes
                        .borrow_mut()
                        .insert(shape.slot, Arc::clone(&shape_value));
                    shape_value
                };
                Value::Dict(Dict::new(shape, values))
            }
            RuntimeValue::Func(handle) => {
                let Object::Closure {
                    prototype,
                    upvalues,
                } = self.enter_object(handle, visiting)?
                else {
                    return Err(HeapError("Func handle refers to another object kind"));
                };
                let prototype = self.export_prototype(prototype, visiting)?;
                let upvalues = upvalues
                    .iter()
                    .map(|value| self.export_value_with(*value, visiting))
                    .collect::<Result<Vec<_>, _>>()?;
                visiting.remove(&handle);
                Value::Func(Arc::new(Closure::from_parts(prototype, upvalues)))
            }
        })
    }

    fn enter_object<'a>(
        &'a self,
        handle: Handle,
        visiting: &mut HashSet<Handle>,
    ) -> Result<&'a Object, HeapError> {
        if !visiting.insert(handle) {
            return Err(HeapError(
                "cyclic heap values cannot cross the legacy Value boundary",
            ));
        }
        self.object(handle)
    }

    fn export_prototype(
        &self,
        prototype: &RuntimePrototype,
        visiting: &mut HashSet<Handle>,
    ) -> Result<Prototype, HeapError> {
        Ok(match prototype {
            RuntimePrototype::Native(function) => Prototype::Native(*function),
            RuntimePrototype::Bytecode(handle) => {
                let Object::ByteCodeProto {
                    code,
                    values,
                    text,
                    prototypes,
                } = self.enter_object(*handle, visiting)?
                else {
                    return Err(HeapError("prototype handle refers to another object kind"));
                };
                let values = values
                    .iter()
                    .map(|value| self.export_value_with(*value, visiting))
                    .collect::<Result<Vec<_>, _>>()?;
                let text = text
                    .iter()
                    .map(|id| self.text(*id).map(Arc::<str>::from))
                    .collect::<Result<Vec<_>, _>>()?;
                let prototypes = prototypes
                    .iter()
                    .map(
                        |prototype| match self.export_prototype(prototype, visiting)? {
                            Prototype::Bytecode(function) => Ok(function),
                            Prototype::Native(_) => Err(HeapError(
                                "native prototype cannot occupy a bytecode link slot",
                            )),
                        },
                    )
                    .collect::<Result<Vec<_>, _>>()?;
                visiting.remove(handle);
                Prototype::Bytecode(Arc::new(BytecodeFunction::from_linked_parts(
                    Arc::clone(code),
                    values,
                    text,
                    prototypes,
                )))
            }
        })
    }
}

pub(crate) fn copy_roots(
    target: &mut Heap,
    source: HeapView<'_>,
    roots: &[RuntimeValue],
) -> Result<Vec<RuntimeValue>, HeapError> {
    let mut pending = PendingCopy::new(target);
    let roots = roots
        .iter()
        .map(|root| pending.copy_value(target, &source, *root))
        .collect::<Result<Vec<_>, _>>()?;
    pending.validate()?;
    pending.commit(target);
    Ok(roots)
}

struct PendingCopy {
    target_id: HeapId,
    object_base: u32,
    objects: Vec<Object>,
    text_base: u32,
    text: TextTable,
    shape_base: u32,
    shapes: Vec<Box<[InternId]>>,
    objects_forwarded: HashMap<Handle, Handle>,
    text_forwarded: HashMap<InternId, InternId>,
    shapes_forwarded: HashMap<ShapeId, ShapeId>,
}

impl PendingCopy {
    fn new(target: &Heap) -> Self {
        Self {
            target_id: target.id,
            object_base: target.objects.len() as u32,
            objects: Vec::new(),
            text_base: target.text.values.len() as u32,
            text: TextTable::default(),
            shape_base: target.shapes.len() as u32,
            shapes: Vec::new(),
            objects_forwarded: HashMap::new(),
            text_forwarded: HashMap::new(),
            shapes_forwarded: HashMap::new(),
        }
    }

    fn copy_value(
        &mut self,
        target: &Heap,
        source: &HeapView<'_>,
        value: RuntimeValue,
    ) -> Result<RuntimeValue, HeapError> {
        Ok(match value {
            RuntimeValue::Int(_) | RuntimeValue::Float(_) | RuntimeValue::BuiltinAtom(_) => value,
            RuntimeValue::Atom(id) => RuntimeValue::Atom(self.copy_text(target, source, id)?),
            RuntimeValue::ShortString(id) => {
                RuntimeValue::ShortString(self.copy_text(target, source, id)?)
            }
            RuntimeValue::String(handle) => {
                RuntimeValue::String(self.copy_object(target, source, handle)?)
            }
            RuntimeValue::Bytes(handle) => {
                RuntimeValue::Bytes(self.copy_object(target, source, handle)?)
            }
            RuntimeValue::Array(handle) => {
                RuntimeValue::Array(self.copy_object(target, source, handle)?)
            }
            RuntimeValue::Tuple(handle) => {
                RuntimeValue::Tuple(self.copy_object(target, source, handle)?)
            }
            RuntimeValue::Dict(handle) => {
                RuntimeValue::Dict(self.copy_object(target, source, handle)?)
            }
            RuntimeValue::Func(handle) => {
                RuntimeValue::Func(self.copy_object(target, source, handle)?)
            }
        })
    }

    fn copy_object(
        &mut self,
        target: &Heap,
        source: &HeapView<'_>,
        handle: Handle,
    ) -> Result<Handle, HeapError> {
        if handle.heap == self.target_id {
            target.object(handle)?;
            return Ok(handle);
        }
        if let Some(forwarded) = self.objects_forwarded.get(&handle) {
            return Ok(*forwarded);
        }
        let object = source.object(handle)?;
        if matches!(object, Object::Reserved) {
            return Err(HeapError("cannot copy an uninitialized object"));
        }
        let copied = Handle {
            heap: self.target_id,
            slot: self.object_base + self.objects.len() as u32,
        };
        self.objects_forwarded.insert(handle, copied);
        self.objects.push(Object::Reserved);
        let object = self.copy_object_data(target, source, object)?;
        self.objects[(copied.slot - self.object_base) as usize] = object;
        Ok(copied)
    }

    fn copy_object_data(
        &mut self,
        target: &Heap,
        source: &HeapView<'_>,
        object: &Object,
    ) -> Result<Object, HeapError> {
        let copy_values = |this: &mut Self, values: &[RuntimeValue]| {
            values
                .iter()
                .map(|value| this.copy_value(target, source, *value))
                .collect::<Result<Box<[_]>, _>>()
        };
        Ok(match object {
            Object::Reserved => return Err(HeapError("cannot copy an uninitialized object")),
            Object::String(value) => Object::String(value.clone()),
            Object::Bytes(value) => Object::Bytes(value.clone()),
            Object::Array(values) => Object::Array(copy_values(self, values)?),
            Object::Tuple(values) => Object::Tuple(copy_values(self, values)?),
            Object::Dict { shape, values } => Object::Dict {
                shape: self.copy_shape(target, source, *shape)?,
                values: copy_values(self, values)?,
            },
            Object::Closure {
                prototype,
                upvalues,
            } => Object::Closure {
                prototype: self.copy_prototype(target, source, prototype)?,
                upvalues: copy_values(self, upvalues)?,
            },
            Object::ByteCodeProto {
                code,
                values,
                text,
                prototypes,
            } => Object::ByteCodeProto {
                code: Arc::clone(code),
                values: copy_values(self, values)?,
                text: text
                    .iter()
                    .map(|id| self.copy_text(target, source, *id))
                    .collect::<Result<Box<[_]>, _>>()?,
                prototypes: prototypes
                    .iter()
                    .map(|prototype| self.copy_prototype(target, source, prototype))
                    .collect::<Result<Box<[_]>, _>>()?,
            },
        })
    }

    fn copy_prototype(
        &mut self,
        target: &Heap,
        source: &HeapView<'_>,
        prototype: &RuntimePrototype,
    ) -> Result<RuntimePrototype, HeapError> {
        Ok(match prototype {
            RuntimePrototype::Bytecode(handle) => {
                RuntimePrototype::Bytecode(self.copy_object(target, source, *handle)?)
            }
            RuntimePrototype::Native(function) => RuntimePrototype::Native(*function),
        })
    }

    fn copy_text(
        &mut self,
        target: &Heap,
        source: &HeapView<'_>,
        id: InternId,
    ) -> Result<InternId, HeapError> {
        if id.heap == self.target_id {
            target.resolve_text(id)?;
            return Ok(id);
        }
        if let Some(forwarded) = self.text_forwarded.get(&id) {
            return Ok(*forwarded);
        }
        let text = source.text(id)?;
        let copied = if let Some(id) = target.find_text(text) {
            id
        } else if let Some(slot) = self.text.find(text) {
            InternId {
                heap: self.target_id,
                slot: self.text_base + slot,
            }
        } else {
            let local_slot = self.text.insert(text);
            InternId {
                heap: self.target_id,
                slot: self.text_base + local_slot,
            }
        };
        self.text_forwarded.insert(id, copied);
        Ok(copied)
    }

    fn copy_shape(
        &mut self,
        target: &Heap,
        source: &HeapView<'_>,
        id: ShapeId,
    ) -> Result<ShapeId, HeapError> {
        if id.heap == self.target_id {
            target.shape(id)?;
            return Ok(id);
        }
        if let Some(forwarded) = self.shapes_forwarded.get(&id) {
            return Ok(*forwarded);
        }
        let fields = source
            .shape(id)?
            .iter()
            .map(|field| self.copy_text(target, source, *field))
            .collect::<Result<Vec<_>, _>>()?;
        let copied = if let Some(slot) = target.shape_slots.get(&fields) {
            ShapeId {
                heap: self.target_id,
                slot: *slot,
            }
        } else if let Some(index) = self.shapes.iter().position(|shape| **shape == fields) {
            ShapeId {
                heap: self.target_id,
                slot: self.shape_base + index as u32,
            }
        } else {
            let copied = ShapeId {
                heap: self.target_id,
                slot: self.shape_base + self.shapes.len() as u32,
            };
            self.shapes.push(fields.into());
            copied
        };
        self.shapes_forwarded.insert(id, copied);
        Ok(copied)
    }

    fn validate(&self) -> Result<(), HeapError> {
        if self
            .objects
            .iter()
            .any(|object| object_contains_foreign(object, self.target_id))
        {
            return Err(HeapError(
                "copied object graph is not target-self-contained",
            ));
        }
        Ok(())
    }

    fn commit(self, target: &mut Heap) {
        target.objects.extend(self.objects);
        for value in self.text.values {
            target.text.insert(&value);
        }
        for shape in self.shapes {
            target.intern_shape(shape.into_vec());
        }
    }
}

fn value_contains_foreign(value: RuntimeValue, target: HeapId) -> bool {
    match value {
        RuntimeValue::Atom(id) | RuntimeValue::ShortString(id) => id.heap != target,
        RuntimeValue::String(handle)
        | RuntimeValue::Bytes(handle)
        | RuntimeValue::Array(handle)
        | RuntimeValue::Tuple(handle)
        | RuntimeValue::Dict(handle)
        | RuntimeValue::Func(handle) => handle.heap != target,
        RuntimeValue::Int(_) | RuntimeValue::Float(_) | RuntimeValue::BuiltinAtom(_) => false,
    }
}

fn object_contains_foreign(object: &Object, target: HeapId) -> bool {
    match object {
        Object::Reserved => true,
        Object::Array(values) | Object::Tuple(values) => values
            .iter()
            .any(|value| value_contains_foreign(*value, target)),
        Object::Dict { shape, values } => {
            shape.heap != target
                || values
                    .iter()
                    .any(|value| value_contains_foreign(*value, target))
        }
        Object::Closure { upvalues, .. } => upvalues
            .iter()
            .any(|value| value_contains_foreign(*value, target)),
        Object::ByteCodeProto {
            values,
            text,
            prototypes,
            ..
        } => {
            values
                .iter()
                .any(|value| value_contains_foreign(*value, target))
                || text.iter().any(|id| id.heap != target)
                || prototypes.iter().any(|prototype| match prototype {
                    RuntimePrototype::Bytecode(handle) => handle.heap != target,
                    RuntimePrototype::Native(_) => false,
                })
        }
        Object::String(_) | Object::Bytes(_) => false,
    }
}

fn builtin_atom(text: &str) -> Option<BuiltinAtom> {
    match text {
        "None" => Some(BuiltinAtom::None),
        "Some" => Some(BuiltinAtom::Some),
        "Ok" => Some(BuiltinAtom::Ok),
        "Err" => Some(BuiltinAtom::Err),
        "True" => Some(BuiltinAtom::True),
        "False" => Some(BuiltinAtom::False),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_is_reachable_reinterning_and_target_self_contained() {
        let mut world = Heap::new(1);
        let shared = world.allocate(Object::Bytes(vec![9].into()));
        let mut current = Heap::new(2);
        let atom = current.atom(Some(&world), "Custom");
        let string = current.string(Some(&world), "Custom");
        let root = current.allocate(Object::Tuple(
            vec![atom, string, RuntimeValue::Bytes(shared)].into(),
        ));
        current.allocate(Object::Bytes(vec![1, 2, 3].into()));

        let copied = copy_roots(
            &mut world,
            HeapView {
                current: &current,
                background: None,
            },
            &[RuntimeValue::Tuple(root)],
        )
        .unwrap();

        assert_eq!(world.counts(), (2, 1, 0));
        let RuntimeValue::Tuple(root) = copied[0] else {
            panic!("expected tuple root")
        };
        let Object::Tuple(values) = world.object(root).unwrap() else {
            panic!("expected tuple object")
        };
        assert_eq!(values[2], RuntimeValue::Bytes(shared));
        assert!(
            !values
                .iter()
                .any(|value| value_contains_foreign(*value, world.id))
        );
    }

    #[test]
    fn copy_preserves_cycles_and_failure_is_atomic() {
        let mut world = Heap::new(1);
        let mut current = Heap::new(2);
        let cycle = current.reserve();
        current
            .initialize(
                cycle,
                Object::Array(vec![RuntimeValue::Array(cycle)].into()),
            )
            .unwrap();
        copy_roots(
            &mut world,
            HeapView {
                current: &current,
                background: None,
            },
            &[RuntimeValue::Array(cycle)],
        )
        .unwrap();
        assert_eq!(world.counts().0, 1);

        let before = world.counts();
        let invalid = RuntimeValue::Array(Handle {
            heap: current.id,
            slot: 99,
        });
        assert!(
            copy_roots(
                &mut world,
                HeapView {
                    current: &current,
                    background: None,
                },
                &[invalid],
            )
            .is_err()
        );
        assert_eq!(world.counts(), before);
    }

    #[test]
    fn multiple_roots_share_one_forwarding_context() {
        let mut target = Heap::new(1);
        let mut source = Heap::new(2);
        let shared = source.allocate(Object::Bytes(vec![1].into()));
        let roots = copy_roots(
            &mut target,
            HeapView {
                current: &source,
                background: None,
            },
            &[RuntimeValue::Bytes(shared), RuntimeValue::Bytes(shared)],
        )
        .unwrap();
        assert_eq!(roots[0], roots[1]);
        assert_eq!(target.counts().0, 1);
    }

    #[test]
    fn linked_prototype_copy_shares_code_and_rebuilds_links() {
        let function = Arc::new(crate::compile_source("test", "fn(value) { value }").unwrap());
        let closure = Value::Func(Arc::new(Closure::new(
            Arc::clone(&function),
            vec![Value::string("capture")],
        )));
        let mut current = Heap::new(2);
        let root = current.import_value(None, &closure).unwrap();
        let mut world = Heap::new(1);
        let copied = copy_roots(
            &mut world,
            HeapView {
                current: &current,
                background: None,
            },
            &[root],
        )
        .unwrap()[0];
        let exported = HeapView {
            current: &world,
            background: None,
        }
        .export_value(copied)
        .unwrap();
        let Value::Func(exported) = exported else {
            panic!("expected exported closure")
        };
        let Prototype::Bytecode(exported_function) = exported.prototype() else {
            panic!("expected bytecode prototype")
        };
        assert!(Arc::ptr_eq(function.code(), exported_function.code()));
        assert!(
            matches!(exported.upvalues(), [Value::String(value)] if value.as_ref() == "capture")
        );
    }

    #[test]
    fn legacy_value_boundary_round_trips_heap_values() {
        let value = Value::Tuple(
            vec![
                Value::Int(42),
                Value::string("short"),
                Value::Atom(Atom::named("Custom")),
                Value::Array(vec![Value::Float(1.5)].into()),
            ]
            .into(),
        );
        let mut heap = Heap::new(1);
        let runtime = heap.import_value(None, &value).unwrap();
        let exported = HeapView {
            current: &heap,
            background: None,
        }
        .export_value(runtime)
        .unwrap();
        assert_eq!(exported.to_string(), value.to_string());
    }
}
