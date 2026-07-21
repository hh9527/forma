use crate::ast::{BinaryOperator, BindingKind, Block, Expr, Pattern, Program, UnaryOperator};
use crate::compiler::compile_expression_with_bindings;
use crate::json::{Provenance, ValuePath, ValuePathSegment};
use crate::lexer::{FrontendError, SourceLocation};
use crate::parser::parse_registered;
use crate::source::{Diagnostic, SourceDatabase};
use crate::value::{Atom, Callable, NativeError, NativeFunction, Value};
use crate::{BuiltinAtom, Vm};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

const DEFAULT_TOOL_BUDGET: usize = 100_000;

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
    Union(Vec<TypeDescriptor>),
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
            Self::Union(variants) => variants
                .iter()
                .map(Self::display_name)
                .collect::<Vec<_>>()
                .join(" | "),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub declared_types: BTreeMap<String, TypeDescriptor>,
    pub binding_types: BTreeMap<String, TypeDescriptor>,
    pub result_type: TypeDescriptor,
    pub(crate) resolved_types: HashMap<String, Value>,
    pub(crate) prelude: BTreeMap<String, Value>,
    pub(crate) external_values: BTreeMap<String, Value>,
    pub(crate) dynamic_bindings: HashSet<String>,
}

pub fn analyze_source(source_name: &str, source: &str) -> Result<Analysis, FrontendError> {
    analyze_source_with_budget(source_name, source, DEFAULT_TOOL_BUDGET)
}

pub fn analyze_source_with_budget(
    source_name: &str,
    source: &str,
    instruction_budget: usize,
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
    analyze_program_with_bindings(
        source_name,
        &program,
        instruction_budget,
        &BTreeMap::new(),
        &HashSet::new(),
        Some(&sources),
        &BTreeMap::new(),
    )
}

pub(crate) fn analyze_program_registered(
    source_name: &str,
    sources: &SourceDatabase,
    program: &Program,
    instruction_budget: usize,
) -> Result<Analysis, FrontendError> {
    analyze_program_with_bindings(
        source_name,
        program,
        instruction_budget,
        &BTreeMap::new(),
        &HashSet::new(),
        Some(sources),
        &BTreeMap::new(),
    )
}

