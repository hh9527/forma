use crate::value::{
    CoreArrayFunction, CoreAttributesFunction, CoreCodecFunction, CoreDebugFunction,
    CoreDictFunction, CoreHashFunction, CoreJsonFunction, CoreResultFunction, NativeFunction,
};

pub(crate) const ARRAY_MODULE: &str = "core:array";
pub(crate) const ATTRIBUTES_MODULE: &str = "core:attributes";
pub(crate) const DICT_MODULE: &str = "core:dict";
pub(crate) const DEBUG_MODULE: &str = "core:debug";
pub(crate) const CODEC_MODULE: &str = "core:codec";
pub(crate) const OPTION_MODULE: &str = "core:option";
pub(crate) const RESULT_MODULE: &str = "core:result";
pub(crate) const JSON_MODULE: &str = "core:json";
pub(crate) const HASH_MODULE: &str = "core:hash";

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
native normalize: Fn(Any) -> Any;
native add: Fn(Any, Any) -> Any;
native get: Fn(Any, String) -> Any;
native has: Fn(Any, String) -> Any;
native all: Fn(Any) -> Any;
native strip: Fn(Any) -> Any;
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
native length: for(A) Fn(Array(A)) -> Int;
native map: for(A, B) Fn(Array(A), Fn(A) -> B) -> Array(B);
native filter: for(A) Fn(Array(A), Fn(A) -> Bool) -> Array(A);
native flat_map: for(A, B) Fn(Array(A), Fn(A) -> Array(B)) -> Array(B);
native fold: for(A, B) Fn(Array(A), B, Fn(B, A) -> B) -> B;
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
native keys: Fn(Any) -> Array(String);
native values: Fn(Any) -> Array(Any);
native pairs: Fn(Any) -> Array(Tuple(String, Any));
native from_pairs: Fn(Array(Tuple(String, Any))) -> Any;
native merge: Fn(Any, Any) -> Any;
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
native dbg: for(A) Fn(A) -> A;
native dbg_with: for(A) Fn(String, A) -> A;
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
native decode: for(A) Fn(TypeOf(A), Any) -> Result(A, BlameError);
native encode: for(A) Fn(TypeOf(A), A) -> Result(Any, BlameError);
def format_error: Fn(BlameError) -> String = fn(error) { error.message };
{
    decode: decode,
    encode: encode,
    format_error: format_error,
}
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
            name: OPTION_MODULE,
            source: r#"
def map: for(A, B) Fn(Option(A), Fn(A) -> B) -> Option(B) = fn(option, function) {
    match option { 'None => 'None, 'Some(value) => 'Some(function(value)) }
};
def flat_map: for(A, B) Fn(Option(A), Fn(A) -> Option(B)) -> Option(B) = fn(option, function) {
    match option { 'None => 'None, 'Some(value) => function(value) }
};
def unwrap_or: for(A) Fn(Option(A), A) -> A = fn(option, fallback) {
    match option { 'None => fallback, 'Some(value) => value }
};
def is_some: for(A) Fn(Option(A)) -> Bool = fn(option) {
    match option { 'None => 'False, 'Some(_) => 'True }
};
{ map: map, flat_map: flat_map, unwrap_or: unwrap_or, is_some: is_some }
"#,
            functions: vec![],
        },
        CoreModuleSpec {
            name: RESULT_MODULE,
            source: r#"
native unwrap: for(A, E) Fn(Result(A, E)) -> A;
def map: for(A, E, B) Fn(Result(A, E), Fn(A) -> B) -> Result(B, E) = fn(result, function) {
    match result { 'Ok(value) => 'Ok(function(value)), 'Err(error) => 'Err(error) }
};
def flat_map: for(A, E, B) Fn(Result(A, E), Fn(A) -> Result(B, E)) -> Result(B, E) = fn(result, function) {
    match result { 'Ok(value) => function(value), 'Err(error) => 'Err(error) }
};
def map_err: for(A, E, F) Fn(Result(A, E), Fn(E) -> F) -> Result(A, F) = fn(result, function) {
    match result { 'Ok(value) => 'Ok(value), 'Err(error) => 'Err(function(error)) }
};
def unwrap_or: for(A, E) Fn(Result(A, E), A) -> A = fn(result, fallback) {
    match result { 'Ok(value) => value, 'Err(_) => fallback }
};
def is_ok: for(A, E) Fn(Result(A, E)) -> Bool = fn(result) {
    match result { 'Ok(_) => 'True, 'Err(_) => 'False }
};
{
    unwrap: unwrap,
    map: map,
    flat_map: flat_map,
    map_err: map_err,
    unwrap_or: unwrap_or,
    is_ok: is_ok,
}
"#,
            functions: vec![(
                "unwrap",
                NativeFunction::core_result(CoreResultFunction::Unwrap),
            )],
        },
        CoreModuleSpec {
            name: HASH_MODULE,
            source: r#"
native sha256: Fn(String) -> String;
{sha256: sha256}
"#,
            functions: vec![(
                "sha256",
                NativeFunction::core_hash(CoreHashFunction::Sha256),
            )],
        },
        CoreModuleSpec {
            name: JSON_MODULE,
            source: r#"
native parse: Fn(String) -> Result(Any, BlameError);
native decode: for(A) Fn(TypeOf(A), String) -> Result(A, BlameError);
native stringify: Fn(Any) -> String;
native stringify_pretty: Fn(Int) -> Fn(Any) -> String;
native rename: Fn(String) -> Fn(Any, Any) -> Any;
native rename_all: Fn(Any) -> Fn(Any, Any) -> Any;
native flatten: Fn(Any, Any) -> Any;
native untagged: Fn(Any, Any) -> Any;
native schema: Fn(Any) -> Any;
native default: Fn(Any) -> Fn(Any, Any) -> Any;
native skip_serializing_if: Fn(Any) -> Fn(Any, Any) -> Any;
{
    parse: parse,
    decode: decode,
    stringify: stringify,
    stringify_pretty: stringify_pretty,
    rename: rename,
    rename_all: rename_all,
    flatten: flatten,
    untagged: untagged,
    schema: schema,
    default: default,
    skip_serializing_if: skip_serializing_if,
}
"#,
            functions: vec![
                ("parse", NativeFunction::core_json(CoreJsonFunction::Parse)),
                (
                    "decode",
                    NativeFunction::core_json(CoreJsonFunction::Decode),
                ),
                (
                    "default",
                    NativeFunction::core_json(CoreJsonFunction::Default),
                ),
                (
                    "flatten",
                    NativeFunction::core_json(CoreJsonFunction::Flatten),
                ),
                (
                    "untagged",
                    NativeFunction::core_json(CoreJsonFunction::Untagged),
                ),
                (
                    "schema",
                    NativeFunction::core_json(CoreJsonFunction::Schema),
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
