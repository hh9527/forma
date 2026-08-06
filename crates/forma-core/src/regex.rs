use crate::types::TypeDescriptor;
use crate::{CallContext, NativeError, NativeType};
use regex_syntax::hir::{Hir, HirKind};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct CompiledRegex {
    pattern: String,
    regex: regex::Regex,
    captures: BTreeSet<String>,
    required: BTreeSet<String>,
}

impl PartialEq for CompiledRegex {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for CompiledRegex {}

#[derive(Clone, Copy)]
enum ScalarKind {
    String,
    Int,
}

struct FieldPlan {
    kind: ScalarKind,
    optional: bool,
}

fn regex_type(context: &CallContext<'_, '_>) -> Result<NativeType, NativeError> {
    context
        .value(context.upvalue(0)?)?
        .as_native_type()
        .cloned()
        .ok_or_else(|| NativeError::new("Regex native type is not linked"))
}

fn compiled_argument(
    context: &CallContext<'_, '_>,
    index: usize,
    native_type: &NativeType,
) -> Result<CompiledRegex, NativeError> {
    context
        .value(context.argument(index)?)?
        .as_opaque::<CompiledRegex>(native_type)
        .cloned()
        .ok_or_else(|| NativeError::new("expected std/regex#Regex"))
}

fn required_captures(hir: &Hir) -> BTreeSet<String> {
    match hir.kind() {
        HirKind::Capture(capture) => {
            let mut required = required_captures(&capture.sub);
            if let Some(name) = &capture.name {
                required.insert(name.to_string());
            }
            required
        }
        HirKind::Concat(items) => items.iter().fold(BTreeSet::new(), |mut all, item| {
            all.extend(required_captures(item));
            all
        }),
        HirKind::Alternation(items) => {
            let mut items = items.iter();
            let Some(first) = items.next() else {
                return BTreeSet::new();
            };
            items.fold(required_captures(first), |required, item| {
                required
                    .intersection(&required_captures(item))
                    .cloned()
                    .collect()
            })
        }
        HirKind::Repetition(repetition) if repetition.min == 0 => BTreeSet::new(),
        HirKind::Repetition(repetition) => required_captures(&repetition.sub),
        _ => BTreeSet::new(),
    }
}

fn compile_pattern(pattern: String) -> Result<CompiledRegex, NativeError> {
    let hir = regex_syntax::Parser::new()
        .parse(&pattern)
        .map_err(|error| NativeError::new(format!("invalid regular expression: {error}")))?;
    let regex = regex::Regex::new(&pattern)
        .map_err(|error| NativeError::new(format!("invalid regular expression: {error}")))?;
    let mut captures = BTreeSet::new();
    for (index, name) in regex.capture_names().enumerate().skip(1) {
        let name = name
            .ok_or_else(|| NativeError::new(format!("capture group {index} must have a name")))?;
        if !captures.insert(name.to_owned()) {
            return Err(NativeError::new(format!("duplicate capture name {name:?}")));
        }
    }
    Ok(CompiledRegex {
        pattern,
        regex,
        captures,
        required: required_captures(&hir),
    })
}

pub(crate) fn native_compile(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let native_type = regex_type(context)?;
    let pattern = context
        .value(context.argument(0)?)?
        .as_str()
        .ok_or_else(|| NativeError::new("std/regex.compile expects String"))?
        .to_owned();
    let compiled = compile_pattern(pattern)?;
    context.set_opaque(context.result(), native_type, compiled)
}

pub(crate) fn native_is_match(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let native_type = regex_type(context)?;
    let compiled = compiled_argument(context, 0, &native_type)?;
    let input = context
        .value(context.argument(1)?)?
        .as_str()
        .ok_or_else(|| NativeError::new("std/regex.is_match expects String"))?
        .to_owned();
    context.set_atom(
        context.result(),
        if compiled.regex.is_match(&input) {
            "True"
        } else {
            "False"
        },
    )
}

fn option_scalar(descriptor: &TypeDescriptor) -> Option<ScalarKind> {
    let TypeDescriptor::Enum(variants) = descriptor else {
        return None;
    };
    if variants.len() != 2 || variants.get("None")?.is_some() {
        return None;
    }
    match variants.get("Some")?.as_deref()? {
        TypeDescriptor::String => Some(ScalarKind::String),
        TypeDescriptor::Int => Some(ScalarKind::Int),
        _ => None,
    }
}

fn field_plan(descriptor: &TypeDescriptor) -> Option<FieldPlan> {
    match descriptor {
        TypeDescriptor::String => Some(FieldPlan {
            kind: ScalarKind::String,
            optional: false,
        }),
        TypeDescriptor::Int => Some(FieldPlan {
            kind: ScalarKind::Int,
            optional: false,
        }),
        descriptor => option_scalar(descriptor).map(|kind| FieldPlan {
            kind,
            optional: true,
        }),
    }
}

fn validate_relation(
    compiled: &CompiledRegex,
    descriptor: &TypeDescriptor,
) -> Result<BTreeMap<String, FieldPlan>, NativeError> {
    let TypeDescriptor::Struct(fields) = descriptor else {
        return Err(NativeError::new("std/regex.parse requires a struct type"));
    };
    let names = fields.keys().cloned().collect::<BTreeSet<_>>();
    if names != compiled.captures {
        let missing = names
            .difference(&compiled.captures)
            .cloned()
            .collect::<Vec<_>>();
        let extra = compiled
            .captures
            .difference(&names)
            .cloned()
            .collect::<Vec<_>>();
        return Err(NativeError::new(format!(
            "regex captures must match struct fields; missing captures {missing:?}, extra captures {extra:?}"
        )));
    }
    fields
        .iter()
        .map(|(name, descriptor)| {
            let plan = field_plan(descriptor).ok_or_else(|| {
                NativeError::new(format!(
                    "regex field {name:?} must be String, Int, Option(String), or Option(Int)"
                ))
            })?;
            let capture_optional = !compiled.required.contains(name);
            if capture_optional != plan.optional {
                return Err(NativeError::new(format!(
                    "regex capture {name:?} is {}, but its field is {}",
                    if capture_optional {
                        "optional"
                    } else {
                        "required"
                    },
                    if plan.optional {
                        "optional"
                    } else {
                        "required"
                    }
                )));
            }
            Ok((name.clone(), plan))
        })
        .collect()
}

pub(crate) fn native_prepare(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let native_type = regex_type(context)?;
    let compiled = compiled_argument(context, 0, &native_type)?;
    let descriptor = crate::types::decode_native_type(context.value(context.argument(2)?)?)?;
    validate_relation(&compiled, &descriptor)?;

    let regex = context.scratch()?;
    context.copy(regex, context.argument(0)?)?;
    let attributes = context.scratch()?;
    context.make_dict(attributes, &[("std/regex.parse".into(), regex)])?;
    let kind = context.scratch()?;
    context.set_atom(kind, "WithAttributes")?;
    let inner = context.scratch()?;
    context.copy(inner, context.argument(2)?)?;
    context.make_dict(
        context.result(),
        &[
            ("kind".into(), kind),
            ("inner".into(), inner),
            ("attributes".into(), attributes),
        ],
    )
}

fn attached_regex(
    context: &CallContext<'_, '_>,
    native_type: &NativeType,
) -> Result<CompiledRegex, NativeError> {
    let mut metadata = context.value(context.argument(0)?)?;
    loop {
        if let Some(compiled) = metadata
            .dict_get("attributes")
            .and_then(|attributes| attributes.dict_get("std/regex.parse"))
            .and_then(|value| value.as_opaque::<CompiledRegex>(native_type))
        {
            return Ok(compiled.clone());
        }
        if metadata.dict_get("kind").and_then(|kind| kind.as_atom()) != Some("WithAttributes") {
            return Err(NativeError::new(
                "type has no valid std/regex.parse metadata",
            ));
        }
        metadata = metadata
            .dict_get("inner")
            .ok_or_else(|| NativeError::new("attributed type has no inner metadata"))?;
    }
}

fn set_error(context: &mut CallContext<'_, '_>, message: String) -> Result<(), NativeError> {
    let tag = context.scratch()?;
    context.set_atom(tag, "Err")?;
    let error = context.scratch()?;
    let message_register = context.scratch()?;
    context.set_string(message_register, message)?;
    context.make_dict(
        error,
        &[
            ("message".into(), message_register),
            ("data".into(), context.argument(1)?),
            ("rule".into(), context.argument(0)?),
        ],
    )?;
    context.make_tagged(context.result(), tag, error)
}

pub(crate) fn native_decode(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let native_type = regex_type(context)?;
    let compiled = attached_regex(context, &native_type)?;
    let descriptor = crate::types::decode_native_type(context.value(context.argument(0)?)?)?;
    let plans = validate_relation(&compiled, &descriptor)?;
    let input = context
        .value(context.argument(1)?)?
        .as_str()
        .ok_or_else(|| NativeError::new("std/regex.decode expects String"))?
        .to_owned();
    let Some(captures) = compiled.regex.captures(&input) else {
        return set_error(context, "input does not match regular expression".into());
    };

    let mut fields = Vec::with_capacity(plans.len());
    for (name, plan) in plans {
        let register = context.scratch()?;
        match captures.name(&name).map(|capture| capture.as_str()) {
            Some(text) => {
                let value = context.scratch()?;
                match plan.kind {
                    ScalarKind::String => context.set_string(value, text)?,
                    ScalarKind::Int => match text.parse::<i64>() {
                        Ok(number) => context.set_int(value, number)?,
                        Err(_) => {
                            return set_error(
                                context,
                                format!("capture {name:?} is not a valid Int"),
                            );
                        }
                    },
                }
                if plan.optional {
                    let tag = context.scratch()?;
                    context.set_atom(tag, "Some")?;
                    context.make_tagged(register, tag, value)?;
                } else {
                    context.copy(register, value)?;
                }
            }
            None if plan.optional => context.set_none(register)?,
            None => return set_error(context, format!("required capture {name:?} is absent")),
        }
        fields.push((name, register));
    }
    let value = context.scratch()?;
    context.make_dict(value, &fields)?;
    let tag = context.scratch()?;
    context.set_atom(tag, "Ok")?;
    context.make_tagged(context.result(), tag, value)
}
