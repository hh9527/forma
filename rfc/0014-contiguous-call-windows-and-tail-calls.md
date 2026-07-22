# RFC 0014: Contiguous Call Windows and Tail Calls

- Status: Implemented

## Summary

XL adopts a Lua-inspired bytecode calling convention. A call names one
contiguous register window containing the callee followed by its arguments:

```text
R[base]              = callee
R[base + 1]          = argument 0
...
R[base + argc]       = argument argc - 1
```

`Call { base, argc }` replaces the callee slot with the single result.
`TailCall { base, argc }` transfers the same window to a replacement frame and
returns the eventual result directly to the current frame's caller.

Syntactic tail calls are a language guarantee: they do not increase XL call
depth or retain the replaced frame's register window.

## Motivation

RFC 0013 makes explicit recursion part of XL, but ordinary calls still consume
one of the VM's 1,024 frames. Recursion is therefore semantically available but
cannot yet express loops or deep immutable-data traversal reliably.

The current bytecode also stores a destination register, a callee register, and
a variable-length argument register list. This is convenient for an initial
LIR but is not a stable boundary for the intended fixed-width bytecode. The
compiler already copies arguments into a contiguous range, so including the
callee in that range and overwriting it with the result does not increase the
current monotonic register requirement.

This RFC defines the logical Opcode ABI. It does not choose a packed binary
instruction width or bit allocation.

## Call window

For `argc = N`, a call window occupies exactly `N + 1` registers starting at
`base`:

```text
[callee, argument 0, ..., argument N - 1]
```

The verifier checks that `base + argc` is representable and inside the
function's register range. `argc` does not include the callee.

An ordinary call has the logical form:

```text
Call { base, argc }
```

On success its one result replaces `R[base]`. The remaining argument slots are
dead after the call and may be reused by a future register allocator. XL does
not yet support zero or multiple return values.

The compiler and LIR may initially use monotonic temporary allocation. They
must nevertheless materialize the final contiguous window before emitting the
Opcode. Register reuse and move elimination are optimizations, not call
semantics.

## Proper tail calls

A tail call has the logical form:

```text
TailCall { base, argc }
```

Before modifying the current frame, the VM resolves and copies the callee,
arguments, and closure upvalues out of the source window. For a bytecode
callee, it then:

1. retains the current frame's `return_destination`;
2. truncates the current frame's register window;
3. creates the callee window at the same frame base;
4. replaces the current `ExecutionFrame` rather than pushing another frame.

The replacement frame is checked against the ordinary stack-slot quota but
does not consume call depth. It may have more or fewer registers than the frame
it replaces.

For a native callee, the callback uses the ordinary `CallContext` ABI. Its
result immediately performs the equivalent of returning from the current
bytecode frame; no additional bytecode frame is retained.

Both `Call` and `TailCall` consume one unit of evaluation fuel. A tail call does
not receive a discount for reusing a frame.

## Tail positions

The final expression of a function body is in tail position. Tail position
propagates through:

- the result expression of a block;
- both branches of `if` when the `if` itself is in tail position;
- every arm of `match` when the `match` itself is in tail position.

A call expression in tail position emits `TailCall`. Calls used as binding
values, operands, arguments, receivers, conditions, interpolation parts, or
other intermediate expressions remain ordinary `Call` operations.

Tail position is structural and does not depend on callee identity. Calls
through an up-link are first resolved to an ordinary `Func` by `ReadUpLink` and
then use the same `TailCall` Opcode. `CallUpLink` is not introduced here.

## Validation and diagnostics

`Call` and `TailCall` reject malformed windows, non-functions, arity mismatch,
quota exhaustion, and invalid closure metadata through the existing structured
runtime errors and debug origins.

Tail-call frame replacement intentionally removes the replaced frame from the
active runtime trace. XL does not retain an unbounded logical tail-call history,
because that would recreate an unbounded stack under another name. The failing
callee and all non-tail callers remain in the trace. Bounded tail-call
breadcrumbs may be added later without changing execution semantics.

