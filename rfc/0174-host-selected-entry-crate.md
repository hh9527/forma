# RFC 0174: Host-selected entry crate

- Status: Implemented
- Amends: RFC 0138, RFC 0170 through RFC 0173

## Summary

Replace the synthetic `@entry` module identity with a Host-selected module
from the reserved `entry/` crate. Exec selects `entry/exec.forma`; the module
keeps its stable resolved ID and authored source while receiving the exact,
non-transitive permission to resolve `@main` and invocation-local injected
modules.

Module options are restricted to `@main` until a concrete file-local consumer
exists. An option in an ordinary source or dependency module is an error rather
than a silently ineffective setting.

## Entry registry

`entry/` joins `core/` and `std/` as a runtime-reserved crate. Its modules are
embedded or registered by the Host and cannot be supplied, shadowed, or
imported by ordinary Forma source. Initial entries are ordinary Forma source:

```text
entry/exec.forma
```

The `.native.forma` suffix remains reserved for modules that declare native
functions or types. Host trust and native declarations are independent, so an
entry does not acquire that suffix merely because the Host selects it.

The loading API accepts a registered entry ID rather than generated source:

```text
load_entry(main_path, "entry/exec.forma", bindings, injected_modules)
```

Unknown or non-entry IDs fail before graph construction.

## Resolution

The resolver carries the selected entry module ID as invocation context:

1. only the exact selected entry may resolve `@main`;
2. only that entry may resolve invocation-local injected module names;
3. entry privilege is tested from the requester ID on every edge and is not
   inherited by imports;
4. ordinary imports of `entry/`, including the selected template, fail;
5. `@entry` is no longer a request or resolved ID;
6. `@main` remains the dynamic reference to the Host-selected user root.

Relative imports from an entry follow the registered entry crate's normal
module layout. Stable entry IDs improve diagnostics, semantic snapshots, and
future LSP inspection without making entry templates public libraries.

## Exec migration

The exec adapter moves from a Rust string constant to
`modules/entry/exec.forma` and is embedded in `forma-core`. Runtime and ordered
option snapshots remain invocation-local injected modules, renamed under the
entry namespace:

```text
entry/rt.priv.forma
entry/opts.priv.forma
```

They are not registered entry templates and remain visible only to the
selected entry. The CLI selects `entry/exec.forma`, supplies snapshots, and
defensively consumes the two encoded String exports as before.

## Root-only options

Options currently describe Host/crate invocation policy. Only `@main` has a
defined consumer through the selected entry, so option actions in every other
module are rejected with their authored location. Dependencies communicate
configuration through ordinary exports that `@main` explicitly composes.

Future file-local compiler options require a separate RFC naming their phase,
scope, merge behavior, and consumer; this RFC does not reserve silent option
storage for that possibility.

## Acceptance criteria

1. `ModuleId::Entry` and the `@entry` request disappear;
2. exec loads embedded `entry/exec.forma` by stable module ID;
3. ordinary modules cannot import any `entry/` template;
4. only the selected entry resolves `@main` and injected modules;
5. privilege remains non-transitive;
6. unknown entry selections fail deterministically;
7. exec behavior, atomic output, provenance, and structural contract checking
   remain unchanged;
8. options in `@main` preserve repetition and order;
9. options in ordinary and dependency modules are diagnosed;
10. formatting, full workspace tests, and warning-denied Clippy pass.

## Non-goals

- user-selectable or downloaded entry templates;
- actual exec effects;
- migrating run/build in this RFC;
- granting native declaration authority to entry modules;
- defining file-local compiler options.

## Implementation result

Implemented in August 2026. The exec adapter now lives in the embedded
`modules/entry/exec.forma` source and is loaded with the stable resolved ID
`entry/exec.forma`. `Engine::load_entry` and `load_entry_with_modules` accept
only registered entry names; unknown selections fail before resolver or VM
work.

`ModuleId::Entry` and generated exec source were removed. The resolver instead
holds the exact selected `ModuleId` and compares every requester against it.
Only that ID resolves `@main`, private dependency edges, and the exact injected
`entry/rt.priv.forma` and `entry/opts.priv.forma` names. Ordinary `entry/`
imports fail, and imported children do not inherit the selected-root identity.

The strict and recoverable loaders now diagnose every option outside
`ModuleId::Main` at its authored location. Main options retain their ordered,
repeated action representation and continue to reach the exec entry snapshot.
CLI contract, malformed-output atomicity, structural alias, environment,
GCC-wrapper, and dual-source provenance coverage remains intact.
