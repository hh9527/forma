# RFC 0015: Core Array Functions and Native Continuations

- Status: Implemented

## Summary

XL gains its first explicit built-in core module:

```xl
import arrays from "core:array";
```

The module exports one immutable Dict with five pure functions:

```text
length(array)
map(array, callback)
filter(array, predicate)
flat_map(array, callback)
fold(array, initial, callback)
```

Higher-order Array operations execute callbacks through the ordinary XL VM
call machinery. Trusted native code may suspend into a callback and resume
through a private VM continuation, but XL code cannot observe continuation or
builder state.

This RFC deliberately excludes Dict operations, a general iterator protocol,
effects, async execution, and a public native-continuation API.

## Motivation

XL's VM, two-stage metadata model, modules, recursion, and proper tail calls are
now sufficient to run general functions, but the language cannot yet perform a
basic transformation over imported JSON arrays. Adding collection syntax or
configuration-specific comprehensions would work against the language's goal
of expressing data processing through ordinary functional composition.

Array operations are the smallest useful scenario boundary. They also expose
an execution issue that should not be hidden inside a library callback: the
current synchronous `CallContext` can read arguments and write one result, but
cannot invoke an XL closure without recursively entering `Vm`. Recursive VM
entry would split frame accounting, traces, fuel, stack quotas, and future GC
roots across nested host calls.

The VM therefore owns callback scheduling. A trusted operation retains bounded
private state, requests an ordinary function call, and resumes when that call
returns.

## Core module identity

`core:array` is a reserved, toolchain-provided module identity. It is resolved
before filesystem path handling and cannot be shadowed by a file. Core module
names are static strings, participate in the same import binding rules as file
modules, and are fixed by the XL toolchain version.

The module exports a Dict:

```xl
{
    length: <native function>,
    map: <native continuation function>,
    filter: <native continuation function>,
    flat_map: <native continuation function>,
    fold: <native continuation function>,
}
```

Using a lowercase imported namespace avoids conflict with the existing
`Array(T)` type-metadata constructor:

```xl
import arrays from "core:array";

type Users = Array(User);
users |> arrays.map(normalize_user)
```

Core values are available identically to tool-stage and program-stage
evaluation. They are published into the engine's persistent world like other
module exports. They have no source text or data provenance; errors at their
public boundary use the importing XL call expression and the core function
name.

## Array API

### `length`

```text
length(array: Array) -> Int
```

Returns the number of elements as an `Int`. A length that cannot fit in `i64`
is a runtime error. Non-Array input is a type mismatch.

### `map`

```text
map(array: Array, callback: fn(item) -> value) -> Array
```

Calls `callback` once for each element in ascending index order and collects
the results in the same order.

### `filter`

```text
filter(array: Array, predicate: fn(item) -> Bool) -> Array
```

Calls `predicate` once for each element in ascending index order. A result of
`'True` retains the original element and `'False` discards it. Any other result
is a type mismatch attributed to `core:array.filter` and the predicate call.

### `flat_map`

```text
flat_map(array: Array, callback: fn(item) -> Array) -> Array
```

Calls `callback` once for each element in ascending index order and appends all
elements of each returned Array. A non-Array callback result is a type mismatch.
Both outer and inner order are preserved.

### `fold`

```text
fold(array: Array, initial: value, callback: fn(accumulator, item) -> value)
    -> value
```

Performs a strict left fold. The callback receives the current accumulator
first and the array element second. An empty Array returns `initial` without
calling the callback.

All callback arities are checked before the first callback. `map`, `filter`,
and `flat_map` require arity one; `fold` requires arity two. The initial API
does not pass indexes.

## Native continuation boundary

Ordinary public `NativeFunction::new` callbacks remain synchronous. This RFC
adds a private trusted implementation category for VM-managed core functions;
it does not let embedders retain `CallContext` or arbitrary Rust state across a
call.

The VM generalizes a bytecode frame's return destination from only a register
to a private return target:

```text
Root
Register(register)
NativeContinuation(state)
```

Starting a higher-order Array function creates bounded continuation state with:

- the operation kind and source function name;
- the source Array values and next index;
- the callback function;
- the original return target;
- an accumulator or output builder where required;
- the initiating call's debug origin for diagnostics.

The continuation requests a normal call using the same callee resolution,
arity checking, fuel charge, frame stack, stack-slot quota, and closure
upvalues as bytecode `Call`. When the callback returns, the VM resumes the
continuation. It either requests the next callback or completes into its saved
return target.

Continuation nesting is allowed: a callback may call another Array core
function. Each active operation has one bounded continuation record; iteration
count does not grow the frame or continuation depth.

Synchronous native callbacks may also be used as Array callbacks. Their result
resumes the continuation immediately without recursively entering `Vm`.

## Purity and builders

The source Array, callback, accumulator values, and returned Arrays remain
immutable XL values. `map`, `filter`, and `flat_map` may use a mutable Rust
builder internally, but it is unreachable from XL and is converted exactly
once into a new immutable runtime Array.

`filter` retains existing element references. `flat_map` retains references to
the returned inner elements. No legacy `Value` export or local/persistent heap
copy is performed on the hot path.

## Fuel and quotas

