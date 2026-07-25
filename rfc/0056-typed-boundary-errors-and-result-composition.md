# RFC 0056: Typed boundary errors and Result composition

- Status: Accepted
- Depends on: RFC 0020, RFC 0021, RFC 0031, RFC 0032, RFC 0036, RFC 0052, RFC 0053, RFC 0055

## Summary

XL gives codec and validation failures public structural types and completes
the small Result surface needed to compose typed external-data boundaries:

```xl
import codec from "core:codec";
import result from "core:result";

@struct type User = {name: String};

codec.decode(User, input)
|> result.map(fn(user) { user.name })
|> result.map_err(codec.format_error)
```

The boundary contracts become:

```xl
codec.decode:
    for(A) Fn(TypeOf(A), Any) -> Result(A, codec.DecodeError)
codec.encode:
    for(A) Fn(TypeOf(A), A) -> Result(Any, codec.EncodeError)
validate:
    for(A) Fn(TypeOf(A), Any) -> Result(A, ValidationError)
```

`DecodeError`, `EncodeError`, and `ValidationError` are semantic names for the
same first public boundary-diagnostic shape:

```xl
@struct type BoundaryError = {
    message: String,
    data: Any,
    rule: Any,
};
```

XL structural typing intentionally makes the three types interchangeable in
this RFC. Their names document which boundary produced a failure and leave
room for compatible evolution without introducing nominal error classes.

## Motivation

RFC 0055 preserves the successful instance type through `TypeOf(A)`, but the
codec error parameter remains `Any` and validation still returns `String`.
That loses information at the exact boundary where applications need to
format, inspect, transform, or propagate failures.

The runtime codec already returns a deterministic Dict containing `message`,
the rejected `data`, and the responsible `rule`, with source locations carried
by rich values. This RFC exposes that existing representation rather than
inventing a second diagnostic model. Validation adopts the same representation
so callers can handle both boundaries uniformly.

Typed boundaries also remove the reason RFC 0053 kept `result.unwrap` dynamic.
Its runtime behavior already preserves the Ok payload, so its declaration can
now express that relationship directly.

## Public error model

`core:codec` exports two TypeMetadata values in addition to its functions:

```xl
@struct type DecodeError = {message: String, data: Any, rule: Any};
@struct type EncodeError = {message: String, data: Any, rule: Any};

native decode:
    for(A) Fn(TypeOf(A), Any) -> Result(A, DecodeError);
native encode:
    for(A) Fn(TypeOf(A), A) -> Result(Any, EncodeError);

def format_error:
    Fn(DecodeError) -> String
= fn(error) { error.message };
```

Because `DecodeError` and `EncodeError` are structurally equal,
`format_error` accepts either one. It returns the deterministic message already
produced by the runtime and does not discard locations from the original error
value; it merely projects a String for display-oriented consumers.

The prelude exports:

```xl
@struct type ValidationError = {message: String, data: Any, rule: Any};

native validate:
    for(A) Fn(TypeOf(A), Any) -> Result(A, ValidationError);
```

On validation failure, `data` is the rejected value and `rule` is the supplied
TypeMetadata value. Success still returns the original input value without
copying or normalization.

The initial `message` remains the existing deterministic logical-path String.
For example, `value.user.name must be String` remains a String rather than
being parsed into path segments. A typed path algebra should be based on real
consumer requirements and is deferred.

## Result composition

`core:result` refines and extends its ordinary generic API:

```xl
native unwrap: for(A, E) Fn(Result(A, E)) -> A;

def flat_map:
    for(A, E, B) Fn(Result(A, E), Fn(A) -> Result(B, E)) -> Result(B, E);
```

`flat_map` calls its callback only for `Ok` and preserves `Err` unchanged. It
uses the same name as `core:option.flat_map`; no method or trait dispatch is
introduced. Existing `map`, `map_err`, `unwrap_or`, and `is_ok` contracts are
unchanged.

