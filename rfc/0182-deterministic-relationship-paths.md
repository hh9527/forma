# RFC 0182: Deterministic relationship paths

- Status: Implemented
- Depends on: RFC 0181

## Summary

Move ordinary entity joins out of measure capabilities into a Forma relation
catalog. Measures declare a base entity and semantic support entities;
dimensions declare the entity that supplies their value. A relationship
planner computes the ordered join set required to connect the base to all
targets.

## Planning model

For this bounded ontology the planner computes two closures:

1. forward reachability from the measure base;
2. backward necessity from all requested targets.

Catalog edges in the intersection form the plan. Catalog order gives stable
SQL order, and selecting each catalog edge at most once merges shared prefixes
without a separate deduplication table. Six explicit expansion rounds cover
the maximum path length in this fixture without introducing recursive runtime
values.

This is deterministic reachability, not cost-based search. Multiple competing
semantic paths remain a future diagnostic rather than an arbitrary choice.

## Acceptance criteria

1. ordinary customer, organization, region, product, and category joins no
   longer live in measure capabilities;
2. net revenue plus customer region selects the Order-to-Region path;
3. units sold plus month/category/SKU combines Order and Product/Category
   paths without a duplicate Product join;
4. missing targets are represented explicitly by `PathPlan.missing`;
5. existing valid SQL remains executable and invalid intent remains rejected.

## Implementation result

`relations.forma` defines `Entity`, `Relation`, the ordered catalog, closure
operations, and `plan`. `QueryPlan` now carries its base and required entities;
`GroupRequirement` carries its target entity. Finalization plans relationships
once for the union of targets and converts the selected relations to SQL AST
joins. Both valid reports retain their expected SQL shape and results.
