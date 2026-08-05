# RFC 0102: Structural value provenance

- Status: Proposed
- Depends on: RFC 0046, RFC 0100, RFC 0101

## Summary

Forma gives every runtime value edge an optional provenance classification:

```text
Provenance = Original(Location) | Generated(Location) | Unknown
```

Original provenance comes from parsed external data and survives aliases,
selection, calls, heap copying, and module publication. Generated provenance
comes from an authored Forma expression. It is preserved within the current
Function, then rebased to the authored call site when returned across a
Function or native boundary.

Containers carry provenance both at the root and on each child edge. A newly
constructed container therefore has a generated root while unchanged children
retain their original locations.

## Motivation

Forma already stores an optional source location on each `RichValue`, including
container edges, and imported JSON/TOML/YAML data populates those locations.
This supports source-aware errors but cannot distinguish imported data from a
value computed in a Function body. Consequently, a generated return may point
at the Function definition rather than the call that generated the caller's
value.

The distinction is required by the user-facing rule:

> An unchanged value keeps its original position. A regenerated value uses its
> generation position.

At Function boundaries, the authored call is the useful generation position.
The implementation needs one bit of provenance kind, not an unbounded history.

## Representation

The runtime replaces `Option<Loc>` on `RichValue` with a compact optional
provenance value. Conceptually:

```rust
enum Provenance {
    Original(Loc),
    Generated(Loc),
}
```

Absence represents Unknown. The exact packed representation is internal, but
`RichValue` must remain cheap to copy and must not allocate provenance objects
per scalar. Container child provenance remains stored on existing child edges;
there is no parallel path map after import.

Provenance equality is deliberately ignored by runtime value equality, hashing,
shape interning, and serialization just as locations are today.

## Construction and preservation rules

Runtime operations apply these rules:

| Operation | Result provenance |
| --- | --- |
| imported scalar/container root | `Original(parsed location)` |
| imported child edge | `Original(child location)` |
| literal or computed scalar | `Generated(expression location)` |
| constructed container root | `Generated(expression location)` |
| child inserted unchanged | child's existing provenance |
| alias or register move | unchanged |
| field/index/pattern selection | selected child provenance |
| metadata-only wrapper removal | underlying value provenance |
| unknown Host input | Unknown unless Host supplies provenance |

Selection is naturally path-sensitive because Array, Tuple, Tagged, Dict, and
Dyn objects already store rich child edges. A selected value is not relabeled
with the selector expression.

## Call-site rebasing

When a Function or native call returns:

1. `Original(location)` is preserved recursively at the returned root;
2. `Generated(_)` at the returned root becomes `Generated(call_site)`;
3. Unknown becomes `Generated(call_site)` for an authored successful call; and
4. child edges are not recursively rebased merely because their container
   crossed the boundary.

This yields the intended mixed-container behavior:

```forma
let b = {
    user: src.user,
    count: src.count + 1,
};
```

`b` is generated at its Dict expression (or at the caller if returned),
`b.user` retains the imported location, and `b.count` points to the authored
addition (or its generating call boundary when returned independently).

Rebasing only the returned root is intentional. Recursively rebasing all
generated descendants would erase the distinction between inherited children
and the container assembly operation.

## Native boundary

Native functions obey the same contract as Forma Functions:

- a native projection or identity operation returns the selected/input
  provenance;
- a native computation labels its new root with the authored call site;
- a native-built container retains unchanged argument/child provenance; and
- native code cannot manufacture Original provenance from a rule location.

Existing core operations are audited by category. Generic VM return handling
provides a safe generated-root fallback, while projection-like natives retain
the explicit provenance they return.

## Imports, copying, and publication

JSON, TOML, and YAML lowerers mark parsed value and child locations Original.
Heap copying, promotion from Work to Main, external link resolution, recursive
up-links, and module-cache reuse copy provenance exactly. Canonical module IDs
remain the source database identity; physical filesystem paths are neither
embedded in values nor exposed to Forma code.

Values entering through the legacy unsourced Host `Value` boundary remain
Unknown. This RFC does not add a public sourced-input API, but it preserves
provenance for all existing sourced module imports.

## Diagnostics and observability

Runtime errors continue to expose data and rule `Location`s, not provenance
enums. A data anchor uses the location inside either Original or Generated
provenance. The classification guides preservation and rebasing only.

Forma code cannot inspect provenance. Debug formatting, `==`, core equality,
JSON output, schema generation, and type metadata remain unchanged. Tests use
internal VM/heap inspection and rendered cross-source diagnostics.

## Goals

1. distinguish imported origins from authored generated locations;
2. preserve source locations through aliases and structural selection;
3. retain child provenance in newly constructed containers;
4. rebase generated Function/native results to authored call sites;
5. preserve provenance through copy, promotion, links, and module caches;
6. keep provenance cheap, bounded, and observational; and
7. establish the successful-value substrate for Dyn and `blame!` in RFC 0105.

## Non-goals

- exposing provenance to Forma programs;
- storing a complete transformation or call history;
- assigning provenance to types independently from their metadata values;
- changing value equality, hashing, serialization, or type inference;
- adding Host best-effort recovery or internal Never;
- implementing `blame!` or failure lineage; or
- recursively rebasing every descendant at every call boundary.

## Acceptance criteria

1. imported JSON, TOML, and YAML roots and children are marked Original;
2. literals, arithmetic, interpolation, tags, and constructed containers are
   marked Generated at their authored expressions;
3. aliases retain provenance and selectors return child provenance;
4. mixed generated containers retain original and generated child locations;
5. an unchanged value returned through nested Functions retains Original;
6. a generated scalar or root returned from a Function is rebased to each
   caller's authored call site;
7. native identity/projection and computation cases follow the same rules;
8. Dyn descriptor and payload edges preserve their independent provenance;
9. Work/Main copying, module publication, and cache reuse preserve provenance;
10. provenance remains ignored by equality and serialization;
11. existing source-aware diagnostics do not regress; and
12. full Forma, CLI, LSP, formatting, and strict static checks pass.

## Implementation plan

1. introduce the compact Provenance kind and replace raw runtime locations;
2. mark sourced imports Original and authored bytecode results Generated;
3. preserve provenance through selection, aliases, heap copying, and links;
4. carry call-site context in Function and native return targets and rebase
   generated roots;
5. audit native projections, computations, containers, and Dyn packing;
6. add focused import, mixed-container, nested-call, native, copy, equality,
   and diagnostic regressions; and
7. run the full quality gate and record the implementation result.

## Stopping rules

Work returns to discussion if implementation requires:

1. user-visible provenance reflection;
2. an unbounded transformation graph or per-operation history;
3. changing equality, hashing, serialization, or types;
4. recursively cloning container graphs solely to rewrite provenance;
5. exposing physical filesystem paths;
6. combining successful provenance with Never/failure lineage; or
7. operation-specific source policy outside the documented native categories.
