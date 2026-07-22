# RFC 0013: Single-Assignment Definitions

- Status: Accepted for implementation
- Implementation: Complete

## Summary

XL modules and blocks gain `decl` and `def`. A declaration allocates a private
single-assignment upvalue slot during construction; a definition initializes
that slot exactly once. Closures may capture a declared slot before it is
initialized, but evaluating its value before initialization is an error. A
block cannot complete until every declaration in that block is initialized.

Named functions lower onto the same mechanism. This supports self recursion,
mutual recursion, and functions produced by arbitrary higher-order evaluation
without adding general mutable variables to XL.

## Motivation

`let` is deliberately sequential and immutable. Making it implicitly
recursive would obscure initialization order and turn every binding into a
cell. Recognizing only statically adjacent function declarations would be
efficient, but would not cover a recursive function returned by a builder.

The construction model is instead explicit:

```xl
decl walk: fn(Int) -> Int;

def walk = make_walker(fn(value) {
    if value > 0 { walk(value - 1) } else { 0 }
});

{walk}
```

A module remains a function evaluated once. `decl` allocates its relocation
slots, `def` fills them, successful completion seals them, and the module result
is published into the persistent world. Missing definitions are therefore a
module-evaluation contract failure rather than a separate linker language.

## Surface syntax

Bindings are:

```text
let  name [':' expression] '=' expression ';'
decl name ':' contract ';'
def  name '=' expression ';'
```

`let` is a normal sequential binding and may shadow an outer binding. `decl`
and `def` establish stable definition names and may not shadow another visible
definition name. A `def` fills a same-block `decl` when present; otherwise it
creates a non-forward, single-assignment definition whose name becomes visible
only after its right-hand side has evaluated.

A `def` never searches an outer block for a declaration to fill. `let` never
fills a declaration.

The focused function contract syntax is:

```text
fn(T1, T2, ...) -> R
```

It is a tool-stage metadata expression describing fixed arity, parameter
contracts, and one result contract. This RFC does not add polymorphism,
variance inference, traits, HKT, varargs, or multiple results. Missing or
unavailable inference remains `Any`.

Named functions are elaborated onto definitions:

```xl
fn increment(value: Int) -> Int { value + 1 }
```

has the core shape:

```xl
decl increment: fn(Int) -> Int;
def increment = fn(value) { value + 1 };
```

When the result annotation is omitted, the tool stage uses the inferred body
result or `Any`. A preceding explicit `decl` supplies the contract and the
named function acts as its `def`.

## Construction semantics

Each declaration creates a slot with this abstract state machine:

```text
Uninitialized --def(value)--> Ready(value)
```

There is no second transition. Implementations may use private mutation to
perform the transition, but XL code cannot obtain a slot, initialize it, or
observe its representation.

Resolving a declared name inside a closure captures the slot reference. It
does not read the slot while constructing the closure:

```xl
decl countdown: fn(Int) -> Int;
def countdown = fn(n) {
    if n > 0 { countdown(n - 1) } else { 0 }
};
```

Direct evaluation reads the slot and therefore fails while it is uninitialized:

```xl
decl value: Int;
def value = value + 1;
```

Before a block returns its result, all declarations owned by that block must be
Ready. The toolchain diagnoses statically obvious missing or duplicate
definitions, while the runtime seal check remains authoritative.

Definition failure aborts the containing module initialization. As established
by RFC 0011 and RFC 0012, failed module construction is fatal, publishes no
root, and requires no heap rollback.

## Runtime representation

The heap gains a private definition-cell object. Runtime bytecode can:

```text
MakeDefinitionCell
ReadDefinitionCell
InitializeDefinitionCell
AssertDefinitionCellReady
```

The compiler environment distinguishes a direct register from a register that
contains a cell reference. Variable evaluation emits a read for the latter;
closure capture copies the cell reference itself. This distinction is never a
runtime `Value` category exposed to XL or native functions.

Local cells may be initialized once. Persistent cells are always Ready and
read-only. Promotion copies Ready cells and their reachable values, preserving
cycles and function identities. Promotion rejects an uninitialized cell as an
internal construction-contract violation.

