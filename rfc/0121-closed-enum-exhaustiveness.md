# RFC 0121: Closed-Enum exhaustiveness

- Status: Proposed
- Depends on: RFC 0119, RFC 0120

## Summary

Forma rejects a `match` that omits a possible variant when its scrutinee has a
statically known closed Enum type:

```forma
match option {
    'Some(value) => use(value),
}
# error: non-exhaustive match; missing 'None
```

The check consumes conservative whole-variant coverage from RFC 0119. Unit
Atom patterns cover matching unit variants. A Tagged pattern covers a payload
variant only when its payload pattern is irrefutable for the declared payload
type. A wildcard or plain binding covers every remaining variant.

## Conservative boundary

The checker unions whole-variant coverage across arms and compares it with the
known Enum descriptor. It does not combine refutable payload patterns:

```forma
match value {
    'Some(0) => zero,
    'Some(1) => one,
    'None => none,
}
# still missing whole-variant coverage for 'Some
```

Authors make the remaining case explicit with `'Some(_)` or a catch-all. This
rule is intentionally less clever than a general pattern matrix and cannot
become unsound as payload domains grow.

No exhaustiveness claim is made for Any, Dyn, unresolved type parameters,
unions, standalone Atom/Tagged singleton types, or other open information.
Their existing runtime no-match behavior remains available.

## Diagnostics and runtime

The diagnostic points at the `match`, lists missing variants in canonical name
order, and distinguishes unit and payload variants in source-like form. The
check runs after inference has resolved the scrutinee type and shares the
pattern diagnostic channel introduced for Struct patterns.

Runtime matching and its defensive `NoPatternMatched` error remain unchanged.
Well-typed exhaustive closed-Enum matches cannot reach that fallback through
ordinary language values.

## Acceptance criteria

1. a complete Atom/Tagged Enum match is accepted;
2. every omitted unit or payload variant is listed deterministically;
3. wildcard and binding patterns complete the remaining coverage;
4. a Tagged binding/wildcard payload covers its whole variant;
5. refutable payload literals do not claim whole-variant coverage;
6. several refutable payload arms are not combined into totality;
7. Any, Dyn, unresolved, and non-Enum scrutinees retain current behavior;
8. nested Struct/Tuple payload patterns use shared irrefutability facts;
9. runtime bytecode and value representation do not change; and
10. full tests and strict static checks pass.

## Non-goals

- general nested-pattern usefulness or witness generation;
- exhaustiveness for Int, String, Array, Dict, Any, Dyn, or unions;
- flow-sensitive refinement;
- changing match result inference; or
- removing the defensive VM no-match error.

## Implementation result

Pending.
