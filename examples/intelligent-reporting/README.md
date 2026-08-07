# Intelligent reporting experiment

This is the first executable slice of the intent-compiler discussions. It
models a ten-table B2B commerce domain, accepts a small high-level report
intent, validates domain compatibility, and lowers accepted intent to SQLite
SQL using ordinary Forma code.

Read [DOMAIN.md](DOMAIN.md) first. It defines the physical schema, semantic
identities, relationships, measure grain, and initial legal combinations.

## Files

- `schema.sql`: ten SQLite tables and deterministic fixture rows;
- `DOMAIN.md`: the textual ontology and business rules;
- `ontology.forma`: domain validation and SQL lowering;
- `valid.forma`: net revenue by month and customer region;
- `valid-units.forma`: units sold by month and product category;
- `invalid.forma`: four independently discoverable domain errors;
- `valid-sql.forma`: exposes generated SQL for SQLite execution;
- `net-revenue.sql`: hand-written reference query.

## Run

```sh
cargo run -p forma -- check examples/intelligent-reporting/valid.forma
cargo run -p forma -- run examples/intelligent-reporting/valid.forma
cargo run -p forma -- run examples/intelligent-reporting/valid-units.forma
cargo run -p forma -- run examples/intelligent-reporting/invalid.forma
```

With sqlite installed through mise, the reference fixture can be checked with:

```sh
mise x -- sqlite3 :memory: \
  ".read examples/intelligent-reporting/schema.sql" \
  ".read examples/intelligent-reporting/net-revenue.sql"
```

The SQL emitted by `valid-sql.forma` was also fed directly into the same
SQLite session. Both paths produce:

```text
2026-01|East|10000
2026-02|West|12000
```

## What this proves

- A Forma library can carry a small operational ontology and business rules.
- A Code Agent-facing intent stays close to measures and dimensions rather
  than tables, joins, CTEs, payment/refund semantics, or SQL syntax.
- Validation and lowering are one operation: an unsupported measure/dimension
  grain combination is rejected while constructing the query plan.
- Independent validation passes can accumulate four errors in one result.
- A rejected compilation publishes no SQL.
- A successful compilation emits SQL that SQLite can execute without another
  layer interpreting domain policy.

## Deliberate limitations

This is not yet a general query planner. The registry and relation paths are
encoded directly in Forma functions, and SQL FROM/CTE fragments are defined
per measure. The experiment does not yet implement:

- data-driven ontology registries and relation-path search;
- typed field/expression facades;
- drill and roll-up rules;
- filters, parameters, authorization, or catalog context;
- a structured SQL AST;
- result schema or render lowering;
- causal blocked facts or automatic cascade suppression;
- ergonomic diagnostic accumulation.

The explicit diagnostic arrays are useful evidence: they achieve multi-error
feedback, but they also expose the bookkeeping cost that a narrow accumulation
facility would need to remove.

