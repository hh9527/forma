# RFC 0030: JSON model attribute vocabulary

- Status: Proposed
- Depends on: RFC 0025, RFC 0026, RFC 0027

## Summary

`core:json` adds ordinary decorator functions that attach a stable initial
vocabulary of JSON model attributes:

```xl
@json.rename("externalName")
@json.rename_all('CamelCase)
@json.flatten
@json.default(value)
@json.skip_serializing_if('None)
```

The functions produce only flat RFC 0026 WithAttributes data. They do not
perform encoding, change binding names, or rely on compiler-recognized syntax.

## Exported functions

`core:json` additionally exports:

```xl
native rename: fn(String) -> fn(Any, Any) -> Any;
native rename_all: fn(Atom) -> fn(Any, Any) -> Any;
native flatten: fn(Any, Any) -> Any;
native default: fn(Any) -> fn(Any, Any) -> Any;
native skip_serializing_if: fn(Atom) -> fn(Any, Any) -> Any;
```

Configured functions return ordinary two-argument decorator closures. The two
arguments are the RFC 0025 `ctx` and RHS value. `flatten` is directly usable as
a bare decorator. Explicit calls remain possible by supplying a context.

## Attribute keys and payloads

The canonical keys are:

| Key | Payload | Intended target |
| --- | --- | --- |
| `core:json.rename` | String | Struct field or Enum variant |
| `core:json.rename_all` | Atom | Struct or Enum root |
| `core:json.flatten` | `'True` | Struct field |
| `core:json.default` | arbitrary XL value | Struct field |
| `core:json.skip_serializing_if` | Atom policy | Struct field |

RFC 0030 accepts only `'CamelCase` for `rename_all`. Later naming policies can
extend the Atom vocabulary without changing the key.

`skip_serializing_if` accepts these deterministic policies:

- `'None`: skip the XL Option unit value `'None`;
- `'False`: skip the Atom `'False`;
- `'Empty`: skip empty String, Array, or Dict values.

The payload is a policy Atom rather than a Func in this RFC. Calling arbitrary
XL predicates from inside the current VM-managed codec requires a resumable
native continuation; standard policies establish the user-facing behavior
without hiding an unmetered callback path. Function predicates are deferred.

`default(value)` stores the supplied rich XL value directly. Deserialization in
the next RFC copies that value into a missing field without invoking code.
Default factories are deferred for the same continuation reason.

## Normalization

Every decorator behaves like:

```text
attributes.add(rhs, {canonical_key: payload})
```

Existing wrappers are flattened. Existing unknown keys and payload locations
are retained. The decorator's key overrides the same key already present on the
RHS, matching outer-decorator precedence. Context is accepted but not stored.

Stacking remains Python ordered:

```xl
@json.default("guest")
@json.rename("displayName")
name: String
```

produces one wrapper containing both keys.

## Validation boundary

Decorator calls validate their immediate configuration types and policy atoms.
They do not validate target placement. A field-only attribute may be attached
to another value as ordinary metadata; attribute-aware codec planning reports
misplaced or conflicting attributes with the attribute payload as the rule
location.

This separation keeps decorators domain-neutral transformations and lets the
consumer own semantic interpretation.

## Diagnostics and quota

Invalid naming policies and skip policies fail at the configured decorator
call. Wrapper and attributes Dict allocation is charged to the active tool or
runtime quota. Captured configuration values keep their original rich
locations. Applying a decorator preserves the RHS inner location.

## Deferred work

- function-valued defaults and skip predicates;
- additional rename_all cases;
- deserialize-only and serialize-only rename values;
- Enum tagging attributes;
- JSON Schema annotations such as title, description, examples, and format;
- aliases and unknown-field policy.

## Acceptance criteria

1. All five functions are ordinary exports from `core:json`.
2. Configured factories return ordinary two-argument Func values.
3. Decorator application produces exactly one flat WithAttributes wrapper.
4. Stacked decorators preserve unrelated and unknown keys.
5. Same-key outer decorators override inner decorators.
6. Payloads and RHS inner values preserve rich locations.
7. Invalid rename_all and skip policies fail with useful call locations.
8. Factory and wrapper allocations obey the active quota.
9. Decorators work on Struct roots and fields through RFC 0025 syntax.
10. No codec behavior changes in this RFC.

## Implementation plan

1. Extend the declarative `core:json` interface and native registry.
2. Add configured decorator closure variants to the existing VM-managed JSON
   native family.
3. Reuse direct HeapView flattening and canonical wrapper allocation.
4. Add raw metadata, ordering, validation, location, and quota tests.

## Implementation result

Pending.
