# RFC 0178: Typed instantiated-module exports

- Status: Proposed
- Depends on: RFC 0176, RFC 0177

## Summary

Expose initialized main exports to trusted entry code through a dynamic value
boundary followed by an explicit, authoritative type projection:

```forma
native def module_export:
    Fn(InstantiatedModule, String) -> Result(Dyn, BlameError);
native def check_type:
    for(A) Fn(TypeOf(A), Dyn) -> Result(A, BlameError);
```

Entry code uses both operations visibly:

```forma
let raw = rt.module_export(initialized, "exec")?;
let exec = rt.check_type(ExecFn, raw)?;
```

## Dynamic boundary

`module_export` performs name lookup only. It preserves the export's runtime
value, source origin, and inferred/published type descriptor inside `Dyn`.
Missing names produce blame listing the requested name and main root.

`check_type` decodes the `TypeOf(A)` witness and compares the Dyn descriptor
with it using Forma's existing assignability relation. It then projects the
unchanged value as `A`. It is a checked bridge, not `Any`, `static_cast`, or a
Rust schema traversal.

## Diagnostics

Projection failures identify the real main export definition when available,
the requested type witness in the entry source, and the structural mismatch.
Equivalent aliases and inferred structural functions pass. Wrong arity,
parameters, results, opaque nominal IDs, and unresolved `Any` fail.

## Acceptance criteria

1. missing export lookup returns structured blame;
2. exported scalars, records, closures, native functions, and opaque values can
   cross the Dyn boundary;
3. structural aliases pass projection;
4. incompatible exports fail before invocation;
5. projection uses the authoritative Forma descriptor/assignability code;
6. values retain main source provenance;
7. no complete interface checker is added to the CLI;
8. full workspace tests and warning-denied Clippy pass.

## Non-goals

- reflective mutation of module exports;
- implicit field syntax on an unknown export record;
- unchecked projection from `Any`;
- general dynamic imports for main code.