## Rejected alternatives

### `Call { argc }` derived from a dynamic top

This is appropriate for a stack-oriented embedding API such as `lua_call`, but
XL bytecode is register based. An explicit `base` lets the verifier validate a
window locally and does not require control-flow analysis of an implicit
operand-stack height. `CallContext` may continue to present stack-like argument
indices independently of the Opcode ABI.

### Keep a variable-length register list in Opcode

It avoids moves in an unoptimized compiler but prevents the call instruction
from approaching a fixed-width encoding and makes the runtime collect arbitrary
operands. LIR is the appropriate place for convenient higher-level operands.

### Recognize only self recursion

Tail position is independent of whether the callee is statically known. Mutual
recursion, higher-order calls, imported closures, and native calls use one
calling convention.

### Implement tail calls as `Call` followed by `Return`

That sequence still pushes a frame and therefore does not provide proper tail
calls. A peephole rewrite also misses calls under tail-position branches unless
the compiler or optimizer already performs control-flow analysis.

### Preserve every eliminated logical frame

An unbounded logical trace would defeat the memory guarantee of proper tail
calls. Complete historical tracing belongs in an explicitly bounded debugging
facility.

## Deferred work

- packed fixed-width bytecode encoding and concrete operand bit widths;
- reusable call scratch windows, lifetime-aware register allocation, and move
  elimination;
- `CallUpLink` or other fused resolver/call instructions;
- bounded tail-call diagnostic breadcrumbs;
- varargs and multiple return values;
- indirect calls through future runtime linking facilities.

## Implementation plan

1. Replace bytecode `Call { dst, callee, arguments }` with the logical
   `Call { base, argc }` window and update linking and verification.
2. Change compiler call lowering to copy the callee and arguments into one
   contiguous window and use its base as the expression result.
3. Add `TailCall { base, argc }` to LIR and bytecode.
4. Compile function-body, block, conditional, and match tail positions into
   terminal operations.
5. Implement bytecode frame replacement and native-result forwarding while
   retaining fuel, stack quota, arity, origin, and trace behavior.
6. Add focused assembler, compiler, VM, module, and quota tests.

## Acceptance criteria

1. Opcode calls contain only a base register and argument count; the callee and
   arguments occupy a verified contiguous window.
2. An ordinary bytecode or native call writes its single result to the window's
   base register.
3. Direct, mutual, higher-order, and up-link-backed calls in tail position do
   not increase physical call depth.
4. Tail calls beneath block, `if`, and `match` result positions are recognized.
5. Non-tail calls continue to consume frames and enforce the independent call
   depth limit.
6. Every ordinary or tail call consumes exactly one fuel unit.
7. Replacing a frame correctly handles callees with larger and smaller register
   windows and enforces the stack-slot quota.
8. Native tail calls forward their result directly to the original caller.
9. Runtime errors retain the failing callee and non-tail callers without
   retaining an unbounded eliminated-frame history.
10. Existing language, module, heap, quota, source-origin, and CLI behavior
    remains unchanged.

## Implementation result

Implemented across the compiler, register LIR, linked bytecode, and VM. The
logical `Call` and `TailCall` Opcodes now contain only a base register and an
argument count. The compiler materializes `[callee, arguments...]` windows,
and ordinary results overwrite the callee slot.

Function bodies compile structurally in tail context. Blocks, both `if`
branches, and every `match` arm propagate that context; intermediate calls
remain ordinary calls. Bytecode tail calls replace the active frame at the
same stack base, while native tail calls release the bytecode window before
opening `CallContext` and forward their result to the original caller.

Runtime validation rejects malformed call windows without panicking. Tests
cover calls through direct, mutual, higher-order, and up-link-backed recursion
beyond the physical frame limit; non-tail recursion still reaches the limit.
Native tail calls, fuel, larger replacement frames, stack quota, and the
physical-trace policy are covered independently.

The compiler still allocates monotonic temporary registers and emits moves to
form each call window. Reusable scratch windows and move elimination remain the
deferred optimization described above.
