# RFC 0115: Host native module registry

- Status: Implemented
- Depends on: RFC 0113, RFC 0114

## Summary

Forma adds a build-time registry for trusted Host native modules:

```rust
let mut builder = Engine::builder(config);
let assigned = builder.register_native_module(
    None,
    NativeModuleSpec::new(
        "@host/acme/secrets",
        DECLARATIONS,
        functions,
    ),
)?;
let engine = builder.build();
```

`Engine::new(config)` remains the no-Host-module convenience path.

## Registration

`register_native_module(id: Option<u32>, spec)` returns the assigned ID.

- `Some(id)` must be greater than 1023 and unused;
- `None` receives the smallest unused ID beginning at 1024;
- zero and reserved IDs are rejected by the public Host path;
- exhaustion is reported without partial registration;
- logical names must begin with `@host/`, have a non-empty remainder, and be
  unique within the builder.

The `@bim/` namespace remains exclusively owned by Forma. Module IDs and names
cannot alias one another. Registration order affects automatic IDs and is part
of the builder input; automatic IDs carry no cross-Engine stability promise.

## Specification

NativeModuleSpec contains a logical name, trusted Forma declaration source,
and native Function implementations. Declarations use the existing forms:

```forma
native type Secret = @1;
native load: Fn(String) -> Secret;
```

Functions remain linked by exported symbol name. Native type witnesses remain
linked by explicit local slot. This RFC does not add FuncId.

The source and implementation table are immutable after registration. Engine
construction consumes the builder and stores the completed registry behind
immutable shared ownership. Loaded modules and workspace snapshots cannot
observe subsequent registration because there is no registration API on
Engine.

Full declaration parsing, contract checking, and callback linking happen
through the same trusted-module linker used by core modules when a workspace
is built. Registry-level ID/name failures happen immediately at registration.

## Acceptance criteria

1. EngineBuilder registers Host module specs and returns assigned IDs;
2. explicit Host IDs are accepted only above the reserved range;
3. automatic allocation returns the smallest available Host ID;
4. duplicate IDs and names are rejected without modifying prior entries;
5. invalid and `@bim/` names are rejected;
6. Engine::new remains a compatible empty-registry shortcut;
7. built Engines expose no mutation path for their registry;
8. core module IDs and Host module IDs cannot collide;
9. native Functions remain name-linked and native types remain slot-linked;
10. registration tests cover boundaries, gaps, collisions, and isolation; and
11. full workspace tests and strict Clippy pass.

## Implementation plan

1. publish NativeModuleSpec and EngineBuilder;
2. implement range/name validation and deterministic allocation;
3. freeze registered specs into Engine;
4. generalize the trusted module installer to accept core and Host specs;
5. add focused registry and linker tests;
6. record the implementation result.

## Non-goals

- import resolution, which belongs to RFC 0116;
- registry mutation after build;
- dynamic code loading or unloading;
- FuncId, binary ABI tables, or bytecode specialization;
- persistent identity for automatically assigned IDs; or
- untrusted declaration source.

## Implementation result

Implemented public NativeModuleSpec and EngineBuilder APIs while retaining
`Engine::new` as the empty-registry shortcut. Registration validates the
`@host/...` namespace, rejects reserved and duplicate IDs and duplicate names,
and allocates the smallest free Host ID for `None`. Failed registrations leave
both indexes unchanged. `build` consumes the builder and stores modules as an
immutable ID-ordered Arc slice in Engine.

The trusted installer now links fixed core specs and frozen Host specs through
one path. It revalidates the core/Host ID partition, explicit native type
slots, declaration/implementation completeness, arity, and requested type
witnesses. Tests cover explicit and automatic IDs, gap-free allocation after
failed attempts, all registry conflicts, frozen ordering, Host type-witness
linking, and compatibility of an ordinary Engine load. Import visibility is
intentionally deferred to RFC 0116.
