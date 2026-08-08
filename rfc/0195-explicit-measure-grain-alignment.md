# RFC 0195: Explicit measure grain alignment

- Status: Implemented
- Depends on: RFC 0194

## Summary

Allow a report intent to select multiple measures only through an explicit
grain-alignment policy. Measure capabilities consume that policy and either
produce a fragment at the requested base grain or report a domain diagnostic.
The fragment combiner accepts only fragments whose base entities agree.

```telora
@enum type Alignment = {
    Natural: 'None,
    PreAggregate: Entity,
};
```

`Natural` preserves every measure's natural grain. It therefore composes only
when all selected measures already agree. `PreAggregate('Order)` is the first
implemented transformation: units sold is grouped by order in a CTE and then
joined to the Order-grain revenue fragment. No policy is selected implicitly.

## Capability contract

```telora
type MeasureLowerer = Fn(Alignment) -> Option(QueryPlan);
```

The returned fragment is evidence that the measure supports the requested
alignment. A fragment carries arrays of measure identities and projections so
the generic combiner can concatenate CTEs, joins, filters, requirements, and
projections without inspecting a measure-specific compatibility table.

Dimension capabilities receive all selected measures. A restricted dimension
must accept the complete selection; universal dimensions remain reusable.

## Acceptance criteria

1. existing reports use `Natural` and retain their results;
2. `NetRevenue + UnitsSold` with `PreAggregate(Order)` produces one query with
   both projections and correct SQLite results;
3. the same measures with `Natural` are rejected with both grains and the
   supported repair in the diagnostic;
4. no measure is silently dropped from a successful plan;
5. dimension capability lowering checks the complete measure selection;
6. independent existing diagnostics remain collected.

## Implementation result

`ReportIntent` now carries `alignment`. Query fragments carry arrays of measure
IDs and projections. Net revenue accepts its natural Order grain; units sold
supports both its natural OrderItem fragment and an explicit Order-grain CTE.
The ordinary `combine_query_plans` transform merges only equal-base fragments.

The new `valid-multi.telora` fixture combines both measures by month and is
executed against SQLite. `invalid-alignment.telora` proves that natural-grain
composition is rejected with a repair-oriented diagnostic. This remains a
bounded policy implementation: it does not claim arbitrary grain conversion.
