# RFC 0181: Structured SQL AST

- Status: Implemented
- Depends on: RFC 0180

## Summary

Replace SQL strings in reporting capabilities with ordinary Forma values.
`sql.forma` defines the minimal expressions, projections, sources, joins,
CTEs, grouping, ordering, and selects needed by the experiment. One renderer
owns SQLite syntax, identifier quoting, and text escaping.

Measure lowerers produce a base structured query plan. Dimension lowerers
produce structured projection and grouping requirements. Only the final
successful compilation invokes `render_select`; rejected compilation exposes
neither SQL nor a partial executable plan.

## Representation

The intended recursive `Expr` and `Select` graph cannot currently cross a
module boundary because recursive type metadata is a cyclic heap value and the
legacy module-value boundary rejects it. The implemented representation is a
bounded, non-recursive hierarchy:

```text
SqlAtom -> SqlTerm -> SqlScalar -> SqlSelectExpr
SelectBody -> Select with top-level CommonTable values
```

This is not stringly typed: columns, integers, text, functions, binary
operators, aggregates, aliases, sources, and joins remain distinct nodes. It
deliberately excludes arbitrary expression depth and nested CTEs until cyclic
metadata can be published across modules.

## Acceptance criteria

1. domain capabilities contain no SQL fragments;
2. all identifiers and text literals pass through one renderer quoting path;
3. both valid report intents produce executable SQLite SQL;
4. the invalid intent still returns four independent diagnostics and no SQL;
5. AST types and renderer are ordinary importable Forma definitions;
6. the recursive module-boundary limitation is recorded rather than hidden.

## Implementation result

`examples/intelligent-reporting/sql.forma` implements the bounded AST and
SQLite renderer. `ontology.forma` constructs CTE, join, expression,
projection, grouping, and ordering nodes exclusively. Generated net-revenue
SQL executed against the fixture and returned `East/10000` and `West/12000`;
the units query retained its expected three grouped rows. The same invalid
intent continues to report the measure-count error and three independent
dimension errors.
