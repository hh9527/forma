# RFC 0019: Structured Debug Observation

- Status: Accepted for implementation

## Summary

XL gains an explicit debugging core module:

```xl
import debug from "core:debug";

debug.dbg(value)
debug.dbg_with("loaded", value)
```

Both functions emit one bounded structured debug event and return the exact
input runtime value. They may be used with uniform pipelines and explicit call
sections:

```xl
value |> debug.dbg
value |> debug.dbg_with\("loaded", _)
```

Debug observation is a trusted host boundary, not a general XL effect system.
The VM emits events through an injected sink. CLI execution renders them to
stderr; embedding APIs may capture or discard them. Sink behavior cannot alter
XL evaluation.

This RFC deliberately excludes JSON serialization, configurable pretty-print
policies, logging levels, tracing spans, filesystem output, and general I/O.

## Motivation

XL can now compose imported data through Array and Dict core functions, but
users cannot inspect intermediate pipeline values without changing the final
result. Returning debug text would disrupt composition, while letting native
code write process stderr directly would make the VM difficult to embed and
test.

A narrow observer boundary provides useful diagnostics without exposing host
I/O to XL code. The debug functions remain ordinary imported functions and the
sink remains an engine concern.

## Core module

`core:debug` is reserved and resolved before filesystem modules through the
same persistent-world cache as `core:array` and `core:dict`. It exports exactly:

```text
dbg(value)
dbg_with(label, value)
```

The label of `dbg_with` must be a String. The data argument is last so an
explicit section forms a configured unary pipeline stage:

```xl
debug.dbg_with\("normalized", _)
```

No global debug name or special syntax is introduced.

## Return identity

After successful event construction, both functions return the same
`RuntimeValue` bits they received. Composite values therefore retain their
local or persistent handles and functions retain identity. No promotion,
legacy `Value` export, validation, or deep copy occurs.

Debug observation is not part of XL value equality. The returned value behaves
as though the debug call were absent, except for ordinary call fuel and the
external event.

## Events and sinks

The embedding boundary exposes the conceptual types:

```text
DebugEvent {
    stage: Tool | Runtime,
    label: Option<String>,
    value: String,
    location: Option<Location>,
}

DebugSink.emit(event)
```

`DebugSink::emit` has no XL-visible result. A sink may write, capture, or ignore
an event. Sink failure, lock poisoning, closed output, or an embedding callback
decision must not become an XL branch or runtime error; the host may report its
own failure separately.

Existing convenience execution APIs use a discard sink. This prevents the Rust
library from unexpectedly writing its host process stderr. Explicit observed
module-load and runtime-execution entry points accept a sink. The CLI installs
a stderr sink for both module initialization and runtime execution.

Events are emitted synchronously in evaluation order. A sink must not retain
references into the XL stack or heaps; event fields are owned or compact copied
metadata.

## Stages

An event identifies whether it was emitted during:

- `Tool`: module initialization and type-metadata evaluation;
- `Runtime`: execution of the loaded root module/session.

Each module initialization has its own execution, but all initialization
events use the `Tool` stage in this RFC. Module identity remains available from
the source location when present.

The same core functions and formatter are used in both stages. Debug calls do
not change closed-world analysis or make external data available.

## Locations

The VM records the initiating call instruction's debug origin. When it maps to
a source `Location`, the event carries that location. Synthetic or unavailable
origins yield `None` rather than inventing a byte offset.

The VM does not own source text and does not convert locations to line/column.
Engine and CLI layers that own the shared `SourceDatabase` render locations as
`source:line:column`. Direct low-level VM embedding may use the raw location or
omit source rendering.

## Debug representation

Debug formatting reads runtime values directly through one `HeapView`. It
supports every runtime category:

- Int and Float use deterministic scalar representations;
- String uses escaped quoted text;
- Bytes uses an escaped byte representation;
- Atom includes its leading `'`;
- Array, Tuple, and Dict recursively show contained values;
- Dict fields use canonical shape order;
- Func shows an opaque function name/identity representation and never expands
  prototypes or upvalues;
- an internal up-link is resolved defensively, or shown as an internal marker
  if it is not ready.

Formatting never crosses the legacy `Value` export boundary. It detects active
composite handles and renders a repeated active handle as `<cycle>`.

The first version uses fixed implementation limits:

- maximum nesting depth: 8;
- maximum displayed items per collection: 32;
- maximum complete event value bytes: 4096.

