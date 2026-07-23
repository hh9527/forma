# RFC 0025: Contextual functional decorators

- Status: Implemented
- Depends on: RFC 0006, RFC 0017, RFC 0022, RFC 0024

## Summary

XL adds Python-shaped decorators as a syntax-level, domain-neutral RHS
transformation. The compiler derives a small ordinary-data context solely from
the decorated syntax node and invokes an ordinary XL function.

The first implementation supports type bindings and Dict fields:

```xl
@f(1)
type a = rhs;

{
    @g
    field: rhs,
}
```

Their semantics are:

```xl
type a = f(1)({ kind: 'Type, name: "a" }, rhs);

{
    field: g({ kind: 'Field, name: "field" }, rhs),
}
```

Decorators do not imply attributes. A decorator may attach conventional
metadata, transform the RHS, validate it and return it unchanged, or perform
any other pure computation allowed at that position.

## Syntax

```text
decorator      := "@" decorator_path [arguments]
decorator_path := Identifier ("." Identifier)*
```

One or more decorators may immediately precede a type binding or Dict field.
The restricted path-plus-arguments grammar gives an unambiguous end without
making newline significant. Arbitrary decorator expressions are deferred.

Decorators are retained as located CST and semantic AST nodes. They are not
discarded by eagerly rewriting source AST into calls.

## Context protocol

Context depends only on the target's syntax category:

```xl
{ kind: 'Type, name: <declared type name> }
{ kind: 'Field, name: <source Dict field name> }
```

Both records have the same fixed shape. `kind` is an Atom and `name` is a
String. The context carries the target location through normal rich-value
provenance, but file paths, spans, module identities, and compiler BindingIds
are not exposed as fields.

The compiler never creates domain categories such as JsonField, ProtoMember,
or DatabaseColumn. Such meanings belong to decorator functions and libraries.

## Application

A bare decorator is itself the transforming function:

```xl
@f
type a = rhs;

// f(ctx, rhs)
```

A configured decorator evaluates its expression first:

```xl
@f(1)
type a = rhs;

// f(1)(ctx, rhs)
```

Multiple decorators use Python nesting order. The decorator nearest the target
runs first:

```xl
@outer
@inner(1)
type a = rhs;

// outer(ctx, inner(1)(ctx, rhs))
```

Every decorator receives an equivalent immutable context. Decorator
expressions and applications obey ordinary lexical scope, function ABI,
locations, quota accounting, and runtime errors.

## Position-specific validation

The transformed result is checked only by the existing semantics of its
target:

- a decorated type binding must ultimately produce valid TypeMetadata;
- a decorated Dict field may produce any XL value;
- when such a Dict is consumed by `Struct`, Struct validates the resulting
  field values as usual.

The compiler does not require a decorator to produce attributes or preserve
the original value.

## Optional attribute convention

Libraries may conventionally represent annotated values as a flat wrapper:

```xl
{
    kind: 'WithAttributes,
    inner: value,
    attributes: {
        "core:json.rename": "type",
    },
}
```

Attribute keys are stable fully qualified Strings selected by decorator
implementations, never derived from import aliases. Nested wrappers should be
normalized into one attributes Dict with outer decorators taking precedence.
This RFC defines no mandatory attribute behavior and adds no domain keys.

## Scope restrictions

The first implementation rejects decorators on let, fn, decl/def, native, and
import bindings. It also does not add `Struct { ... }` syntax; field decorators
operate on the existing Dict literal used by `Struct({...})` and other models.

Decorators cannot rename a binding or Dict key, create bindings or imports, or
inspect AST/HIR. They receive only context data and the RHS value.

## Diagnostics and tooling

Unresolved paths, invalid factory results, arity errors, quota failures, and
invalid final TypeMetadata use ordinary diagnostics. Decorator application
origins point to the `@...` syntax, while the transformed value and target keep
their own locations.

Future LSP support can show both syntactic decorators and evaluated metadata
because decorators remain explicit AST data.

## Deferred work

- decorators on other binding and member categories;
- arbitrary decorator expressions;
- a uniform explicit-call convention in which decorator-capable functions
  accept `ctx | 'None` as their first argument, so `@struct type T = fields`
  and `struct('None, fields)` share one ordinary XL function;
- replacing the preferred surface use of built-in TypeMetadata constructors
  such as `Struct(fields)` with decorator-capable lowercase XL library
  functions; the canonical `{kind: 'Struct, fields}` data remains authoritative;
- standard WithAttributes constructors and scanning library;
- resolved HIR representation and decorator expansion views;
- LSP hover, definition navigation, and evaluated metadata display.

## Acceptance criteria

1. Decorators are lossless, located CST/AST structures.
2. Type and Field contexts have exactly `{ kind, name }` and ordinary values.
3. Bare, configured, qualified, and stacked decorators follow the specified
   application semantics.
4. Type decorators execute in the existing tool stage and share module quota.
5. Dict field decorators execute as ordinary expression computation.
6. Invalid targets and decorator failures retain useful source locations.
7. Existing undecorated syntax, metadata, quotas, and runtime behavior remain
   compatible.

## Implementation result

The Logos/Lelwel frontend now accepts located bare, configured, qualified, and
stacked decorators on type bindings and Dict fields. Lossless CST rules and
typed TypeBinding queries retain the original syntax. Semantic `BindingData`
and `DictFieldKind` retain `Decorator` records containing the callee,
arguments, configuration form, and location.

Lowering additionally builds the specified ordinary call expression around
the RHS. Context is an ordinary Dict with exactly `kind` and `name`; its Atom,
String, fields, and locations use the same AST/runtime representations as
hand-written XL. This lets the existing analyzer, compiler, VM, function ABI,
quota account, and debug origins execute decorators without a dedicated
runtime mechanism.

Type decorators run during existing metadata evaluation and their final result
is checked as TypeMetadata. Field decorators run as normal expression code.
Invalid type results point at the outer decorator application. Unsupported
binding targets remain syntax errors. No WithAttributes constructor, key, or
domain behavior was added.

Tests cover lossless reconstruction, retained decorator AST, bare/configured
and qualified application, Python nesting order, Type/Field contexts, shared
tool fuel, invalid metadata origins, and unsupported targets.
