# RFC 0184: Staged report lowering

- Status: Implemented
- Depends on: RFC 0183

## Summary

Expand report intent beyond measures and dimensions, and make the compiler's
intermediate boundaries explicit Forma types. The first expanded surface adds
typed filters, explicit dimension ordering, limit, and render mode.

```text
ReportIntent
  -> SemanticPlan
  -> RelationalPlan
  -> SqlPlan
  -> rendered SQLite text
```

## Stage responsibilities

`lower_semantics` combines selected capabilities into projections, filters,
ordering intent, limit, and render policy. Filters carry required entities in
the same way as dimensions.

`lower_relationships` plans the union of measure, dimension, and filter entity
requirements and retains the grain proof.

`lower_sql` combines the proven relationships with the SQL AST. Rendering is
the final pure operation and cannot introduce domain policy.

Compilation publishes SQL only when diagnostics from measure selection,
capability lowering, relationship proof, and ordering validation are empty.

## Acceptance criteria

1. a typed customer-region filter adds both its relation path and SQL predicate;
2. explicit ordering accepts selected dimensions and rejects unselected ones;
3. limit is represented structurally and rendered by the SQL module;
4. render mode survives all stages without changing relational semantics;
5. invalid independent components are diagnosed together and publish no SQL;
6. all three lowering stages are named, typed, ordinary Forma functions.

## Implementation result

`ReportIntent` now carries `filters`, `order_by`, `limit`, and `render`.
`CustomerRegion` is the first typed filter provider. The valid net-revenue
fixture filters East, orders by its selected dimensions, and emits `LIMIT 10`.
The invalid fixture adds an unselected ordering dimension and now returns five
independent diagnostics. `SemanticPlan`, `RelationalPlan`, and `SqlPlan` are
exported with their lowering functions for inspection and reuse.