pub(crate) fn analyze_program_with_bindings(
    source_name: &str,
    program: &Program,
    instruction_budget: usize,
    external_values: &BTreeMap<String, Value>,
    dynamic_bindings: &HashSet<String>,
    sources: Option<&SourceDatabase>,
    external_provenance: &BTreeMap<String, Provenance>,
) -> Result<Analysis, FrontendError> {
    let mut tool_vm = Vm::new();
    let prelude = core_prelude(&mut tool_vm);
    let mut tool_values = prelude.clone();
    let mut static_environment = HashMap::new();
    let mut declared_types = BTreeMap::new();
    let mut binding_types = BTreeMap::new();
    let mut resolved_types = HashMap::new();
    let mut declared_type_spans = HashMap::new();

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

    for binding in &program.body.bindings {
        match binding.kind {
            BindingKind::Type => {
                let value = evaluate_tool_expression(
                    source_name,
                    &binding.value,
                    &tool_values,
                    instruction_budget,
                )?;
                let descriptor = TypeDescriptor::from_value(&value).map_err(|message| {
                    frontend_error(
                        source_name,
                        format!("type {} produced invalid metadata: {message}", binding.name),
                    )
                })?;
                declared_types.insert(binding.name.clone(), descriptor);
                declared_type_spans.insert(binding.name.clone(), binding.span.clone());
                resolved_types.insert(binding.name.clone(), value.clone());
                tool_values.insert(binding.name.clone(), value);
            }
            BindingKind::Let => {
                let inferred = infer_expr(&binding.value, &static_environment);
                let checked = if let Some(annotation) = &binding.annotation {
                    let metadata = evaluate_tool_expression(
                        source_name,
                        annotation,
                        &tool_values,
                        instruction_budget,
                    )?;
                    let expected = TypeDescriptor::from_value(&metadata).map_err(|message| {
                        frontend_error(
                            source_name,
                            format!("annotation on {} is invalid: {message}", binding.name),
                        )
                    })?;
                    if !assignable(&inferred, &expected) {
                        let message = format!(
                            "binding {} has type {}, which is not assignable to {}",
                            binding.name,
                            inferred.display_name(),
                            expected.display_name()
                        );
                        if let Some(sources) = sources {
                            let path =
                                incompatibility_path(&inferred, &expected).unwrap_or_default();
                            let data_span = match binding.value.unspanned() {
                                Expr::Variable(name) => external_provenance
                                    .get(name)
                                    .and_then(|provenance| {
                                        provenance
                                            .values
                                            .get(&path)
                                            .or_else(|| provenance.values.get(&Vec::new()))
                                    })
                                    .cloned(),
                                _ => binding.value.span().cloned(),
                            }
                            .unwrap_or_else(|| binding.span.clone());
                            let rule_span = match annotation.unspanned() {
                                Expr::Variable(name) => declared_type_spans.get(name).cloned(),
                                _ => annotation.span().cloned(),
                            }
                            .unwrap_or_else(|| binding.span.clone());
                            let diagnostic = Diagnostic::error(message, data_span)
                                .with_secondary("type requirement declared here", rule_span);
                            return Err(FrontendError::from_diagnostic(sources, diagnostic));
                        }
                        return Err(frontend_error(source_name, message));
                    }
                    expected
                } else {
                    inferred
                };
                static_environment.insert(binding.name.clone(), checked.clone());
                binding_types.insert(binding.name.clone(), checked);

                if let Ok(value) = evaluate_tool_expression(
                    source_name,
                    &binding.value,
                    &tool_values,
                    instruction_budget,
                ) {
                    tool_values.insert(binding.name.clone(), value);
                }
            }
            BindingKind::Import => {
                let value = external_values.get(&binding.name).cloned().ok_or_else(|| {
                    frontend_error(
                        source_name,
                        format!("import {} has not been resolved", binding.name),
                    )
                })?;
                let inferred = infer_value(&value);
                static_environment.insert(binding.name.clone(), inferred.clone());
                binding_types.insert(binding.name.clone(), inferred);
                tool_values.insert(binding.name.clone(), value);
            }
        }
    }

    let result_type = infer_expr(&program.body.result, &static_environment);
    Ok(Analysis {
        declared_types,
        binding_types,
        result_type,
        resolved_types,
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
        Value::Func(_) => TypeDescriptor::Any,
    }
}

fn evaluate_tool_expression(
    source_name: &str,
    expression: &Expr,
    bindings: &BTreeMap<String, Value>,
    instruction_budget: usize,
) -> Result<Value, FrontendError> {
    let function =
        compile_expression_with_bindings(source_name, "<tool-stage>", expression, bindings)?;
    Vm::new()
        .execute(&function, instruction_budget)
        .map_err(|error| {
            frontend_error(
                source_name,
                format!("tool-stage evaluation failed: {error}"),
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
    for function in [
        NativeFunction::new("Atom", 1, native_atom_type),
        NativeFunction::new("Array", 1, native_array_type),
        NativeFunction::new("Tuple", 1, native_tuple_type),
        NativeFunction::new("Struct", 1, native_struct_type),
        NativeFunction::new("Union", 1, native_union_type),
        NativeFunction::new("validate", 2, native_validate),
    ] {
        prelude.insert(
            function.name().into(),
            Value::Func(Arc::new(Callable::Native(function))),
        );
    }
    prelude
}

fn native_atom_type(vm: &mut Vm, arguments: &[Value]) -> Result<Value, NativeError> {
    let Value::Atom(atom) = &arguments[0] else {
        return Err(NativeError::new("Atom expects an Atom value"));
    };
    Ok(TypeDescriptor::Atom(atom.clone()).to_value(vm))
}

fn native_array_type(vm: &mut Vm, arguments: &[Value]) -> Result<Value, NativeError> {
    let item = decode_native_type(&arguments[0])?;
    Ok(TypeDescriptor::Array(Box::new(item)).to_value(vm))
}

fn native_tuple_type(vm: &mut Vm, arguments: &[Value]) -> Result<Value, NativeError> {
    let Value::Array(items) = &arguments[0] else {
        return Err(NativeError::new("Tuple expects an Array of Types"));
    };
    let items = items
        .iter()
        .map(decode_native_type)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TypeDescriptor::Tuple(items).to_value(vm))
}

fn native_struct_type(vm: &mut Vm, arguments: &[Value]) -> Result<Value, NativeError> {
    let Value::Dict(fields) = &arguments[0] else {
        return Err(NativeError::new("Struct expects a Dict of Types"));
    };
    let fields = fields
        .shape()
        .fields()
        .iter()
        .zip(fields.values())
        .map(|(name, value)| Ok((name.clone(), decode_native_type(value)?)))
        .collect::<Result<BTreeMap<_, _>, NativeError>>()?;
    Ok(TypeDescriptor::Struct(fields).to_value(vm))
}

fn native_union_type(vm: &mut Vm, arguments: &[Value]) -> Result<Value, NativeError> {
    let Value::Array(variants) = &arguments[0] else {
        return Err(NativeError::new("Union expects an Array of Types"));
    };
    if variants.is_empty() {
        return Err(NativeError::new("Union requires at least one variant"));
    }
    let variants = variants
        .iter()
        .map(decode_native_type)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TypeDescriptor::Union(variants).to_value(vm))
}

fn native_validate(_vm: &mut Vm, arguments: &[Value]) -> Result<Value, NativeError> {
    let descriptor = decode_native_type(&arguments[0])?;
    match validate_value(&descriptor, &arguments[1], "value") {
        Ok(()) => Ok(Value::Tuple(
            vec![
                Value::Atom(Atom::builtin(BuiltinAtom::Ok)),
                arguments[1].clone(),
            ]
            .into(),
        )),
        Err(message) => Ok(Value::Tuple(
            vec![
                Value::Atom(Atom::builtin(BuiltinAtom::Err)),
                Value::string(message),
            ]
            .into(),
        )),
    }
}

fn decode_native_type(value: &Value) -> Result<TypeDescriptor, NativeError> {
    TypeDescriptor::from_value(value).map_err(NativeError::new)
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
        other => return Err(format!("{path}.kind has unknown value '{other}")),
    };
    Ok(descriptor)
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
    match expression {
        Expr::Spanned { expression, .. } => infer_expr(expression, environment),
        Expr::Int(_) => TypeDescriptor::Int,
        Expr::Float(_) => TypeDescriptor::Float,
        Expr::String(_) => TypeDescriptor::String,
        Expr::Bytes(_) => TypeDescriptor::Bytes,
        Expr::Atom(name) => TypeDescriptor::Atom(atom_from_name(name)),
        Expr::Variable(name) => environment
            .get(name)
            .cloned()
            .unwrap_or(TypeDescriptor::Any),
        Expr::Array(items) => {
            let item_types = items
                .iter()
                .map(|item| infer_expr(item, environment))
                .collect::<Vec<_>>();
            let item = common_type(item_types).unwrap_or(TypeDescriptor::Any);
            TypeDescriptor::Array(Box::new(item))
        }
        Expr::Tuple(items) => TypeDescriptor::Tuple(
            items
                .iter()
                .map(|item| infer_expr(item, environment))
                .collect(),
        ),
        Expr::Dict(fields) => TypeDescriptor::Struct(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), infer_expr(value, environment)))
                .collect(),
        ),
        Expr::Block(block) => infer_block(block, environment),
        Expr::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => infer_expr(operand, environment),
        Expr::Binary {
            operator,
            left,
            right,
        } => match operator {
            BinaryOperator::LessThan | BinaryOperator::Equal => TypeDescriptor::Union(vec![
                TypeDescriptor::Atom(Atom::builtin(BuiltinAtom::True)),
                TypeDescriptor::Atom(Atom::builtin(BuiltinAtom::False)),
            ]),
            _ => {
                let left = infer_expr(left, environment);
                let right = infer_expr(right, environment);
                if left == right {
                    left
                } else {
                    TypeDescriptor::Any
                }
            }
        },
        Expr::Field { receiver, field } => match infer_expr(receiver, environment) {
            TypeDescriptor::Struct(fields) => {
                fields.get(field).cloned().unwrap_or(TypeDescriptor::Any)
            }
            _ => TypeDescriptor::Any,
        },
        Expr::Call { .. } | Expr::Closure { .. } => TypeDescriptor::Any,
        Expr::If {
            then_branch,
            else_branch,
            ..
        } => union_or_single(vec![
            infer_block(then_branch, environment),
            infer_block(else_branch, environment),
        ]),
        Expr::Match { arms, .. } => union_or_single(
            arms.iter()
                .map(|arm| {
                    let mut arm_environment = environment.clone();
                    bind_pattern_types(&arm.pattern, &mut arm_environment);
                    infer_expr(&arm.value, &arm_environment)
                })
                .collect(),
        ),
    }
}

