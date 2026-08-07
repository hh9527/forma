# RFC 0179: Exec as a staged entry function

- Status: Implemented
- Depends on: RFC 0175 through RFC 0178

## Summary

Migrate `entry/exec.forma` from top-level `@main` imports and injected literal
snapshots to an exported orchestration function:

```forma
export def entry: Fn(Module) -> Result(ExecDryRun, BlameError) = fn(pending) {
    let settings = settings_from_host(pending.module)?;
    let request = request_from_options(pending.module, pending.options)?;
    let initialized = rt.initialize_module(pending.module)?;
    let raw_exec = rt.module_export(initialized, "exec")?;
    let exec = rt.check_type(ExecFn, raw_exec)?;
    encode_dry_run(exec(settings, request)?)
};
```

The Host selects the entry, prepares the pending main and arguments, evaluates
the entry module, invokes its explicit `entry` export, and prints the two
already encoded output channels. Rust no longer interprets capture options or
generates runtime/options source modules.

## Environment policy

Entry consumes ordered `pending.options`, extracts repeated
`exec.capture-envs` actions, and calls `rt.var(handle, name)` for the selected
names. Missing variables remain absent. The resulting ordinary `Dict(String)`
is passed to main. The entry may later replace this policy or inject a module
without a CLI change.

## Dry-run and future effects

This RFC retains dry-run output as the only CLI behavior. The staged function
boundary is compatible with future entries that call installer and process
native functions, but those effects require separate protocol RFCs.

## Acceptance criteria

1. exec entry has no static `@main` import;
2. CLI does not generate `entry/rt.priv.forma` or interpret capture options;
3. options and direct Host reads happen in Forma entry code;
4. main imports begin only at explicit initialization;
5. ExecFn is checked through RFC 0178 before invocation;
6. codec/JSON encoding remains Forma code;
7. malformed results publish no partial stdout;
8. GCC wrapper behavior, provenance, argument rewriting, environment policy,
   and deterministic recipe IDs remain byte-compatible;
9. full workspace tests and warning-denied Clippy pass.

## Non-goals

- actual installation or process execution;
- external entry selection;
- migrating run/build;
- changing the public ExecFn protocol.

## Implementation result

`entry/exec.forma` exports `entry(handle)` and owns option selection,
settings/request construction, export projection, invocation, codec, and JSON
encoding policy. The CLI only prepares main, invokes the entry, and prints its
two encoded channels; it no longer synthesizes runtime/options modules or
interprets exec schemas.

Forma selects environment names from ordered `exec.capture-envs` actions; the
narrow native `capture_vars` operation materializes only those names. Existing
install IDs, argument rewriting, provenance, diagnostics, and atomic dry-run
output are retained by integration tests.
