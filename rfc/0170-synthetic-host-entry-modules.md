# RFC 0170: Synthetic Host entry modules

- Status: Implemented
- Depends on: RFC 0057, RFC 0059, RFC 0157 through RFC 0169

## Summary

Forma Host modes will be expressed as trusted synthetic Forma modules. For
`forma exec`, the Host generates an `@entry` module that imports `@main`,
checks its `exec` export against `ExecFn`, prepares `ExecSettings` and
`ExecRequest`, calls the function, encodes the resulting plan through ordinary
Forma libraries, and exports the encoded effect inputs.

Conceptually:

```forma
import "@main" as main;
import "std/dict" as dict;
import "std/json" as json;
import "std/rt-types/exec.forma" { ExecFn, ExecRequest, ExecEnv };
import "std/rt.native.forma" as rt;
import "std/opts.priv.forma" as opts;

let checked: ExecFn = main.exec;
let request: ExecRequest = {
    args: rt.args(),
    env: rt.envs(dict.get(opts.values, "exec.capture-envs")),
    cwd: rt.cwd(),
};
let output: ExecEnv = checked(rt.exec_settings(), request);
let install = json.encode(output.install)?;
let exec_opts = json.encode({
    cwd: output.cwd,
    bin: output.bin,
    args: output.args,
    env: output.env,
})?;

export { install, exec_opts };
```

The exact source may use narrower helpers while preserving this architecture.
Parsing, name resolution, type checking, evaluation, Result propagation, JSON
encoding, and export publication remain ordinary Forma semantics.

## Motivation

The current CLI selects `@main.exec`, checks only its runtime arity, constructs
request values in Rust, calls it directly, and manually validates and encodes
the returned `ExecEnv`. That duplicates protocol knowledge outside the
authoritative Forma type module and produces a bespoke typed boundary.

A synthetic entry turns the Host requirement into an ordinary authored type
assignment. A structurally equivalent `MyExecFn` passes; an incompatible
function fails before invocation through the normal checker. The same entry
can prepare Host input, propagate structured errors, and encode output without
a second Rust implementation of the protocol.

This also makes execution modes replaceable. A future embedding may provide a
different trusted entry source while reusing the same resolver, compiler, VM,
and narrow native resources.

## Module topology

Two reserved logical identities participate:

```text
@entry -> @main -> ordinary main-crate graph
   |
   +-> entry-only runtime/options modules
   +-> ordinary built-ins and dependencies
```

Resolution follows these rules:

1. only `@entry` may resolve `@main`;
2. no module, including `@main`, may resolve `@entry`;
3. `@entry` may resolve every registered or graph-visible module without
   ordinary crate/private restrictions;
4. this privilege belongs only to edges whose requester is exactly `@entry`;
   it is not inherited by imported modules;
5. `@main` retains its existing relative, source-root, dependency, private,
   and native-module rules.

`@entry` is Host-authored and cannot be supplied, shadowed, or imported by
user source. Its module ID and source identity are deterministic within one
mode invocation.

## Entry-only inputs

The Host exposes narrow modules rather than ambient globals:

- `std/rt.native.forma` supplies arguments, cwd, selected environment values,
  platform, and cache prefixes through Host-backed native functions;
- `std/opts.priv.forma` supplies the parsed immutable option actions from
  `@main` without rereading or re-evaluating source.

Ordinary modules cannot resolve either entry-only module. Importing `@main`
does not convey `@entry` privilege, so application code cannot use an indirect
module to acquire runtime access.

Option injection preserves action order and repeated keys. Any normalization
policy used by an entry is visible Forma code, not an implicit last-write-wins
Host rule.

## Typed boundary and diagnostics

The entry states:

```forma
let checked: ExecFn = main.exec;
```

This is structural compatibility, not nominal identity. A complete inferred
or differently named equivalent type is accepted. `Any`, incomplete facts,
wrong arity, incompatible parameters, and incompatible results are rejected.

