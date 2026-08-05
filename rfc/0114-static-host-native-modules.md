# RFC 0114: Static Host native modules

- Status: Proposed
- Depends on: RFC 0059, RFC 0113

## Summary

Forma lets an embedding Host assemble a fixed native module registry before it
builds an Engine. Forma programs may import registered modules, but cannot
register modules, choose native IDs, load dynamic libraries, or mutate the
registry at runtime.

```text
EngineBuilder
  -> register native module specifications
  -> validate and allocate Host module IDs
  -> build immutable Engine
  -> resolve and link Forma workspaces
```

This is an umbrella RFC. RFC 0115 defines registration, ID allocation, and
freezing. RFC 0116 defines deterministic import resolution and workspace
projection. The umbrella becomes Implemented after both children pass the full
workspace quality gate.

## Authority boundary

Native modules are Host capabilities. Their declaration source and Rust
callbacks are trusted inputs to Engine construction. Forma source, embedded
manifests, dependency manifests, and imported packages cannot create or alter
them.

Core modules retain reserved IDs `1..=1023`. Host modules use
`1024..=u32::MAX`. An explicitly assigned Host ID is stable when the embedding
application preserves the assignment. An automatically assigned ID is unique
only within the built Engine and must not be treated as a persistent ABI.

## Phase sequence

1. RFC 0115: add an Engine builder, Host native module specifications,
   explicit/automatic ID allocation, collision checks, and immutable registry
   ownership;
2. RFC 0116: resolve registered `@host/...` imports in strict and recoverable
   loading, publish module facts, and test snapshot sharing.

Each child receives a proposal commit and a separate implementation/result
commit.

## Shared acceptance criteria

1. only the Host can register native modules;
2. explicit IDs must be in the Host range and unique;
3. automatic IDs are deterministic within one builder state and never occupy
   the reserved range;
4. logical module names are unique and cannot use the `@bim/` namespace;
5. Engine construction freezes an immutable registry;
6. registered native declarations use explicit type slots and name-linked
   Functions;
7. strict loading, recovery, async queries, and snapshots observe the same
   frozen registry;
8. unknown Host imports produce sourced module diagnostics;
9. no dynamic loading, unloading, or runtime registration is added; and
10. full workspace tests and strict Clippy pass.

## Non-goals

- dynamic libraries, plugins, FFI negotiation, or native package discovery;
- Forma-driven capability requests or manifest permissions;
- stable FuncId or `CALL_NATIVE` bytecode specialization;
- Host resource tables, finalizers, invalidation, or ownership;
- persistent bytecode/type caches; or
- registration changes after Engine construction.

