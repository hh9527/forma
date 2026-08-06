# RFC 0127: Typed syntax elaboration

- Status: Implemented
- Depends on: RFC 0118 through RFC 0125

## Summary

Forma introduces a small typed-elaboration boundary between successful type
analysis and runtime compilation. Postfix `?` becomes its first client: type
analysis chooses the Option-shaped or Result-shaped family, then elaboration
rewrites the surface expression to ordinary hygienic block, match, and return
nodes.

This is not parser-level desugaring. The family cannot be selected before the
operand has a resolved structural Enum type.

## Elaboration

Conceptually, Option propagation becomes:

```forma
{
    let $subject = operand;
    match $subject {
        'Some($payload) => $payload,
        'None => return $subject;
    }
}
```

Result propagation uses `Ok` and `Err(_)` in the corresponding arms. The
generated names are outside Forma's authored identifier grammar and unique
within one elaboration pass. The operand is evaluated once. Returning the
bound subject rather than rebuilding the failure constructor preserves its
value and provenance.

All generated nodes retain the source location of the authored `?`. Nested
surface expressions are elaborated recursively before their enclosing sugar.

## Static and runtime boundaries

Type inference retains the dedicated static rules from RFC 0124: exact family
recognition, boundary-family consistency, error assignability, and success
payload facts. Analysis records only the selected family for each accepted
propagation expression.

The runtime compiler no longer lowers `?`. It accepts only the elaborated core
forms. An unelaborated propagation node is an internal frontend error. No VM,
bytecode, LIR, or runtime value changes are required.

This boundary is intentionally narrow. It is reusable by later typed sugars
such as `if let` and `let else`, but it does not define a macro system, expose
generated syntax, or create a second public IR.

## Acceptance criteria

1. every accepted `?` records exactly one structural family during analysis;
2. elaboration produces hygienic block, match, and return core forms;
3. the operand remains single-evaluation;
4. failure returns the original subject and success selects its payload;
5. generated nodes retain authored source origins;
6. compiler-specific propagation lowering is removed;
7. Option, Result, module, nested-Function, inference, and diagnostics behavior
   from RFC 0124 remains unchanged; and
8. no runtime instruction or value is added.

## Non-goals

- parser-level untyped desugaring;
- user macros or syntax reflection;
- exposing generated bindings through semantic facts; or
- implementing `if let` or `let else` in this RFC.

## Implementation result

Implemented with a dedicated internal elaboration pass invoked after successful
analysis and before runtime compilation. Generic inference records the resolved
family at each accepted propagation location. Elaboration recursively replaces
those nodes with hygienic block, match, and return forms whose generated nodes
retain the authored source location.

The compiler's propagation-specific tag-test and return lowering was removed;
an unelaborated propagation node is now an internal frontend error. Existing
LIR, bytecode, and VM behavior is unchanged.

Tests cover the generated core shape, hygienic names, failure return, source
locations, and the complete RFC 0124 runtime and inference behavior. The full
workspace suite passed with 20 LSP tests, 15 CLI tests, and 354 core tests
passing with one ignored. Strict Clippy, formatting, diff, and workspace
metadata checks also passed.