Invoking a core function consumes the ordinary one call unit. Each callback
invocation consumes another ordinary call unit, regardless of whether the
callback is bytecode or native. Array traversal itself does not assign a
virtual CPU price per element; this preserves RFC 0010's control-flow fuel
model.

Output builders charge the execution account for every retained output slot
before growing. Final heap allocation does not charge those slots a second
time. `fold` allocates no collection output. All callback allocations use the
same account.

Callback frames count against the ordinary stack-slot limit. Active native
continuation nesting counts against call depth even when a tail call has
removed its initiating bytecode frame, so nested core operations cannot create
an unbounded host-side chain. Iteration within one operation does not increase
continuation depth, reset, or fork any quota.

## Static analysis

The existing focused checker infers the imported Dict shape and fixed function
arities. Generic relationships such as `Array<A> -> (A -> B) -> Array<B>` are
not added to the type calculus by this RFC; unresolved parameter and result
types remain `Any`.

Because tool-stage and program-stage execution share the VM, closed Array
computations may use these functions in metadata expressions subject to the
ordinary module initialization quota.

## Diagnostics and traces

Errors include:

- a non-Array collection argument;
- a non-function callback;
- callback arity mismatch;
- non-boolean `filter` result;
- non-Array `flat_map` result;
- fuel, stack, call-depth, or allocation exhaustion;
- any error raised by the callback.

Callback failures retain the callback's ordinary frames and the initiating XL
call site. A continuation contributes at most one logical core-operation frame
to a trace, independent of the number of processed elements. Successful
iterations do not accumulate trace history.

## Rejected alternatives

### Add Array methods or collection syntax

XL currently has no nominal method dispatch, and `Array(T)` already denotes a
type-metadata constructor. An explicit module keeps operations ordinary values
without introducing method or comprehension semantics.

### Put every function in the global prelude

An imported core namespace makes capability origin visible and avoids growing
a privileged global name set.

### Implement higher-order operations in Rust by recursively entering `Vm`

This fragments execution accounting and makes nested callbacks depend on the
host call stack. VM-managed continuations keep one scheduler and one set of
runtime invariants.

### Expose mutable builders to XL

Builders would add observable mutation and escaping lifetime rules solely for
an optimization. They remain trusted implementation state.

### Add primitive `ArrayGet` and build all operations in XL

It would permit a small pure library, but constructing immutable Arrays one
element at a time would either be quadratic or require exposing a builder. The
continuation boundary is also needed by other future higher-order core
operations.

## Deferred work

- Dict operations and merge policies;
- general iterators, generators, lazy sequences, and transducers;
- indexes in callbacks;
- parallel collection evaluation;
- public native suspension or continuation APIs;
- precise generic static signatures for core functions;
- source-visible implementations for standard-library wrappers;
- core module manifests, version selection, and dependency-list reporting.

## Implementation plan

1. Add reserved core-module resolution and publish `core:array` into the shared
   persistent world.
2. Represent trusted Array functions distinctly from synchronous native
   callbacks while retaining one runtime `Func` category.
3. Generalize frame return targets and centralize callable dispatch for normal,
   tail, native, and continuation callback calls.
4. Implement bounded Array continuation states and allocation-charged builders.
5. Add tool/runtime bindings and focused static arity behavior.
6. Add unit, module, quota, trace, nested-callback, and end-to-end JSON Array
   transformation tests.

## Acceptance criteria

1. `import arrays from "core:array"` resolves without filesystem access and
   exports exactly the five specified functions.
2. `length`, `map`, `filter`, `flat_map`, and left `fold` produce deterministic
   results with the documented order and empty-Array behavior.
3. Bytecode and synchronous native functions both work as callbacks.
4. A callback may invoke another Array higher-order function without recursive
   VM entry or split quota accounts.
5. Iterating more than the physical call-depth limit does not grow frame or
   continuation depth per element.
6. The core call and every callback consume ordinary call fuel.
7. Output growth enforces allocation quota without charging final Array slots
   twice.
8. Type, arity, callback-result, fuel, stack, allocation, and callback failures
   retain useful source origins and bounded traces.
9. Tool-stage closed computations can use `core:array` through the same VM.
10. Existing language, heap, module, quota, and CLI behavior remains unchanged.

## Implementation result

`core:array` is resolved before filesystem modules and published once per
module-loading world. Its five functions operate directly on runtime Array
handles. Higher-order operations use VM-owned continuation records and the
ordinary call dispatcher, so bytecode closures, synchronous native functions,
nested Array operations, fuel, allocation accounting, call depth, and traces
all retain one execution model without recursive VM entry.

The implementation charges one call unit for the core operation and one for
each callback. Output slots are charged as builders grow and are not charged
again when the final immutable Array is installed in the local heap. Tests
cover ordering, empty inputs, nested operations, native callbacks, 1,500-item
iteration without frame growth, exact fuel and allocation boundaries,
tool-stage evaluation, boundary errors, callback traces, and continuation
depth exhaustion.

As planned, the focused checker currently exposes the module's exact Dict
shape and function arities while generic input/result relationships remain
`Any`. Core modules are also not yet included in filesystem dependency
reporting; manifests and versioned core dependency identities remain deferred.
