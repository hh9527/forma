use crate::module::{InstantiatedModule, PendingModule};
use crate::value::{DynValue, NativeModuleId, NativeType, NativeTypeId};
use crate::{CallContext, NativeError, Value, Vm};

pub(crate) const MODULE_ID: u32 = 23;
pub(crate) const HANDLE_LOCAL: u32 = 1;
pub(crate) const INSTANTIATED_LOCAL: u32 = 2;
pub(crate) const MODULE_NAME: &str = "entry/rt.native.forma";

fn linked_handle_type(context: &CallContext<'_, '_>) -> Result<NativeType, NativeError> {
    context
        .value(context.upvalue(0)?)?
        .as_native_type()
        .cloned()
        .ok_or_else(|| NativeError::new("ModuleHandle native type is not linked"))
}

pub(crate) fn handle_type() -> NativeType {
    NativeType::bind(
        NativeTypeId {
            module: NativeModuleId(MODULE_ID),
            local: HANDLE_LOCAL,
        },
        format!("{MODULE_NAME}#ModuleHandle"),
    )
}

fn instantiated_type() -> NativeType {
    NativeType::bind(
        NativeTypeId {
            module: NativeModuleId(MODULE_ID),
            local: INSTANTIATED_LOCAL,
        },
        format!("{MODULE_NAME}#InstantiatedModule"),
    )
}

fn pending(context: &CallContext<'_, '_>, index: usize) -> Result<PendingModule, NativeError> {
    let native_type = linked_handle_type(context)?;
    context
        .value(context.argument(index)?)?
        .as_opaque::<PendingModule>(&native_type)
        .cloned()
        .ok_or_else(|| NativeError::new("expected ModuleHandle"))
}

pub(crate) fn native_args(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let pending = pending(context, 0)?;
    let mut items = Vec::new();
    for argument in pending.arguments() {
        let register = context.scratch()?;
        context.set_string(register, argument)?;
        items.push(register);
    }
    context.make_array(context.result(), &items)
}

pub(crate) fn native_options(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let pending = pending(context, 0)?;
    let mut items = Vec::new();
    for action in pending.option_actions() {
        let key = context.scratch()?;
        let value = context.scratch()?;
        let item = context.scratch()?;
        context.set_string(key, &action.key)?;
        context.set_value(value, &action.value)?;
        context.make_dict(item, &[("key".into(), key), ("value".into(), value)])?;
        items.push(item);
    }
    context.make_array(context.result(), &items)
}

pub(crate) fn native_option_names(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let value = context.export_value(context.argument(0)?)?;
    let value = match &value {
        Value::Dyn(value) => value.value(),
        value => value,
    };
    let Value::Array(values) = value else {
        return result_error(context, "option value must be an Array(String)".into());
    };
    if !values.iter().all(|value| matches!(value, Value::String(_))) {
        return result_error(context, "option value must be an Array(String)".into());
    }
    result_ok_value(context, &Value::Array(values.clone()))
}

pub(crate) fn native_capture_vars(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let _pending = pending(context, 0)?;
    let names = context.export_value(context.argument(1)?)?;
    let Value::Array(names) = names else {
        return Err(NativeError::new("capture_vars expects Array(String)"));
    };
    let mut fields = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for name in names.iter() {
        let Value::String(name) = name else {
            return Err(NativeError::new("capture_vars expects Array(String)"));
        };
        if !seen.insert(name.to_string()) {
            continue;
        }
        let Some(value) = std::env::var_os(name.as_ref()) else {
            continue;
        };
        let value = value
            .into_string()
            .map_err(|_| NativeError::new(format!("environment variable {name:?} is not UTF-8")))?;
        let register = context.scratch()?;
        context.set_string(register, value)?;
        fields.push((name.to_string(), register));
    }
    context.make_dict(context.result(), &fields)
}

pub(crate) fn native_cwd(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let _pending = pending(context, 0)?;
    let cwd = std::env::current_dir().map_err(|error| NativeError::new(error.to_string()))?;
    let cwd = cwd
        .to_str()
        .ok_or_else(|| NativeError::new("current directory is not UTF-8"))?;
    context.set_string(context.result(), cwd)
}

pub(crate) fn native_var(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let _pending = pending(context, 0)?;
    let name = context
        .value(context.argument(1)?)?
        .as_str()
        .ok_or_else(|| NativeError::new("environment variable name must be a String"))?
        .to_owned();
    match std::env::var_os(&name) {
        None => context.set_none(context.result()),
        Some(value) => {
            let value = value.into_string().map_err(|_| {
                NativeError::new(format!("environment variable {name:?} is not UTF-8"))
            })?;
            let payload = context.scratch()?;
            let tag = context.scratch()?;
            context.set_string(payload, value)?;
            context.set_atom(tag, "Some")?;
            context.make_tagged(context.result(), tag, payload)
        }
    }
}

