use crate::Vm;
use crate::value::{Closure, CoreArrayFunction, NativeFunction, Value};
use std::sync::Arc;

pub(crate) const ARRAY_MODULE: &str = "core:array";

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