fn infer_block(block: &Block, environment: &HashMap<String, TypeDescriptor>) -> TypeDescriptor {
    let mut environment = environment.clone();
    for binding in &block.bindings {
        if matches!(binding.kind, BindingKind::Let | BindingKind::Import) {
            let inferred = infer_expr(&binding.value, &environment);
            environment.insert(binding.name.clone(), inferred);
        }
    }
    infer_expr(&block.result, &environment)
}

fn bind_pattern_types(pattern: &Pattern, environment: &mut HashMap<String, TypeDescriptor>) {
    match pattern {
        Pattern::Binding(name) => {
            environment.insert(name.clone(), TypeDescriptor::Any);
        }
        Pattern::Tuple(items) => {
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
        _ => actual == expected,
    }
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

fn validate_value(descriptor: &TypeDescriptor, value: &Value, path: &str) -> Result<(), String> {
    match descriptor {
        TypeDescriptor::Any => Ok(()),
        TypeDescriptor::Int if matches!(value, Value::Int(_)) => Ok(()),
        TypeDescriptor::Float if matches!(value, Value::Float(_)) => Ok(()),
        TypeDescriptor::String if matches!(value, Value::String(_)) => Ok(()),
        TypeDescriptor::Bytes if matches!(value, Value::Bytes(_)) => Ok(()),
        TypeDescriptor::Atom(expected) => match value {
            Value::Atom(actual) if actual == expected => Ok(()),
            _ => Err(format!("{path} must be '{}", expected.name())),
        },
        TypeDescriptor::Array(item) => {
            let Value::Array(values) = value else {
                return Err(format!("{path} must be an Array"));
            };
            for (index, value) in values.iter().enumerate() {
                validate_value(item, value, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        TypeDescriptor::Tuple(items) => {
            let Value::Tuple(values) = value else {
                return Err(format!("{path} must be a Tuple"));
            };
            if items.len() != values.len() {
                return Err(format!("{path} must have {} tuple items", items.len()));
            }
            for (index, (item, value)) in items.iter().zip(values.iter()).enumerate() {
                validate_value(item, value, &format!("{path}.{index}"))?;
            }
            Ok(())
        }
        TypeDescriptor::Struct(fields) => {
            let Value::Dict(values) = value else {
                return Err(format!("{path} must be a Dict"));
            };
            if fields.keys().ne(values.shape().fields().iter()) {
                return Err(format!("{path} has a different field shape"));
            }
            for (name, field) in fields {
                validate_value(
                    field,
                    values.get(name).expect("matching shape"),
                    &format!("{path}.{name}"),
                )?;
            }
            Ok(())
        }
        TypeDescriptor::Union(variants) => {
            if variants
                .iter()
                .any(|variant| validate_value(variant, value, path).is_ok())
            {
                Ok(())
            } else {
                Err(format!("{path} does not match any Union variant"))
            }
        }
        descriptor => Err(format!(
            "{path} must be {}, got {}",
            descriptor.display_name(),
            value.type_name()
        )),
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
        let descriptor = TypeDescriptor::Struct(BTreeMap::from([
            ("age".into(), TypeDescriptor::Int),
            ("name".into(), TypeDescriptor::String),
        ]));
        let value = descriptor.to_value(&mut Vm::new());
        assert_eq!(TypeDescriptor::from_value(&value).unwrap(), descriptor);
    }

    #[test]
    fn ordinary_closure_computes_type_metadata() {
        let analysis = analyze_source(
            "test",
            "fn Optional(item) { Union([Atom('None), Tuple([Atom('Some), item])]) }\
             type MaybeInt = Optional(Int);\
             let value: MaybeInt = ('Some, 42);\
             value",
        )
        .unwrap();
        let maybe = analysis.declared_types.get("MaybeInt").unwrap();
        assert!(matches!(maybe, TypeDescriptor::Union(variants) if variants.len() == 2));
    }

    #[test]
    fn reports_structural_annotation_mismatch() {
        let error = analyze_source(
            "test",
            "type User = Struct({name: String, age: Int});\
             let user: User = {name: \"Ada\", age: \"old\"};\
             user",
        )
        .unwrap_err();
        assert!(error.message.contains("not assignable"));
    }

    #[test]
    fn tool_stage_respects_its_budget() {
        let error = analyze_source_with_budget("test", "type Number = Int; 0", 1).unwrap_err();
        assert!(error.message.contains("budget"));
    }

    #[test]
    fn rejects_invalid_metadata_protocol() {
        let error = analyze_source("test", "type Broken = {kind: 'Unknown}; 0").unwrap_err();
        assert!(error.message.contains("unknown value"));
    }

    #[test]
    fn runtime_validation_uses_computed_metadata() {
        let accepted = crate::run_source(
            "test",
            "type User = Struct({name: String, age: Int});\
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
            "type User = Struct({name: String, age: Int});\
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
            "type User = Struct({name: String}); let user: User = {name: \"Ada\"}; user.name",
        )
        .unwrap();
        assert!(!erased.constants().iter().any(is_type_metadata));

        let retained =
            crate::compile_source("test", "type User = Struct({name: String}); User").unwrap();
        assert!(retained.constants().iter().any(is_type_metadata));
    }

    fn is_type_metadata(value: &Value) -> bool {
        TypeDescriptor::from_value(value).is_ok()
    }
}
