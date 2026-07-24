pub mod ast;
pub mod bytecode;
pub mod compiler;
mod core;
mod heap;
pub mod hir;
pub mod json;
pub mod lexer;
pub mod lir;
pub mod module;
pub mod parser;
pub mod semantic;
pub mod source;
pub mod syntax;
pub mod types;
pub mod value;
pub mod vm;

pub use bytecode::{
    BytecodeFunction, DebugOriginRange, FuncByteCode, Instruction, LinkingTable, Opcode,
    ProtoLinkId, Register, TextLinkId, ValueLinkId,
};
pub use compiler::{ExecutionError, compile_source, run_source};
pub use hir::{
    HirDefinition, HirDefinitionId, HirDefinitionKind, HirExpression, HirExpressionId, HirProgram,
    HirReference, HirReferenceId, HirResolution,
};
pub use json::{
    JsonError, JsonParse, Provenance, SourcedValue, ValuePath, ValuePathSegment, parse_json,
    parse_json_registered, parse_json_with_provenance,
};
pub use lexer::{FrontendError, SourceLocation};
pub use module::{
    Engine, EngineConfig, LoadedModule, ModuleError, load_module, load_module_with_quota,
    load_module_with_quota_and_debug_sink,
};
pub use semantic::{
    Conflict, Definition, DefinitionId, DefinitionKind, DiagnosticId, FactIdentity, FactState,
    IncomputableReason, Reference, ReferenceId, SemanticFact, UnknownReason, WorkspaceExport,
    WorkspaceExpression, WorkspaceExpressionId, WorkspaceModule, WorkspaceModuleId,
    WorkspaceModuleKind, WorkspaceSnapshot, WorkspaceTypeGraph, WorkspaceTypeId, WorkspaceTypeNode,
};
pub use source::{
    Diagnostic, Label, Loc, Located, Location, Origin, SourceDatabase, SourceId, TextRange,
    WithOrigin,
};
pub use types::{
    Analysis, PartialAnalysis, SemanticDependencyGraph, SemanticDependencyNode, TypeGraph, TypeId,
    TypeNode, analyze_partial_types, analyze_partial_types_with_bindings, analyze_source,
    analyze_source_with_fuel, analyze_source_with_quota,
};
pub use value::{
    Atom, BuiltinAtom, Callable, Closure, Dict, NativeError, NativeFunction, Prototype, Shape,
    Value,
};
pub use vm::{
    CallContext, DebugEvent, DebugSink, DiscardDebugSink, Quota, QuotaAccount, RuntimeError,
    RuntimeErrorKind, RuntimeFrame, ValueKind, ValueRef, Vm,
};
