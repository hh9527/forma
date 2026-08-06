# RFC 0120: Struct patterns

- Status: Implemented
- Depends on: RFC 0119

## Summary

Forma adds recursive Struct patterns to existing `match` arms:

```forma
match user {
    { name, address: { city } } => (name, city),
}
```

`{ name }` is shorthand for `{ name: name }`. Fields may appear in any order,
omitted fields are ignored, and the long form accepts every existing pattern.
The construct selects fields from Struct values; it does not convert them to
Dict, capture a remainder, or introduce an open-record type.

## Static semantics

For a known Struct scrutinee, every named field must exist. Nested patterns are
analyzed against the declared field type, so bindings receive precise types.
A Struct pattern is irrefutable when every named field exists and every nested
pattern is irrefutable for its field type.

A known non-Struct scrutinee is incompatible. An Any or otherwise unresolved
scrutinee remains conservatively unknown: nested bindings receive Any and the
runtime pattern may either match or continue to the next arm.

Duplicate field names and duplicate binding names are errors. Diagnostics
point at the repeated field or binding. Unknown fields name the field and the
known Struct shape.

## Runtime semantics

Struct values retain their current Dict-shaped runtime representation. Pattern
lowering first performs a non-throwing field-presence test for each selected
field. Failure transfers to the next match arm. Success uses the existing
field-read operation, which preserves the selected child's structural
provenance, and recursively matches the child pattern.

The non-throwing test is a narrow bytecode operation usable by pattern
lowering. It returns False for non-Dict values and absent fields. Ordinary
field expressions retain their existing type and missing-field errors.

## Grammar

```text
pattern:
  ...
  | '{' [struct_pattern_field (',' struct_pattern_field)* [',']] '}'

struct_pattern_field:
  Identifier [':' pattern]
```

String-named fields are deferred because they cannot use binding shorthand and
have not demonstrated a need in typed Struct declarations.

## Acceptance criteria

1. empty, shorthand, renamed, nested, and trailing-comma Struct patterns parse;
2. known Struct fields bind at their declared types;
3. field order and omitted fields do not affect matching;
4. unknown or duplicate fields receive source-positioned diagnostics;
5. a known non-Struct scrutinee is rejected;
6. an unknown scrutinee may match dynamically without unsound static types;
7. missing fields and non-Struct runtime values continue to the next arm;
8. selected children retain field provenance;
9. HIR and workspace queries index nested Struct bindings; and
10. full tests and strict static checks pass.

## Non-goals

- rest capture or exact-field-set matching;
- Dict patterns, computed field names, or string-named fields;
- row polymorphism or open Struct types;
- changing Struct or Dict runtime representation; or
- enabling Struct patterns in `let` before RFC 0123.

## Implementation result

Added Struct pattern AST/CST forms with empty, shorthand, nested, renamed, and
trailing-comma syntax. A dedicated Struct-list predicate preserves the
surrounding match-arm grammar. Shared typed pattern analysis now selects known
field types, classifies irrefutability, and records precise unknown-shape,
missing-field, duplicate-field, and duplicate-binding diagnostics.

Lowering emits a non-throwing runtime Dict-kind test, field-presence tests, and
the existing provenance-preserving field read before recursively matching each
child. Missing fields and non-Dict dynamic inputs therefore fall through rather
than raising ordinary field-access errors; the empty Struct pattern still
checks runtime kind. HIR indexes nested bindings, and inferred pattern binding
types now populate semantic definition facts for hover and lookup consumers.

Focused parser, inference, runtime, dynamic-fallback, empty-pattern, and
semantic-fact tests pass together with the complete core suite. Struct/Dict
representation, ordinary field access, open-record typing, and `let` syntax
remain unchanged.
