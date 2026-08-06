# RFC 0138: Core prelude system module

- Status: Proposed
- Depends on: RFC 0137

## Summary

Forma adds a real built-in system module at `core/prelude`, backed by:

```text
crates/forma-core/modules/core/prelude.forma-sys
```

It owns the public declarations of the ordinary callable prelude capabilities:

```forma
native struct: Fn(Any, Any) -> Type;
native enum: Fn(Any, Any) -> Type;
native union: Fn(Any, Any) -> Type;
native validate: for(A) Fn(TypeOf(A), Any) -> Result(A, BlameError);

{ struct, enum, union, validate }
```

The module receives a stable reserved native module ID and is registered by
the resolver like every other runtime built-in. Its privilege derives from
registration, not the `core/` spelling.

## Bootstrap staging

The module is compiled using the RFC 0137 bootstrap artifact. During this RFC,
the existing implicit bindings remain available as a compatibility bootstrap
input; the native declarations in `core/prelude` shadow them with the same
implementations. This makes the module independently importable and validates
its authored interface before RFC 0139 changes the implicit projection source.

This staging is intentional: module registration, execution, and explicit
imports become testable before strict and recoverable analysis are switched
together.

## Semantics

Explicit imports expose the ordinary module record:

```forma
import prelude from "core/prelude";
prelude.validate(Int, 1)
```

The imported functions use the existing VM-managed implementations. Model
normalization, attributes, validation results, blame provenance, quotas, and
fuel behavior do not change.

## Acceptance criteria

1. `core/prelude` resolves as a RuntimeSystem built-in with a stable reserved
   module ID.
2. Its declarations are sourced from an embedded `.forma-sys` file.
3. Every declared native symbol has exactly one registered implementation and
   matching arity.
4. Explicit imports expose `struct`, `enum`, `union`, and generic `validate`.
5. Explicit calls preserve existing model and validation behavior.
6. Host registration cannot replace `core/prelude`.
7. Existing implicit calls remain compatible pending RFC 0139.
