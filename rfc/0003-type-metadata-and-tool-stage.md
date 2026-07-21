# RFC 0003: Type Metadata and Tool-Stage Evaluation

- Status: Accepted for MVP
- Implementation: Pending

## Summary

This RFC introduces canonical `Type` metadata, type declarations evaluated by
the existing bytecode VM, value annotations, a focused structural checker, and
runtime validation driven by the same metadata.

The RFC exists to test XL's central hypothesis: useful higher-order type
computation can be ordinary pure language computation in a closed tool stage,
without a second type-level evaluator.

## Two execution stages

The toolchain creates a tool-stage VM with a deterministic instruction budget.
It evaluates declarations in source order. Static source and data modules are
available; external IO and effects are not.

The program-stage VM executes the compiled result after analysis. Both stages
use the same bytecode instructions, call semantics, values, and instruction
budget implementation.

## Type declarations

A type declaration binds a metadata value:

```text
type User = Struct({
    name: String,
    age: Int,
});
```

The right side is an ordinary XL expression that must evaluate at tool stage to
valid canonical Type metadata. It is not a separate type-expression grammar.

Ordinary functions can compute metadata:

```text
fn Optional(item) {
    Union([
        Atom('None),
        Tuple([Atom('Some), item]),
    ])
}

type OptionalInt = Optional(Int);
```

`Optional` is an ordinary closure compiled to the same bytecode used at program
stage. The type analyzer does not reimplement its body.

## Canonical metadata protocol

Every Type is an immutable Dict with a required `kind` field. The MVP protocol
is equivalent to:

```text
Any                         = {kind: 'Any}
Int                         = {kind: 'Int}
Float                       = {kind: 'Float}
String                      = {kind: 'String}
Bytes                       = {kind: 'Bytes}
Atom(tag)                   = {kind: 'Atom, tag: tag}
Array(item)                 = {kind: 'Array, item: item}
Tuple(items)                = {kind: 'Tuple, items: items}
Struct(fields)              = {kind: 'Struct, fields: fields}
Union(variants)             = {kind: 'Union, variants: variants}
```

`fields` is a Dict from field name to Type metadata. `items` and `variants` are
Arrays. The toolchain validates metadata recursively and rejects unknown kinds,
missing fields, extra fields, invalid child values, and empty unions.

The primitive metadata values and constructor functions are a tool-stage core
prelude. Constructors are ordinary native `Func` values, not special bytecode
instructions. A native call consumes one instruction in addition to its caller
instruction and shares the enclosing budget.

## Annotations and checking

An immutable binding may carry a metadata expression:

```text
let user: User = {
    name: "Ada",
    age: 36,
};
```

The annotation expression is evaluated at tool stage and must produce Type
metadata. The checker infers literal, Array, Tuple, Dict, block, conditional,
and match-result shapes. Values that cannot be usefully inferred become `Any`.

Assignability in the MVP is structural:

- `Any` accepts and is assignable to every type;
- primitive kinds must match exactly;
- an Atom type accepts only its named atom;
- Arrays check every statically visible item against their element type;
- Tuples require equal lengths and corresponding item types;
- Structs require every declared field and reject undeclared fields;
- Unions accept a value assignable to at least one variant.

The MVP deliberately uses exact Struct checking. Optional or open fields are
expressed by future metadata functions and protocol additions rather than
implicit behavior.

An annotation mismatch is a toolchain error and prevents `check`/`run` in the
MVP CLI. The dynamic VM itself remains capable of running unannotated code.

## Runtime validation

The core prelude provides:

```text
validate(type, value) -> ('Ok, value) | ('Err, message)
```

It interprets the same canonical metadata protocol used by the static checker.
Validation recursively checks actual runtime values. It never mutates or
normalizes the input.

Normalization policy remains an ordinary library concern. This RFC proves the
shared-metadata path with validation before defining correction/default rules.

## Analysis result

Analysis exposes structured information for future LSP and CLI consumers:

- each declared type's canonical metadata and decoded structural description;
- each value binding's inferred type;
- the program result type;
- tool-stage diagnostics.

No JSON diagnostic format or LSP protocol is required until RFC 0004.

## Erasure and retention

A type declaration used only by annotations is omitted from program bytecode.
If ordinary runtime code refers to the type name, the compiler retains its
already-computed canonical metadata as a constant. The type computation itself
is not rerun at program stage.

Core type constructor functions are not automatically exposed at program stage
unless retained code refers to them.

## Native functions

`Func` gains a trusted native-function variant so the small core prelude can
construct and interpret runtime values. Native functions:

- have a stable diagnostic name and fixed arity;
- receive immutable arguments;
- may use private VM facilities such as shape interning;
- return a value or a runtime error;
- cannot perform external IO in this RFC.

Native functions are an implementation boundary, not user-defined FFI.

## Deferred work

- annotations on parameters and function results;
- polymorphic static function signatures;
- narrowing, exhaustiveness, and flow-sensitive checking;
- recursive Type metadata;
- row polymorphism, traits, HKT, and dedicated higher-order type syntax;
- normalization/default metadata protocol;
- user-defined native functions or effects;
- proving termination instead of enforcing a resource budget.

## Implementation plan

Extend the AST and parser with `type` declarations and `let` annotations. Add a
native callable representation, Type metadata encoder/decoder, core prelude,
sequential tool-stage analyzer, structural inference/checking, and compiler
support for erasing or retaining resolved type bindings.

## Acceptance criteria

1. Primitive and composite metadata round-trip between canonical XL values and
   the toolchain's structural view.
2. A user-written closure computes Type metadata in the tool-stage bytecode VM.
3. Invalid metadata and tool-stage budget exhaustion produce deterministic
   frontend diagnostics.
4. Correct structural annotations pass and incorrect annotations fail before
   program execution.
5. `validate` uses the same metadata to accept and reject runtime values.
6. Types used only as annotations are absent from program constants.
7. A type explicitly referenced by runtime code is retained as a canonical
   value without rerunning its constructor expression.
8. Analysis returns declared and inferred types in a structured Rust API.
9. Existing dynamic programs continue to compile and run.
10. Workspace tests and strict Clippy pass.

