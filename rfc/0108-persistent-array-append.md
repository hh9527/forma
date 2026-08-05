# RFC 0108: Persistent Array append

- Status: Implemented
- Depends on: RFC 0015, RFC 0053, RFC 0102, RFC 0107

## Summary

`@bim/std/array` adds the ordinary pure Function:

```forma
native push: for(A) Fn(Array(A), A) -> Array(A);
```

`push(array, value)` returns the elements of `array` followed by `value` and
never mutates the input Array. The current handle heap implements this as an
O(n) persistent copy. It does not claim a COW fast path or uniqueness proof.

## Semantics

The result root is Generated at the authored `push` call. Every copied element
retains its complete RichValue provenance, and the appended value retains its
own provenance. Aliases of the input continue to observe the original Array.

The operation charges deterministic logical allocation for the complete output
and uses no callback continuation. Empty, nested, imported, and generated
Arrays follow the same rules. Type inference enforces one element type through
the existing generic native contract.

## Goals

1. support explicit Array state threading for diagnostic records;
2. preserve immutable alias and source-provenance semantics;
3. retain deterministic quota behavior; and
4. avoid an unjustified Array representation rewrite.

## Non-goals

- amortized O(1) append, builders, ropes, or persistent-vector trees;
- in-place mutation or static/dynamic uniqueness;
- changing Array equality, iteration, codec, or serialization;
- accumulation syntax or hidden state propagation; or
- changing Tuple or Dict storage.

## Acceptance criteria

1. `push([], x) == [x]` and repeated push preserves order;
2. the input alias remains unchanged;
3. generic typing rejects a mismatched appended value;
4. result-root provenance is the call site;
5. old and appended child provenance are retained exactly;
6. allocation quota is charged for the complete logical result;
7. heap copy/promotion preserves the result normally;
8. existing Array functions remain behavior compatible; and
9. full workspace tests and strict Clippy pass.

## Implementation plan

1. add the generic core declaration and native registry member;
2. implement direct persistent append in the Array native dispatcher;
3. add behavior, typing, alias, quota, and provenance tests;
4. record the explicit O(n) implementation result; and
5. run the full quality gate.

## Stopping rules

Work returns to discussion if correctness requires observable mutation,
Handle-uniqueness assumptions, provenance relabeling of children, or a general
collection/storage redesign.

## Implementation result

Implemented without changing Array storage. `push` is a direct two-argument
native operation in `@bim/std/array`; it validates the source Array, charges
logical allocation for all `n + 1` output edges, copies existing RichValues,
appends the supplied RichValue, and creates a result root at the authored call
site. It does not enter the callback continuation machinery.

Tests cover empty and repeated append, input alias preservation, generic type
rejection, exact allocation success/failure, and both sides of provenance:
an imported JSON element remains anchored in JSON after push, while an
appended authored element remains anchored at its Forma expression after
passing through an Array callback. Existing heap copy/promotion and Array
behavior remain unchanged.

The full gate passes with 317 Forma library tests and 1 ignored, 14 CLI tests,
20 LSP tests, documentation tests, formatting, and warning-denied Clippy.
