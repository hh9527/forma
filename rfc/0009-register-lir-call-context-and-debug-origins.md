# RFC 0009: Register LIR, Call Context, and Debug Origins

- Status: Accepted
- Implementation: Complete

## Summary

This RFC introduces the first explicit boundary between XL semantic lowering
and the virtual machine. The compiler produces a symbolic, register-based LIR;
an assembler validates and lowers that LIR into executable bytecode. Bytecode
and native functions share one closure model and execute against a Lua-inspired
call context that exposes only the XL stack through register identifiers.

Owned runtime values remain private to the trusted VM implementation. Native
functions cannot receive, return, retain, or construct an owned `Value`. They
read arguments and write their single result through `CallContext`. Bytecode
debug origins remain in a side table so runtime failures and call traces can be
rendered as source diagnostics without adding locations to instructions or
runtime values.

## Motivation

The current compiler emits executable instructions directly. Each bytecode
call recursively enters Rust and allocates a new `Vec<Option<Value>>`; native
functions receive `&[Value]` and return an owned `Value`. Runtime errors retain
only the current function name and instruction offset. This implementation was
appropriate for proving the MVP semantics, but it leaves no stable boundary
for instruction validation, stack frames, a future GC, or source-aware runtime
diagnostics.

XL adopts the useful parts of Lua's VM shape:

- register operands address a frame window in an XL value stack;
- a VM state owns runtime facilities and one execution context identifies the
  active stack;
- bytecode and native closures use the same call operation;
- native code manipulates rooted stack slots instead of owning VM values.

XL does not copy Lua's mutable lexical cells, dynamic multiple-result rules,
table semantics, or public C ABI.

## Pipeline boundary

The compilation pipeline becomes:

```text
Located AST
  -> symbolic register LIR with Origin
  -> validation and assembly
  -> bytecode prototype + PC-to-Origin debug map
  -> VM execution through CallContext
```

LIR is an owned compiler representation. It is not a stable serialization
format and is not exposed as a language feature. It uses symbolic labels before
assembly and never embeds bytecode instruction offsets in branch operations.

The initial LIR remains linear and register-based. It is not SSA and does not
introduce basic-block parameters, phi nodes, an optimization framework, HIR, or
a general control-flow graph API.

## Identifiers and operands

LIR uses compact typed identifiers:

```rust
pub struct RegisterId(u32);
pub struct ConstantId(u32);
pub struct LabelId(u32);
```

Instructions may refer only to identifiers, immediate scalar metadata, field
names, and labels. An instruction operand is never an owned runtime `Value`.
Runtime constants are stored in the enclosing prototype's private constant
pool and are addressed by `ConstantId`.

Registers are function-local. Parameters occupy the first registers, followed
by immutable closure upvalues and compiler temporaries. Assembly rejects any
register, constant, label, prototype, or argument range outside its declared
function boundary.

## LIR instruction contract

The initial instruction families preserve the existing language semantics:

```text
LoadConst, Move
Add, Subtract, Multiply, Divide, Negate
Equal, LessThan
MakeArray, MakeTuple, MakeDict
InterpolateString
GetField, TupleLengthEquals, GetTuple
MakeClosure
Call, Jump, JumpIfFalse, Return, Fail
```

Collection and interpolation instructions may consume explicit register lists
in the symbolic representation. A future encoded format may require contiguous
windows or auxiliary storage; this RFC does not make that encoding choice part
of LIR semantics.

Every LIR instruction has an `Origin`. Source operations use
`Origin::Source(location)`. Compiler-generated control flow uses a synthetic
origin derived from the source construct that caused it. Assembly coalesces
adjacent equal origins into a compact PC range side table.

## Prototypes and closures

All XL function values have one runtime representation:

```text
Closure = Prototype + immutable upvalues
Prototype = BytecodePrototype | NativePrototype
```

A bytecode prototype contains its signature, register count, constants,
instructions, nested prototypes, and debug map. A native prototype contains a
name, arity, and trusted callback. A simple built-in function is a native
prototype with no upvalues.

XL lexical bindings are immutable, so upvalues are captured by value. The VM
does not implement Lua-style open and closed mutable upvalue cells. Bytecode
and native closures have the same observable `Func` category and the same call
instruction. Function equality remains unsupported.

## VM state, stack, and call context

The conceptual ownership model is:

```text
VmState
  = heap + interners + module cache + prototypes + stack storage

CallContext
  = a shared VmState reference + exclusive capability for one XL stack
```

Except for the logical XL stack, runtime facilities are reached through shared
references and may use private interior mutability. The exact representation
of a stack capability, including whether it is a linear handle, a branded
reference, or an arena identifier, is deliberately left private. Public
semantics require only that one stack cannot be concurrently executed through
safe APIs.

