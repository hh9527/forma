# RFC 0129: Let-else binding

- Status: Implemented
- Depends on: RFC 0123, RFC 0125 through RFC 0128

## Summary

Forma adds refutable local binding with a diverging fallback:

```forma
let 'Some(value) = option else {
    return fallback;
};
use(value)
```

The success bindings cover the remainder of the containing block. The else
block must have type Never, normally through `return` or `panic!`.

The parser retains the remainder as an explicit continuation. Typed analysis
checks the pattern and divergence, then elaboration produces a match whose
success arm is that continuation and whose failure arm is the else block.

## Acceptance criteria

1. refutable structural patterns bind across the remaining block;
2. the initializer is evaluated once;
3. the else block must resolve to Never;
4. return and panic satisfy the divergence requirement;
5. bindings do not enter the else scope;
6. incompatible and irrefutable patterns receive diagnostics; and
7. runtime compilation receives only ordinary match/block forms.

## Implementation result

Implemented with a dedicated local continuation AST node produced while the
parser folds block entries from right to left. HIR resolution scopes pattern
bindings only over the continuation. Generic inference reuses typed pattern
analysis, rejects irrefutable/incompatible patterns, and requires the fallback
block to resolve to Never. Typed elaboration replaces the node with an ordinary
two-arm match before runtime compilation.

Tests cover successful continuation binding, return and panic divergence,
non-Never fallback rejection, and irrefutable-pattern rejection. The full
workspace suite passed with 20 LSP tests, 15 CLI tests, and 356 core tests
passing with one ignored. Strict Clippy, formatting, diff, and metadata checks
also passed.
