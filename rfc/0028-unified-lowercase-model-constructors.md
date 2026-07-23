# RFC 0028: Unified lowercase model constructors

- Status: Implemented
- Depends on: RFC 0026, RFC 0027
- Supersedes: the uppercase `Struct` and `Union` prelude APIs from RFC 0003

## Summary

XL removes the legacy `Struct(fields)` and `Union(variants)` prelude functions.
The normalized decorator-compatible model API is exactly:

```text
struct(ctx, fields)
enum(ctx, variants)
union(ctx, variants)
```

Each function accepts the RFC 0027 context protocol, normalizes its root and
members through the flat WithAttributes protocol, and produces ordinary
TypeMetadata. This is an intentional breaking change with no deprecated aliases.

## Surface forms

Recommended declarations are:

```xl
@struct
type User = {
    name: String,
};

@enum
type Result = {
    Ok: User,
    Err: String,
};

@union
type Scalar = [Int, Float, String];
```

Explicit construction supplies `'None`:

```xl
struct('None, {name: String})
enum('None, {None: 'None, Some: Int})
union('None, [Int, String])
```

All three functions accept only `'None` or the exact Type decorator context
`{kind: 'Type, name: String}`.

## Normalized Union

`union(ctx, variants)` requires a non-empty Array of valid, optionally
attributed TypeMetadata. Its result is equivalent to:

```xl
attributes.normalize({
    kind: 'Union,
    variants: variants.map(attributes.normalize),
})
```

Canonical output therefore contains exactly one root wrapper and exactly one
wrapper around each Array element. Existing attributes and inner rich-value
locations survive flattening. The supplied Array is never mutated.

Union remains an anonymous alternative of types. Enum remains a named mapping
from tags to unit markers or payload types. They are not aliases and retain
their existing TypeDescriptor semantics.

## Removal and migration

The following names cease to exist in the prelude:

```text
Struct
Union
```

There was no uppercase `Enum` prelude function. Canonical metadata kinds remain
the Atoms `'Struct`, `'Enum`, and `'Union`; only constructor bindings change.
Hand-written ordinary metadata Dicts remain valid.

Repository sources migrate mechanically:

```xl
type T = Struct(fields);
type T = Union(variants);
```

becomes:

```xl
@struct type T = fields;
@union type T = variants;
```

Inside ordinary functions, explicit construction uses `struct('None, fields)`
or `union('None, variants)`.

## Rationale

Keeping uppercase and lowercase constructors would expose two APIs with
different attribute invariants. The uppercase functions preserve optional raw
wrappers, while lowercase functions guarantee a normalized graph. XL is still
experimental, so removing ambiguity now is cheaper and clearer than carrying
compatibility aliases.

Lowercase functions also make decorator and explicit construction one semantic
operation. Metadata remains ordinary data; capitalization no longer signals a
privileged runtime constructor.

## Scope

This RFC does not rename `Atom`, `Array`, `Tuple`, or `Fn`. They are focused
TypeMetadata constructors rather than normalized named-model constructors.
Whether they should gain lowercase decorator-compatible forms is deferred.

## Acceptance criteria

1. `union` is an ordinary two-argument VM-managed prelude Func.
2. `@union` and `union('None, variants)` share identical implementation.
3. Union roots and all Array variants have exactly one WithAttributes wrapper.
4. Existing variant attributes and inner locations survive normalization.
5. Empty Arrays, invalid contexts, malformed wrappers, and non-Type variants
   are rejected with useful locations.
6. Union construction charges the active allocation quota.
7. `Struct` and `Union` are absent from tool-stage and runtime preludes.
8. All repository XL sources, examples, tests, and current documentation use
   lowercase normalized constructors.
9. Canonical metadata Dicts and TypeDescriptor kinds remain unchanged.
10. Existing `struct` and `enum` behavior remains unchanged.

## Implementation plan

1. Extend the VM-managed model constructor family with Union normalization.
2. Remove uppercase prelude registration and dead native callbacks.
3. Migrate executable XL and current documentation to decorator or explicit
   lowercase construction.
4. Add removal, normalization, error, quota, and regression tests.

## Implementation result

`CoreModelFunction` now includes `Union`, published as the ordinary two-argument
prelude `union` Func beside `struct` and `enum`. Both decorator and explicit
calls enter the same VM-managed operation. It validates the shared context,
requires a non-empty Array, directly flattens and validates each variant through
`HeapView`, allocates one canonical wrapper per element, builds a fresh Array
and Union metadata Dict, and wraps the root. All generated slots and Dicts are
charged to the active allocation account; retained inner and attribute rich
values are not deep-exported or rebuilt.

The uppercase `Struct` and `Union` registrations and their synchronous native
callbacks have been removed. Attempts to use either name now produce the
ordinary unknown-binding diagnostic. Canonical metadata decoding and internal
`TypeDescriptor::Struct` and `TypeDescriptor::Union` remain unchanged, so
hand-written metadata data is compatible.

All executable XL in examples, CLI coverage, module fixtures, type-analysis
tests, and the current README now uses `@struct`, `@union`, or explicit
lowercase construction. Historical RFCs retain their original API descriptions
and this RFC records the superseding change.

Tests cover decorator and explicit Union construction, uniform root and member
wrappers, retained variant attributes, static Union annotations, empty and
invalid variants, malformed wrappers, allocation exhaustion, removal of both
uppercase names, existing Struct/Enum behavior, codec behavior, examples, and
CLI workflows.