Diagnostics must retain the `@main` export definition or annotation as the
user-facing anchor and may show the synthetic `@entry` requirement as a
secondary anchor. Users must not receive only an unexplained generated-source
location.

The Host retains a final defensive check on entry exports and effect payload
types. This protects embedding and VM boundaries but does not duplicate the
complete `ExecEnv` schema.

## Atomic effects

The synthetic module exports encoded values only after request preparation,
typed invocation, and all JSON encoding succeeds. Failed evaluation publishes
no partial install or process plan. The external executor receives concrete
payloads and performs no cache addressing, environment interpretation beyond
the declared protocol, or template expansion.

This phase remains dry-run only. It changes who computes and encodes the plan,
not whether effects are performed.

## Child sequence

1. RFC 0171: add reserved `@entry` identity and the non-transitive resolver
   permission matrix, with synthetic-source loading support;
2. RFC 0172: add entry-only runtime/options modules and preserve parsed option
   actions as immutable injected data;
3. RFC 0173: generate the exec entry adapter, move request preparation and
   JSON encoding into Forma, and remove the Rust `ExecEnv` serializer.

Each child is independently reviewable. An implementation may narrow helper
names but may not restore ambient Host globals or a parallel Rust schema.

## Goals

1. make Host mode contracts ordinary Forma type assignments and calls;
2. keep privilege in one explicit, non-transitive resolver root;
3. prepare Host input through narrow entry-only modules;
4. encode effect payloads through Forma's own typed codecs;
5. remove duplicated Rust knowledge of `ExecEnv` and `Install` shapes;
6. preserve deterministic, atomic, effect-free dry-run behavior;
7. establish a reusable architecture for run, build, and external entries.

## Non-goals

- user-authored or dynamically downloaded trusted entry modules;
- exposing runtime/options modules to `@main` or dependencies;
- general module capabilities, privilege delegation, or an effect system;
- actual download, installation, or process execution;
- changing the public `ExecFn` or `ExecEnv` protocol in this phase;
- making all CLI commands synthetic entries immediately;
- persisting entry modules across incompatible modes or Host inputs.

## Shared acceptance criteria

1. only `@entry` can resolve `@main` and entry-only modules;
2. imports from `@entry` do not transfer resolver privilege;
3. structurally equivalent entry functions pass the ordinary type checker;
4. incompatible entry functions fail before invocation with a source-linked
   diagnostic;
5. parsed repeated options reach entry code without lossy Host merging;
6. args, cwd, captured environment, platform, and prefixes are prepared by
   entry-visible runtime capabilities;
7. ExecRequest construction and ExecFn invocation occur in Forma source;
8. install and process payload encoding uses Forma JSON codecs;
9. Rust no longer contains a complete manual `ExecEnv`/`Install` serializer;
10. malformed plans and encoding failures publish no partial payload;
11. GCC-wrapper success and dual-source failure diagnostics remain intact;
12. repeated dry-runs are byte-identical and create no cache;
13. full workspace tests, formatting, and warning-denied Clippy pass.

## Stopping rules

Work returns to discussion if a child requires transitive privilege,
user-controlled trusted imports, ambient globals, weakened type checking,
partial publication, a second protocol schema in Rust, or real external
effects.

## Implementation result

RFCs 0171 through 0173 completed the phase. `ModuleId::Entry` and exact
resolver permissions establish the non-transitive trusted root; synthetic
loading and entry-only injected source modules expose closed Host snapshots;
and `forma exec --dry-run` now uses a generated Forma adapter for its contract,
invocation, structured encoding, and atomic publication.

The implementation does not add ambient globals, delegated privilege, an
effect system, or a parallel Rust protocol checker. Ordinary modules cannot
resolve `@entry`, `@main`, or entry-only inputs outside the explicitly allowed
edges. Full resolver, module, value-literal, adversarial CLI, GCC-wrapper, and
workspace tests provide the phase evidence described by the shared acceptance
criteria.
