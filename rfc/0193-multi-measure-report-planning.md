# RFC 0193: Multi-measure report planning

- Status: Implemented
- Depends on: RFC 0192

## Summary

Extend the intelligent-reporting experiment from one selected measure to a
small but honest multi-measure compiler. The sequence makes measure semantics,
grain alignment, and the final query/result/render contract explicit Telora
values. It continues to use ordinary capability records, transforms, `Option`,
and Host-observed diagnostics; it does not add an effect system or a
report-specific kernel feature.

The target example combines net revenue at Order grain with units sold at
OrderItem grain. It is accepted only when the intent explicitly requests a
supported pre-aggregation to Order grain. Omitting that policy must produce a
domain-level, repairable diagnostic rather than arbitrary planner fallback.

## Child sequence

1. RFC 0194 gives every measure an explicit semantic value type, natural
   grain, aggregation behavior, and independently lowerable query fragment;
2. RFC 0195 adds an explicit alignment policy, combines compatible fragments,
   and pre-aggregates units sold from OrderItem to Order before joining it to a
   revenue query;
3. RFC 0196 derives SQL parameters, result schema, and render bindings together
   with the statement and publishes them as one typed execution plan.

## Pipeline

```text
ReportIntent
  -> Array(MeasureCapability)
  -> Array(MeasureFragment)
  -> aligned QueryPlan
  -> RelationalPlan
  -> SqlPlan
  -> ExecutionPlan {
       statement,
       parameters,
       result_schema,
       render_plan,
     }
```

Measure lowering remains the compatibility proof. There is no parallel
Boolean matrix saying which measures or dimensions can combine. Alignment is
an authored policy and a transform: successful transformation produces a
fragment at the common grain; failure reports why the requested composition is
not currently available.

## Shared acceptance criteria

1. existing single-measure reports retain their SQLite results;
2. net revenue and units sold can be selected together by explicitly
   pre-aggregating units to Order grain;
3. the same intent without an alignment policy is rejected with a diagnostic
   that identifies both the conflicting grains and the supported repair;
4. dimensions are checked against every selected measure without a second
   compatibility table;
5. independent dimension, ordering, relationship, alignment, and render
   errors remain observable in one best-effort compilation where their inputs
   are reliable;
6. a successful plan atomically contains matching SQL projections, parameter
   declarations, result fields, and render bindings;
7. the Host does not infer aliases, parameter types, or render channels;
8. each child records what is proved and any remaining fallback honestly.

## Non-goals

- arbitrary joins between unrelated measure grains;
- cost-based planning or automatic choice among alignment policies;
- ratio, distinct, snapshot, currency conversion, or allocation semantics in
  this first slice;
- a general nominal semantic-type system;
- database execution inside Telora;
- diagnostic accumulation as a language effect; or
- claiming that every semantically reasonable multi-measure report is already
  expressible.

## Stopping rules

Return to discussion if implementation requires a reporting-specific VM
operation, a second Host-side business checker, an implicit grain choice, or a
compatibility table separate from capability lowering. A reusable array, Dict,
or diagnostic combinator may be proposed independently when the example proves
the need.

## Completion

RFCs 0194 through 0196 are implemented. Measures now expose semantic value,
grain, and aggregation descriptors; explicit pre-aggregation composes revenue
and units without fan-out; and the final plan atomically carries parameterized
SQL, result schema, and checked render fields. The implementation stayed in
ordinary Telora capability and transform code and did not require accumulation
or a reporting-specific kernel feature.

The supported policy space remains intentionally bounded to natural grain and
Order pre-aggregation. Render checking currently proves field existence rather
than chart semantics. Context, authorization, richer aggregation behavior, and
provenance through every intermediate plan remain future work.
