# RFC 0022: First-class rich Type metadata and contract blame

- Status: Implemented
- Depends on: RFC 0003, RFC 0020, RFC 0021

## Summary

Type definitions produce canonical XL data. No compiler-only schema tree is an
additional authority for Type semantics or source locations. When a retained
type binding is evaluated during module initialization, its ordinary XL
expression constructs an ordinary rich runtime value whose nested edges retain
their source locations.

TypeDescriptor and codec plans remain host-side interpretations and caches.
Every interpreted node keeps the corresponding rich Type metadata value as its
source. Contract failures are represented by ordinary structured XL data that
contains the message, offending data value, and rejecting rule value. The
diagnostic boundary derives primary and secondary labels from those rich
values.

## Motivation

RFC 0021 carries locations on all runtime values, but the initial Type path
discarded them twice:

1. tool-stage evaluation exported metadata to the legacy location-free Value;
2. native Type constructors decoded their arguments to TypeDescriptor and
   rebuilt fresh metadata.

The codec could therefore locate invalid JSON data but only label the codec or
unwrap expression as the rule origin. Adding a privileged located schema AST
would repair the diagnostic while violating the language model: Struct and
user-written metadata functions would no longer produce the complete
first-class representation consumed by the runtime.

## Canonical Type values

The canonical protocol remains ordinary XL data:

```text
Int                 = {kind: 'Int}
Array(T)            = {kind: 'Array, item: T}
Struct(fields)      = {kind: 'Struct, fields: fields}
Union(variants)     = {kind: 'Union, variants: variants}
Fn(parameters, out) = {kind: 'Function, parameters: parameters, result: out}
```

The constructors validate their inputs, but preserve the original rich
arguments in these records. In particular, Struct(fields) stores the supplied
Dict rather than decoding and re-encoding each field Type. User functions can
compute the same records and are semantically indistinguishable from built-in
constructors.

A `type Name = expression;` binding has two evaluations in the current
two-stage implementation when Name is retained at runtime:

- the tool-stage evaluation validates metadata and supplies static analysis;
- module initialization evaluates expression as ordinary XL code and binds its
  rich result.

The tool-stage Value is not embedded as the runtime authority. Closed-world
memoization may remove duplicate work later if it preserves the same rich XL
value semantics.

## Interpreters and plans

TypeDescriptor is a static-analysis projection. CodecPlan is a runtime
execution projection. Neither is observable XL state or an authority for
metadata identity.

Each CodecPlan node retains its source RichValue from the canonical Type graph:

```text
CodecPlanNode = { operation, rule: RichValue }
```

The plan may normalize field lookup and dispatch, but a failure selects the
rule value from the node that rejected the data. Rebuilding a plan is always
valid and cannot change language behavior.

## Structured contract failure

Codec failure is an ordinary tagged XL value conceptually equivalent to:

```text
('Err, {
    message: "$.v: expected String",
    data: offending_value,
    rule: rejecting_type_metadata,
})
```

The names and shape are an initial core codec protocol, not a hidden VM object.
Programs may inspect or transform the record before calling result.unwrap.
Successful codec results remain `('Ok, value)`.

result.unwrap accepts both the structured payload and the legacy String
payload. For a structured payload it creates a runtime error with:

- data.loc as the primary label;
- rule.loc as the contract/rule secondary label;
- the unwrap opcode origin retained in the runtime trace.

If either location is unknown, rendering degrades to the remaining locations.
Location metadata remains observational: equality and serialization rules do
not change.

## Propagation discipline

Native constructors must choose their location behavior explicitly:

- preserved Type arguments are copied as complete RichValues;
- the canonical record root uses the constructor call location;
- newly created `kind` atoms use the constructor call location;
- collection arguments retain their root and child-edge locations.

Tests inspect nested runtime metadata and diagnostic labels. This is necessary
because a propagation mistake does not alter XL value equality and would
otherwise fail silently.

## Rejected alternatives

### A privileged located schema AST

It makes compiler data, rather than XL metadata, authoritative and prevents
ordinary metadata computations from being fully equivalent to built-in forms.

### Put only Loc in CodecType

It repairs labels but loses the first-class rule value and creates a second
source model. A plan retains the canonical RichValue instead.

### Keep embedding tool-stage Value constants

Legacy Value intentionally drops locations. Treating that export as runtime
metadata permanently erases nested rule provenance.

### Encode diagnostics in a special VM error payload

Codec decode is an ordinary Result-returning operation. Its failure should
remain ordinary XL data until an explicit boundary such as result.unwrap turns
it into a runtime diagnostic.

## Deferred work

- recursive Type metadata and plan memoization;
- a public standard diagnostic/result protocol beyond core:codec;
- eliminating duplicate retained Type evaluation through rich snapshots;
- provenance DAGs for rules synthesized from multiple metadata values;
- static checker blame directly from rich tool-stage values.

## Implementation plan

1. Compile retained type bindings from their original expression rather than a
   location-free tool-stage Value constant.
2. Make native Type constructors validate and wrap original rich arguments.
3. Replace CodecType with plan nodes that retain corresponding rule RichValues.
4. Return structured ordinary XL codec failures containing message, data, and
   rule.
5. Teach result.unwrap to render data/rule blame while retaining legacy String
   compatibility and opcode traces.
6. Add metadata-edge propagation and exact JSON/schema double-label tests.

## Acceptance criteria

1. A retained type binding evaluates to valid canonical XL metadata.
2. Struct and other constructors preserve nested metadata RichValues rather
   than rebuilding them from TypeDescriptor.
3. User-computed canonical metadata is accepted identically to constructor
   output.
4. Codec plans retain original rule values and do not become a semantic
   authority.
5. Codec Err payloads are ordinary inspectable XL records.
6. A nested mismatch labels the exact JSON scalar as primary and the exact
   schema metadata expression as secondary.
7. result.unwrap remains compatible with legacy String Err payloads.
8. Existing static analysis, module, quota, codec, and CLI behavior remains
   compatible.

## Implementation result

Retained type bindings now compile their original XL expression into module
initialization instead of embedding the location-free tool-stage Value. A
fixed-point name scan retains constructor and dependent type bindings used by
those expressions. Primitive prelude Type values materialize at each reference
expression, so a field such as `v: String` carries the location of that exact
rule occurrence. The separate tool-stage evaluation remains the static
TypeDescriptor projection used by analysis.

Atom, Array, Tuple, Struct, Union, and Fn constructors validate the canonical
protocol but store their original rich arguments directly in newly constructed
XL Dict metadata. User-written canonical Dict metadata and constructor output
are accepted through the same codec path.

The runtime codec projection is now a plan whose nodes retain their canonical
rule RichValue. A failed transform returns the ordinary XL payload
`{message, data, rule}` under `'Err`; it remains inspectable before unwrap.
`core:result.unwrap` accepts this payload, derives exact data and rule labels,
and retains its opcode origin in the trace. Legacy String Err payloads remain
supported. Tests exercise an exact JSON scalar primary label, exact schema
primitive secondary label, structured payload inspection, hand-built Type
metadata, and legacy compatibility.
