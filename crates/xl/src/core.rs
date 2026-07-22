use crate::Vm;
use crate::value::{
    Closure, CoreArrayFunction, CoreCodecFunction, CoreDebugFunction, CoreDictFunction,
    CoreJsonFunction, CoreResultFunction, NativeFunction, Value,
};
use std::sync::Arc;

pub(crate) const ARRAY_MODULE: &str = "core:array";
pub(crate) const DICT_MODULE: &str = "core:dict";
pub(crate) const DEBUG_MODULE: &str = "core:debug";
pub(crate) const CODEC_MODULE: &str = "core:codec";
pub(crate) const RESULT_MODULE: &str = "core:result";
pub(crate) const JSON_MODULE: &str = "core:json";

pub(crate) fn array_module_value() -> Value {
    let functions = [
        ("filter", CoreArrayFunction::Filter),
        ("flat_map", CoreArrayFunction::FlatMap),
        ("fold", CoreArrayFunction::Fold),
        ("length", CoreArrayFunction::Length),
        ("map", CoreArrayFunction::Map),
    ];
    Vm::new()
        .make_dict(functions.into_iter().map(|(name, function)| {
            (
                name.to_owned(),
                Value::Func(Arc::new(Closure::native(NativeFunction::core_array(
                    function,
                )))),
            )
        }))
        .expect("core:array fields are unique")
}

pub(crate) fn dict_module_value() -> Value {
    let functions = [
        ("from_pairs", CoreDictFunction::FromPairs),
        ("keys", CoreDictFunction::Keys),
        ("merge", CoreDictFunction::Merge),
        ("pairs", CoreDictFunction::Pairs),
        ("values", CoreDictFunction::Values),
    ];
    Vm::new()
        .make_dict(functions.into_iter().map(|(name, function)| {
            (
                name.to_owned(),
                Value::Func(Arc::new(Closure::native(NativeFunction::core_dict(
                    function,
                )))),
            )
        }))
        .expect("core:dict fields are unique")
}

pub(crate) fn debug_module_value() -> Value {
    let functions = [
        ("dbg", CoreDebugFunction::Dbg),
        ("dbg_with", CoreDebugFunction::DbgWith),
    ];
    Vm::new()
        .make_dict(functions.into_iter().map(|(name, function)| {
            (
                name.to_owned(),
                Value::Func(Arc::new(Closure::native(NativeFunction::core_debug(
                    function,
                )))),
            )
        }))
        .expect("core:debug fields are unique")
}

pub(crate) fn codec_module_value() -> Value {
    core_function_dict(
        [
            (
                "decode",
                NativeFunction::core_codec(CoreCodecFunction::Decode),
            ),
            (
                "encode",
                NativeFunction::core_codec(CoreCodecFunction::Encode),
            ),
        ]
        .into_iter(),
        "core:codec",
    )
}

pub(crate) fn result_module_value() -> Value {
    core_function_dict(
        [(
            "unwrap",
            NativeFunction::core_result(CoreResultFunction::Unwrap),
        )]
        .into_iter(),
        "core:result",
    )
}

pub(crate) fn json_module_value() -> Value {
    core_function_dict(
        [
            (
                "stringify",
                NativeFunction::core_json(CoreJsonFunction::Stringify),
            ),
            (
                "stringify_pretty",
                NativeFunction::core_json(CoreJsonFunction::StringifyPretty),
            ),
        ]
        .into_iter(),
        "core:json",
    )
}

fn core_function_dict(
    functions: impl Iterator<Item = (&'static str, NativeFunction)>,
    module: &str,
) -> Value {
    Vm::new()
        .make_dict(functions.map(|(name, function)| {
            (
                name.to_owned(),
                Value::Func(Arc::new(Closure::native(function))),
            )
        }))
        .unwrap_or_else(|_| panic!("{module} fields are unique"))
}
