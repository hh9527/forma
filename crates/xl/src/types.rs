use crate::ast::{
    BinaryOperator, BindingKind, Block, Expr, ExprKind, Pattern, PatternKind, Program,
    StringPartKind,
};
use crate::compiler::compile_expression_with_bindings;
use crate::json::{Provenance, ValuePath, ValuePathSegment};
use crate::lexer::{FrontendError, SourceLocation};
use crate::lir::RegisterId;
use crate::parser::parse_registered;
use crate::source::{Diagnostic, SourceDatabase};
use crate::value::{Atom, Closure, CoreModelFunction, NativeError, NativeFunction, Value};
use crate::{
    BuiltinAtom, CallContext, DebugSink, DiscardDebugSink, Quota, QuotaAccount, ValueKind,
    ValueRef, Vm,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

const DEFAULT_TOOL_FUEL: usize = 100_000;

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
    pub declared_types: BTreeMap<String, TypeDescriptor>,
    pub binding_types: BTreeMap<String, TypeDescriptor>,
    pub result_type: TypeDescriptor,
    pub(crate) prelude: BTreeMap<String, Value>,
    pub(crate) external_values: BTreeMap<String, Value>,
    pub(crate) dynamic_bindings: HashSet<String>,
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
    let mut tool_values = prelude.clone();
    let mut static_environment = HashMap::new();
    let mut declared_types = BTreeMap::new();
    let mut binding_types = BTreeMap::new();
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
        if let Some(annotation) = &binding.value.annotation {
            check_interpolations(annotation, &static_environment, sources)?;
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
                let inferred = infer_expr(&binding.value.value, &static_environment);
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
                let inferred = infer_expr(&binding.value.value, &static_environment);
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
    let result_type = infer_expr(&program.value.body.value.result, &static_environment);
    Ok(Analysis {
        declared_types,
        binding_types,
        result_type,
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
    for function in [
        NativeFunction::core_model(CoreModelFunction::Struct),
        NativeFunction::core_model(CoreModelFunction::Enum),
        NativeFunction::new("Atom", 1, native_atom_type),
        NativeFunction::new("Array", 1, native_array_type),
        NativeFunction::new("Tuple", 1, native_tuple_type),
        NativeFunction::new("Struct", 1, native_struct_type),
        NativeFunction::new("Union", 1, native_union_type),
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
    decode_native_type(context.value(item)?)?;
    write_native_type_record(context, "Array", &[("item", item)])
}

fn native_tuple_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let value = context.value(context.argument(0)?)?;
    if value.kind() != ValueKind::Array {
        return Err(NativeError::new("Tuple expects an Array of Types"));
    }
    for index in 0..value.sequence_len().expect("Array has a length") {
        decode_native_type(value.sequence_get(index).expect("valid Array index"))?;
    }
    write_native_type_record(context, "Tuple", &[("items", context.argument(0)?)])
}

fn native_struct_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let value = context.value(context.argument(0)?)?;
    let Some(names) = value.dict_fields() else {
        return Err(NativeError::new("Struct expects a Dict of Types"));
    };
    for name in names {
        decode_native_type(value.dict_get(name).expect("Dict field exists"))?;
    }
    write_native_type_record(context, "Struct", &[("fields", context.argument(0)?)])
}

fn native_union_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let value = context.value(context.argument(0)?)?;
    if value.kind() != ValueKind::Array {
        return Err(NativeError::new("Union expects an Array of Types"));
    }
    let length = value.sequence_len().expect("Array has a length");
    if length == 0 {
        return Err(NativeError::new("Union requires at least one variant"));
    }
    for index in 0..length {
        decode_native_type(value.sequence_get(index).expect("valid Array index"))?;
    }
    write_native_type_record(context, "Union", &[("variants", context.argument(0)?)])
}

fn native_function_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let parameters_value = context.value(context.argument(0)?)?;
    if parameters_value.kind() != ValueKind::Array {
        return Err(NativeError::new("Fn expects an Array of parameter Types"));
    }
    for index in 0..parameters_value.sequence_len().expect("Array has a length") {
        decode_native_type(
            parameters_value
                .sequence_get(index)
                .expect("valid Array index"),
        )?;
    }
    let result = context.argument(1)?;
    decode_native_type(context.value(result)?)?;
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
        ExprKind::Call { callee, .. } => match infer_expr(callee, environment) {
            TypeDescriptor::Function { result, .. } => *result,
            _ => TypeDescriptor::Any,
        },
        ExprKind::Closure { parameters, body } => {
            let mut closure_environment = environment.clone();
            for parameter in parameters {
                closure_environment.insert(parameter.value.clone(), TypeDescriptor::Any);
            }
            TypeDescriptor::Function {
                parameters: vec![TypeDescriptor::Any; parameters.len()],
                result: Box::new(infer_block(body, &closure_environment)),
            }
        }
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

fn infer_block(block: &Block, environment: &HashMap<String, TypeDescriptor>) -> TypeDescriptor {
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
        if matches!(
            binding.value.kind,
            BindingKind::Let | BindingKind::Def | BindingKind::NamedFunction | BindingKind::Import
        ) {
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
    fn checks_interpolation_inside_nested_binding_annotations() {
        let error = analyze_source("test", r#"let outer = { let x: "\{[1]}" = "x"; x }; outer"#)
            .unwrap_err();
        assert!(error.message.contains("does not support Array<Int>"));
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
