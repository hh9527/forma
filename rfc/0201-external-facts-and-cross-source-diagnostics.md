# RFC 0201: External facts and cross-source diagnostics

- Status: Implemented
- Depends on: RFC 0199

## Summary

Let an ordinary imported JSON restriction participate in domain verification
and lowering. The data source has no special Context status: the intent module
imports it, passes it to the reporting compiler, and the domain model decodes
and interprets it.

```telora
import "./restriction.json" as restriction;
import "./ontology.telora" { compile };

export let output = compile(restriction, intent);
```

## Model

The first restriction carries a revision and an allowed Entity set. Dimension
and filter requirements are checked against that set after capability lowering.
The same intent therefore succeeds or fails deterministically when supplied a
different ordinary data module.

Successful plans record `restriction_revision`. This is plan data, not an
ambient Host token. An executor may compare it with external state before
running, but Telora owns the domain decision.

## Diagnostic contract

An unauthorized requirement reports with subjects from:

1. the authored intent dimension or filter;
2. the decoded `allowed_entities` JSON field; and
3. the Telora rule containing `emit_error!`.

Codec decoding must retain JSON provenance. No Host-side authorization checker
or fabricated location is accepted.

## Acceptance criteria

1. unrestricted existing reports retain SQL and SQLite results;
2. an orders-only restriction rejects Region dimension and filter requirements;
3. independent unauthorized requirements are both observed;
4. diagnostics contain intent, JSON, and rule source locations;
5. no plan is published after an authorization diagnostic;
6. successful wire plans contain the decoded revision;
7. restriction interpretation remains ordinary domain-library code.

## Implementation result

The reporting compiler now accepts raw restriction data, decodes it to the
domain `Restriction` type, verifies dimension and filter requirements, and
copies the revision into `ExecutionPlan`. `invalid-restriction.telora` uses an
orders-only JSON module and preserves all three provenance layers.

This proves imported facts driving a closed domain model. It does not yet prove
dynamic catalogs, row predicates, revision freshness, or arbitrary policy
languages. The Host still owns checking whether a recorded revision remains
current immediately before execution.
