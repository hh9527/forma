# RFC 0029: Built-in Bool, Option, and Result types

- Status: Proposed
- Depends on: RFC 0027, RFC 0028

## Summary

XL completes its focused built-in type vocabulary with one fixed type value and
two type constructors:

```xl
Bool
Option(T)
Result(T, E)
```

All three are canonical normalized `'Enum` metadata. They introduce no runtime
value kinds and no privileged validation rules.

## Canonical definitions

They are semantically equivalent to these normalized models:

```xl
@enum
type Bool = {
    False: 'None,
    True: 'None,
};

fn Option(item) {
    enum('None, {
        None: 'None,
        Some: item,
    })
}

fn Result(ok, err) {
    enum('None, {
        Err: err,
        Ok: ok,
    })
}
```

The implementation may provide them directly through the prelude, but their
observable metadata must be indistinguishable from the ordinary definitions.
Root and variant values therefore contain exactly one flat WithAttributes
wrapper, with empty attributes introduced where none were supplied.

## Runtime values

The definitions reuse the RFC 0027 Enum representation:

```xl
'True
'False

'None
('Some, value)

('Ok, value)
('Err, error)
```

`Bool` is extensionally equivalent to the anonymous union of `Atom('True)` and
`Atom('False)`, but its authoritative metadata remains `'Enum`. Static analysis
may project Enum variants into Atom/Tuple alternatives without rewriting the
metadata graph.

## Naming

`Bool` is a TypeMetadata value. `Option` and `Result` are focused type
constructors analogous to `Array`, `Tuple`, and `Fn`, so their uppercase names
remain appropriate. They are not decorator-compatible model declaration
functions and do not accept a context argument.

```xl
type MaybeName = Option(String);
type Outcome = Result(User, String);
```

The lowercase `struct`, `enum`, and `union` names remain reserved for
contextual model normalization.

## Attributes

If `T` or `E` is already attributed, Option and Result flatten its wrapper and
preserve the attributes on the corresponding payload variant. Unit variants
receive empty attributes. The Enum root also receives empty attributes.

Attributes on the constructed root can be added by an outer decorator or by
`core:attributes.add`, as with any other TypeMetadata.

## Core integration

- conditions continue to require the runtime atoms `'True` and `'False`;
- comparison operators continue to infer Bool's two accepted Atom variants;
- annotations and `validate` use ordinary Enum interpretation;
- Option remains compatible with the existing structural codec convention for
  `'None` and `('Some, value)`;
- Result is ordinary Enum metadata; no Result-specific codec policy is added.

## Removal of ad hoc definitions

Current examples and tests that define an `Optional` closure over Union migrate
to the prelude `Option`. User-defined equivalent metadata remains valid, but
the standard spelling no longer requires repeating tagged Tuple structure.

## Acceptance criteria

1. `Bool` is prelude TypeMetadata with `'Enum` kind and unit False/True variants.
2. `Option(T)` produces normalized None/Some Enum metadata.
3. `Result(T, E)` produces normalized Err/Ok Enum metadata.
4. Roots and every variant have exactly one WithAttributes wrapper.
5. Payload attributes and rich locations survive construction.
6. Bool, Option, and Result annotations accept their specified runtime values.
7. Incorrect tags, payload presence, and payload types fail through ordinary
   Enum validation.
8. Option remains usable by derived structural codecs.
9. Constructor calls obey active fuel, stack, and allocation quotas.
10. Invalid payload TypeMetadata is rejected with a useful call location.
11. Examples and current documentation use the built-in Option spelling.
12. Existing hand-written Enum and Union equivalents remain compatible.

## Implementation plan

1. Publish normalized Bool metadata in both tool-stage and runtime preludes.
2. Add VM-managed focused Option and Result constructor functions.
3. Reuse direct HeapView flattening, Enum construction, and quota accounting.
4. Migrate Optional examples and add metadata, validation, codec, attributes,
   diagnostics, and quota tests.

## Implementation result

Pending.
