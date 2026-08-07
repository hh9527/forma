# RFC 0172: Entry-only injected modules

- Status: Implemented
- Depends on: RFC 0162, RFC 0163, RFC 0170, RFC 0171

## Summary

Synthetic entries will receive Host inputs through two immutable synthetic
Forma modules:

```forma
import "std/rt.native.forma" as rt;
import "std/opts.priv.forma" as opts;
```

`std/rt.native.forma` exports the concrete invocation arguments, cwd,
captured environment, platform, and cache prefixes. `std/opts.priv.forma`
exports the parsed ordered option actions from `@main`. Both are generated and
registered by the Host for one entry load, compiled as ordinary Forma source,
and unavailable to every requester except `@entry`.

## Runtime input surface

The initial runtime module exports concrete immutable values:

```forma
export let args = ["-c", "main.c"];
export let cwd = "/workspace";
export let env = { TARGET: "x86_64-linux-gnu" };
export let platform = { os: "linux", arch: "x86_64" };
export let download_prefix = "/cache/downloads";
export let install_prefix = "/cache/installs";
```

Values, rather than effectful getter functions, keep the injected module
closed and deterministic. The Host reads process state once before module
evaluation. Repeated reads in Forma observe the same value and perform no
additional effect.

The `.native.forma` suffix marks Host ownership and entry-only authority; this
phase does not require authored `native fn` declarations inside the generated
source.

## Option surface

Options remain ordered actions rather than a merged mapping:

```forma
export let actions = [
    { key: "exec.capture-envs", value: ["TARGET"] },
    { key: "exec.capture-envs", value: ["CCACHE_DIR"] },
];
```

Each action value is the already parsed immediate Forma value. The generated
module does not reread `@main`, reinterpret syntax, or apply last-write-wins.
Entry library code may select, concatenate, validate, or reject actions
explicitly.

## Resolver authority

The two exact requests are registered only for a synthetic entry load.
Resolution succeeds only when requester ID is `ModuleId::Entry`. A direct
import from `@main`, a dependency, a built-in, or a module imported by entry
fails even though the registration exists.

This exact allowlist is narrower than RFC 0171's physical private-module
privilege. No prefix such as `std/` or suffix such as `.priv.forma` grants
entry-only authority by itself.

## Source and types

The Host serializes only values already accepted at its boundary into Forma
literal syntax. Generated source is registered under its logical module ID and
passes the ordinary parser, immediate expression semantics, type inference,
compiler, quota, and publication path. Unsupported values fail entry
construction rather than widening to `Any` or becoming opaque.

The module interface is produced by normal analysis. Entry code therefore
gets structural types for args, env, platform, prefixes, and option actions;
Rust does not hand-author a parallel `ModuleInterface`.

## Non-goals

- ambient process access during evaluation;
- general Host-resource modules or native closures with captured state;
- exposing secrets not explicitly selected for the invocation;
- merging option actions in Rust;
- making arbitrary generated modules entry-only;
- persisting generated input modules across invocations.

## Acceptance criteria

1. only `@entry` resolves the two exact injected requests;
2. authority is non-transitive and exact-name based;
3. runtime input is snapshotted before evaluation and represented as literals;
4. repeated options preserve source order and duplicate keys;
5. generated modules obtain interfaces through ordinary Forma analysis;
6. entry source can construct typed request/settings records from exports;
7. unsupported injected values fail before entry execution;
8. `@main` cannot import or observe either module;
9. modules exist only for the current synthetic load;
10. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. add exact entry-only request registration to the synthetic loader;
2. compile and publish injected source modules before compiling `@entry`;
3. add deterministic literal serialization for the required closed values;
4. generate runtime and option action modules from Host snapshots;
5. cover permission, ordering, typing, lifetime, and rejection behavior.

## Stopping rules

Work returns to discussion if completion requires ambient mutable state,
transitive privilege, hand-built semantic interfaces, lossy option merging,
or bypassing ordinary Forma compilation.

## Implementation result

Implemented in August 2026. Synthetic entry loading accepts an exact map of
logical module names to in-memory Forma sources. Those names are registered in
a resolver allowlist visible only to `ModuleId::Entry`; importing one from
`@main` or a transitive module produces a private-module diagnostic.

Injected modules compile, evaluate, publish, and derive interfaces through the
ordinary ModuleLoader before entry compilation. They have virtual source
identities and no fabricated physical paths. `Value::to_forma_literal`
serializes the closed immediate value subset used by runtime snapshots and
option actions while rejecting functions, opaque/dynamic/type values,
non-finite floats, unsafe Dict keys, and unsafe constructors. Tests generate a
runtime module from Host values, preserve repeated option action order, verify
entry visibility and main denial, and exercise unsupported-value rejection.