`unwrap` remains an explicit runtime-failure boundary. A structured error uses
its `message`, `data`, and `rule` fields to retain the existing dual-source
diagnostic. Only its static contract changes: a known `Result(A, E)` now
returns `A`, while `Any` remains accepted through XL's gradual boundary and is
checked at runtime.

## Runtime and provenance semantics

Codec failure representation is unchanged. Validation changes its Err payload
from a String to a Dict with the public error shape. The Dict and its fields
retain compact rich-value locations:

- `data` refers to the rejected value and carries its data-side location;
- `rule` refers to the TypeMetadata argument and carries its rule-side location;
- `message` is located at the rejected value when such a location exists.

`result.unwrap` continues to recognize both legacy String errors and public
structured errors at runtime. This preserves compatibility for user-created
Results and previously compiled modules.

No exception hierarchy, stack unwinding rule, VM opcode, heap object kind, or
native ABI is added.

## Static semantics and module boundaries

The three error bindings have type `TypeOf({message: String, data: Any,
rule: Any})`. Their use in native declarations is trusted exactly like every
other declarative native capability.

The concrete structural error descriptors cross `ModuleInterface` and
workspace snapshots through the existing type graph. Member observation must
show typed error parameters rather than `Any` or `String`.

`format_error` and `flat_map` are ordinary XL definitions compiled with their
explicit generic contracts. Their implementation must not require privileged
Rust callbacks.

## Non-goals

- interface, trait, associated-type, or effect systems;
- nominal distinction among decode, encode, and validation errors;
- a general language-wide `Error` supertype;
- parsing message Strings back into structured paths;
- accumulating multiple failures or defining applicative validation;
- attaching host filesystem or network errors to this model;
- changing codec matching, normalization, or JSON representation rules.

## Implementation plan

1. export `DecodeError` and `EncodeError` metadata from `core:codec`;
2. refine codec native declarations and add ordinary `format_error`;
3. add `ValidationError` metadata to the runtime and static preludes;
4. return the structured payload from validation failures;
5. refine `result.unwrap` and add ordinary `result.flat_map`;
6. preserve legacy unwrap handling for String Err payloads;
7. publish all new types through module and workspace interfaces;
8. add runtime, static-observation, provenance, and composition tests.

## Acceptance criteria

1. `codec.DecodeError` and `codec.EncodeError` are observable as precise
   `TypeOf` witnesses;
2. `decode(User, input)` has type `Result(User, DecodeError)`;
3. `encode(User, user)` has type `Result(Any, EncodeError)`;
4. `validate(User, input)` has type `Result(User, ValidationError)`;
5. all three failures contain `message`, `data`, and `rule` with stable runtime
   values and useful source locations;
6. `codec.format_error` accepts codec and validation errors by structural
   compatibility and returns their message;
7. `result.flat_map` preserves the error type and transforms the Ok type;
8. `result.unwrap` returns the statically known Ok type;
9. legacy String Err values still unwrap into runtime failures;
10. workspace tests, formatting, clippy, and strict static checks pass.

## Deferred work

- a structured `ValuePath` with field, index, and variant segments;
- diagnostic accumulation and `validate_all`-style APIs;
- stable error codes and machine-oriented expectation payloads;
- host I/O errors and source-provider diagnostics;
- nominal or capability-based error abstraction.

## Rejected alternatives

### Keep validation errors as String

This preserves a smaller value but throws away the rejected data and rule
locations precisely when validation is used as an application boundary. It
also forces codec and validation callers into different error pipelines.

### Expose only one global Error type

A single global name obscures which API owns the contract and prematurely
claims a language-wide error abstraction. Structural aliases provide uniform
handling while keeping ownership explicit.

### Add a structured path immediately

The runtime currently produces deterministic path Strings across Struct,
Enum, attributes, flattening, and recursion. Converting every failure site to
a new path value is a separate behavioral change and should be motivated by
concrete consumers rather than bundled into type exposure.
