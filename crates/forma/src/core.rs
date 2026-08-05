use crate::value::{
    CoreArrayFunction, CoreAttributesFunction, CoreCodecFunction, CoreDebugFunction,
    CoreDictFunction, CoreHashFunction, CoreJsonFunction, CorePathFunction, CoreResultFunction,
    CoreStringFunction, CoreTypeDescFunction, NativeFunction,
};

pub(crate) const ARRAY_MODULE: &str = "@bim/std/array";
pub(crate) const ATTRIBUTES_MODULE: &str = "@bim/std/attributes";
pub(crate) const DICT_MODULE: &str = "@bim/std/dict";
pub(crate) const DEBUG_MODULE: &str = "@bim/std/debug";
pub(crate) const BUILD_MODULE: &str = "@bim/std/build";
pub(crate) const EXEC_MODULE: &str = "@bim/std/exec";
pub(crate) const CODEC_MODULE: &str = "@bim/std/codec";
pub(crate) const OPTION_MODULE: &str = "@bim/std/option";
pub(crate) const RESULT_MODULE: &str = "@bim/std/result";
pub(crate) const JSON_MODULE: &str = "@bim/std/json";
pub(crate) const HASH_MODULE: &str = "@bim/std/hash";
pub(crate) const STRING_MODULE: &str = "@bim/std/string";
pub(crate) const PATH_MODULE: &str = "@bim/std/path";
pub(crate) const TOML_MODULE: &str = "@bim/std/toml";
pub(crate) const TYPE_DESC_MODULE: &str = "@bim/std/type-desc";