The stack holds one contiguous value area and explicit call-frame metadata. A
frame maps `RegisterId(n)` to a slot relative to its base. Calls are executed by
an explicit interpreter frame loop rather than recursive Rust VM calls. The VM
therefore owns call-depth checks, instruction budgeting, and trace capture.

## Native call boundary

Native callbacks receive a restricted `CallContext`. They do not receive
`&[Value]`, return `Value`, or access raw stack or heap storage:

```rust
type NativeCallback =
    fn(&mut CallContext<'_>) -> Result<(), NativeError>;
```

The context exposes frame-relative register operations in these categories:

- kind tests and copied scalar reads such as `get_int`;
- scoped access to reference data such as `with_string`;
- immediate queries such as collection length and field presence;
- copying a child or existing value into another register;
- constructing a value directly into a destination register;
- reading immutable upvalues through an identifier;
- reporting a structured native error.

It does not expose an owned-value getter, a raw heap accessor, or a raw stack
slice. Any scoped borrow ends before an operation that may allocate, resize the
stack, or re-enter XL. These constraints preserve roots and permit a future
moving collector without changing the native ABI.

Native callbacks are trusted implementation code but are still subject to the
current execution budget. A native invocation consumes at least one unit. A
callback that performs work proportional to input size must charge additional
units through the context.

## Calling convention

Every successful function call produces exactly one XL value:

```text
Call destination, callee, argument_base, argument_count
Return source
```

Arguments occupy a contiguous frame-relative register range. The callee and
destination are explicit operands and need not overlap that range. A native
callback reads the same argument window and writes the designated result slot.
Functions with no business result return `'None`. Multiple logical results are
represented by a Tuple.

Dynamic VM multiple returns, result adjustment, implicit expansion, and a
mutable frame top are deferred. The LIR stays explicit rather than adopting
Lua's encoded `B - 1` and `C - 1` operand conventions.

## Validation and assembly

Assembly is a required boundary, even while executable instructions remain a
readable Rust enum. It resolves labels, checks declarations and operands, and
constructs a bytecode prototype only on success.

Validation includes:

- register and contiguous argument ranges;
- constant and nested-prototype identifiers;
- unique and defined labels;
- resolved jump targets within the instruction sequence;
- parameter and upvalue counts within the register window;
- at least one reachable or structural return contract as appropriate;
- aligned instruction and debug-origin coverage.

Malformed bytecode constructed through compatibility APIs must still fail as
`InvalidBytecode` rather than panic. The assembler is not yet an untrusted
bytecode loader or a proof of definite register initialization.

## Errors, traces, and diagnostics

A runtime error records its kind, message, and an ordered trace of bytecode
frames. Each frame contains a prototype identity/name and the instruction PC
that was executing when control failed. Native errors are attributed to their
call instruction and also retain the native prototype name in the message.

The VM does not own a `SourceDatabase`. Compiler or module code combines each
`(prototype, pc)` with the prototype debug map to obtain an `Origin`, then
creates a source diagnostic. The innermost source origin is the primary label;
caller origins may be secondary labels or rendered as a stack trace.

Budget exhaustion, numeric errors, missing fields, invalid interpolation,
invalid conditions, unsupported equality, and unmatched patterns all follow
this path. Runtime values remain location-free.

## Compatibility and visibility

`Value` may remain publicly inspectable temporarily for host-facing MVP APIs
and tests. This RFC specifically forbids owned `Value` at the native-function
ABI and LIR instruction boundary. Tightening the host embedding API is separate
work and must not delay removal of owned values from BIF callbacks.

Existing `BytecodeFunction`, `Instruction`, and `Vm` entry points may remain as
compatibility wrappers. New compiler paths must pass through LIR assembly, and
ordinary execution must use the explicit frame stack and debug map.

## Rejected alternatives

### Continue compiling directly to executable instructions

This keeps label patching, validation, source-origin propagation, and VM
encoding concerns mixed into AST lowering. It also makes later instruction
encoding changes invasive.

### Give native functions owned values

Owned values can escape the stack root set, couple BIFs to heap representation,
and make a moving collector or alternate value encoding an ABI break.

### Pass `&mut Vm` and `&[Value]` to native functions

This exposes more authority than a call requires and permits references or
clones to outlive stack operations. A register-oriented context keeps bytecode
and native execution on the same abstraction boundary.

### Copy Lua multiple-result semantics now

Multiple results require dynamic frame tops, adjustment rules, and language
decisions for expressions and pipelines. Tuple already represents multiple
logical results without expanding the MVP VM contract.

### Encode instructions as fixed-width words now

