# RFC 0199: Analytics industry methods

- Status: Implemented
- Depends on: RFC 0198

## Summary

Extract analytics-specific method from the intelligent-reporting model without
moving concrete entities, tables, measures, dimensions, SQL payloads, or
company rules into the method library.

The first industry surface provides:

```text
bounded relation expansion through caller-supplied edge accessors
selection of connecting edges from reachable and needed node sets
result-schema name resolution for render bindings
```

## Boundary

`analytics-method` is allowed to understand directed relations, bounded
closure, and output field binding. It remains generic over `Node`, `Edge`, and
`Field`. The reporting model retains:

- its closed `Entity`, `Measure`, and `Dimension` enums;
- cardinality classification and the safe/fan-out catalog split;
- physical table, alias, and join-column payloads;
- measure and dimension capability functions;
- pre-aggregation policy and SQL lowering; and
- domain diagnostic messages.

The six-step closure is recorded as an explicit bounded policy, not presented
as a general fixed-point engine.

## Acceptance criteria

1. the industry module contains no company entity, table, measure, dimension,
   or SQL identifier;
2. the concrete relation model supplies typed accessors rather than erasing
   entities to String;
3. existing safe paths, fan-out rejection, missing paths, and render failures
   retain their diagnostics;
4. single- and multi-measure SQL and SQLite results remain unchanged;
5. the cross-industry method module remains independent of analytics.

## Implementation result

`examples/analytics-method/src/analytics.telora` now owns generic bounded graph
expansion, connecting-edge selection, and missing output-name detection.
`relations.telora` is a concrete model and thin adapter; `ontology.telora`
retains domain diagnostics while delegating name resolution.

The extraction demonstrates a useful industry layer but also a boundary:
without user-defined parameterized record types, generic graph functions can
abstract traversal while the typed `Relation` and `PathPlan` records remain in
the concrete model. No dynamic-value fallback was introduced to conceal that
limit.
