use crate::ast::{
    BinaryOperator, BindingKind, Block, Expr, ExprKind, Pattern, PatternKind, Program,
    StringPartKind,
};
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
        &sources,
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
        sources,
        &BTreeMap::new(),
    )
}

pub(crate) fn analyze_program_with_bindings(
    source_name: &str,
    program: &Program,
    instruction_budget: usize,
    external_values: &BTreeMap<String, Value>,
    dynamic_bindings: &HashSet<String>,
    sources: &SourceDatabase,
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

    for binding in &program.value.body.value.bindings {
        check_interpolations(&binding.value.value, &static_environment, sources)?;
        if let Some(annotation) = &binding.value.annotation {
            check_interpolations(annotation, &static_environment, sources)?;
        }
        match binding.value.kind {
            BindingKind::Type => {
                let value = evaluate_tool_expression(
                    source_name,
                    &binding.value.value,
                    &tool_values,
                    instruction_budget,
                    sources,
                )?;
                let descriptor = TypeDescriptor::from_value(&value).map_err(|message| {
                    frontend_error(
                        source_name,
                        format!(
                            "type {} produced invalid metadata: {message}",
                            binding.value.name.value
                        ),
                    )
                })?;
                declared_types.insert(binding.value.name.value.clone(), descriptor);
                declared_type_spans.insert(binding.value.name.value.clone(), binding.location);
                resolved_types.insert(binding.value.name.value.clone(), value.clone());
                tool_values.insert(binding.value.name.value.clone(), value);
            }
            BindingKind::Let => {
                let inferred = infer_expr(&binding.value.value, &static_environment);
                let checked = if let Some(annotation) = &binding.value.annotation {
                    let metadata = evaluate_tool_expression(
                        source_name,
                        annotation,
                        &tool_values,
                        instruction_budget,
                        sources,
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
                    instruction_budget,
                    sources,
                ) {
                    tool_values.insert(binding.value.name.value.clone(), value);
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

    check_interpolations(
        &program.value.body.value.result,
        &static_environment,
        sources,
    )?;
    let result_type = infer_expr(&program.value.body.value.result, &static_environment);
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
    sources: &SourceDatabase,
) -> Result<Value, FrontendError> {
    let function = compile_expression_with_bindings(
        source_name,
        "<tool-stage>",
        expression,
        bindings,
        sources.get(expression.location.source),
    )?;
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
    match &expression.value {
        ExprKind::Int(_) => TypeDescriptor::Int,
        ExprKind::Float(_) => TypeDescriptor::Float,
        ExprKind::String(_) => TypeDescriptor::String,
        ExprKind::InterpolatedString(_) => TypeDescriptor::String,
        ExprKind::Bytes(_) => TypeDescriptor::Bytes,
        ExprKind::Atom(name) => TypeDescriptor::Atom(atom_from_name(name)),
        ExprKind::Variable(name) => environment
            .get(&name.value)
            .cloned()
            .unwrap_or(TypeDescriptor::Any),
        ExprKind::Array(items) => {
            let item_types = items
                .iter()
                .map(|item| infer_expr(item, environment))
                .collect::<Vec<_>>();
            let item = common_type(item_types).unwrap_or(TypeDescriptor::Any);
            TypeDescriptor::Array(Box::new(item))
        }
        ExprKind::Tuple(items) => TypeDescriptor::Tuple(
            items
                .iter()
                .map(|item| infer_expr(item, environment))
                .collect(),
        ),
        ExprKind::Dict(fields) => TypeDescriptor::Struct(
            fields
                .iter()
                .map(|field| {
                    (
                        field.value.name.value.clone(),
                        infer_expr(&field.value.value, environment),
                    )
                })
                .collect(),
        ),
        ExprKind::Block(block) => infer_block(block, environment),
        ExprKind::Unary { operand, .. } => infer_expr(operand, environment),
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
                let left = infer_expr(left, environment);
                let right = infer_expr(right, environment);
                if left == right {
                    left
                } else {
                    TypeDescriptor::Any
                }
            }
        },
        ExprKind::Field { receiver, field } => match infer_expr(receiver, environment) {
            TypeDescriptor::Struct(fields) => fields
                .get(&field.value)
                .cloned()
                .unwrap_or(TypeDescriptor::Any),
            _ => TypeDescriptor::Any,
        },
        ExprKind::Call { .. } | ExprKind::Closure { .. } => TypeDescriptor::Any,
        ExprKind::If {
            then_branch,
            else_branch,
            ..
        } => union_or_single(vec![
            infer_block(then_branch, environment),
            infer_block(else_branch, environment),
        ]),
        ExprKind::Match { arms, .. } => union_or_single(
            arms.iter()
                .map(|arm| {
                    let mut arm_environment = environment.clone();
                    bind_pattern_types(&arm.value.pattern, &mut arm_environment);
                    infer_expr(&arm.value.value, &arm_environment)
                })
                .collect(),
        ),
    }
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
        check_interpolations(&binding.value.value, &environment, sources)?;
        if matches!(binding.value.kind, BindingKind::Let | BindingKind::Import) {
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
        TypeDescriptor::Float
        | TypeDescriptor::Bytes
        | TypeDescriptor::Array(_)
        | TypeDescriptor::Tuple(_)
        | TypeDescriptor::Struct(_) => false,
    }
}

fn infer_block(block: &Block, environment: &HashMap<String, TypeDescriptor>) -> TypeDescriptor {
    let mut environment = environment.clone();
    for binding in &block.value.bindings {
        if matches!(binding.value.kind, BindingKind::Let | BindingKind::Import) {
            let inferred = infer_expr(&binding.value.value, &environment);
            environment.insert(binding.value.name.value.clone(), inferred);
        }
    }
    infer_expr(&block.value.result, &environment)
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
