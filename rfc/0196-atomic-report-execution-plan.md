# RFC 0196: Atomic report execution plan

- Status: Implemented
- Depends on: RFC 0195

## Summary

Publish SQL text, typed parameters, result schema, and render bindings as one
atomic execution plan. Every component is derived during the same lowering
pass. The Host serializes and executes the plan; it does not infer aliases,
parameter types, or render channels from SQL text.

```telora
@struct type ExecutionPlan = {
    version: Int,
    dialect: SqlDialect,
    statement: String,
    parameters: Array(QueryParameter),
    result_schema: Array(ResultField),
    render: RenderPlan,
    read_only: Bool,
};
```

## Parameters

SQL `Expr` gains a structural `Parameter` node rendered as `?`. Each filter
requirement produces the expression and its `QueryParameter` together. The
first supported values are Text and Int. The parameter array preserves filter
order, so placeholder and value order have one construction source.

No SQL literal interpolation is used for the customer-region input.

## Result and render contract

Dimension requirements and measure fragments carry `ResultField` values beside
their projections. `lower_sql` concatenates both arrays in projection order to
derive the result schema.

The intent supplies a render mode and field names. `compile_render` accepts a
binding only when every requested name exists in the derived schema. Invalid
bindings report at the authored field and prevent plan publication. The first
`RenderPlan` deliberately records ordered fields rather than introducing a
chart grammar.

## Wire boundary

`encode_plan` explicitly maps parameter and result enums to stable strings. It
publishes parameters, result schema, and render policy in the versioned JSON
record. The Host still owns database access and runtime failure handling.

## Acceptance criteria

1. customer-region SQL uses a placeholder and the plan carries its Text value;
2. parameter order and placeholder order share one filter-requirement source;
3. result fields have the same order and aliases as SQL projections;
4. net revenue is `MoneyCents` and units sold is `Count` in the public plan;
5. an unknown render field emits a domain diagnostic and no plan;
6. wire JSON contains all four atomic components;
7. existing single- and multi-measure SQLite results remain unchanged.

## Implementation result

`sql.telora` now renders structural parameter nodes. `execution.telora` defines
typed parameter, result, and render records and encodes them explicitly.
`ontology.telora` creates parameter/projection metadata at capability-lowering
sites, derives the complete schema, validates render bindings, and constructs
the execution plan only after all stages succeed.

The implementation proves positional parameters for the current filter model,
not arbitrary SQL placeholder analysis. It proves that requested render fields
exist, not chart-specific arity or semantic compatibility. Those are honest
future extensions of the same typed plan rather than Host fallback hidden by
this RFC.