pub(crate) struct CoreModuleSpec {
    pub(crate) name: &'static str,
    pub(crate) source: &'static str,
    pub(crate) functions: Vec<(&'static str, NativeFunction)>,
}

pub(crate) fn module_specs() -> Vec<CoreModuleSpec> {
    vec![
        CoreModuleSpec {
            name: TYPE_DESC_MODULE,
            source: r#"
@enum type TypeDescKind = {
    Any: 'None, Never: 'None, Type: 'None, TypeOf: 'None,
    Int: 'None, Float: 'None, String: 'None, Bytes: 'None,
    Atom: 'None, Array: 'None, Dict: 'None, Tagged: 'None,
    Tuple: 'None, Struct: 'None, Enum: 'None, Union: 'None,
    Function: 'None, WithAttributes: 'None, Bound: 'None, Ref: 'None,
};
native kind: Fn(Type) -> TypeDescKind;
native children: Fn(Type) -> Array(Type);
native resolve: Fn(Type) -> Result(Type, BlameError);
{
    TypeDesc: Type,
    TypeDescKind: TypeDescKind,
    kind: kind,
    children: children,
    resolve: resolve,
}
"#,
            functions: vec![
                (
                    "children",
                    NativeFunction::core_type_desc(CoreTypeDescFunction::Children),
                ),
                (
                    "kind",
                    NativeFunction::core_type_desc(CoreTypeDescFunction::Kind),
                ),
                (
                    "resolve",
                    NativeFunction::core_type_desc(CoreTypeDescFunction::Resolve),
                ),
            ],
        },
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
native concat: for(A) Fn(Array(Array(A))) -> Array(A);
native map: for(A, B) Fn(Array(A), Fn(A) -> B) -> Array(B);
native filter: for(A) Fn(Array(A), Fn(A) -> Bool) -> Array(A);
native flat_map: for(A, B) Fn(Array(A), Fn(A) -> Array(B)) -> Array(B);
native fold: for(A, B) Fn(Array(A), B, Fn(B, A) -> B) -> B;
native any: for(A) Fn(Array(A), Fn(A) -> Bool) -> Bool;
native all: for(A) Fn(Array(A), Fn(A) -> Bool) -> Bool;
native find: for(A) Fn(Array(A), Fn(A) -> Bool) -> Option(A);
{ length: length, concat: concat, map: map, filter: filter, flat_map: flat_map, fold: fold, any: any, all: all, find: find }
"#,
            functions: vec![
                ("all", NativeFunction::core_array(CoreArrayFunction::All)),
                ("any", NativeFunction::core_array(CoreArrayFunction::Any)),
                (
                    "concat",
                    NativeFunction::core_array(CoreArrayFunction::Concat),
                ),
                (
                    "filter",
                    NativeFunction::core_array(CoreArrayFunction::Filter),
                ),
                (
                    "flat_map",
                    NativeFunction::core_array(CoreArrayFunction::FlatMap),
                ),
                ("fold", NativeFunction::core_array(CoreArrayFunction::Fold)),
                ("find", NativeFunction::core_array(CoreArrayFunction::Find)),
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
native map_values: for(A, B) Fn(Dict(A), Fn(A) -> B) -> Dict(B);
native filter: for(A) Fn(Dict(A), Fn(A) -> Bool) -> Dict(A);
native fold: for(A, B) Fn(Dict(A), B, Fn(B, String, A) -> B) -> B;
{ keys: keys, values: values, pairs: pairs, from_pairs: from_pairs, merge: merge, map_values: map_values, filter: filter, fold: fold }
"#,
            functions: vec![
                (
                    "filter",
                    NativeFunction::core_dict(CoreDictFunction::Filter),
                ),
                ("fold", NativeFunction::core_dict(CoreDictFunction::Fold)),
                (
                    "from_pairs",
                    NativeFunction::core_dict(CoreDictFunction::FromPairs),
                ),
                ("keys", NativeFunction::core_dict(CoreDictFunction::Keys)),
                ("merge", NativeFunction::core_dict(CoreDictFunction::Merge)),
                (
                    "map_values",
                    NativeFunction::core_dict(CoreDictFunction::MapValues),
                ),
                ("pairs", NativeFunction::core_dict(CoreDictFunction::Pairs)),
                (
                    "values",
                    NativeFunction::core_dict(CoreDictFunction::Values),
                ),
            ],
        },
        CoreModuleSpec {
            name: STRING_MODULE,
            source: r#"
native length: Fn(String) -> Int;
native join: Fn(Array(String), String) -> String;
native join_lines: Fn(Array(String)) -> String;
native split: Fn(String, String) -> Array(String);
native lines: Fn(String) -> Array(String);
native starts_with: Fn(String, String) -> Bool;
native ends_with: Fn(String, String) -> Bool;
native contains: Fn(String, String) -> Bool;
native replace: Fn(String, String, String) -> String;
native indent: Fn(String, Int) -> String;
native ensure_trailing_newline: Fn(String) -> String;
native trim_margin: Fn(String, String) -> String;
{ length: length, join: join, join_lines: join_lines, split: split, lines: lines, starts_with: starts_with, ends_with: ends_with, contains: contains, replace: replace, indent: indent, ensure_trailing_newline: ensure_trailing_newline, trim_margin: trim_margin }
"#,
            functions: vec![
                (
                    "contains",
                    NativeFunction::core_string(CoreStringFunction::Contains),
                ),
                (
                    "ends_with",
                    NativeFunction::core_string(CoreStringFunction::EndsWith),
                ),
                (
                    "join",
                    NativeFunction::core_string(CoreStringFunction::Join),
                ),
                (
                    "join_lines",
                    NativeFunction::core_string(CoreStringFunction::JoinLines),
                ),
                (
                    "length",
                    NativeFunction::core_string(CoreStringFunction::Length),
                ),
                (
                    "lines",
                    NativeFunction::core_string(CoreStringFunction::Lines),
                ),
                (
                    "replace",
                    NativeFunction::core_string(CoreStringFunction::Replace),
                ),
                (
                    "indent",
                    NativeFunction::core_string(CoreStringFunction::Indent),
                ),
                (
                    "ensure_trailing_newline",
                    NativeFunction::core_string(CoreStringFunction::EnsureTrailingNewline),
                ),
                (
                    "trim_margin",
                    NativeFunction::core_string(CoreStringFunction::TrimMargin),
                ),
                (
                    "split",
                    NativeFunction::core_string(CoreStringFunction::Split),
                ),
                (
                    "starts_with",
                    NativeFunction::core_string(CoreStringFunction::StartsWith),
                ),
            ],
        },
        CoreModuleSpec {
            name: PATH_MODULE,
            source: r#"
native join: Fn(Array(String)) -> String;
native normalize: Fn(String) -> String;
native parent: Fn(String) -> Option(String);
native file_name: Fn(String) -> Option(String);
{ join: join, normalize: normalize, parent: parent, file_name: file_name }
"#,
            functions: vec![
                (
                    "file_name",
                    NativeFunction::core_path(CorePathFunction::FileName),
                ),
                ("join", NativeFunction::core_path(CorePathFunction::Join)),
                (
                    "normalize",
                    NativeFunction::core_path(CorePathFunction::Normalize),
                ),
                (
                    "parent",
                    NativeFunction::core_path(CorePathFunction::Parent),
                ),
            ],
        },
        CoreModuleSpec {
            name: TOML_MODULE,
            source: r#"
@enum type DateTime = {
    OffsetDateTime: String,
    LocalDateTime: String,
    LocalDate: String,
    LocalTime: String,
};
{DateTime: DateTime}
"#,
            functions: vec![],
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
            name: BUILD_MODULE,
            source: r#"
@struct type TextFile = {
    path: String,
    content: String,
};
@enum type Artifact = {
    TextFile: TextFile,
};
@struct type OutputPlan = {
    files: Array(Artifact),
};
{
    TextFile: TextFile,
    Artifact: Artifact,
    OutputPlan: OutputPlan,
}
"#,
            functions: vec![],
        },
        CoreModuleSpec {
            name: EXEC_MODULE,
            source: r#"
@struct type Platform = {
    os: String,
    arch: String,
};
@struct type ExecSettings = {
    platform: Platform,
    install_prefix: String,
};
@struct type ExecRequest = {
    args: Array(String),
    env: Dict(String),
    cwd: String,
};
@enum type UnpackType = {
    TarGzip: 'None,
    Tar: 'None,
};
@struct type UnpackOpt = {
    dest: String,
    ty: UnpackType,
    src: String,
    strip: Int,
    digest: Option(String),
};
@enum type Install = {
    Unpack: UnpackOpt,
};
@struct type ExecEnv = {
    install: Array(Install),
    cwd: Option(String),
    bin: String,
    args: Array(String),
    env: Dict(String),
};
{
    Platform: Platform,
    ExecSettings: ExecSettings,
    ExecRequest: ExecRequest,
    UnpackType: UnpackType,
    UnpackOpt: UnpackOpt,
    Install: Install,
    ExecEnv: ExecEnv,
}
"#,
            functions: vec![],
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
