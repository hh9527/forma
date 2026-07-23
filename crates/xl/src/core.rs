use crate::value::{
    CoreArrayFunction, CoreAttributesFunction, CoreCodecFunction, CoreDebugFunction,
    CoreDictFunction, CoreJsonFunction, CoreResultFunction, NativeFunction,
};

pub(crate) const ARRAY_MODULE: &str = "core:array";
pub(crate) const ATTRIBUTES_MODULE: &str = "core:attributes";
pub(crate) const DICT_MODULE: &str = "core:dict";
pub(crate) const DEBUG_MODULE: &str = "core:debug";
pub(crate) const CODEC_MODULE: &str = "core:codec";
pub(crate) const RESULT_MODULE: &str = "core:result";
pub(crate) const JSON_MODULE: &str = "core:json";

pub(crate) struct CoreModuleSpec {
    pub(crate) name: &'static str,
    pub(crate) source: &'static str,
    pub(crate) functions: Vec<(&'static str, NativeFunction)>,
}

pub(crate) fn module_specs() -> Vec<CoreModuleSpec> {
    vec![
        CoreModuleSpec {
            name: ATTRIBUTES_MODULE,
            source: r#"
native normalize: fn(Any) -> Any;
native add: fn(Any, Any) -> Any;
native get: fn(Any, String) -> Any;
native has: fn(Any, String) -> Any;
native all: fn(Any) -> Any;
native strip: fn(Any) -> Any;
{ normalize: normalize, add: add, get: get, has: has, all: all, strip: strip }
"#,
            functions: vec![
                (
                    "add",
                    NativeFunction::core_attributes(CoreAttributesFunction::Add),
                ),
                (
                    "all",
                    NativeFunction::core_attributes(CoreAttributesFunction::All),
                ),
                (
                    "get",
                    NativeFunction::core_attributes(CoreAttributesFunction::Get),
                ),
                (
                    "has",
                    NativeFunction::core_attributes(CoreAttributesFunction::Has),
                ),
                (
                    "normalize",
                    NativeFunction::core_attributes(CoreAttributesFunction::Normalize),
                ),
                (
                    "strip",
                    NativeFunction::core_attributes(CoreAttributesFunction::Strip),
                ),
            ],
        },
        CoreModuleSpec {
            name: ARRAY_MODULE,
            source: r#"
native length: fn(Array(Any)) -> Int;
native map: fn(Array(Any), fn(Any) -> Any) -> Array(Any);
native filter: fn(Array(Any), fn(Any) -> Any) -> Array(Any);
native flat_map: fn(Array(Any), fn(Any) -> Array(Any)) -> Array(Any);
native fold: fn(Array(Any), Any, fn(Any, Any) -> Any) -> Any;
{ length: length, map: map, filter: filter, flat_map: flat_map, fold: fold }
"#,
            functions: vec![
                (
                    "filter",
                    NativeFunction::core_array(CoreArrayFunction::Filter),
                ),
                (
                    "flat_map",
                    NativeFunction::core_array(CoreArrayFunction::FlatMap),
                ),
                ("fold", NativeFunction::core_array(CoreArrayFunction::Fold)),
                (
                    "length",
                    NativeFunction::core_array(CoreArrayFunction::Length),
                ),
                ("map", NativeFunction::core_array(CoreArrayFunction::Map)),
            ],
        },
        CoreModuleSpec {
            name: DICT_MODULE,
            source: r#"
native keys: fn(Any) -> Array(String);
native values: fn(Any) -> Array(Any);
native pairs: fn(Any) -> Array(Any);
native from_pairs: fn(Array(Any)) -> Any;
native merge: fn(Any, Any) -> Any;
{ keys: keys, values: values, pairs: pairs, from_pairs: from_pairs, merge: merge }
"#,
            functions: vec![
                (
                    "from_pairs",
                    NativeFunction::core_dict(CoreDictFunction::FromPairs),
                ),
                ("keys", NativeFunction::core_dict(CoreDictFunction::Keys)),
                ("merge", NativeFunction::core_dict(CoreDictFunction::Merge)),
                ("pairs", NativeFunction::core_dict(CoreDictFunction::Pairs)),
                (
                    "values",
                    NativeFunction::core_dict(CoreDictFunction::Values),
                ),
            ],
        },
        CoreModuleSpec {
            name: DEBUG_MODULE,
            source: r#"
native dbg: fn(Any) -> Any;
native dbg_with: fn(String, Any) -> Any;
{ dbg: dbg, dbg_with: dbg_with }
"#,
            functions: vec![
                ("dbg", NativeFunction::core_debug(CoreDebugFunction::Dbg)),
                (
                    "dbg_with",
                    NativeFunction::core_debug(CoreDebugFunction::DbgWith),
                ),
            ],
        },
        CoreModuleSpec {
            name: CODEC_MODULE,
            source: r#"
native decode: fn(Any, Any) -> Any;
native encode: fn(Any, Any) -> Any;
{ decode: decode, encode: encode }
"#,
            functions: vec![
                (
                    "decode",
                    NativeFunction::core_codec(CoreCodecFunction::Decode),
                ),
                (
                    "encode",
                    NativeFunction::core_codec(CoreCodecFunction::Encode),
                ),
            ],
        },
        CoreModuleSpec {
            name: RESULT_MODULE,
            source: r#"
native unwrap: fn(Any) -> Any;
{ unwrap: unwrap }
"#,
            functions: vec![(
                "unwrap",
                NativeFunction::core_result(CoreResultFunction::Unwrap),
            )],
        },
        CoreModuleSpec {
            name: JSON_MODULE,
            source: r#"
native stringify: fn(Any) -> String;
native stringify_pretty: fn(Int) -> fn(Any) -> String;
native rename: fn(String) -> fn(Any, Any) -> Any;
native rename_all: fn(Any) -> fn(Any, Any) -> Any;
native flatten: fn(Any, Any) -> Any;
native default: fn(Any) -> fn(Any, Any) -> Any;
native skip_serializing_if: fn(Any) -> fn(Any, Any) -> Any;
{
    stringify: stringify,
    stringify_pretty: stringify_pretty,
    rename: rename,
    rename_all: rename_all,
    flatten: flatten,
    default: default,
    skip_serializing_if: skip_serializing_if,
}
"#,
            functions: vec![
                (
                    "default",
                    NativeFunction::core_json(CoreJsonFunction::Default),
                ),
                (
                    "flatten",
                    NativeFunction::core_json(CoreJsonFunction::Flatten),
                ),
                (
                    "rename",
                    NativeFunction::core_json(CoreJsonFunction::Rename),
                ),
                (
                    "rename_all",
                    NativeFunction::core_json(CoreJsonFunction::RenameAll),
                ),
                (
                    "skip_serializing_if",
                    NativeFunction::core_json(CoreJsonFunction::SkipSerializingIf),
                ),
                (
                    "stringify",
                    NativeFunction::core_json(CoreJsonFunction::Stringify),
                ),
                (
                    "stringify_pretty",
                    NativeFunction::core_json(CoreJsonFunction::StringifyPretty),
                ),
            ],
        },
    ]
}
