#![allow(dead_code)]

use crate::{BuiltinAtom, BytecodeFunction, NativeFunction};
use std::collections::HashMap;
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

#[derive(Clone, Debug)]
pub(crate) enum RuntimePrototype {
    Bytecode(Arc<BytecodeFunction>),
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
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct HeapError(&'static str);

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
}

impl Heap {
    pub(crate) fn new(id: u32) -> Self {
        Self {
            id: HeapId(id),
            objects: Vec::new(),
            text: TextTable::default(),
            shapes: Vec::new(),
            shape_slots: HashMap::new(),
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
}

pub(crate) struct HeapView<'a> {
    pub(crate) current: &'a Heap,
    pub(crate) background: Option<&'a Heap>,
}

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
                prototype: match prototype {
                    RuntimePrototype::Bytecode(function) => {
                        RuntimePrototype::Bytecode(Arc::new(function.relink()))
                    }
                    RuntimePrototype::Native(function) => RuntimePrototype::Native(*function),
                },
                upvalues: copy_values(self, upvalues)?,
            },
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
}
