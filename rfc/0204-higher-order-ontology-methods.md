# RFC 0204: Higher-order ontology methods

- Status: Implemented
- Depends on: RFC 0199, RFC 0203

## Summary

Create an ordinary `ontology-method` module whose functions operate on closed
model types supplied by callers. The first surface covers capability lookup and
independent lowering, completion, relation closure and edge selection, and
restriction verification.

The module does not own an erased universal model record. It is generic over
the caller's complete Capability, Id, Input, Output, Node, Edge, and Requirement
types and receives typed projections:

```telora
lower_requested:
    for(Id, Capability, Input, Output)
    Fn(
        Array(Id),
        Array(Capability),
        Fn(Capability) -> Id,
        Fn(Capability) -> Fn(Input) -> Option(Output),
        Input,
    ) -> Array(Option(Output))
```

This is the fallback enabled by RFC 0203: a concrete model instantiates its
generated capability type once; shared code remains generic over that exact
type and manipulates it through statically checked selectors.

## Surface

- `find_capability` resolves a closed model identity;
- `lower_requested` independently invokes all requested capabilities and emits
  a source-linked error for a missing definition;
- `completed` and `all_complete` separate reliable results from publication
  policy;
- `expand_once`, `close_six`, and `select_connecting_edges` express the bounded
  relation method already exercised by reporting; and
- `verify_allowed` applies an external allow-list to typed requirements.

The six-step closure policy remains visible and bounded. This RFC does not
rename it to a universal planner.

## Acceptance criteria

1. the shared module contains no SQL, table, metric, B2B, or B2C vocabulary;
2. a model-generated capability record is consumed without `Any`;
3. callback input and output types remain statically connected;
4. missing capabilities report the concrete requested identity and shared rule
   location;
5. relation functions remain generic over closed Node and Edge types; and
6. no compiler, VM, or Host special case is added.

## Implementation result

`ontology-method/ontology.telora` implements the surface as ordinary generic
functions. The valid construction fixture now resolves and runs a generated
MeasureCapability through `lower_requested` instead of invoking its field
directly. The missing fixture requests an undefined Units capability and emits
a domain diagnostic while still evaluating the independent Revenue capability.

The API is intentionally projection-based. Until user-defined metadata
families can expose a precise named result witness, attempting to make shared
code reconstruct arbitrary capability records would require `Any` or an
unchecked cast. Typed selectors preserve safety at the cost of several explicit
arguments. RFC 0205 determines whether that cost remains readable in a real
model rather than hiding it in this minimal fixture.
