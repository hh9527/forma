# RFC 0205: B2B model over ontology methods

- Status: Implemented
- Depends on: RFC 0196, RFC 0204

## Summary

Reframe the existing ten-table reporting example as one concrete B2B model
that consumes the reusable ontology-method library. Rename its former
`ontology.telora` module to `b2b-model.telora` so the file no longer implies
that B2B entities and SQL mappings are the reusable ontology definition method.

## Ownership after the split

`ontology-method` owns:

- typed capability lookup and independent lowering;
- reliable-result completion;
- generic bounded relation closure and connecting-edge selection; and
- generic allow-list verification.

The B2B model owns:

- Entity, Measure, Dimension, Filter, Alignment, and plan types;
- NetRevenue and UnitsSold semantics;
- Organization, Customer, Product, Category, and Region relationships;
- physical tables, aliases, columns, joins, predicates, and SQL expressions;
- B2B-specific grain alignment and unavailable-dimension policy; and
- assembly of its semantic, relational, SQL, render, and execution plans.

The split deliberately leaves plan assembly in the model. The method library
does not know the model's concrete intermediate plan types and should not erase
them merely to own more code.

## Implementation

Measure and dimension capability collections are now explicit model data.
Both are processed by `ontology.lower_requested` with typed `id` and `lower`
projections. Query plans and group requirements use `ontology.completed`.

The relation catalog now uses `ontology.contains`, `ontology.close_six`, and
`ontology.select_connecting_edges`. The concrete Relation type and physical
join fields remain local.

`analytics-method` remains temporarily for result-field name comparison. That
small helper is not evidence for a separate universal analytics framework and
may be folded into a later presentation or rendering method.

## Acceptance criteria

1. all existing intents import the explicitly named B2B model;
2. Measure and Dimension remain closed concrete enums;
3. capability callbacks remain typed from Alignment or selected Measure to
   concrete QueryPlan or GroupRequirement;
4. relation traversal uses the shared method without changing the catalog;
5. valid single- and multi-measure SQL remain unchanged;
6. the four-error invalid intent and cross-source restriction diagnostics
   remain intact;
7. the Host wire plan retains its revision and complete shape; and
8. no B2B concept moves into `ontology-method`.

## Implementation result

The B2B model now consumes the shared capability and relation protocols while
retaining all business semantics and physical mappings. The canonical valid
plan is unchanged, including payment/refund CTEs, parameter ordering, result
schema, render fields, and restriction revision.

The explicit projection arguments add some ceremony:

```telora
ontology.lower_requested(
    measures,
    measure_capabilities,
    fn(capability) { capability.id },
    fn(capability, measure, alignment) { capability.lower(measure, alignment) },
    alignment,
)
```

That ceremony is the current safe boundary from RFC 0203. It is readable and
keeps the model types intact, but RFC 0206 must show whether a second model can
use the same shape without modifying the shared API. One successful refactor
alone does not establish reuse.

The first B2B integration exposed one necessary API correction: capability
lookup must pass the original requested identity to the lower callback. Using
only the capability's stored `id` preserved value equality but changed
requirement provenance from the intent to the model catalog. The callback is
therefore `Fn(Capability, Id, Input) -> Option(Output)`. Concrete lowerers use
that requested value when constructing requirements, retaining intent, model,
and shared-rule source boundaries.
