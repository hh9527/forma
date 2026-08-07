# RFC 0185: Checked report execution plan

- Status: Implemented
- Depends on: RFC 0184

## Summary

Replace the public optional SQL string with a typed, inert execution plan. The
plan carries a version, SQL dialect, statement, read-only declaration, and
output mode. It grants no database authority: a Host must type-check, encode,
authorize, and execute it explicitly.

## Boundary

```forma
@struct type ExecutionPlan = {
    version: Int,
    dialect: SqlDialect,
    statement: String,
    read_only: Bool,
    output: OutputMode,
};
```

Internal dialect and output values remain enums. `encode_plan` explicitly maps
them to stable wire strings rather than asking generic JSON serialization or
the Host to invent a representation. The first wire format is version 1.

Compilation returns `Option(ExecutionPlan)`. Any diagnostic forces `None`, so
the Host cannot accidentally execute a partially lowered statement.

## Acceptance criteria

1. a consumer can statically assign the result to `Option(ExecutionPlan)`;
2. a valid plan has explicit version, dialect, read-only, and output fields;
3. encoding produces deterministic JSON with explicit enum wire names;
4. invalid compilation returns all diagnostics and `plan: None`;
5. the plan contains no connection, credential, filesystem, or execution
   capability;
6. SQLite execution of the contained statement retains expected results.

## Implementation result

`execution.forma` defines the boundary and encoder. `host-plan.forma` models a
Host adapter by statically checking the optional plan and encoding it. The
result is deterministic JSON with `version: 1`, `dialect: "sqlite"`,
`read_only: true`, the rendered statement, and the requested output mode. The
invalid fixture publishes no plan.
