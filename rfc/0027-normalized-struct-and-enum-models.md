# RFC 0027: Normalized Struct and Enum models

- Status: Implemented
- Depends on: RFC 0022, RFC 0025, RFC 0026

## Summary

XL adds decorator-compatible lowercase `struct` and `enum` model constructors.
They produce ordinary canonical TypeMetadata while establishing a stronger
model invariant: the model root and every field or variant are represented by
exactly one flat `WithAttributes` wrapper.

```xl
@struct
type User = {
    name: String,

    @json.rename("type")
    ty: String,
};

@enum
type Result = {
    Ok: User,
    Err: Error,
};
```

`Enum` is one stable metadata kind for both unit-only and payload-carrying
variants. Whether all variants are units is a computed property, not a change
to the canonical kind.

## Constructor protocol

Both constructors are ordinary prelude functions with two arguments:

```text
struct(ctx, fields)
enum(ctx, variants)
```

Decorator application supplies the RFC 0025 context:

```xl
@struct
type User = fields;

# struct({kind: 'Type, name: "User"}, fields)
```

Explicit construction uses `'None` when no syntax target exists:

```xl
struct('None, fields)
enum('None, variants)
```

The first implementation accepts only `'None` or the exact Type context Dict
`{kind: 'Type, name: String}`. The context is not automatically stored as an
attribute. A later library constructor may use the name, but normalization
itself is domain-neutral.

The existing uppercase TypeMetadata constructors remain compatible. They are
lower-level functions and do not promise normalized model attributes. Removing
or de-emphasizing them is deferred until ordinary XL library bootstrapping can
replace all prelude construction duties.

## Normalized Struct metadata

`struct(ctx, fields)` requires a Dict whose values are valid, optionally
attributed TypeMetadata. It produces the equivalent of:

```xl
attributes.normalize({
    kind: 'Struct,
    fields: map_values(fields, attributes.normalize),
})
```

Its canonical output therefore has this shape:

```xl
{
    kind: 'WithAttributes,
    inner: {
        kind: 'Struct,
        fields: {
            name: {
                kind: 'WithAttributes,
                inner: String,
                attributes: {},
            },
        },
    },
    attributes: {},
}
```

Existing member wrappers are flattened and their attributes preserved. The
constructor creates a new fields Dict and never mutates the supplied value.

## Enum metadata

Canonical Enum metadata before its root wrapper is:

```xl
{
    kind: 'Enum,
    variants: {
        None: <normalized variant>,
        Some: <normalized variant>,
    },
}
```

Each variant's stripped inner value is one of:

- `'None`, denoting a unit variant;
- valid TypeMetadata, denoting one payload value.

For example:

```xl
@enum
type OptionInt = {
    None: 'None,
    Some: Int,
};
```

normalizes both `None` and `Some` entries into flat `WithAttributes` wrappers.
The `'None` marker is unambiguous: an Atom payload constraint is expressed as
TypeMetadata such as `Atom('None)`.

An Enum must contain at least one variant. Variant names are deterministic Dict
field Strings and become runtime Atom tags.

## Runtime representation

Runtime representation has exactly two forms:

```text
'Variant             # unit variant
('Variant, payload)  # payload variant
```

The outer tagged tuple always has two elements. Multiple positional values use
a Tuple payload; named values use a Dict payload:

```xl
('Color, (255, 128, 0))
('Move, {x: 1, y: 2})
```

Existing Atom and Tuple patterns already destructure these values. This RFC
adds no constructors, case syntax, discriminant integers, or runtime Enum
object kind.

## Type interpretation

`TypeDescriptor::Enum` retains the deterministic variant map and whether each
variant is unit or payload-carrying. Validation dispatches on the tag:

- a declared unit variant accepts only its matching Atom;
- a declared payload variant accepts only a two-element Tuple with its matching
  Atom in element zero and a payload satisfying its TypeMetadata in element one;
- unknown tags, missing payloads, and unexpected payloads are errors.

Assignable Enum types have the same variant names and unit/payload structure,
and corresponding payload types must be assignable. This initial invariant is
closed and exact; open variants and width subtyping are deferred.

The raw TypeMetadata graph remains authoritative. Static descriptors are only
an analysis projection and do not erase member attributes from runtime values.

## Attributes and locations

Root, field, and variant wrappers are ordinary RFC 0026 data. The constructors
preserve existing attribute payload values and their rich locations. Newly
created empty attributes Dicts and wrapper structure carry the constructor call
origin; each retained inner TypeMetadata or `'None` marker keeps its own origin.

Consumers of normalized model metadata can unconditionally call `all`, `get`,
or `strip` without branching between plain and wrapped member shapes. Unknown
attributes remain intact.

## Codec boundary

