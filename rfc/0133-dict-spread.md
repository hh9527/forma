# RFC 0133: Dict spread

- Status: Implemented
- Depends on: RFC 0060, RFC 0132

## Summary

Dict literals accept `...expression` entries:

```forma
let env: Dict(String) = {
    ...base_env,
    "PATH": path,
};
```

The operand must have type `Dict(T)`. Entries are evaluated exactly once from
left to right, and later entries replace earlier values with the same key. This
applies across spread boundaries and to explicit fields following a spread.

An ordinary literal still rejects duplicate explicit fields. This preserves
the existing typo diagnostic; intentional replacement is expressed at a
composition boundary with spread.

Struct values are not Dict spread operands. Struct update changes exact field
shape and has separate inference requirements, so it remains reserved for a
later RFC rather than being introduced implicitly here.

## Implementation result

Implemented in Dict parsing, AST representation and traversal, type inference,
compiler, bytecode, and VM. Dict literals without spread retain their existing
direct construction path. Literals with spread compile ordered Dict fragments
and combine them with one internal right-biased merge operation.

Tests cover multiple spreads, explicit-field replacement, contextual typing of
nested Dict literals, non-Dict diagnostics, canonical output order, and the
preserved duplicate-explicit-field error.
