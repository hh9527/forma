# RFC 0188: Severity-aware reporting

- Status: Implemented
- Depends on: RFC 0187

## Summary

Add `Severity` and an ordinary runtime BIF:

```forma
@enum type Severity = { Info: 'None, Warn: 'None, Error: 'None };
native report: Fn(Severity, BlameError) -> BlameError;
```

`report` appends a write-only diagnostic event to the current evaluation
account and returns the identical error value. Forma code cannot inspect the
event stream. Info and Warn preserve successful evaluation; any Error prevents
the Host from publishing a successful value even if validation code continues.

## Subject labels

A direct blame subject becomes the primary diagnostic label. Variadic subjects
stored in the internal Tuple retain their individual locations: the first is
primary and later subjects are related. The authored blame invocation remains
the rule label. No string-internal span model is introduced.

## Acceptance criteria

1. `report` is a first-class function supplied by the default prelude;
2. it accepts only Severity and canonical BlameError values;
3. it returns the original BlameError;
4. Info and Warn do not change a successful value;
5. Error invalidates final success without requiring immediate control exit;
6. workspace diagnostics preserve severity and all available subject labels;
7. LSP maps Info to Information and Warn to Warning;
8. diagnostic events remain absent from Forma's observable value world.

## Implementation result

`QuotaAccount` owns the evaluation-local diagnostic list. The report BIF
normalizes severity and structured blame while the relevant rich values still
retain provenance. VM publication boundaries reject newly reported Error
events with `ReportedDiagnostic`; recovery evaluation collects those events and
continues independent bindings. Tests prove identity return, three-label
variadic blame, successful warning output, workspace warning publication, and
Error invalidation.

Cross-query cache replay remains required before diagnostic-producing calls are
memoized across evaluation sessions. Current tool-stage and authoritative
evaluation share one account, so retained events are neither lost nor exposed
as values.
