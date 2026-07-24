use crate::ast::{
    BinaryOperator, BindingKind, Block, Expr, ExprKind, Pattern, PatternKind, Program,
    StringPartKind,
};
use crate::compiler::compile_expression_with_bindings;
use crate::heap::{Handle, Heap, PersistentValue};
use crate::hir::{HirDefinitionId, HirDefinitionKind, HirExpressionId, HirProgram};
use crate::json::{Provenance, ValuePath, ValuePathSegment};
use crate::lexer::{FrontendError, SourceLocation};
use crate::lir::RegisterId;
use crate::parser::parse_registered;
use crate::semantic::{
    Conflict, DiagnosticId, FactIdentity, FactState, IncomputableReason, SemanticFact,
    UnknownReason,
};
use crate::source::{Diagnostic, SourceDatabase};
use crate::value::{
    Atom, Closure, CoreBuiltinTypeFunction, CoreModelFunction, NativeError, NativeFunction, Value,
};
use crate::{
    BuiltinAtom, CallContext, DebugSink, DiscardDebugSink, Quota, QuotaAccount, ValueKind,
    ValueRef, Vm,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

const DEFAULT_TOOL_FUEL: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeId(u32);

impl TypeId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeNode {
    Pending,
    Ref(TypeId),
    Any,
    Int,
    Float,
    String,
    Bytes,
    Atom(Atom),
    Array(TypeId),
    Tuple(Vec<TypeId>),
    Struct(BTreeMap<String, TypeId>),
    Enum(BTreeMap<String, Option<TypeId>>),
    Union(Vec<TypeId>),
    Function {
        parameters: Vec<TypeId>,
        result: TypeId,
    },
}

#[derive(Clone, Debug, Default)]
pub struct TypeGraph {
    nodes: Vec<TypeNode>,
    names: BTreeMap<String, TypeId>,
}

impl TypeGraph {
    pub fn node(&self, id: TypeId) -> &TypeNode {
        &self.nodes[id.index()]
    }

    pub fn named(&self, name: &str) -> Option<TypeId> {
        self.names.get(name).copied()
    }

    pub fn names(&self) -> impl Iterator<Item = (&str, TypeId)> {
        self.names.iter().map(|(name, id)| (name.as_str(), *id))
    }

    pub fn nodes(&self) -> impl ExactSizeIterator<Item = (TypeId, &TypeNode)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (TypeId(index as u32), node))
    }

    pub fn display(&self, id: TypeId) -> String {
        self.display_with(id, &mut HashSet::new())
    }

    pub fn is_assignable(&self, actual: TypeId, expected: TypeId) -> bool {
        self.assignable_with(actual, expected, &mut HashSet::new())
    }

    fn push(&mut self, node: TypeNode) -> TypeId {
        let id = TypeId(u32::try_from(self.nodes.len()).expect("type graph exceeds u32"));
        self.nodes.push(node);
        id
    }

    fn intern_descriptor(&mut self, descriptor: &TypeDescriptor) -> TypeId {
        let node = match descriptor {
            TypeDescriptor::Any => TypeNode::Any,
            TypeDescriptor::Int => TypeNode::Int,
            TypeDescriptor::Float => TypeNode::Float,
            TypeDescriptor::String => TypeNode::String,
            TypeDescriptor::Bytes => TypeNode::Bytes,
            TypeDescriptor::Atom(atom) => TypeNode::Atom(atom.clone()),
            TypeDescriptor::Array(item) => TypeNode::Array(self.intern_descriptor(item)),
            TypeDescriptor::Tuple(items) => TypeNode::Tuple(
                items
                    .iter()
                    .map(|item| self.intern_descriptor(item))
                    .collect(),
            ),
            TypeDescriptor::Struct(fields) => TypeNode::Struct(
                fields
                    .iter()
                    .map(|(name, item)| (name.clone(), self.intern_descriptor(item)))
                    .collect(),
            ),
            TypeDescriptor::Enum(variants) => TypeNode::Enum(
                variants
                    .iter()
                    .map(|(name, payload)| {
                        (
                            name.clone(),
                            payload.as_deref().map(|item| self.intern_descriptor(item)),
                        )
                    })
                    .collect(),
            ),
            TypeDescriptor::Union(variants) => TypeNode::Union(
                variants
                    .iter()
                    .map(|item| self.intern_descriptor(item))
                    .collect(),
            ),
            TypeDescriptor::Function { parameters, result } => TypeNode::Function {
                parameters: parameters
                    .iter()
                    .map(|item| self.intern_descriptor(item))
                    .collect(),
                result: self.intern_descriptor(result),
            },
        };
        self.push(node)
    }

    fn decode_persistent(
        &mut self,
        value: ValueRef<'_>,
        path: &str,
        links: &mut HashMap<Handle, TypeId>,
    ) -> Result<TypeId, String> {
        if let Some(handle) = value.hidden_up_link_handle() {
            if let Some(id) = links.get(&handle) {
                return Ok(*id);
            }
            let resolved = value.resolve_hidden_up_link().map_err(|message| {
                format!("{path} contains an uninitialized recursive type link: {message}")
            })?;
            let id = self.decode_persistent(resolved, path, links)?;
            links.insert(handle, id);
            return Ok(id);
        }
        if let Some(handle) = value.object_handle() {
            if let Some(id) = links.get(&handle) {
                return Ok(*id);
            }
            let id = self.push(TypeNode::Pending);
            links.insert(handle, id);
            let node = self.decode_persistent_node(value, path, links)?;
            self.nodes[id.index()] = node;
            return Ok(id);
        }
        let node = self.decode_persistent_node(value, path, links)?;
        Ok(self.push(node))
    }

    fn decode_persistent_node(
        &mut self,
        mut value: ValueRef<'_>,
        path: &str,
        links: &mut HashMap<Handle, TypeId>,
    ) -> Result<TypeNode, String> {
        loop {
            let fields = value
                .dict_fields()
                .ok_or_else(|| format!("{path} must be a Dict"))?;
            let kind = value
                .dict_get("kind")
                .and_then(ValueRef::as_atom)
                .ok_or_else(|| format!("{path}.kind must be an Atom"))?;
            if kind != "WithAttributes" {
                break;
            }
            if fields != ["attributes", "inner", "kind"] {
                return Err(format!("{path} has an invalid WithAttributes wrapper"));
            }
            let attributes = value.dict_get("attributes").expect("wrapper field exists");
            if attributes.kind() != ValueKind::Dict {
                return Err(format!("{path}.attributes must be a Dict"));
            }
            value = value.dict_get("inner").expect("wrapper field exists");
            if value.is_hidden_up_link() {
                let id = self.decode_persistent(value, path, links)?;
                return Ok(TypeNode::Ref(id));
            }
        }

        let fields = value.dict_fields().expect("metadata Dict checked above");
        let kind = value
            .dict_get("kind")
            .and_then(ValueRef::as_atom)
            .expect("metadata kind checked above");
        let require = |expected: &[&str]| {
            fields
                .iter()
                .copied()
                .eq(expected.iter().copied())
                .then_some(())
                .ok_or_else(|| format!("{path} has invalid fields for {kind}"))
        };
        Ok(match kind {
            "Any" => {
                require(&["kind"])?;
                TypeNode::Any
            }
            "Int" => {
                require(&["kind"])?;
                TypeNode::Int
            }
            "Float" => {
                require(&["kind"])?;
                TypeNode::Float
            }
            "String" => {
                require(&["kind"])?;
                TypeNode::String
            }
            "Bytes" => {
                require(&["kind"])?;
                TypeNode::Bytes
            }
            "Atom" => {
                require(&["kind", "tag"])?;
                let tag = value
                    .dict_get("tag")
                    .and_then(ValueRef::as_atom)
                    .ok_or_else(|| format!("{path}.tag must be an Atom"))?;
                TypeNode::Atom(atom_from_name(tag))
            }
            "Array" => {
                require(&["item", "kind"])?;
                let item = self.decode_persistent(
                    value.dict_get("item").expect("field exists"),
                    &format!("{path}.item"),
                    links,
                )?;
                TypeNode::Array(item)
            }
            "Tuple" | "Union" => {
                let field = if kind == "Tuple" { "items" } else { "variants" };
                require(if kind == "Tuple" {
                    &["items", "kind"]
                } else {
                    &["kind", "variants"]
                })?;
                let sequence = value.dict_get(field).expect("field exists");
                if sequence.kind() != ValueKind::Array {
                    return Err(format!("{path}.{field} must be an Array"));
                }
                let mut values = Vec::new();
                for index in 0..sequence.sequence_len().expect("Array length") {
                    values.push(self.decode_persistent(
                        sequence.sequence_get(index).expect("Array item"),
                        &format!("{path}.{field}[{index}]"),
                        links,
                    )?);
                }
                if kind == "Union" && values.is_empty() {
                    return Err(format!("{path}.variants must not be empty"));
                }
                if kind == "Tuple" {
                    TypeNode::Tuple(values)
                } else {
                    TypeNode::Union(values)
                }
            }
            "Struct" => {
                require(&["fields", "kind"])?;
                let values = value.dict_get("fields").expect("field exists");
                let names = values
                    .dict_fields()
                    .ok_or_else(|| format!("{path}.fields must be a Dict"))?;
                let mut decoded = BTreeMap::new();
                for name in names {
                    let id = self.decode_persistent(
                        values.dict_get(name).expect("Dict field"),
                        &format!("{path}.fields.{name}"),
                        links,
                    )?;
                    decoded.insert(name.to_owned(), id);
                }
                TypeNode::Struct(decoded)
            }
            "Enum" => {
                require(&["kind", "variants"])?;
                let values = value.dict_get("variants").expect("field exists");
                let names = values
                    .dict_fields()
                    .ok_or_else(|| format!("{path}.variants must be a Dict"))?;
                if names.is_empty() {
                    return Err(format!("{path}.variants must not be empty"));
                }
                let mut decoded = BTreeMap::new();
                for name in names {
                    let variant_path = format!("{path}.variants.{name}");
                    let inner = strip_attributes_ref(
                        values.dict_get(name).expect("Dict field"),
                        &variant_path,
                    )?;
                    let payload = if inner.as_atom() == Some("None") {
                        None
                    } else {
                        Some(self.decode_persistent(inner, &variant_path, links)?)
                    };
                    decoded.insert(name.to_owned(), payload);
                }
                TypeNode::Enum(decoded)
            }
            "Function" => {
                require(&["kind", "parameters", "result"])?;
                let values = value.dict_get("parameters").expect("field exists");
                if values.kind() != ValueKind::Array {
                    return Err(format!("{path}.parameters must be an Array"));
                }
                let mut parameters = Vec::new();
                for index in 0..values.sequence_len().expect("Array length") {
                    parameters.push(self.decode_persistent(
                        values.sequence_get(index).expect("Array item"),
                        &format!("{path}.parameters[{index}]"),
                        links,
                    )?);
                }
                let result = self.decode_persistent(
                    value.dict_get("result").expect("field exists"),
                    &format!("{path}.result"),
                    links,
                )?;
                TypeNode::Function { parameters, result }
            }
            _ => return Err(format!("{path}.kind has unknown value '{kind}'")),
        })
    }

    fn display_with(&self, id: TypeId, active: &mut HashSet<TypeId>) -> String {
        if !active.insert(id) {
            return self
                .names
                .iter()
                .find_map(|(name, candidate)| (*candidate == id).then(|| name.clone()))
                .unwrap_or_else(|| "recursive".into());
        }
        let shown = match self.node(id) {
            TypeNode::Pending => "<pending>".into(),
            TypeNode::Ref(target) => self.display_with(*target, active),
            TypeNode::Any => "Any".into(),
            TypeNode::Int => "Int".into(),
            TypeNode::Float => "Float".into(),
            TypeNode::String => "String".into(),
            TypeNode::Bytes => "Bytes".into(),
            TypeNode::Atom(atom) => format!("'{}", atom.name()),
            TypeNode::Array(item) => format!("Array<{}>", self.display_with(*item, active)),
            TypeNode::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|item| self.display_with(*item, active))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            TypeNode::Struct(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(name, item)| format!("{name}: {}", self.display_with(*item, active)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            TypeNode::Enum(variants) => format!(
                "enum {{{}}}",
                variants
                    .iter()
                    .map(|(name, payload)| payload.map_or_else(
                        || name.clone(),
                        |payload| format!("{name}({})", self.display_with(payload, active))
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            TypeNode::Union(variants) => variants
                .iter()
                .map(|item| self.display_with(*item, active))
                .collect::<Vec<_>>()
                .join(" | "),
            TypeNode::Function { parameters, result } => format!(
                "fn({}) -> {}",
                parameters
                    .iter()
                    .map(|item| self.display_with(*item, active))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.display_with(*result, active)
            ),
        };
        active.remove(&id);
        shown
    }

    fn assignable_with(
        &self,
        actual: TypeId,
        expected: TypeId,
        visited: &mut HashSet<(TypeId, TypeId)>,
    ) -> bool {
        if !visited.insert((actual, expected)) {
            return true;
        }
        match (self.node(actual), self.node(expected)) {
            (TypeNode::Ref(actual), _) => self.assignable_with(*actual, expected, visited),
            (_, TypeNode::Ref(expected)) => self.assignable_with(actual, *expected, visited),
            (TypeNode::Any, _) | (_, TypeNode::Any) => true,
            (TypeNode::Array(a), TypeNode::Array(e)) => self.assignable_with(*a, *e, visited),
            (TypeNode::Tuple(a), TypeNode::Tuple(e)) => {
                a.len() == e.len()
                    && a.iter()
                        .zip(e)
                        .all(|(a, e)| self.assignable_with(*a, *e, visited))
            }
            (TypeNode::Struct(a), TypeNode::Struct(e)) => {
                a.len() == e.len()
                    && e.iter().all(|(name, e)| {
                        a.get(name)
                            .is_some_and(|a| self.assignable_with(*a, *e, visited))
                    })
            }
            (TypeNode::Enum(a), TypeNode::Enum(e)) => {
                a.len() == e.len()
                    && e.iter().all(|(name, e)| {
                        a.get(name).is_some_and(|a| match (a, e) {
                            (None, None) => true,
                            (Some(a), Some(e)) => self.assignable_with(*a, *e, visited),
                            _ => false,
                        })
                    })
            }
            (TypeNode::Union(a), _) => a
                .iter()
                .all(|a| self.assignable_with(*a, expected, visited)),
            (_, TypeNode::Union(e)) => e.iter().any(|e| self.assignable_with(actual, *e, visited)),
            (
                TypeNode::Function {
                    parameters: ap,
                    result: ar,
                },
                TypeNode::Function {
                    parameters: ep,
                    result: er,
                },
            ) => {
                ap.len() == ep.len()
                    && ap
                        .iter()
                        .zip(ep)
                        .all(|(a, e)| self.assignable_with(*a, *e, visited))
                    && self.assignable_with(*ar, *er, visited)
            }
            (a, e) => a == e,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeDescriptor {
    Any,
    Int,
    Float,
    String,
    Bytes,
    Atom(Atom),
    Array(Box<TypeDescriptor>),
    Tuple(Vec<TypeDescriptor>),
    Struct(BTreeMap<String, TypeDescriptor>),
    Enum(BTreeMap<String, Option<Box<TypeDescriptor>>>),
    Union(Vec<TypeDescriptor>),
    Function {
        parameters: Vec<TypeDescriptor>,
        result: Box<TypeDescriptor>,
    },
}

impl TypeDescriptor {
    pub fn to_value(&self, vm: &mut Vm) -> Value {
        let entries = match self {
            Self::Any => vec![kind_entry("Any")],
            Self::Int => vec![kind_entry("Int")],
            Self::Float => vec![kind_entry("Float")],
            Self::String => vec![kind_entry("String")],
            Self::Bytes => vec![kind_entry("Bytes")],
            Self::Atom(tag) => vec![kind_entry("Atom"), ("tag".into(), Value::Atom(tag.clone()))],
            Self::Array(item) => vec![kind_entry("Array"), ("item".into(), item.to_value(vm))],
            Self::Tuple(items) => vec![
                kind_entry("Tuple"),
                (
                    "items".into(),
                    Value::Array(
                        items
                            .iter()
                            .map(|item| item.to_value(vm))
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                ),
            ],
            Self::Struct(fields) => {
                let field_values = fields
                    .iter()
                    .map(|(name, field)| (name.clone(), field.to_value(vm)))
                    .collect::<Vec<_>>();
                let fields = vm
                    .make_dict(field_values)
                    .expect("Type Struct fields are unique");
                vec![kind_entry("Struct"), ("fields".into(), fields)]
            }
            Self::Enum(variants) => {
                let variants = variants
                    .iter()
                    .map(|(name, payload)| {
                        (
                            name.clone(),
                            payload
                                .as_ref()
                                .map_or_else(Value::none, |payload| payload.to_value(vm)),
                        )
                    })
                    .collect::<Vec<_>>();
                let variants = vm
                    .make_dict(variants)
                    .expect("Type Enum variants are unique");
                vec![kind_entry("Enum"), ("variants".into(), variants)]
            }
            Self::Union(variants) => vec![
                kind_entry("Union"),
                (
                    "variants".into(),
                    Value::Array(
                        variants
                            .iter()
                            .map(|variant| variant.to_value(vm))
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                ),
            ],
            Self::Function { parameters, result } => vec![
                kind_entry("Function"),
                (
                    "parameters".into(),
                    Value::Array(
                        parameters
                            .iter()
                            .map(|parameter| parameter.to_value(vm))
                            .collect::<Vec<_>>()
                            .into(),
                    ),
                ),
                ("result".into(), result.to_value(vm)),
            ],
        };
        vm.make_dict(entries)
            .expect("Type metadata fields are unique")
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        decode_type(value, "Type")
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Any => "Any".into(),
            Self::Int => "Int".into(),
            Self::Float => "Float".into(),
            Self::String => "String".into(),
            Self::Bytes => "Bytes".into(),
            Self::Atom(atom) => format!("'{}", atom.name()),
            Self::Array(item) => format!("Array<{}>", item.display_name()),
            Self::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(Self::display_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Struct(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(name, item)| format!("{name}: {}", item.display_name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Enum(variants) => format!(
                "enum {{{}}}",
                variants
                    .iter()
                    .map(|(name, payload)| payload.as_ref().map_or_else(
                        || name.clone(),
                        |payload| format!("{name}({})", payload.display_name())
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Union(variants) => variants
                .iter()
                .map(Self::display_name)
                .collect::<Vec<_>>()
                .join(" | "),
            Self::Function { parameters, result } => format!(
                "fn({}) -> {}",
                parameters
                    .iter()
                    .map(Self::display_name)
                    .collect::<Vec<_>>()
                    .join(", "),
                result.display_name()
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub types: TypeGraph,
    pub declared_types: BTreeMap<String, TypeId>,
    pub binding_types: BTreeMap<String, TypeId>,
    pub result_type: TypeId,
    pub hir: HirProgram,
    pub definition_types: BTreeMap<HirDefinitionId, TypeId>,
    pub expression_types: BTreeMap<HirExpressionId, TypeId>,
    pub(crate) prelude: BTreeMap<String, Value>,
    pub(crate) external_values: BTreeMap<String, Value>,
    pub(crate) dynamic_bindings: HashSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticDependencyNode {
    pub definition: HirDefinitionId,
    pub dependencies: Vec<HirDefinitionId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SemanticDependencyGraph {
    pub nodes: Vec<SemanticDependencyNode>,
}

#[derive(Clone, Debug)]
pub struct PartialAnalysis {
    pub hir: HirProgram,
    pub dependencies: SemanticDependencyGraph,
    pub definition_facts: BTreeMap<HirDefinitionId, SemanticFact<TypeId>>,
    pub diagnostics: Vec<Diagnostic>,
    pub types: TypeGraph,
}

impl Analysis {
    pub fn display(&self, id: TypeId) -> String {
        self.types.display(id)
    }

    pub(crate) fn install_promoted_types(
        &mut self,
        heap: &Heap,
        roots: &BTreeMap<String, PersistentValue>,
    ) -> Result<(), String> {
        let mut links = HashMap::<Handle, TypeId>::new();
        for (name, root) in roots {
            let id = self.types.decode_persistent(
                ValueRef::persistent(*root, heap),
                &format!("type {name}"),
                &mut links,
            )?;
            self.types.names.insert(name.clone(), id);
            self.declared_types.insert(name.clone(), id);
            self.binding_types.insert(name.clone(), id);
            for definition in self.hir.definitions() {
                if definition.top_level
                    && definition.kind == HirDefinitionKind::Type
                    && definition.name == *name
                {
                    self.definition_types.insert(definition.id, id);
                }
            }
        }
        Ok(())
    }
}

pub fn analyze_source(source_name: &str, source: &str) -> Result<Analysis, FrontendError> {
    analyze_source_with_fuel(source_name, source, DEFAULT_TOOL_FUEL)
}

pub fn analyze_source_with_fuel(
    source_name: &str,
    source: &str,
    evaluation_fuel: usize,
) -> Result<Analysis, FrontendError> {
    analyze_source_with_quota(source_name, source, Quota::with_fuel(evaluation_fuel))
}

pub fn analyze_source_with_quota(
    source_name: &str,
    source: &str,
    quota: Quota,
) -> Result<Analysis, FrontendError> {
    let mut sources = SourceDatabase::default();
    let source_id = sources.add(source_name, source);
    let parsed = parse_registered(&sources, source_id);
    let program = parsed.program.ok_or_else(|| {
        FrontendError::from_diagnostic(
            &sources,
            parsed
                .diagnostics
                .into_iter()
                .next()
                .expect("failed parse has a diagnostic"),
        )
    })?;
    let mut account = QuotaAccount::new(quota);
    analyze_program_with_bindings(
        source_name,
        &program,
        &mut account,
        &BTreeMap::new(),
        &HashSet::new(),
        &sources,
        &BTreeMap::new(),
    )
}

pub fn analyze_partial_types(source_name: &str, source: &str, quota: Quota) -> PartialAnalysis {
    analyze_partial_types_with_bindings(source_name, source, quota, &BTreeMap::new())
}

pub fn analyze_partial_types_with_bindings(
    source_name: &str,
    source: &str,
    quota: Quota,
    external_values: &BTreeMap<String, Value>,
) -> PartialAnalysis {
    let mut sources = SourceDatabase::default();
    let source_id = sources.add(source_name, source);
    let parsed = parse_registered(&sources, source_id);
    let mut vm = Vm::new();
    let prelude = core_prelude(&mut vm);
    let hir = HirProgram::resolve_recovered(
        &parsed.recovered,
        prelude
            .keys()
            .chain(external_values.keys())
            .cloned()
            .collect::<Vec<_>>(),
    );
    let mut bindings = BTreeMap::new();
    for binding in &parsed.recovered.bindings {
        if binding.value.kind != BindingKind::Type {
            continue;
        }
        if let Some(definition) = hir.definitions().iter().find(|definition| {
            definition.top_level
                && definition.kind == HirDefinitionKind::Type
                && definition.location == binding.value.name.location
        }) {
            bindings.insert(definition.id, binding);
        }
    }
    let type_definitions = bindings.keys().copied().collect::<HashSet<_>>();
    let mut nodes = bindings
        .keys()
        .map(|definition| {
            let root = hir
                .definition(*definition)
                .and_then(|definition| definition.value)
                .expect("retained type definition has a value expression");
            let mut dependencies = hir
                .expressions()
                .iter()
                .filter(|expression| expression_descends_from(&hir, expression.id, root))
                .filter_map(|expression| expression.reference)
                .filter_map(|reference| hir.reference(reference))
                .filter_map(|reference| match reference.resolution {
                    crate::hir::HirResolution::Definition(dependency)
                        if type_definitions.contains(&dependency) =>
                    {
                        Some(dependency)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            dependencies.sort_unstable();
            dependencies.dedup();
            SemanticDependencyNode {
                definition: *definition,
                dependencies,
            }
        })
        .collect::<Vec<_>>();
    nodes.sort_by_key(|node| node.definition);
    let dependencies = SemanticDependencyGraph { nodes };

    let mut diagnostics = parsed.diagnostics;
    let mut facts: BTreeMap<HirDefinitionId, SemanticFact<TypeId>> = BTreeMap::new();
    let mut types = TypeGraph::default();
    let mut tool_values = prelude;
    tool_values.extend(external_values.clone());
    let any_metadata = tool_values
        .get("Any")
        .expect("core prelude defines Any")
        .clone();
    for binding in bindings.values() {
        tool_values.insert(binding.value.name.value.clone(), any_metadata.clone());
    }
    let mut account = QuotaAccount::new(quota);
    let debug_sink: Arc<dyn DebugSink> = Arc::new(DiscardDebugSink);

    while facts.len() < bindings.len() {
        let mut progressed = false;
        for node in &dependencies.nodes {
            if facts.contains_key(&node.definition) {
                continue;
            }
            let blocked = node.dependencies.iter().find(|dependency| {
                facts
                    .get(*dependency)
                    .is_some_and(|fact| fact.state != FactState::Known)
            });
            if let Some(dependency) = blocked {
                let cause = FactIdentity::HirDefinition(*dependency);
                let mut fact = SemanticFact::unknown(UnknownReason::BlockedBy(cause));
                fact.causes.push(cause);
                facts.insert(node.definition, fact);
                progressed = true;
                continue;
            }
            if node
                .dependencies
                .iter()
                .any(|dependency| !facts.contains_key(dependency))
            {
                continue;
            }

            let binding = bindings[&node.definition];
            let outcome = evaluate_tool_expression(
                source_name,
                &binding.value.value,
                &tool_values,
                &mut account,
                &sources,
                &debug_sink,
            )
            .and_then(|value| {
                TypeDescriptor::from_value(&value)
                    .map(|descriptor| (value, descriptor))
                    .map_err(|message| {
                        FrontendError::from_diagnostic(
                            &sources,
                            Diagnostic::error(
                                format!(
                                    "type {} produced invalid metadata: {message}",
                                    binding.value.name.value
                                ),
                                binding.value.value.location,
                            ),
                        )
                    })
            });
            match outcome {
                Ok((value, descriptor)) => {
                    let id = types.intern_descriptor(&descriptor);
                    types.names.insert(binding.value.name.value.clone(), id);
                    tool_values.insert(binding.value.name.value.clone(), value);
                    facts.insert(node.definition, SemanticFact::known(id));
                }
                Err(error) => {
                    let state = classify_partial_error(&error.message);
                    let diagnostic = DiagnosticId::from_index(diagnostics.len());
                    diagnostics.push(error.diagnostic.map_or_else(
                        || Diagnostic::error(error.message, binding.value.value.location),
                        |diagnostic| *diagnostic,
                    ));
                    let mut fact = match state {
                        FactState::Conflicted(conflict) => SemanticFact::conflicted(None, conflict),
                        FactState::Incomputable(reason) => SemanticFact::incomputable(None, reason),
                        FactState::Unknown(reason) => SemanticFact::unknown(reason),
                        FactState::Known => unreachable!("errors cannot produce known facts"),
                    };
                    fact.diagnostics.push(diagnostic);
                    facts.insert(node.definition, fact);
                }
            }
            progressed = true;
        }
        if progressed {
            continue;
        }

        let cyclic = dependencies
            .nodes
            .iter()
            .filter(|node| !facts.contains_key(&node.definition))
            .filter(|node| dependency_reaches(&dependencies, node.definition, node.definition))
            .map(|node| node.definition)
            .collect::<Vec<_>>();
        let had_cycle = !cyclic.is_empty();
        for definition in cyclic {
            let binding = bindings[&definition];
            let diagnostic = DiagnosticId::from_index(diagnostics.len());
            diagnostics.push(Diagnostic::error(
                format!(
                    "recursive type component containing {:?} cannot be partially evaluated",
                    binding.value.name.value
                ),
                binding.value.name.location,
            ));
            let mut fact = SemanticFact::incomputable(None, IncomputableReason::CyclicEvaluation);
            fact.diagnostics.push(diagnostic);
            facts.insert(definition, fact);
        }
        if had_cycle {
            continue;
        }
        break;
    }
    let mut indexed_diagnostics = diagnostics.into_iter().enumerate().collect::<Vec<_>>();
    indexed_diagnostics.sort_by_key(|(_, diagnostic)| {
        diagnostic
            .labels
            .first()
            .map_or(0, |label| label.location.start)
    });
    let mut remapped_diagnostics = vec![DiagnosticId::from_index(0); indexed_diagnostics.len()];
    for (new, (old, _)) in indexed_diagnostics.iter().enumerate() {
        remapped_diagnostics[*old] = DiagnosticId::from_index(new);
    }
    for fact in facts.values_mut() {
        for diagnostic in &mut fact.diagnostics {
            *diagnostic = remapped_diagnostics[diagnostic.index()];
        }
    }
    let diagnostics = indexed_diagnostics
        .into_iter()
        .map(|(_, diagnostic)| diagnostic)
        .collect();
    PartialAnalysis {
        hir,
        dependencies,
        definition_facts: facts,
        diagnostics,
        types,
    }
}

fn expression_descends_from(
    hir: &HirProgram,
    mut expression: HirExpressionId,
    root: HirExpressionId,
) -> bool {
    loop {
        if expression == root {
            return true;
        }
        let Some(parent) = hir
            .expression(expression)
            .and_then(|expression| expression.parent)
        else {
            return false;
        };
        expression = parent;
    }
}

fn dependency_reaches(
    graph: &SemanticDependencyGraph,
    current: HirDefinitionId,
    target: HirDefinitionId,
) -> bool {
    fn visit(
        graph: &SemanticDependencyGraph,
        current: HirDefinitionId,
        target: HirDefinitionId,
        visited: &mut HashSet<HirDefinitionId>,
    ) -> bool {
        let Some(node) = graph.nodes.iter().find(|node| node.definition == current) else {
            return false;
        };
        node.dependencies.iter().any(|dependency| {
            *dependency == target
                || visited.insert(*dependency) && visit(graph, *dependency, target, visited)
        })
    }
    visit(graph, current, target, &mut HashSet::new())
}

fn classify_partial_error(message: &str) -> FactState {
    if message.contains("not assignable") || message.contains("incompatible") {
        FactState::Conflicted(Conflict::IncompatibleContract)
    } else if message.contains("fuel exhausted")
        || message.contains("quota")
        || message.contains("stack limit")
    {
        FactState::Incomputable(IncomputableReason::QuotaExceeded)
    } else if message.contains("native symbol") || message.contains("has not been resolved") {
        FactState::Incomputable(IncomputableReason::RuntimeOnly)
    } else {
        FactState::Incomputable(IncomputableReason::UnsupportedOperation)
    }
}

pub(crate) fn analyze_program_registered(
    source_name: &str,
    sources: &SourceDatabase,
    program: &Program,
    evaluation_fuel: usize,
) -> Result<Analysis, FrontendError> {
    let mut account = QuotaAccount::new(Quota::with_fuel(evaluation_fuel));
    analyze_program_with_bindings(
        source_name,
        program,
        &mut account,
        &BTreeMap::new(),
        &HashSet::new(),
        sources,
        &BTreeMap::new(),
    )
}

pub(crate) fn analyze_program_with_bindings(
    source_name: &str,
    program: &Program,
    account: &mut QuotaAccount,
    external_values: &BTreeMap<String, Value>,
    dynamic_bindings: &HashSet<String>,
    sources: &SourceDatabase,
    external_provenance: &BTreeMap<String, Provenance>,
) -> Result<Analysis, FrontendError> {
    let debug_sink: Arc<dyn DebugSink> = Arc::new(DiscardDebugSink);
    analyze_program_with_bindings_observed(
        source_name,
        program,
        account,
        external_values,
        dynamic_bindings,
        sources,
        external_provenance,
        &debug_sink,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_program_with_bindings_observed(
    source_name: &str,
    program: &Program,
    account: &mut QuotaAccount,
    external_values: &BTreeMap<String, Value>,
    dynamic_bindings: &HashSet<String>,
    sources: &SourceDatabase,
    external_provenance: &BTreeMap<String, Provenance>,
    debug_sink: &Arc<dyn DebugSink>,
) -> Result<Analysis, FrontendError> {
    let mut tool_vm = Vm::new();
    let prelude = core_prelude(&mut tool_vm);
    let hir = HirProgram::resolve(
        program,
        prelude
            .keys()
            .chain(external_values.keys())
            .cloned()
            .collect::<Vec<_>>(),
    );
    if let Some(reference) = hir.unresolved().next() {
        return Err(FrontendError::from_diagnostic(
            sources,
            Diagnostic::error(
                format!("unknown binding {:?}", reference.name),
                reference.location,
            ),
        ));
    }
    let mut tool_values = prelude.clone();
    let mut static_environment = HashMap::new();
    let mut declared_types = BTreeMap::new();
    let mut binding_types = BTreeMap::new();
    let mut declared_type_spans = HashMap::new();
    let mut expression_descriptors = HashMap::new();

    // Tool-stage descriptors are currently trees. Predeclare type names with
    // conservative metadata so self and forward references can be evaluated;
    // the runtime retains the authoritative recursive metadata graph.
    let any_metadata = prelude
        .get("Any")
        .expect("core prelude defines Any")
        .clone();
    for binding in &program.value.body.value.bindings {
        if binding.value.kind == BindingKind::Type {
            tool_values.insert(binding.value.name.value.clone(), any_metadata.clone());
        }
    }

    for name in dynamic_bindings {
        if !external_values.contains_key(name) {
            return Err(frontend_error(
                source_name,
                format!("dynamic binding {name:?} has no value"),
            ));
        }
        static_environment.insert(name.clone(), TypeDescriptor::Any);
        binding_types.insert(name.clone(), TypeDescriptor::Any);
    }

    let mut definition_contracts = HashMap::new();
    let mut declaration_locations = HashMap::new();
    let mut definition_counts = HashMap::<String, usize>::new();
    for binding in &program.value.body.value.bindings {
        let name = &binding.value.name.value;
        if matches!(
            binding.value.kind,
            BindingKind::Def | BindingKind::NamedFunction
        ) {
            *definition_counts.entry(name.clone()).or_default() += 1;
        }
        if !matches!(binding.value.kind, BindingKind::Decl | BindingKind::Native) {
            continue;
        }
        if definition_contracts.contains_key(name) {
            return Err(FrontendError::from_diagnostic(
                sources,
                Diagnostic::error(format!("duplicate declaration {name:?}"), binding.location),
            ));
        }
        let contract = binding
            .value
            .annotation
            .as_ref()
            .expect("declaration has a lowered contract");
        let metadata = evaluate_tool_expression(
            source_name,
            contract,
            &tool_values,
            account,
            sources,
            debug_sink,
        )?;
        let descriptor = TypeDescriptor::from_value(&metadata).map_err(|message| {
            frontend_error(
                source_name,
                format!("declaration {name} has invalid contract metadata: {message}"),
            )
        })?;
        static_environment.insert(name.clone(), descriptor.clone());
        binding_types.insert(name.clone(), descriptor.clone());
        if binding.value.kind == BindingKind::Decl {
            definition_contracts.insert(name.clone(), descriptor);
            declaration_locations.insert(name.clone(), binding.location);
        }
    }
    for binding in &program.value.body.value.bindings {
        if binding.value.kind != BindingKind::NamedFunction {
            continue;
        }
        let name = &binding.value.name.value;
        let Some(contract) = &binding.value.annotation else {
            continue;
        };
        let metadata = evaluate_tool_expression(
            source_name,
            contract,
            &tool_values,
            account,
            sources,
            debug_sink,
        )?;
        let descriptor = TypeDescriptor::from_value(&metadata).map_err(|message| {
            frontend_error(
                source_name,
                format!("function {name} has invalid contract metadata: {message}"),
            )
        })?;
        if let Some(declared) = definition_contracts.get(name) {
            if !assignable(&descriptor, declared) {
                return Err(FrontendError::from_diagnostic(
                    sources,
                    Diagnostic::error(
                        format!(
                            "function {name} contract {} is incompatible with declared {}",
                            descriptor.display_name(),
                            declared.display_name()
                        ),
                        contract.location,
                    )
                    .with_secondary("contract declared here", declaration_locations[name]),
                ));
            }
        } else {
            static_environment.insert(name.clone(), descriptor.clone());
            binding_types.insert(name.clone(), descriptor.clone());
            definition_contracts.insert(name.clone(), descriptor);
        }
    }
    for (name, count) in &definition_counts {
        if *count > 1 {
            return Err(frontend_error(
                source_name,
                format!("definition {name:?} is initialized more than once"),
            ));
        }
    }

    for binding in &program.value.body.value.bindings {
        check_interpolations(&binding.value.value, &static_environment, sources)?;
        let inferred_expression = infer_expr_recorded(
            &binding.value.value,
            &static_environment,
            &mut expression_descriptors,
        );
        if let Some(annotation) = &binding.value.annotation {
            check_interpolations(annotation, &static_environment, sources)?;
            infer_expr_recorded(annotation, &static_environment, &mut expression_descriptors);
        }
        match binding.value.kind {
            BindingKind::Decl => continue,
            BindingKind::Native => {
                let value = external_values
                    .get(&binding.value.name.value)
                    .cloned()
                    .ok_or_else(|| {
                        FrontendError::from_diagnostic(
                            sources,
                            Diagnostic::error(
                                format!(
                                    "native symbol {:?} has not been linked",
                                    binding.value.name.value
                                ),
                                binding.location,
                            ),
                        )
                    })?;
                tool_values.insert(binding.value.name.value.clone(), value);
            }
            BindingKind::Type => {
                let value = evaluate_tool_expression(
                    source_name,
                    &binding.value.value,
                    &tool_values,
                    account,
                    sources,
                    debug_sink,
                )?;
                let descriptor = TypeDescriptor::from_value(&value).map_err(|message| {
                    FrontendError::from_diagnostic(
                        sources,
                        Diagnostic::error(
                            format!(
                                "type {} produced invalid metadata: {message}",
                                binding.value.name.value
                            ),
                            binding.value.value.location,
                        ),
                    )
                })?;
                declared_types.insert(binding.value.name.value.clone(), descriptor);
                declared_type_spans.insert(binding.value.name.value.clone(), binding.location);
                tool_values.insert(binding.value.name.value.clone(), value);
            }
            BindingKind::Let => {
                let inferred = inferred_expression;
                let checked = if let Some(annotation) = &binding.value.annotation {
                    let metadata = evaluate_tool_expression(
                        source_name,
                        annotation,
                        &tool_values,
                        account,
                        sources,
                        debug_sink,
                    )?;
                    let expected = TypeDescriptor::from_value(&metadata).map_err(|message| {
                        frontend_error(
                            source_name,
                            format!(
                                "annotation on {} is invalid: {message}",
                                binding.value.name.value
                            ),
                        )
                    })?;
                    if !assignable(&inferred, &expected) {
                        let message = format!(
                            "binding {} has type {}, which is not assignable to {}",
                            binding.value.name.value,
                            inferred.display_name(),
                            expected.display_name()
                        );
                        {
                            let path =
                                incompatibility_path(&inferred, &expected).unwrap_or_default();
                            let data_span = match &binding.value.value.value {
                                ExprKind::Variable(name) => external_provenance
                                    .get(&name.value)
                                    .and_then(|provenance| {
                                        provenance
                                            .values
                                            .get(&path)
                                            .or_else(|| provenance.values.get(&Vec::new()))
                                    })
                                    .cloned(),
                                _ => Some(binding.value.value.location),
                            }
                            .unwrap_or(binding.location);
                            let rule_span = match &annotation.value {
                                ExprKind::Variable(name) => {
                                    declared_type_spans.get(&name.value).copied()
                                }
                                _ => Some(annotation.location),
                            }
                            .unwrap_or(binding.location);
                            let diagnostic = Diagnostic::error(message, data_span)
                                .with_secondary("type requirement declared here", rule_span);
                            return Err(FrontendError::from_diagnostic(sources, diagnostic));
                        }
                    }
                    expected
                } else {
                    inferred
                };
                static_environment.insert(binding.value.name.value.clone(), checked.clone());
                binding_types.insert(binding.value.name.value.clone(), checked);

                if let Ok(value) = evaluate_tool_expression(
                    source_name,
                    &binding.value.value,
                    &tool_values,
                    account,
                    sources,
                    debug_sink,
                ) {
                    tool_values.insert(binding.value.name.value.clone(), value);
                }
            }
            BindingKind::Def | BindingKind::NamedFunction => {
                let name = &binding.value.name.value;
                let inferred = inferred_expression;
                let checked = if let Some(expected) = definition_contracts.get(name) {
                    if !assignable(&inferred, expected) {
                        let declaration = declaration_locations
                            .get(name)
                            .copied()
                            .unwrap_or(binding.location);
                        return Err(FrontendError::from_diagnostic(
                            sources,
                            Diagnostic::error(
                                format!(
                                    "definition {name} has type {}, which is not assignable to {}",
                                    inferred.display_name(),
                                    expected.display_name()
                                ),
                                binding.value.value.location,
                            )
                            .with_secondary("contract declared here", declaration),
                        ));
                    }
                    expected.clone()
                } else {
                    inferred
                };
                static_environment.insert(name.clone(), checked.clone());
                binding_types.insert(name.clone(), checked);
                if let Ok(value) = evaluate_tool_expression(
                    source_name,
                    &binding.value.value,
                    &tool_values,
                    account,
                    sources,
                    debug_sink,
                ) {
                    tool_values.insert(name.clone(), value);
                }
            }
            BindingKind::Import => {
                let value = external_values
                    .get(&binding.value.name.value)
                    .cloned()
                    .ok_or_else(|| {
                        frontend_error(
                            source_name,
                            format!("import {} has not been resolved", binding.value.name.value),
                        )
                    })?;
                let inferred = infer_value(&value);
                static_environment.insert(binding.value.name.value.clone(), inferred.clone());
                binding_types.insert(binding.value.name.value.clone(), inferred);
                tool_values.insert(binding.value.name.value.clone(), value);
            }
        }
    }

    for (name, location) in &declaration_locations {
        if definition_counts.get(name).copied().unwrap_or(0) == 0 {
            return Err(FrontendError::from_diagnostic(
                sources,
                Diagnostic::error(
                    format!("definition {name:?} was declared but never initialized"),
                    *location,
                ),
            ));
        }
    }

    check_interpolations(
        &program.value.body.value.result,
        &static_environment,
        sources,
    )?;
    let result_type = infer_expr_recorded(
        &program.value.body.value.result,
        &static_environment,
        &mut expression_descriptors,
    );
    let mut types = TypeGraph::default();
    let declared_types: BTreeMap<String, TypeId> = declared_types
        .into_iter()
        .map(|(name, descriptor)| {
            let id = types.intern_descriptor(&descriptor);
            types.names.insert(name.clone(), id);
            (name, id)
        })
        .collect();
    let binding_types: BTreeMap<String, TypeId> = binding_types
        .into_iter()
        .map(|(name, descriptor)| (name, types.intern_descriptor(&descriptor)))
        .collect();
    let result_type = types.intern_descriptor(&result_type);
    let expression_types: BTreeMap<HirExpressionId, TypeId> = hir
        .expressions()
        .iter()
        .filter_map(|expression| {
            expression_descriptors
                .get(&expression.location)
                .map(|descriptor| (expression.id, types.intern_descriptor(descriptor)))
        })
        .collect();
    let any_type = types.intern_descriptor(&TypeDescriptor::Any);
    let definition_types = hir
        .definitions()
        .iter()
        .map(|definition| {
            let ty = if definition.top_level {
                if definition.kind == HirDefinitionKind::Type {
                    declared_types.get(&definition.name).copied()
                } else {
                    binding_types.get(&definition.name).copied()
                }
            } else {
                definition
                    .value
                    .and_then(|value| expression_types.get(&value).copied())
            }
            .unwrap_or(any_type);
            (definition.id, ty)
        })
        .collect();
    Ok(Analysis {
        types,
        declared_types,
        binding_types,
        result_type,
        hir,
        definition_types,
        expression_types,
        prelude,
        external_values: external_values.clone(),
        dynamic_bindings: dynamic_bindings.clone(),
    })
}

pub(crate) fn infer_value(value: &Value) -> TypeDescriptor {
    match value {
        Value::Int(_) => TypeDescriptor::Int,
        Value::Float(_) => TypeDescriptor::Float,
        Value::String(_) => TypeDescriptor::String,
        Value::Bytes(_) => TypeDescriptor::Bytes,
        Value::Atom(atom) => TypeDescriptor::Atom(atom.clone()),
        Value::Array(items) => {
            let item =
                common_type(items.iter().map(infer_value).collect()).unwrap_or(TypeDescriptor::Any);
            TypeDescriptor::Array(Box::new(item))
        }
        Value::Tuple(items) => TypeDescriptor::Tuple(items.iter().map(infer_value).collect()),
        Value::Dict(fields) => TypeDescriptor::Struct(
            fields
                .shape()
                .fields()
                .iter()
                .zip(fields.values())
                .map(|(name, value)| (name.clone(), infer_value(value)))
                .collect(),
        ),
        Value::Func(closure) => {
            let arity = match closure.prototype() {
                crate::Prototype::Bytecode(function) => function.parameter_count(),
                crate::Prototype::Native(function) => function.arity(),
            };
            TypeDescriptor::Function {
                parameters: vec![TypeDescriptor::Any; arity],
                result: Box::new(TypeDescriptor::Any),
            }
        }
    }
}

fn evaluate_tool_expression(
    source_name: &str,
    expression: &Expr,
    bindings: &BTreeMap<String, Value>,
    account: &mut QuotaAccount,
    sources: &SourceDatabase,
    debug_sink: &Arc<dyn DebugSink>,
) -> Result<Value, FrontendError> {
    let function = compile_expression_with_bindings(
        source_name,
        "<tool-stage>",
        expression,
        bindings,
        sources.get(expression.location.source),
    )?;
    Vm::new()
        .with_debug_sink(Arc::clone(debug_sink))
        .execute_with_account(&function, &[], account)
        .map_err(|error| {
            frontend_error(
                source_name,
                format!(
                    "tool-stage evaluation failed: {}",
                    error.with_sources(sources)
                ),
            )
        })
}

fn core_prelude(vm: &mut Vm) -> BTreeMap<String, Value> {
    let mut prelude = BTreeMap::new();
    for (name, descriptor) in [
        ("Any", TypeDescriptor::Any),
        ("Int", TypeDescriptor::Int),
        ("Float", TypeDescriptor::Float),
        ("String", TypeDescriptor::String),
        ("Bytes", TypeDescriptor::Bytes),
    ] {
        prelude.insert(name.into(), descriptor.to_value(vm));
    }
    prelude.insert("Bool".into(), normalized_bool_value(vm));
    for function in [
        NativeFunction::core_model(CoreModelFunction::Struct),
        NativeFunction::core_model(CoreModelFunction::Enum),
        NativeFunction::core_model(CoreModelFunction::Union),
        NativeFunction::core_builtin_type(CoreBuiltinTypeFunction::Option),
        NativeFunction::core_builtin_type(CoreBuiltinTypeFunction::Result),
        NativeFunction::new("Atom", 1, native_atom_type),
        NativeFunction::new("Array", 1, native_array_type),
        NativeFunction::new("Tuple", 1, native_tuple_type),
        NativeFunction::new("Fn", 2, native_function_type),
        NativeFunction::new("validate", 2, native_validate),
    ] {
        prelude.insert(
            function.name().into(),
            Value::Func(std::sync::Arc::new(Closure::native(function))),
        );
    }
    prelude
}

fn normalized_bool_value(vm: &mut Vm) -> Value {
    let variants = ["False", "True"]
        .into_iter()
        .map(|name| (name.into(), normalized_legacy_value(vm, Value::none())))
        .collect::<Vec<_>>();
    let variants = vm
        .make_dict(variants)
        .expect("Bool variant names are unique");
    let metadata = vm
        .make_dict(vec![
            ("kind".into(), Value::atom("Enum")),
            ("variants".into(), variants),
        ])
        .expect("Bool metadata fields are unique");
    normalized_legacy_value(vm, metadata)
}

fn normalized_legacy_value(vm: &mut Vm, inner: Value) -> Value {
    let attributes = vm
        .make_dict(Vec::new())
        .expect("empty attributes are unique");
    vm.make_dict(vec![
        ("attributes".into(), attributes),
        ("inner".into(), inner),
        ("kind".into(), Value::atom("WithAttributes")),
    ])
    .expect("WithAttributes fields are unique")
}

fn native_atom_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let argument = context.argument(0)?;
    let Some(atom) = context.value(argument)?.as_atom() else {
        return Err(NativeError::new("Atom expects an Atom value"));
    };
    let _ = atom_from_name(atom);
    write_native_type_record(context, "Atom", &[("tag", argument)])
}

fn native_array_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let item = context.argument(0)?;
    let value = context.value(item)?;
    if !value.is_hidden_up_link() {
        decode_native_type(value)?;
    }
    write_native_type_record(context, "Array", &[("item", item)])
}

fn native_tuple_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let value = context.value(context.argument(0)?)?;
    if value.kind() != ValueKind::Array {
        return Err(NativeError::new("Tuple expects an Array of Types"));
    }
    for index in 0..value.sequence_len().expect("Array has a length") {
        let item = value.sequence_get(index).expect("valid Array index");
        if !item.is_hidden_up_link() {
            decode_native_type(item)?;
        }
    }
    write_native_type_record(context, "Tuple", &[("items", context.argument(0)?)])
}

fn native_function_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let parameters_value = context.value(context.argument(0)?)?;
    if parameters_value.kind() != ValueKind::Array {
        return Err(NativeError::new("Fn expects an Array of parameter Types"));
    }
    for index in 0..parameters_value.sequence_len().expect("Array has a length") {
        let parameter = parameters_value
            .sequence_get(index)
            .expect("valid Array index");
        if !parameter.is_hidden_up_link() {
            decode_native_type(parameter)?;
        }
    }
    let result = context.argument(1)?;
    let result_value = context.value(result)?;
    if !result_value.is_hidden_up_link() {
        decode_native_type(result_value)?;
    }
    write_native_type_record(
        context,
        "Function",
        &[("parameters", context.argument(0)?), ("result", result)],
    )
}

fn write_native_type_record(
    context: &mut CallContext<'_, '_>,
    kind_name: &str,
    preserved_fields: &[(&str, RegisterId)],
) -> Result<(), NativeError> {
    let kind = context.scratch()?;
    context.set_atom(kind, kind_name)?;
    let mut fields = Vec::with_capacity(preserved_fields.len() + 1);
    fields.push(("kind".to_owned(), kind));
    fields.extend(
        preserved_fields
            .iter()
            .map(|(name, register)| ((*name).to_owned(), *register)),
    );
    context.make_dict(context.result(), &fields)
}

fn native_validate(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let type_register = context.argument(0)?;
    let value_register = context.argument(1)?;
    let descriptor = decode_native_type(context.value(type_register)?)?;
    let tag = context.scratch()?;
    let payload = context.scratch()?;
    match validate_value_ref(&descriptor, context.value(value_register)?, "value") {
        Ok(()) => {
            context.set_atom(tag, "Ok")?;
            context.copy(payload, value_register)?;
        }
        Err(message) => {
            context.set_atom(tag, "Err")?;
            context.set_string(payload, message)?;
        }
    }
    context.make_tuple(context.result(), &[tag, payload])
}

fn decode_native_type(value: ValueRef<'_>) -> Result<TypeDescriptor, NativeError> {
    decode_type_ref(value, "Type").map_err(NativeError::new)
}

fn decode_type_ref(value: ValueRef<'_>, path: &str) -> Result<TypeDescriptor, String> {
    let value = value.resolve_hidden_up_link()?;
    let fields = value
        .dict_fields()
        .ok_or_else(|| format!("{path} must be a Dict"))?;
    let kind = value
        .dict_get("kind")
        .and_then(ValueRef::as_atom)
        .ok_or_else(|| format!("{path}.kind must be an Atom"))?;
    if kind == "WithAttributes" {
        if fields != ["attributes", "inner", "kind"] {
            return Err(format!(
                "{path} WithAttributes wrapper must have exactly attributes, inner, and kind fields"
            ));
        }
        let attributes = value
            .dict_get("attributes")
            .expect("validated wrapper field");
        if attributes.kind() != ValueKind::Dict {
            return Err(format!("{path}.attributes must be a Dict"));
        }
        return decode_type_ref(
            value.dict_get("inner").expect("validated wrapper field"),
            path,
        );
    }
    let require = |expected: &[&str]| -> Result<(), String> {
        if fields.iter().copied().eq(expected.iter().copied()) {
            Ok(())
        } else {
            Err(format!("{path} has invalid fields for {kind}"))
        }
    };
    Ok(match kind {
        "Any" => {
            require(&["kind"])?;
            TypeDescriptor::Any
        }
        "Int" => {
            require(&["kind"])?;
            TypeDescriptor::Int
        }
        "Float" => {
            require(&["kind"])?;
            TypeDescriptor::Float
        }
        "String" => {
            require(&["kind"])?;
            TypeDescriptor::String
        }
        "Bytes" => {
            require(&["kind"])?;
            TypeDescriptor::Bytes
        }
        "Atom" => {
            require(&["kind", "tag"])?;
            let tag = value
                .dict_get("tag")
                .and_then(ValueRef::as_atom)
                .ok_or_else(|| format!("{path}.tag must be an Atom"))?;
            TypeDescriptor::Atom(atom_from_name(tag))
        }
        "Array" => {
            require(&["item", "kind"])?;
            let item = value
                .dict_get("item")
                .ok_or_else(|| format!("{path}.item is missing"))?;
            TypeDescriptor::Array(Box::new(decode_type_ref(item, &format!("{path}.item"))?))
        }
        "Tuple" | "Union" => {
            let field = if kind == "Tuple" { "items" } else { "variants" };
            if kind == "Tuple" {
                require(&["items", "kind"])?;
            } else {
                require(&["kind", "variants"])?;
            }
            let sequence = value
                .dict_get(field)
                .ok_or_else(|| format!("{path}.{field} is missing"))?;
            if sequence.kind() != ValueKind::Array {
                return Err(format!("{path}.{field} must be an Array"));
            }
            let values = (0..sequence.sequence_len().expect("Array has a length"))
                .map(|index| {
                    decode_type_ref(
                        sequence.sequence_get(index).expect("valid Array index"),
                        &format!("{path}.{field}[{index}]"),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if kind == "Union" && values.is_empty() {
                return Err(format!("{path}.variants must not be empty"));
            }
            if kind == "Tuple" {
                TypeDescriptor::Tuple(values)
            } else {
                TypeDescriptor::Union(values)
            }
        }
        "Struct" => {
            require(&["fields", "kind"])?;
            let fields_value = value
                .dict_get("fields")
                .ok_or_else(|| format!("{path}.fields is missing"))?;
            let names = fields_value
                .dict_fields()
                .ok_or_else(|| format!("{path}.fields must be a Dict"))?;
            TypeDescriptor::Struct(
                names
                    .iter()
                    .map(|name| {
                        let field = fields_value.dict_get(name).expect("Dict field exists");
                        Ok((
                            (*name).to_owned(),
                            decode_type_ref(field, &format!("{path}.fields.{name}"))?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            )
        }
        "Enum" => {
            require(&["kind", "variants"])?;
            let variants = value
                .dict_get("variants")
                .ok_or_else(|| format!("{path}.variants is missing"))?;
            let names = variants
                .dict_fields()
                .ok_or_else(|| format!("{path}.variants must be a Dict"))?;
            if names.is_empty() {
                return Err(format!("{path}.variants must not be empty"));
            }
            TypeDescriptor::Enum(
                names
                    .iter()
                    .map(|name| {
                        let variant = variants.dict_get(name).expect("Dict field exists");
                        let variant_path = format!("{path}.variants.{name}");
                        let inner = strip_attributes_ref(variant, &variant_path)?;
                        let payload = if inner.as_atom() == Some("None") {
                            None
                        } else {
                            Some(Box::new(decode_type_ref(inner, &variant_path)?))
                        };
                        Ok(((*name).to_owned(), payload))
                    })
                    .collect::<Result<_, String>>()?,
            )
        }
        "Function" => {
            require(&["kind", "parameters", "result"])?;
            let parameters = value
                .dict_get("parameters")
                .ok_or_else(|| format!("{path}.parameters is missing"))?;
            if parameters.kind() != ValueKind::Array {
                return Err(format!("{path}.parameters must be an Array"));
            }
            let parameters = (0..parameters.sequence_len().expect("Array has a length"))
                .map(|index| {
                    decode_type_ref(
                        parameters.sequence_get(index).expect("valid Array index"),
                        &format!("{path}.parameters[{index}]"),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let result = value
                .dict_get("result")
                .ok_or_else(|| format!("{path}.result is missing"))?;
            TypeDescriptor::Function {
                parameters,
                result: Box::new(decode_type_ref(result, &format!("{path}.result"))?),
            }
        }
        _ => return Err(format!("{path}.kind has unknown value '{kind}")),
    })
}

fn strip_attributes_ref<'a>(mut value: ValueRef<'a>, path: &str) -> Result<ValueRef<'a>, String> {
    loop {
        let Some(fields) = value.dict_fields() else {
            return Ok(value);
        };
        if value.dict_get("kind").and_then(ValueRef::as_atom) != Some("WithAttributes") {
            return Ok(value);
        }
        if fields != ["attributes", "inner", "kind"] {
            return Err(format!(
                "{path} WithAttributes wrapper must have exactly attributes, inner, and kind fields"
            ));
        }
        let attributes = value
            .dict_get("attributes")
            .expect("validated wrapper field");
        if attributes.kind() != ValueKind::Dict {
            return Err(format!("{path}.attributes must be a Dict"));
        }
        value = value.dict_get("inner").expect("validated wrapper field");
    }
}

fn validate_value_ref(
    descriptor: &TypeDescriptor,
    value: ValueRef<'_>,
    path: &str,
) -> Result<(), String> {
    match descriptor {
        TypeDescriptor::Any => Ok(()),
        TypeDescriptor::Int if value.kind() == ValueKind::Int => Ok(()),
        TypeDescriptor::Float if value.kind() == ValueKind::Float => Ok(()),
        TypeDescriptor::String if value.kind() == ValueKind::String => Ok(()),
        TypeDescriptor::Bytes if value.kind() == ValueKind::Bytes => Ok(()),
        TypeDescriptor::Atom(expected) if value.as_atom() == Some(expected.name()) => Ok(()),
        TypeDescriptor::Atom(expected) => Err(format!("{path} must be '{}", expected.name())),
        TypeDescriptor::Array(item) => {
            if value.kind() != ValueKind::Array {
                return Err(format!("{path} must be an Array"));
            }
            for index in 0..value.sequence_len().expect("Array has a length") {
                validate_value_ref(
                    item,
                    value.sequence_get(index).expect("valid Array index"),
                    &format!("{path}[{index}]"),
                )?;
            }
            Ok(())
        }
        TypeDescriptor::Tuple(items) => {
            if value.kind() != ValueKind::Tuple {
                return Err(format!("{path} must be a Tuple"));
            }
            if value.sequence_len() != Some(items.len()) {
                return Err(format!("{path} must have {} tuple items", items.len()));
            }
            for (index, item) in items.iter().enumerate() {
                validate_value_ref(
                    item,
                    value.sequence_get(index).expect("valid Tuple index"),
                    &format!("{path}.{index}"),
                )?;
            }
            Ok(())
        }
        TypeDescriptor::Struct(items) => {
            let Some(names) = value.dict_fields() else {
                return Err(format!("{path} must be a Dict"));
            };
            if !items.keys().eq(names.iter()) {
                return Err(format!("{path} has a different field shape"));
            }
            for (name, item) in items {
                validate_value_ref(
                    item,
                    value.dict_get(name).expect("matching shape"),
                    &format!("{path}.{name}"),
                )?;
            }
            Ok(())
        }
        TypeDescriptor::Enum(variants) => {
            if let Some(tag) = value.as_atom() {
                return match variants.get(tag) {
                    Some(None) => Ok(()),
                    Some(Some(_)) => Err(format!("{path} variant '{tag} requires a payload")),
                    None => Err(format!("{path} has unknown Enum variant '{tag}")),
                };
            }
            if value.kind() != ValueKind::Tuple || value.sequence_len() != Some(2) {
                return Err(format!(
                    "{path} must be a unit Atom or a two-element tagged Tuple"
                ));
            }
            let tag = value
                .sequence_get(0)
                .and_then(ValueRef::as_atom)
                .ok_or_else(|| format!("{path}.0 must be an Atom tag"))?;
            match variants.get(tag) {
                Some(Some(payload)) => validate_value_ref(
                    payload,
                    value.sequence_get(1).expect("two-element Tuple"),
                    &format!("{path}.{tag}"),
                ),
                Some(None) => Err(format!("{path} variant '{tag} does not accept a payload")),
                None => Err(format!("{path} has unknown Enum variant '{tag}")),
            }
        }
        TypeDescriptor::Union(variants) => {
            if variants
                .iter()
                .any(|variant| validate_value_ref(variant, value, path).is_ok())
            {
                Ok(())
            } else {
                Err(format!("{path} does not match any Union variant"))
            }
        }
        TypeDescriptor::Function { parameters, .. }
            if value.function_arity() == Some(parameters.len()) =>
        {
            Ok(())
        }
        TypeDescriptor::Function { parameters, .. } if value.kind() == ValueKind::Func => {
            Err(format!("{path} must accept {} arguments", parameters.len()))
        }
        descriptor => Err(format!(
            "{path} must be {}, got {:?}",
            descriptor.display_name(),
            value.kind()
        )),
    }
}

fn kind_entry(kind: &str) -> (String, Value) {
    ("kind".into(), Value::atom(kind))
}

fn decode_type(value: &Value, path: &str) -> Result<TypeDescriptor, String> {
    let Value::Dict(metadata) = value else {
        return Err(format!("{path} must be a Dict"));
    };
    let kind = metadata
        .get("kind")
        .ok_or_else(|| format!("{path}.kind is missing"))?;
    let Value::Atom(kind) = kind else {
        return Err(format!("{path}.kind must be an Atom"));
    };
    if kind.name() == "WithAttributes" {
        require_fields(metadata, path, &["attributes", "inner", "kind"])?;
        if !matches!(metadata.get("attributes"), Some(Value::Dict(_))) {
            return Err(format!("{path}.attributes must be a Dict"));
        }
        return decode_type(metadata.get("inner").expect("required field"), path);
    }
    let descriptor = match kind.name() {
        "Any" => {
            require_fields(metadata, path, &["kind"])?;
            TypeDescriptor::Any
        }
        "Int" => {
            require_fields(metadata, path, &["kind"])?;
            TypeDescriptor::Int
        }
        "Float" => {
            require_fields(metadata, path, &["kind"])?;
            TypeDescriptor::Float
        }
        "String" => {
            require_fields(metadata, path, &["kind"])?;
            TypeDescriptor::String
        }
        "Bytes" => {
            require_fields(metadata, path, &["kind"])?;
            TypeDescriptor::Bytes
        }
        "Atom" => {
            require_fields(metadata, path, &["kind", "tag"])?;
            let Value::Atom(tag) = metadata.get("tag").expect("required field") else {
                return Err(format!("{path}.tag must be an Atom"));
            };
            TypeDescriptor::Atom(tag.clone())
        }
        "Array" => {
            require_fields(metadata, path, &["item", "kind"])?;
            TypeDescriptor::Array(Box::new(decode_type(
                metadata.get("item").expect("required field"),
                &format!("{path}.item"),
            )?))
        }
        "Tuple" => {
            require_fields(metadata, path, &["items", "kind"])?;
            let Value::Array(items) = metadata.get("items").expect("required field") else {
                return Err(format!("{path}.items must be an Array"));
            };
            TypeDescriptor::Tuple(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| decode_type(item, &format!("{path}.items[{index}]")))
                    .collect::<Result<_, _>>()?,
            )
        }
        "Struct" => {
            require_fields(metadata, path, &["fields", "kind"])?;
            let Value::Dict(fields) = metadata.get("fields").expect("required field") else {
                return Err(format!("{path}.fields must be a Dict"));
            };
            let fields = fields
                .shape()
                .fields()
                .iter()
                .zip(fields.values())
                .map(|(name, field)| {
                    Ok((
                        name.clone(),
                        decode_type(field, &format!("{path}.fields.{name}"))?,
                    ))
                })
                .collect::<Result<_, String>>()?;
            TypeDescriptor::Struct(fields)
        }
        "Enum" => {
            require_fields(metadata, path, &["kind", "variants"])?;
            let Value::Dict(variants) = metadata.get("variants").expect("required field") else {
                return Err(format!("{path}.variants must be a Dict"));
            };
            if variants.values().is_empty() {
                return Err(format!("{path}.variants must not be empty"));
            }
            TypeDescriptor::Enum(
                variants
                    .shape()
                    .fields()
                    .iter()
                    .zip(variants.values())
                    .map(|(name, variant)| {
                        let variant_path = format!("{path}.variants.{name}");
                        let inner = strip_attributes_value(variant, &variant_path)?;
                        let payload = if matches!(inner, Value::Atom(atom) if atom.name() == "None")
                        {
                            None
                        } else {
                            Some(Box::new(decode_type(inner, &variant_path)?))
                        };
                        Ok((name.clone(), payload))
                    })
                    .collect::<Result<_, String>>()?,
            )
        }
        "Union" => {
            require_fields(metadata, path, &["kind", "variants"])?;
            let Value::Array(variants) = metadata.get("variants").expect("required field") else {
                return Err(format!("{path}.variants must be an Array"));
            };
            if variants.is_empty() {
                return Err(format!("{path}.variants must not be empty"));
            }
            TypeDescriptor::Union(
                variants
                    .iter()
                    .enumerate()
                    .map(|(index, variant)| {
                        decode_type(variant, &format!("{path}.variants[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
            )
        }
        "Function" => {
            require_fields(metadata, path, &["kind", "parameters", "result"])?;
            let Value::Array(parameters) = metadata.get("parameters").expect("required field")
            else {
                return Err(format!("{path}.parameters must be an Array"));
            };
            TypeDescriptor::Function {
                parameters: parameters
                    .iter()
                    .enumerate()
                    .map(|(index, parameter)| {
                        decode_type(parameter, &format!("{path}.parameters[{index}]"))
                    })
                    .collect::<Result<_, _>>()?,
                result: Box::new(decode_type(
                    metadata.get("result").expect("required field"),
                    &format!("{path}.result"),
                )?),
            }
        }
        other => return Err(format!("{path}.kind has unknown value '{other}")),
    };
    Ok(descriptor)
}

fn strip_attributes_value<'a>(mut value: &'a Value, path: &str) -> Result<&'a Value, String> {
    loop {
        let Value::Dict(metadata) = value else {
            return Ok(value);
        };
        if !matches!(metadata.get("kind"), Some(Value::Atom(kind)) if kind.name() == "WithAttributes")
        {
            return Ok(value);
        }
        require_fields(metadata, path, &["attributes", "inner", "kind"])?;
        if !matches!(metadata.get("attributes"), Some(Value::Dict(_))) {
            return Err(format!("{path}.attributes must be a Dict"));
        }
        value = metadata.get("inner").expect("required field");
    }
}

fn require_fields(metadata: &crate::Dict, path: &str, fields: &[&str]) -> Result<(), String> {
    let actual = metadata
        .shape()
        .fields()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if actual != fields {
        return Err(format!("{path} has fields {actual:?}, expected {fields:?}"));
    }
    Ok(())
}

fn infer_expr(expression: &Expr, environment: &HashMap<String, TypeDescriptor>) -> TypeDescriptor {
    infer_expr_with(expression, environment, &mut |_, _| {})
}

fn infer_expr_recorded(
    expression: &Expr,
    environment: &HashMap<String, TypeDescriptor>,
    facts: &mut HashMap<crate::Location, TypeDescriptor>,
) -> TypeDescriptor {
    infer_expr_with(expression, environment, &mut |location, descriptor| {
        facts.insert(location, descriptor.clone());
    })
}

fn infer_expr_with(
    expression: &Expr,
    environment: &HashMap<String, TypeDescriptor>,
    record: &mut impl FnMut(crate::Location, &TypeDescriptor),
) -> TypeDescriptor {
    let inferred = match &expression.value {
        ExprKind::Int(_) => TypeDescriptor::Int,
        ExprKind::Float(_) => TypeDescriptor::Float,
        ExprKind::String(_) => TypeDescriptor::String,
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let StringPartKind::Expression(expression) = &part.value {
                    infer_expr_with(expression, environment, record);
                }
            }
            TypeDescriptor::String
        }
        ExprKind::Bytes(_) => TypeDescriptor::Bytes,
        ExprKind::Atom(name) => TypeDescriptor::Atom(atom_from_name(name)),
        ExprKind::Variable(name) => environment
            .get(&name.value)
            .cloned()
            .unwrap_or(TypeDescriptor::Any),
        ExprKind::Array(items) => {
            let item_types = items
                .iter()
                .map(|item| infer_expr_with(item, environment, record))
                .collect::<Vec<_>>();
            let item = common_type(item_types).unwrap_or(TypeDescriptor::Any);
            TypeDescriptor::Array(Box::new(item))
        }
        ExprKind::Tuple(items) => TypeDescriptor::Tuple(
            items
                .iter()
                .map(|item| infer_expr_with(item, environment, record))
                .collect(),
        ),
        ExprKind::Dict(fields) => TypeDescriptor::Struct(
            fields
                .iter()
                .map(|field| {
                    (
                        field.value.name.value.clone(),
                        infer_expr_with(&field.value.value, environment, record),
                    )
                })
                .collect(),
        ),
        ExprKind::Block(block) => infer_block_with(block, environment, record),
        ExprKind::Unary { operand, .. } => infer_expr_with(operand, environment, record),
        ExprKind::Binary {
            operator,
            left,
            right,
        } => match operator.value {
            BinaryOperator::LessThan | BinaryOperator::Equal => TypeDescriptor::Union(vec![
                TypeDescriptor::Atom(Atom::builtin(BuiltinAtom::True)),
                TypeDescriptor::Atom(Atom::builtin(BuiltinAtom::False)),
            ]),
            _ => {
                let left = infer_expr_with(left, environment, record);
                let right = infer_expr_with(right, environment, record);
                if left == right {
                    left
                } else {
                    TypeDescriptor::Any
                }
            }
        },
        ExprKind::Field { receiver, field } => {
            match infer_expr_with(receiver, environment, record) {
                TypeDescriptor::Struct(fields) => fields
                    .get(&field.value)
                    .cloned()
                    .unwrap_or(TypeDescriptor::Any),
                _ => TypeDescriptor::Any,
            }
        }
        ExprKind::Call { callee, arguments } => {
            let callee = infer_expr_with(callee, environment, record);
            for argument in arguments {
                infer_expr_with(argument, environment, record);
            }
            match callee {
                TypeDescriptor::Function { result, .. } => *result,
                _ => TypeDescriptor::Any,
            }
        }
        ExprKind::Closure { parameters, body } => {
            let mut closure_environment = environment.clone();
            for parameter in parameters {
                closure_environment.insert(parameter.value.clone(), TypeDescriptor::Any);
            }
            TypeDescriptor::Function {
                parameters: vec![TypeDescriptor::Any; parameters.len()],
                result: Box::new(infer_block_with(body, &closure_environment, record)),
            }
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            infer_expr_with(condition, environment, record);
            union_or_single(vec![
                infer_block_with(then_branch, environment, record),
                infer_block_with(else_branch, environment, record),
            ])
        }
        ExprKind::Match { value, arms } => {
            infer_expr_with(value, environment, record);
            union_or_single(
                arms.iter()
                    .map(|arm| {
                        let mut arm_environment = environment.clone();
                        bind_pattern_types(&arm.value.pattern, &mut arm_environment);
                        infer_expr_with(&arm.value.value, &arm_environment, record)
                    })
                    .collect(),
            )
        }
    };
    record(expression.location, &inferred);
    inferred
}

fn check_interpolations(
    expression: &Expr,
    environment: &HashMap<String, TypeDescriptor>,
    sources: &SourceDatabase,
) -> Result<(), FrontendError> {
    match &expression.value {
        ExprKind::InterpolatedString(parts) => {
            for part in parts {
                if let StringPartKind::Expression(part_expression) = &part.value {
                    let inferred = infer_expr(part_expression, environment);
                    if !interpolation_type_supported(&inferred) {
                        let message = format!(
                            "string interpolation does not support {}",
                            inferred.display_name()
                        );
                        return Err(FrontendError::from_diagnostic(
                            sources,
                            Diagnostic::error(message, part_expression.location),
                        ));
                    }
                    check_interpolations(part_expression, environment, sources)?;
                }
            }
        }
        ExprKind::Array(items) | ExprKind::Tuple(items) => {
            for item in items {
                check_interpolations(item, environment, sources)?;
            }
        }
        ExprKind::Dict(fields) => {
            for field in fields {
                check_interpolations(&field.value.value, environment, sources)?;
            }
        }
        ExprKind::Block(block) => check_block_interpolations(block, environment, sources)?,
        ExprKind::Unary { operand, .. } => {
            check_interpolations(operand, environment, sources)?;
        }
        ExprKind::Binary { left, right, .. } => {
            check_interpolations(left, environment, sources)?;
            check_interpolations(right, environment, sources)?;
        }
        ExprKind::Field { receiver, .. } => {
            check_interpolations(receiver, environment, sources)?;
        }
        ExprKind::Call { callee, arguments } => {
            check_interpolations(callee, environment, sources)?;
            for argument in arguments {
                check_interpolations(argument, environment, sources)?;
            }
        }
        ExprKind::Closure { parameters, body } => {
            let mut closure_environment = environment.clone();
            for parameter in parameters {
                closure_environment.insert(parameter.value.clone(), TypeDescriptor::Any);
            }
            check_block_interpolations(body, &closure_environment, sources)?;
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            check_interpolations(condition, environment, sources)?;
            check_block_interpolations(then_branch, environment, sources)?;
            check_block_interpolations(else_branch, environment, sources)?;
        }
        ExprKind::Match { value, arms } => {
            check_interpolations(value, environment, sources)?;
            for arm in arms {
                let mut arm_environment = environment.clone();
                bind_pattern_types(&arm.value.pattern, &mut arm_environment);
                check_interpolations(&arm.value.value, &arm_environment, sources)?;
            }
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::String(_)
        | ExprKind::Bytes(_)
        | ExprKind::Atom(_)
        | ExprKind::Variable(_) => {}
    }
    Ok(())
}

fn check_block_interpolations(
    block: &Block,
    environment: &HashMap<String, TypeDescriptor>,
    sources: &SourceDatabase,
) -> Result<(), FrontendError> {
    let mut environment = environment.clone();
    for binding in &block.value.bindings {
        if matches!(
            binding.value.kind,
            BindingKind::Decl | BindingKind::Native | BindingKind::NamedFunction
        ) {
            environment.insert(binding.value.name.value.clone(), TypeDescriptor::Any);
        }
    }
    for binding in &block.value.bindings {
        check_interpolations(&binding.value.value, &environment, sources)?;
        if let Some(annotation) = &binding.value.annotation {
            check_interpolations(annotation, &environment, sources)?;
        }
        if matches!(
            binding.value.kind,
            BindingKind::Let | BindingKind::Def | BindingKind::NamedFunction | BindingKind::Import
        ) {
            let inferred = infer_expr(&binding.value.value, &environment);
            environment.insert(binding.value.name.value.clone(), inferred);
        }
    }
    check_interpolations(&block.value.result, &environment, sources)
}

fn interpolation_type_supported(descriptor: &TypeDescriptor) -> bool {
    match descriptor {
        TypeDescriptor::Any
        | TypeDescriptor::Int
        | TypeDescriptor::String
        | TypeDescriptor::Atom(_) => true,
        TypeDescriptor::Union(variants) => variants.iter().all(interpolation_type_supported),
        TypeDescriptor::Enum(variants) => variants.iter().all(|(name, payload)| {
            interpolation_type_supported(&enum_variant_type(name, payload.as_deref()))
        }),
        TypeDescriptor::Float
        | TypeDescriptor::Bytes
        | TypeDescriptor::Array(_)
        | TypeDescriptor::Tuple(_)
        | TypeDescriptor::Struct(_)
        | TypeDescriptor::Function { .. } => false,
    }
}

fn infer_block_with(
    block: &Block,
    environment: &HashMap<String, TypeDescriptor>,
    record: &mut impl FnMut(crate::Location, &TypeDescriptor),
) -> TypeDescriptor {
    let mut environment = environment.clone();
    for binding in &block.value.bindings {
        if matches!(
            binding.value.kind,
            BindingKind::Decl | BindingKind::Native | BindingKind::NamedFunction
        ) {
            environment.insert(binding.value.name.value.clone(), TypeDescriptor::Any);
        }
    }
    for binding in &block.value.bindings {
        if let Some(annotation) = &binding.value.annotation {
            infer_expr_with(annotation, &environment, record);
        }
        let inferred = infer_expr_with(&binding.value.value, &environment, record);
        if matches!(
            binding.value.kind,
            BindingKind::Let | BindingKind::Def | BindingKind::NamedFunction | BindingKind::Import
        ) {
            environment.insert(binding.value.name.value.clone(), inferred);
        }
    }
    infer_expr_with(&block.value.result, &environment, record)
}

fn bind_pattern_types(pattern: &Pattern, environment: &mut HashMap<String, TypeDescriptor>) {
    match &pattern.value {
        PatternKind::Binding(name) => {
            environment.insert(name.value.clone(), TypeDescriptor::Any);
        }
        PatternKind::Tuple(items) => {
            for item in items {
                bind_pattern_types(item, environment);
            }
        }
        _ => {}
    }
}

fn common_type(types: Vec<TypeDescriptor>) -> Option<TypeDescriptor> {
    let first = types.first()?.clone();
    types.iter().all(|item| item == &first).then_some(first)
}

fn union_or_single(mut types: Vec<TypeDescriptor>) -> TypeDescriptor {
    let mut unique = Vec::new();
    for item in types.drain(..) {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    types = unique;
    if types.len() == 1 {
        types.pop().expect("one type")
    } else {
        TypeDescriptor::Union(types)
    }
}

fn assignable(actual: &TypeDescriptor, expected: &TypeDescriptor) -> bool {
    match (actual, expected) {
        (TypeDescriptor::Any, _) | (_, TypeDescriptor::Any) => true,
        (TypeDescriptor::Enum(actual), TypeDescriptor::Enum(expected)) => {
            actual.len() == expected.len()
                && expected.iter().all(|(name, expected)| {
                    actual
                        .get(name)
                        .is_some_and(|actual| match (actual, expected) {
                            (None, None) => true,
                            (Some(actual), Some(expected)) => assignable(actual, expected),
                            _ => false,
                        })
                })
        }
        (TypeDescriptor::Union(variants), expected @ TypeDescriptor::Enum(_)) => {
            variants.iter().all(|variant| assignable(variant, expected))
        }
        (actual, TypeDescriptor::Enum(variants)) => variants.iter().any(|(name, payload)| {
            assignable(actual, &enum_variant_type(name, payload.as_deref()))
        }),
        (TypeDescriptor::Enum(variants), expected) => variants.iter().all(|(name, payload)| {
            assignable(&enum_variant_type(name, payload.as_deref()), expected)
        }),
        (actual, TypeDescriptor::Union(variants)) => {
            variants.iter().any(|variant| assignable(actual, variant))
        }
        (TypeDescriptor::Union(variants), expected) => {
            variants.iter().all(|variant| assignable(variant, expected))
        }
        (TypeDescriptor::Array(actual), TypeDescriptor::Array(expected)) => {
            assignable(actual, expected)
        }
        (TypeDescriptor::Tuple(actual), TypeDescriptor::Tuple(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| assignable(actual, expected))
        }
        (TypeDescriptor::Struct(actual), TypeDescriptor::Struct(expected)) => {
            actual.len() == expected.len()
                && expected.iter().all(|(name, expected)| {
                    actual
                        .get(name)
                        .is_some_and(|actual| assignable(actual, expected))
                })
        }
        (
            TypeDescriptor::Function {
                parameters: actual_parameters,
                result: actual_result,
            },
            TypeDescriptor::Function {
                parameters: expected_parameters,
                result: expected_result,
            },
        ) => {
            actual_parameters.len() == expected_parameters.len()
                && actual_parameters
                    .iter()
                    .zip(expected_parameters)
                    .all(|(actual, expected)| assignable(actual, expected))
                && assignable(actual_result, expected_result)
        }
        _ => actual == expected,
    }
}

fn enum_variant_type(name: &str, payload: Option<&TypeDescriptor>) -> TypeDescriptor {
    let tag = TypeDescriptor::Atom(atom_from_name(name));
    payload.map_or(tag.clone(), |payload| {
        TypeDescriptor::Tuple(vec![tag, payload.clone()])
    })
}

fn incompatibility_path(actual: &TypeDescriptor, expected: &TypeDescriptor) -> Option<ValuePath> {
    fn visit(actual: &TypeDescriptor, expected: &TypeDescriptor, path: &mut ValuePath) -> bool {
        match (actual, expected) {
            (TypeDescriptor::Any, _) | (_, TypeDescriptor::Any) => false,
            (TypeDescriptor::Struct(actual), TypeDescriptor::Struct(expected)) => {
                for (name, expected) in expected {
                    path.push(ValuePathSegment::Key(name.clone()));
                    let mismatch = actual
                        .get(name)
                        .is_none_or(|actual| visit(actual, expected, path));
                    if mismatch {
                        return true;
                    }
                    path.pop();
                }
                if let Some(name) = actual.keys().find(|name| !expected.contains_key(*name)) {
                    path.push(ValuePathSegment::Key(name.clone()));
                    return true;
                }
                false
            }
            (TypeDescriptor::Enum(actual), TypeDescriptor::Enum(expected)) => {
                for (name, expected) in expected {
                    path.push(ValuePathSegment::Key(name.clone()));
                    let mismatch = match (actual.get(name), expected) {
                        (Some(None), None) => false,
                        (Some(Some(actual)), Some(expected)) => visit(actual, expected, path),
                        _ => true,
                    };
                    if mismatch {
                        return true;
                    }
                    path.pop();
                }
                if let Some(name) = actual.keys().find(|name| !expected.contains_key(*name)) {
                    path.push(ValuePathSegment::Key(name.clone()));
                    return true;
                }
                false
            }
            (TypeDescriptor::Tuple(actual), TypeDescriptor::Tuple(expected)) => {
                if actual.len() != expected.len() {
                    return true;
                }
                for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                    path.push(ValuePathSegment::Index(index));
                    if visit(actual, expected, path) {
                        return true;
                    }
                    path.pop();
                }
                false
            }
            (TypeDescriptor::Array(actual), TypeDescriptor::Array(expected)) => {
                visit(actual, expected, path)
            }
            _ => !assignable(actual, expected),
        }
    }
    let mut path = Vec::new();
    visit(actual, expected, &mut path).then_some(path)
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

fn frontend_error(source_name: &str, message: impl Into<String>) -> FrontendError {
    FrontendError::new(
        source_name,
        SourceLocation {
            offset: 0,
            line: 1,
            column: 1,
        },
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trips() {
        let descriptor = TypeDescriptor::Function {
            parameters: vec![TypeDescriptor::Struct(BTreeMap::from([
                ("age".into(), TypeDescriptor::Int),
                ("name".into(), TypeDescriptor::String),
            ]))],
            result: Box::new(TypeDescriptor::Enum(BTreeMap::from([
                ("None".into(), None),
                ("Some".into(), Some(Box::new(TypeDescriptor::String))),
            ]))),
        };
        let value = descriptor.to_value(&mut Vm::new());
        assert_eq!(TypeDescriptor::from_value(&value).unwrap(), descriptor);
    }

    #[test]
    fn ordinary_closure_computes_type_metadata() {
        let analysis = analyze_source(
            "test",
            "fn Optional(item) { union('None, [Atom('None), Tuple([Atom('Some), item])]) }\
             type MaybeInt = Optional(Int);\
             let value: MaybeInt = ('Some, 42);\
             value",
        )
        .unwrap();
        let maybe = analysis.declared_types.get("MaybeInt").unwrap();
        assert!(
            matches!(analysis.types.node(*maybe), TypeNode::Union(variants) if variants.len() == 2)
        );
    }

    #[test]
    fn reports_structural_annotation_mismatch() {
        let error = analyze_source(
            "test",
            "@struct type User = {name: String, age: Int};\
             let user: User = {name: \"Ada\", age: \"old\"};\
             user",
        )
        .unwrap_err();
        assert!(error.message.contains("not assignable"));
    }

    #[test]
    fn checks_interpolation_inside_nested_binding_annotations() {
        let error = analyze_source("test", r#"let outer = { let x: "\{[1]}" = "x"; x }; outer"#)
            .unwrap_err();
        assert!(error.message.contains("does not support Array<Int>"));
    }

    #[test]
    fn records_a_type_fact_for_every_resolved_hir_expression() {
        let analysis = analyze_source(
            "facts.xl",
            "let values = [1, 2]; let first = fn(x) { let y = x; y }; first(values)",
        )
        .unwrap();
        assert_eq!(
            analysis.expression_types.len(),
            analysis.hir.expressions().len()
        );
        assert!(
            analysis
                .expression_types
                .values()
                .any(|ty| matches!(analysis.types.node(*ty), TypeNode::Int))
        );
        assert!(
            analysis
                .expression_types
                .values()
                .any(|ty| matches!(analysis.types.node(*ty), TypeNode::Array(_)))
        );
        assert!(
            analysis
                .expression_types
                .values()
                .any(|ty| matches!(analysis.types.node(*ty), TypeNode::Function { .. }))
        );
    }

    #[test]
    fn partial_type_evaluation_continues_independent_and_transitive_work() {
        let partial = analyze_partial_types(
            "partial.xl",
            "type A = broken(Int);\
             type B = String;\
             type C = Array(B);\
             type D = Array(A);\
             0",
            Quota::with_fuel(100),
        );
        let definition = |name: &str| {
            partial
                .hir
                .definitions()
                .iter()
                .find(|definition| definition.name == name)
                .unwrap()
                .id
        };
        let a = definition("A");
        let b = definition("B");
        let c = definition("C");
        let d = definition("D");
        assert!(matches!(
            partial.definition_facts[&a].state,
            FactState::Incomputable(IncomputableReason::UnsupportedOperation)
        ));
        assert_eq!(partial.definition_facts[&b].state, FactState::Known);
        assert_eq!(partial.definition_facts[&c].state, FactState::Known);
        assert_eq!(
            partial
                .types
                .display(partial.definition_facts[&c].value.unwrap()),
            "Array<String>"
        );
        assert_eq!(
            partial.definition_facts[&d].state,
            FactState::Unknown(UnknownReason::BlockedBy(FactIdentity::HirDefinition(a)))
        );
        assert!(partial.definition_facts[&d].diagnostics.is_empty());
        assert_eq!(partial.diagnostics.len(), 1);
        let c_node = partial
            .dependencies
            .nodes
            .iter()
            .find(|node| node.definition == c)
            .unwrap();
        assert_eq!(c_node.dependencies, vec![b]);
    }

    #[test]
    fn partial_type_evaluation_shares_one_fuel_account() {
        let partial = analyze_partial_types(
            "fuel.xl",
            "type A = Array(Int); type B = Array(Int); 0",
            Quota::with_fuel(1),
        );
        let facts = partial
            .hir
            .definitions()
            .iter()
            .filter(|definition| definition.kind == HirDefinitionKind::Type)
            .map(|definition| &partial.definition_facts[&definition.id])
            .collect::<Vec<_>>();
        assert_eq!(facts[0].state, FactState::Known);
        assert_eq!(
            facts[1].state,
            FactState::Incomputable(IncomputableReason::QuotaExceeded)
        );
    }

    #[test]
    fn partial_type_evaluation_marks_recursive_components_explicitly() {
        let partial = analyze_partial_types(
            "recursive.xl",
            "@struct type Node = {children: Array(Node)}; 0",
            Quota::with_fuel(100),
        );
        let node = partial
            .hir
            .definitions()
            .iter()
            .find(|definition| definition.name == "Node")
            .unwrap();
        assert_eq!(
            partial.definition_facts[&node.id].state,
            FactState::Incomputable(IncomputableReason::CyclicEvaluation)
        );
        assert_eq!(partial.dependencies.nodes[0].dependencies, vec![node.id]);
    }

    #[test]
    fn partial_type_evaluation_accepts_explicit_linked_capabilities() {
        let mut vm = Vm::new();
        let bindings = BTreeMap::from([(
            "LinkedType".to_owned(),
            TypeDescriptor::Int.to_value(&mut vm),
        )]);
        let partial = analyze_partial_types_with_bindings(
            "linked.xl",
            "type Linked = LinkedType; 0",
            Quota::with_fuel(10),
            &bindings,
        );
        let linked = partial
            .hir
            .definitions()
            .iter()
            .find(|definition| definition.name == "Linked")
            .unwrap();
        let fact = &partial.definition_facts[&linked.id];
        assert_eq!(fact.state, FactState::Known);
        assert_eq!(partial.types.node(fact.value.unwrap()), &TypeNode::Int);
        assert!(partial.hir.references().iter().any(|reference| {
            reference.name == "LinkedType"
                && reference.resolution == crate::hir::HirResolution::External
        }));
    }

    #[test]
    fn tool_stage_respects_evaluation_fuel() {
        let error = analyze_source_with_fuel("test", "type Number = Array(Int); 0", 0).unwrap_err();
        assert!(error.message.contains("fuel"));
    }

    #[test]
    fn tool_expressions_share_one_module_account() {
        let error = analyze_source_with_quota(
            "test",
            "type First = Array(Int); type Second = Array(Int); 0",
            Quota::new(1, 1_000, u64::MAX),
        )
        .unwrap_err();
        assert!(error.message.contains("fuel"));
    }

    #[test]
    fn type_decorators_share_tool_fuel_and_report_the_decorator_origin() {
        let source = "let same = fn(ctx, rhs) { rhs }; @same type T = Int; 0";
        let exhausted = analyze_source_with_fuel("decorator.xl", source, 0).unwrap_err();
        assert!(exhausted.message.contains("fuel"));

        let invalid = analyze_source(
            "decorator.xl",
            "let invalid = fn(ctx, rhs) { 1 }; @invalid type T = Int; 0",
        )
        .unwrap_err();
        assert!(invalid.message.contains("invalid metadata"));
        let diagnostic = invalid.diagnostic.expect("located decorator diagnostic");
        assert_eq!(diagnostic.labels[0].location.range(), 34..42);
    }

    #[test]
    fn rejects_invalid_metadata_protocol() {
        let error = analyze_source("test", "type Broken = {kind: 'Unknown}; 0").unwrap_err();
        assert!(error.message.contains("unknown value"));

        let malformed = analyze_source(
            "test",
            "type Broken = {kind: 'WithAttributes, inner: Int, attributes: []}; 0",
        )
        .unwrap_err();
        assert!(malformed.message.contains("attributes must be a Dict"));
    }

    #[test]
    fn runtime_validation_uses_computed_metadata() {
        let accepted = crate::run_source(
            "test",
            "@struct type User = {name: String, age: Int};\
             validate(User, {age: 36, name: \"Ada\"})",
            100_000,
        )
        .unwrap();
        assert!(matches!(
            accepted,
            Value::Tuple(values)
                if matches!(&values[0], Value::Atom(atom) if atom.name() == "Ok")
        ));

        let rejected = crate::run_source(
            "test",
            "@struct type User = {name: String, age: Int};\
             validate(User, {age: \"old\", name: \"Ada\"})",
            100_000,
        )
        .unwrap();
        assert!(matches!(
            rejected,
            Value::Tuple(values)
                if matches!(&values[0], Value::Atom(atom) if atom.name() == "Err")
        ));
    }

    #[test]
    fn program_bytecode_erases_or_retains_type_metadata_by_use() {
        let erased = crate::compile_source(
            "test",
            "@struct type User = {name: String}; let user: User = {name: \"Ada\"}; user.name",
        )
        .unwrap();
        assert!(!erased.constants().iter().any(is_type_metadata));

        let retained =
            crate::compile_source("test", "@struct type User = {name: String}; User").unwrap();
        assert!(retained.constants().iter().any(is_type_metadata));
    }

    fn is_type_metadata(value: &Value) -> bool {
        TypeDescriptor::from_value(value).is_ok()
    }
}
