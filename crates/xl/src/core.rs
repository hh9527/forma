use crate::Vm;
use crate::value::{Closure, CoreArrayFunction, CoreDictFunction, NativeFunction, Value};
use std::sync::Arc;

pub(crate) const ARRAY_MODULE: &str = "core:array";
pub(crate) const DICT_MODULE: &str = "core:dict";

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
