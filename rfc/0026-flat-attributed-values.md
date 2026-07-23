# RFC 0026: Flat attributed values

- Status: Proposed
- Depends on: RFC 0016, RFC 0022, RFC 0025

## Summary

XL standardizes a domain-neutral ordinary-data protocol for attaching opaque
attributes to any value. The `core:attributes` module constructs, normalizes,
and inspects one flat wrapper shape:

```xl
{
    kind: 'WithAttributes,
    inner: value,
    attributes: {
        "core:json.rename": "type",
    },
}
```

Decorators remain ordinary functions. They may use this protocol, transform a
value without attributes, or establish a different convention.

## Motivation

RFC 0025 deliberately made decorators syntax-directed function application
without assigning model semantics to them. Model libraries still need a common
way to retain annotations that later consumers, generators, and language tools
can scan. The representation must itself be legal XL data so that programs can
construct, transform, serialize, validate, and inspect model metadata without a
privileged Rust-only type.

Nested wrappers would make stacked decorators progressively harder to consume
and would make precedence depend on traversal details. This RFC therefore
defines a canonical flat representation and a small normalization library.

## Representation

An attributed value is a Dict with exactly these fields:

```text
kind        = 'WithAttributes
inner       = any XL value other than a WithAttributes wrapper
attributes  = Dict with String field names and arbitrary XL values
```

The shape is a protocol, not a new runtime value kind. Attribute names are
opaque stable Strings chosen by the producing function. They should use a
fully qualified namespace such as `core:json.rename`, `core:db.primary_key`,
or `vendor:acme.postgres.index`. The compiler never derives a name from an
import alias, filesystem path, Rust path, or internal binding identity.

Unknown attributes are preserved. Payload evolution belongs in the payload
when versioning is needed; it does not require encoding versions into every
attribute name.

Malformed Dicts whose `kind` is `'WithAttributes` are errors when consumed by
`core:attributes` or as TypeMetadata. They are not silently treated as plain
values.

## Core module

`core:attributes` exports ordinary native bindings:

```xl
native normalize: fn(Any) -> Any;
native add: fn(Any, Any) -> Any;
native get: fn(Any, String) -> Any;
native has: fn(Any, String) -> Atom;
native all: fn(Any) -> Any;
native strip: fn(Any) -> Any;
```

Their behavior is:

- `normalize(value)` returns a canonical wrapper. A plain value gets an empty
  attributes Dict; nested wrappers are flattened.
- `add(value, additions)` requires `additions` to be a Dict, normalizes
  `value`, and shallow-merges the additions. Addition keys override existing
  keys.
- `get(value, key)` returns `('Some, payload)` when the normalized attributes
  contain `key`, and `'None` otherwise.
- `has(value, key)` returns `'True` or `'False`.
- `all(value)` returns the normalized attributes Dict, or an empty Dict for a
  plain value.
- `strip(value)` recursively removes wrappers and returns the base value.

Normalization follows decorator evaluation order. The decorator nearest the
RHS executes first, so attributes added by an outer decorator are later and
win on duplicate keys.

All functions preserve ordinary rich-value locations on retained values.
New wrapper and merged Dict locations use the call origin. They obey the same
stack, allocation, and fuel accounting as other VM-managed core functions.

## TypeMetadata transparency

`WithAttributes` is meaningful metadata around a type, not itself a type
constructor. TypeMetadata decoding recursively strips valid wrappers before
interpreting the inner type:

```text
validate(WithAttributes(type, attributes), value)
    = validate(type, value)
```

The same transparency applies to annotation checking, assignability, derived
codecs, and nested positions such as Struct fields. Raw metadata is not
canonicalized away. For example, `Struct(fields)` validates wrapped field
types but stores the original supplied fields Dict, wrappers included. This
lets model generators and future LSP views recover attributes after semantic
type checking.

Attributes do not affect type equality or assignability in this RFC. A domain
library may interpret them as additional contracts when it explicitly scans
the raw metadata.

## Decorator use

The module is sufficient to build attribute decorators in XL:

```xl
import attributes from "core:attributes";

let rename = fn(name) {
    fn(ctx, value) {
        value |> attributes.add({"core:json.rename": name})
    }
};

@struct
type User = {
    @rename("type")
    ty: String,
};
```

`ctx` is available to the decorator but the attribute protocol does not store
it automatically. The decorator alone decides whether and how context affects
the transformed RHS.

## Rejected alternatives

### Runtime attribute slots

Adding an attribute pointer to every runtime value would make a library
convention affect the VM ABI, equality, copying, and heap promotion. Ordinary
Dict data is inspectable and sufficient.

### Nested `WithAttributes`

Nested wrappers preserve construction history but force every consumer to
define traversal and duplicate-key precedence. Flat normalization gives one
stable scanning shape.

### Compiler-generated keys

Deriving keys from decorator syntax or module resolution makes metadata
unstable under import aliases and packaging. Keys remain the decorator
implementation's responsibility.

### Attributes as static-only annotations

Discarding attributes after analysis conflicts with XL's premise that type
metadata is ordinary data and prevents runtime generators and validators from
using the same model.

## Deferred work

- standard domain attributes and model libraries;
- attribute-aware structural codecs;
- LSP display and navigation for evaluated attributes;
- policy for attributes that participate in domain-specific contracts;
- optimizer recognition of normalized wrappers.

## Acceptance criteria

1. `core:attributes` resolves without filesystem access and exposes the six
   declared functions through ordinary XL native bindings.
2. Plain and nested values normalize to exactly one flat wrapper.
3. `add` preserves unknown keys and gives precedence to additions.
4. `get`, `has`, `all`, and `strip` work for attributed and plain values.
5. Arbitrary values, not only TypeMetadata, may be wrapped.
6. Type checking and validation accept wrapped top-level and nested metadata.
7. Type constructors preserve wrappers in their raw metadata.
8. Malformed wrappers produce located, human-readable errors.
9. Core operations obey allocation quotas and preserve retained provenance.
10. Existing undecorated metadata and runtime behavior remain compatible.

## Implementation plan

1. Register declarative `core:attributes` bindings and VM-managed operations.
2. Implement direct heap-view wrapper inspection, flattening, merging, and
   construction without exporting values through the legacy representation.
3. Teach both runtime and legacy TypeMetadata decoders to transparently unwrap
   and validate the protocol.
4. Add core, decorator-integration, metadata-preservation, diagnostics, and
   quota tests.

## Implementation result

Pending.