The required register, constant, prototype, and jump widths have not been
measured. A symbolic assembler boundary permits later `u32`, wider, or mixed
encoding without changing compiler lowering or runtime semantics.

### Introduce SSA or an optimization framework

The immediate need is a stable execution and origin boundary. Existing control
flow can be represented by linear operations and labels. Optimization-driven
IR should be justified by concrete passes.

## Deferred work

- fixed-width bytecode encoding and serialization;
- untrusted bytecode loading and full definite-initialization analysis;
- multiple VM return values and varargs;
- tail-call instructions and tail-call trace policy;
- coroutines, scheduling, suspension, and process state;
- a concrete tracing or moving garbage collector;
- public native extension ABI stability;
- module initialization effects and capability security;
- SSA, optimization passes, and a general CFG representation.

## Implementation plan

1. Add symbolic LIR functions, typed identifiers, labels, and per-operation
   `Origin` values.
2. Change the compiler to emit LIR and replace instruction-offset patching with
   symbolic labels.
3. Add an assembler/verifier that resolves labels and creates bytecode plus a
   coalesced debug-origin map.
4. Unify bytecode and native prototypes under closures with immutable
   upvalues.
5. Introduce the register-only `CallContext` native ABI and migrate built-in
   type/validation functions away from owned arguments and results.
6. Replace recursive bytecode calls and per-frame register vectors with one
   value stack and an explicit call-frame loop.
7. Record runtime frame traces and translate debug origins into source-aware
   diagnostics in compiler and module execution paths.
8. Preserve compatibility constructors where useful and add focused LIR,
   verifier, context, VM, diagnostic, module, and CLI tests.

## Acceptance criteria

1. The compiler emits symbolic LIR and all production compilation passes
   through assembly before VM execution.
2. LIR branches use labels, carry `Origin`, and contain no instruction offsets
   or owned `Value` operands.
3. Assembly rejects invalid registers, argument ranges, constants, labels,
   prototypes, and debug coverage without panicking.
4. Bytecode and native functions are closures over a unified prototype model;
   simple BIFs have empty immutable upvalues.
5. Native callbacks only access arguments, results, and upvalues through
   register-oriented `CallContext`; their signature contains no `Value`.
6. Every successful call produces exactly one value, and bytecode-to-bytecode,
   bytecode-to-native, and closure-with-upvalue calls follow the same contract.
7. Nested bytecode calls execute through an explicit frame stack and obey one
   deterministic instruction budget without Rust VM recursion.
8. Runtime errors contain an ordered frame trace and map through PC debug data
   to the originating source expression.
9. Real compiler/module paths render correct source locations for division by
   zero, missing fields, unsupported interpolation, and errors inside nested
   calls.
10. Runtime `Value` equality and representation remain independent of source
    origins, and existing language/tool-stage/module behavior remains intact.
11. Workspace tests, strict Clippy, formatting, and diff checks pass.

## Implementation result

The compiler now emits a symbolic register LIR whose operations carry source
or synthetic origins. Branches use `LabelId`; call arguments are assembled
from validated contiguous register ranges. The assembler recursively validates
nested functions, resolves labels, and emits executable instructions with a
coalesced PC-to-Origin side table. Existing hand-built bytecode constructors
remain available for VM-level compatibility tests.

Runtime functions now use one `Closure` representation over bytecode or native
prototypes and immutable upvalues. Native callbacks receive only
`&mut CallContext`: arguments, upvalues, scratch slots, and the single result
are addressed by `RegisterId`. `ValueRef` supports scoped inspection without
exposing an owned value, while all construction writes directly into a context
register. The type-metadata and validation built-ins were migrated to this ABI.

The interpreter uses one contiguous XL value stack with explicit frame windows
and an iterative frame loop. Bytecode calls no longer recurse through Rust or
allocate a separate register vector per call. One budget covers bytecode and
native dispatch, and successful calls produce exactly one value. Focused tests
exercise native upvalues and 512 nested bytecode frames.

Runtime errors retain ordered frame traces. Each frame resolves its PC through
the prototype debug map, and compiler, tool-stage, loaded-module, and imported
module paths render the resulting origin through their shared source database.
Tests cover nested division-by-zero traces, missing fields, and dynamic string
interpolation at their source expressions.

An implementation follow-up added independent limits of 1,024 bytecode frames
and 1,048,576 XL stack slots. Bytecode frames, native call windows, and native
scratch registers all obey the stack-slot limit and report structured runtime
errors instead of allocation overflow or integer-conversion panics. Trace
assembly now uses an explicit active-frame state rather than comparing function
names and PCs, so recursive frames with identical debug coordinates remain
distinct.