This RFC does not select an external representation for Enum. Unit variants
might map to strings, while payload variants might map to tagged objects,
adjacently tagged objects, or domain-specific forms. The derived codec reports
Enum as unsupported until a later RFC defines attribute-controlled encoding.
Core TypeMetadata checking and `validate` are fully supported now.

## Rejected alternatives

### Automatically choosing `Enum` or `TaggedUnion`

Changing `kind` when one variant gains a payload makes model identity unstable
and forces every consumer to support two top-level protocols. One `Enum` kind
matches Rust's broad enum model; unit-only classification is computed data.

### Optional member wrappers

Allowing both plain and attributed members pushes a union-shaped protocol into
every LSP and generator. Construction is the correct boundary at which to pay
the small normalization cost once.

### Extending every TypeMetadata record with an attributes field

This duplicates RFC 0026, couples attributes to type records, and excludes
attributing ordinary values. Flat wrappers remain the single protocol.

### Lowering Enum metadata permanently to Union

The existing type checker could validate a generated Union of Atom and Tuple
types, but generators would then have to infer whether an arbitrary Union was
intended as a model enum. `TypeDescriptor` may internally reuse logic, while
raw metadata retains `'Enum` and its variant map.

## Deferred work

- attribute-aware Enum codecs;
- generated variant constructor functions;
- variant-oriented match exhaustiveness diagnostics;
- open enums and width subtyping;
- generic model constructor signatures;
- moving lowercase constructors from the native prelude into an XL core module;
- normalization constructors for Array, Tuple, Union, and Function metadata;
- de-emphasizing or removing uppercase convenience constructors.

## Acceptance criteria

1. `@struct type T = fields` and `struct('None, fields)` share one ordinary
   two-argument prelude function.
2. `@enum type T = variants` and `enum('None, variants)` behave analogously.
3. Both roots and every member contain exactly one flat WithAttributes wrapper.
4. Existing attributes and rich inner locations survive normalization.
5. Struct annotations and validation accept normalized metadata.
6. Enum accepts unit and payload variants with the specified runtime forms.
7. Unknown tags and incorrect unit/payload shapes fail with useful diagnostics.
8. Static assignability compares Enum variants structurally and exactly.
9. Raw runtime metadata preserves normalized field and variant attributes.
10. Invalid contexts, empty Enums, malformed wrappers, and invalid member types
    are rejected.
11. Construction obeys the active stack and allocation quotas.
12. Existing uppercase constructors and undecorated TypeMetadata remain
    compatible.

## Implementation plan

1. Add VM-managed normalized-model native functions to the prelude.
2. Reuse direct HeapView wrapper inspection and canonical Dict construction for
   roots and members, charging all generated structure.
3. Add Enum to rich and legacy TypeMetadata decoding, display, validation,
   assignability, and mismatch traversal.
4. Preserve normalized raw metadata while using descriptors only as analysis
   projections.
5. Add decorator, explicit-call, normalization, validation, diagnostics,
   provenance, quota, and compatibility tests.

## Implementation result

The prelude now publishes lowercase `struct` and `enum` as VM-managed native
functions. They are ordinary two-argument `Func` values, so contextual
decorator calls and explicit `'None` calls use the same compiler, call ABI,
fuel, stack, allocation quota, and debug origins. Context validation accepts
only the specified two shapes.

Both constructors work directly against `HeapView`. They flatten existing
member wrappers, preserve inner and attribute rich values, allocate a canonical
wrapper for every member, build a fresh deterministic members Dict, and wrap
the resulting Struct or Enum metadata once at the root. No value crosses the
legacy deep-export boundary during construction. Every new wrapper, attributes
Dict, members Dict, and metadata Dict is allocation-charged.

`TypeDescriptor::Enum` retains a deterministic map from tag names to optional
payload descriptors. Rich and legacy TypeMetadata decoders accept `'Enum`,
transparently strip member wrappers, require at least one variant, and
distinguish the `'None` unit marker from payload TypeMetadata. Validation
accepts matching unit Atoms or two-element tagged Tuples and reports unknown
tags, missing payloads, unexpected payloads, and invalid payload values.
Assignability handles both exact Enum-to-Enum comparison and ordinary inferred
Atom/Tuple values by projecting variants into their runtime structural types.

The runtime codec planner can decode and retain Enum metadata, including nested
payload schemas, but returns a structured codec failure explaining that an
external representation policy is required. It does not silently choose a JSON
encoding.

Tests cover decorator and explicit construction, nested field and variant
normalization, empty and preserved attributes, root attributes applied by
outer decorators, TypeMetadata round trips, unit and payload annotations,
successful and failing validation, unsupported codec behavior, malformed
contexts and members, empty variants, allocation exhaustion, and compatibility
with existing uppercase constructors.
