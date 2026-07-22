pub mod ast;
pub mod bytecode;
pub mod compiler;
pub mod json;
pub mod lexer;
pub mod lir;
pub mod module;
pub mod parser;
pub mod source;
pub mod syntax;
pub mod types;
pub mod value;
pub mod vm;

pub use bytecode::{BytecodeFunction, DebugOriginRange, Instruction, Register};
pub use compiler::{ExecutionError, compile_source, run_source};
pub use json::{
    JsonError, JsonParse, Provenance, SourcedValue, ValuePath, ValuePathSegment, parse_json,
    parse_json_registered, parse_json_with_provenance,
};
pub use lexer::{FrontendError, SourceLocation};
pub use module::{LoadedModule, ModuleError, load_module};
pub use source::{
    Diagnostic, Label, Located, Location, Origin, SourceDatabase, SourceId, TextRange, WithOrigin,
};
pub use types::{Analysis, TypeDescriptor, analyze_source, analyze_source_with_fuel};
pub use value::{
    Atom, BuiltinAtom, Callable, Closure, Dict, NativeError, NativeFunction, Prototype, Shape,
    Value,
};
pub use vm::{CallContext, RuntimeError, RuntimeErrorKind, RuntimeFrame, ValueKind, ValueRef, Vm};
