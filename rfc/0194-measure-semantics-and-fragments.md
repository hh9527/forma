# RFC 0194: Measure semantics and query fragments

- Status: Implemented
- Depends on: RFC 0193

## Summary

Make the semantic value type, natural grain, and aggregation behavior of every
report measure explicit. A measure capability owns these descriptors together
with the function that lowers the measure to an independent query fragment.

```telora
@struct type MeasureCapability = {
    id: Measure,
    value_type: SemanticValueType,
    natural_grain: Entity,
    aggregation: Aggregation,
    lower: MeasureLowerer,
};
```

These values describe business meaning, not database storage types. Both net
revenue and units sold happen to lower to SQL integers in the fixture, but one
is money in cents at Order grain and the other is a count at OrderItem grain.

## Semantics

The first closed vocabularies are deliberately small:

```text
SemanticValueType = MoneyCents | Count
Aggregation       = Additive
```

`natural_grain` reuses the ontology's existing `Entity` identity. New
aggregation behaviors are added only with an executable rule that needs them.

The lowered `QueryPlan` is the first query-fragment representation. It retains
its measure identity and base entity, relation requirements, CTEs, joins,
filters, and projection. RFC 0195 will combine an array of these fragments only
after alignment has made their grains compatible.

## Acceptance criteria

1. `NetRevenue` declares `MoneyCents`, Order, and Additive;
2. `UnitsSold` declares Count, OrderItem, and Additive;
3. descriptors and lowering remain part of one capability record;
4. both existing single-measure reports retain their exact SQLite results;
5. no static or Host-side compatibility matrix is introduced.

## Implementation result

`ontology.telora` now defines and exports `SemanticValueType` and
`Aggregation`. `MeasureCapability` carries both values and its natural grain.
The two existing capability records populate the descriptors, while their
ordinary lowering functions and generated SQL remain unchanged. This RFC does
not yet claim multi-measure composition; it supplies the semantic evidence
consumed by RFC 0195.