Truncation is explicit using `...`. Limits are implementation constants rather
than user configuration in this RFC. The formatter must not panic on invalid
UTF-8 because all XL String/field/Atom text is already valid UTF-8.

## CLI rendering

The CLI writes events to stderr and final program values to stdout. A compact
event is rendered as:

```text
[debug runtime] path/main.xl:12:8 "loaded": {count: 3, state: 'Ready}
```

An unlabeled event omits the quoted label and colon. Tool events use
`[debug tool]`. Missing source locations omit the location segment.

Labels are escaped as strings so embedded newlines cannot forge additional log
records. Each event occupies one physical stderr line; control characters in
the value representation are escaped for the same reason.

## Fuel and quotas

Calling `dbg` or `dbg_with` consumes the ordinary one call fuel unit. Formatting
does not add traversal fuel, consistent with the control-flow fuel model.

The bounded host-side event buffer is observer memory, not an XL heap value,
and is not charged to the execution allocation quota. The debug function does
not allocate an XL String and returns its existing value reference. Fixed
format limits prevent a debug call from retaining or constructing output
proportional to an unbounded reachable graph.

## Static analysis

The focused checker sees the exact module Dict shape and function arities.
Result-type identity such as `dbg(T) -> T` may currently degrade to `Any`, as
with other generic core relationships. Runtime behavior always preserves the
value.

Tool-stage expressions may call debug functions. Their events are observations
only and cannot be consumed by metadata computation.

## Diagnostics

XL-visible failures are limited to ordinary module, function-arity, fuel, and
`dbg_with` label type errors. The latter retains the XL call origin and names
`core:debug.dbg_with`.

Formatter corruption caused by an invalid trusted heap edge is an internal
runtime error rather than silently returning a changed value. Ordinary cycles,
depth limits, item limits, and byte limits are successful truncated output.

## Rejected alternatives

### Call `eprintln!` inside the VM

It couples all embeddings and tests to process-global stderr and gives the host
no capture or suppression boundary.

### Return debug text

Returning a String breaks pipeline identity and turns observation into data
transformation. Future formatting functions may return text explicitly, but
`dbg` is an observer.

### Reuse `Value::Display` through deep export

Export may allocate and rejects internal cycles. Debugging must work on runtime
handles without changing heap topology or copying a large graph.

### Make debug output semantically pure

The returned value is unchanged, but an emitted event is intentionally
observable. Calling it pure would obscure evaluation-order behavior. The
effect remains confined to a trusted core function and host sink.

### Add JSON stringify at the same time

JSON has a narrower value domain, rejection policies, exact escaping rules, and
an output value allocation contract. It deserves a separate RFC after the
debug observer is stable.

## Deferred work

- `core:json.stringify`;
- configurable formatting limits and pretty printing;
- structured logging levels and fields;
- trace spans, profiling, breakpoints, and debugger protocols;
- source snippets in debug output;
- user-defined format/display protocols;
- asynchronous or fallible sinks;
- a general XL effect capability system.

## Implementation plan

1. Add `core:debug` and fixed-arity VM-managed debug function identities.
2. Add an owned debug event, stage, and non-fallible sink boundary with a
   discard default.
3. Thread the observer through module initialization and runtime VM execution
   without placing it in `CallContext` or XL-visible state.
4. Implement a bounded, cycle-safe `HeapView` formatter for every runtime value.
5. Preserve the input `RuntimeValue` as the return and validate only the label.
6. Add observed Engine/module APIs and CLI stderr rendering with source
   positions.
7. Add identity, ordering, stage, capture, truncation, cycle, label-error,
   tool-stage, quota, and CLI stdout/stderr tests.

## Acceptance criteria

1. `core:debug` resolves without filesystem access and exports exactly `dbg`
   and `dbg_with`.
2. Both functions emit one event and return the exact input runtime value.
3. `dbg_with` accepts only a String label and preserves configured pipeline
   behavior through `debug.dbg_with\("label", _)`.
4. Events preserve evaluation order and distinguish tool/runtime stages.
5. Every runtime value category has a deterministic, bounded representation;
   cycles and truncation never panic or deep-export.
6. Debug calls consume ordinary call fuel but allocate no XL result value.
7. Existing convenience embedding APIs discard events; observed APIs capture
   them without granting the sink access to stack or heap references.
8. CLI debug events go only to stderr with escaped labels/values and useful
   source positions; the final value remains on stdout.
9. Sink behavior cannot alter XL results or errors.
10. Existing language, module, core-library, VM, quota, and CLI tests remain
    unchanged.