After sealing, an optimizer may replace cell references with direct value
references. This is not language semantics. Keeping frozen cells indefinitely
is valid, and rewriting must not change equality, function identity, source
diagnostics, quota determinism, or reachable module results.

## Function contracts

Function metadata checks that a definition is a fixed-arity function with the
declared arity. Parameter and result metadata are retained for static analysis.
This RFC does not insert dynamic checks at every function call; explicit
`validate` remains the runtime validation boundary. A later RFC may define
checked public-call boundaries without changing definition-cell semantics.

## Diagnostics

Required errors include:

- duplicate `decl` or `def` in one block;
- a definition name shadowing a visible definition;
- `def` incompatible with its declaration contract;
- reading a slot before initialization;
- initializing a slot more than once;
- completing a block with an uninitialized declaration.

Diagnostics retain the declaration, definition, read, and block source origins
where applicable. A missing-definition diagnostic uses the declaration as its
primary location.

## Rejected alternatives

### Make every `let` recursive

This burdens ordinary bindings with cell semantics and makes initialization
order less visible.

### Only recognize static recursive function groups

Reserved closure handles efficiently support known groups but cannot represent
a recursive function returned by arbitrary higher-order computation.

### `let rec` as the primitive

`let rec` is useful surface shorthand but does not expose the separate module
contract and definition phases. It may later lower to `decl` plus `def`.

### Require cells to be optimized away

Graph rewriting is an optimization and is unnecessarily complex for the first
correct implementation. Frozen private cells preserve XL immutability.

## Deferred work

- `let rec` syntax sugar;
- cell-elimination and recursive-group closure-handle optimization;
- module interface files generated from declarations;
- polymorphic and recursive function contracts;
- dynamic checks on selected public function-call boundaries;
- tail-call optimization.

## Implementation plan

1. Extend Logos, Lelwel, typed CST views, lowering, and the located AST with
   `decl`, `def`, parameter contracts, result contracts, and function-contract
   expressions.
2. Extend tool metadata and block analysis with function descriptors,
   declaration completeness, no-shadow definition rules, and named-function
   elaboration.
3. Add verified LIR/bytecode operations and private heap cells.
4. Compile declaration references as cell reads while closures capture cell
   registers directly.
5. Seal blocks, reject incomplete construction, and teach promotion/export to
   traverse Ready cells.

## Acceptance criteria

1. Existing sequential `let` shadowing remains unchanged.
2. A direct `def` is visible after its RHS and cannot shadow a definition.
3. Self-recursive and mutually recursive definitions execute correctly.
4. A higher-order builder can return a closure that refers to its declared
   definition slot.
5. Reading before initialization and defining twice fail with source origins.
6. A missing `def` is reported at its `decl` before the block completes.
7. Named functions elaborate to the same behavior as explicit `decl`/`def`.
8. Function contracts participate in tool-stage definition checking.
9. Ready recursive graphs publish to the persistent heap and retain function
   identity; uninitialized cells cannot publish.
10. Quotas, call depth, fuel, and module init-once behavior remain effective.
11. Existing tests, strict Clippy, and diff checks pass.

## Implementation result

Implemented across the Logos/Lelwel frontend, located semantic AST, tool-stage
metadata analysis, LIR assembler, bytecode VM, layered heap, and module loader.
The compiler records direct and cell-backed bindings separately, so closure
capture retains a cell while ordinary variable evaluation emits an explicit
read. Named functions use the same cell path as explicit declarations.

Function metadata now represents fixed parameter contracts and one result
contract. Definition initialization includes an arity assertion even when a
higher-order RHS has statically degraded to `Any`; deeper parameter and result
checking remains in the tool stage and explicit `validate` boundary.

Ready definition-cell graphs copy through promotion with cycles and closure
identity intact. Uninitialized cells cannot publish. The legacy tree-shaped
`Value` adapter cannot represent recursive closures; when an XL dependency has
such a root, the module cache retains the authoritative persistent root and
uses an `Any` analysis shadow for that import. Other export failures remain
errors, and ordinary data modules retain exact values and provenance.

The acceptance suite covers explicit self recursion, mutual recursion,
higher-order construction, named-function sugar, annotated functions,
initialization failures, no-shadow rules, dynamic arity checks, cyclic
promotion, and recursive function imports.
