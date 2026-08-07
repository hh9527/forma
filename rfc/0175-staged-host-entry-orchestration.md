# RFC 0175: Staged Host entry orchestration

- Status: Implemented
- Amends: RFC 0170 through RFC 0174

## Summary

Evolve a selected `entry/` module from a statically linked adapter into a
trusted Host orchestration program. The Host parses only the main root's
immediate options, creates an unforgeable pending module handle, loads the
selected entry without the main graph, and invokes its exported `entry`
function with that handle.

Entry code may observe Host state, install typed virtual modules, explicitly
initialize the pending main module, project and check its exports, and finally
interpret the resulting plan. Main imports do not resolve until entry calls
`initialize_module`; that call freezes the virtual module registry and marks
the transition from the open Host world to the closed main world.

```forma
export def entry: Fn(Module) -> Result(EntryOutput, BlameError) = fn(pending) {
    inject_modules_by_options(pending.options)?;
    let initialized = rt.initialize_module(pending.module)?;
    let raw_exec = rt.module_export(initialized, "exec")?;
    let exec = rt.check_type(ExecFn, raw_exec)?;
    encode_output(exec(make_settings(), make_request())?)
};
```

## Authority model

Only the Host-selected entry can initially resolve entry runtime modules.
Those modules may expose effectful native functions such as argument,
environment, filesystem, module injection, installation, or process APIs.
The resolver prevents ambient acquisition by main and dependencies but does
not track capability values: trusted entry code may deliberately pass a
native function or a narrowed closure into main.

This is explicit delegation, not leakage. Forma does not add capability taint,
an effect system, or VM restrictions on ordinary function flow.

## Stages

### Pending

The Host resolves the main physical root and parses immediate options without
loading imports, checking the complete program, or evaluating bindings. It
constructs a `Module` record containing ordered option actions and an opaque
`ModuleHandle`.

### Entry preparation

The selected entry is compiled and evaluated independently. Its native runtime
may read current Host state and register typed virtual modules. Injection is an
ordinary native function with a controlled side effect, not syntax or a macro.

### Initialization

`initialize_module(handle)` atomically freezes the injection registry, loads
the main graph with the ordinary resolver/checker/compiler/VM, and returns an
opaque `InstantiatedModule`. Failure preserves authored import, data, type, and
entry call provenance. A handle cannot select a different physical root.

### Projection and effects

Entry obtains exports as `Dyn` and uses the authoritative Forma type relation
to project them to an expected type. It may then return an encoded dry-run
result or invoke effectful runtime APIs. Effect policy belongs to the selected
entry, not the CLI dispatcher.

## Child sequence

1. RFC 0176 defines pending and instantiated module handles, option exposure,
   explicit initialization, lifecycle, quotas, and diagnostics;
2. RFC 0177 defines `entry/rt.native.forma`, direct Host observations, and the
   typed virtual module registry frozen by initialization;
3. RFC 0178 defines dynamic export lookup and `TypeOf(A)`-guided projection
   using the authoritative type checker;
4. RFC 0179 migrates exec to an exported entry function and retains dry-run as
   the first effect interpreter.

## Shared acceptance criteria

1. loading entry does not resolve or evaluate main imports;
2. main options are available in source order before initialization;
3. only unforgeable Host-created handles can initialize a main root;
4. entry controls the exact moment the injected registry freezes;
5. main imports can consume modules installed before initialization;
6. injection after initialization fails deterministically;
7. selected-entry capability is non-ambient but explicitly delegable as a
   normal value;
8. export checking accepts structural equivalents and rejects incompatible
   values with main and entry provenance;
9. Rust does not duplicate Forma module interfaces or exec result schemas;
10. exec dry-run behavior, deterministic recipes, and atomic output remain;
11. formatting, full workspace tests, and warning-denied Clippy pass.

## Non-goals

- a general dynamic import expression for ordinary Forma code;
- concurrent mutation of a module graph after initialization;
- capability tracking or an effect system;
- user-selected trusted entry source;
- actual installation or process replacement in this phase;
- migrating run and build modes in the child sequence.

## Stopping rules

Return to discussion if implementation requires making main initialization
implicit, exposing entry runtime modules ambiently, weakening export type
checks to unchecked `Any`, allowing registry mutation after freeze, or adding
a second Rust implementation of Forma's type compatibility relation.

## Implementation result

RFCs 0176 through 0179 implement the staged boundary. The Host prepares an
opaque handle, evaluates the selected entry independently, and invokes its
exported function. Main loading starts only at explicit initialization. Typed
modules are invocation-local and frozen by that call; export projection uses
Forma type schemes, including inferred generic exports. Exec remains dry-run
and emits no partial output on failure.

The ABI passes `ModuleHandle` directly instead of wrapping it in a `Module`
record. Ordered options and invocation inputs are explicit queries on that
handle, retaining the same authority without duplicating an aggregate shape.