pub(crate) fn native_platform(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let _pending = pending(context, 0)?;
    let os = context.scratch()?;
    let arch = context.scratch()?;
    context.set_string(os, std::env::consts::OS)?;
    context.set_string(arch, std::env::consts::ARCH)?;
    context.make_dict(
        context.result(),
        &[("arch".into(), arch), ("os".into(), os)],
    )
}

pub(crate) fn native_download_prefix(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let _pending = pending(context, 0)?;
    context.set_string(context.result(), cache_path("downloads")?)
}

pub(crate) fn native_install_prefix(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let _pending = pending(context, 0)?;
    context.set_string(context.result(), cache_path("installs")?)
}

fn cache_path(child: &str) -> Result<String, NativeError> {
    let root = std::env::var_os("FORMA_CACHE_HOME")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".cache"))
        })
        .unwrap_or_else(std::env::temp_dir)
        .join("forma")
        .join("exec")
        .join(child);
    root.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| NativeError::new("cache path is not UTF-8"))
}

pub(crate) fn native_initialize(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let pending = pending(context, 0)?;
    match pending.initialize() {
        Ok(module) => result_ok_module(context, module),
        Err(error) => result_error(context, error.to_string()),
    }
}

pub(crate) fn native_module_export(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let native_type = instantiated_type();
    let module = context
        .value(context.argument(0)?)?
        .as_opaque::<InstantiatedModule>(&native_type)
        .cloned()
        .ok_or_else(|| NativeError::new("expected InstantiatedModule"))?;
    let name = context
        .value(context.argument(1)?)?
        .as_str()
        .ok_or_else(|| NativeError::new("module export name must be a String"))?
        .to_owned();
    let Some((value, scheme)) = module.export(&name) else {
        return result_error(context, format!("main module has no export {name:?}"));
    };
    let mut values = Vm::new();
    let descriptor = scheme.body.to_value(&mut values);
    let origin = module.export_origin(&name);
    let dyn_value = Value::Dyn(
        DynValue::from_module_export(descriptor, value.clone(), scheme.clone(), origin).into(),
    );
    result_ok_value(context, &dyn_value)
}

pub(crate) fn native_check_type(context: &mut CallContext<'_, '_>) -> Result<(), NativeError> {
    let expected_value = context.export_value(context.argument(0)?)?;
    let expected = crate::types::decode_type(&expected_value, "Type").map_err(NativeError::new)?;
    let dyn_value = context.export_value(context.argument(1)?)?;
    let Value::Dyn(dyn_value) = dyn_value else {
        return Err(NativeError::new("check_type expects Dyn"));
    };
    let actual =
        crate::types::decode_type(dyn_value.descriptor(), "Dyn.type").map_err(NativeError::new)?;
    let assignable = dyn_value.scheme().map_or_else(
        || crate::types::assignable(&actual, &expected),
        |scheme| crate::types::scheme_assignable(scheme, &expected),
    );
    if !assignable {
        return result_error(
            context,
            format!(
                "module export at {} has type {}, which is not assignable to {}",
                dyn_value.origin().unwrap_or("an unknown source"),
                actual.display_name(),
                expected.display_name()
            ),
        );
    }
    result_ok_value(context, dyn_value.value())
}

fn result_ok_module(
    context: &mut CallContext<'_, '_>,
    module: InstantiatedModule,
) -> Result<(), NativeError> {
    let payload = context.scratch()?;
    let tag = context.scratch()?;
    context.set_identity_opaque(payload, instantiated_type(), module)?;
    context.set_atom(tag, "Ok")?;
    context.make_tagged(context.result(), tag, payload)
}

fn result_ok_value(context: &mut CallContext<'_, '_>, value: &Value) -> Result<(), NativeError> {
    let payload = context.scratch()?;
    let tag = context.scratch()?;
    context.set_value(payload, value)?;
    context.set_atom(tag, "Ok")?;
    context.make_tagged(context.result(), tag, payload)
}

fn result_error(context: &mut CallContext<'_, '_>, message: String) -> Result<(), NativeError> {
    let data = context.scratch()?;
    let rule = context.scratch()?;
    let message_register = context.scratch()?;
    let payload = context.scratch()?;
    let tag = context.scratch()?;
    context.set_string(data, "module initialization")?;
    context.set_string(rule, MODULE_NAME)?;
    context.set_string(message_register, message)?;
    context.make_dict(
        payload,
        &[
            ("data".into(), data),
            ("message".into(), message_register),
            ("rule".into(), rule),
        ],
    )?;
    context.set_atom(tag, "Err")?;
    context.make_tagged(context.result(), tag, payload)
}
