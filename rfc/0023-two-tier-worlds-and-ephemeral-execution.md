# RFC 0023: Two-tier worlds and ephemeral execution

- Status: Implemented
- Depends on: RFC 0011, RFC 0012, RFC 0021, RFC 0022

## Summary

XL execution uses exactly two ownership tiers:

```text
MainWorld <- WorkWorld
```

MainWorld contains engine-provided core modules and the published results of
one closed module graph. WorkWorld is private to one module initialization or
one serving request and is discarded as a unit.

Worlds do not own parent pointers. A temporary WorkView grants one execution
read access to Main plus mutable access to Work. Publication is a separate
phase after execution.

## Motivation

RFC 0012 established a persistent heap plus execution arena. Its real
lifecycle already has two tiers: stable values shared by a loaded application
and temporary values created by one evaluation. A separate Bottom tier for
engine values would only help multiple Main worlds share one object heap. That
is not currently required and would force every runtime operation to resolve a
third storage origin.

The required lifecycle is:

```text
module loading:
    install core modules in Main
    read already-published Main
    allocate in one module Work
    publish exports Work -> Main
    discard Work

serving:
    read frozen Main
    allocate in one request Work
    serialize or export the result
    discard Work without publication
```

## MainWorld

MainWorld has a building phase and a frozen phase. Building proceeds in this
order:

1. install trusted core native modules outside module quota accounting;
2. initialize the closed module graph in dependency order;
3. atomically publish each successful module root;
4. seal Main before returning a loaded module.

Main stores core roots, module roots, interned text, shapes, linked bytecode,
and immutable heap objects. A frozen Main exposes no mutation API to serving
execution.

## WorkWorld

WorkWorld owns one VM execution's value graph and linked prototypes. It can
reference Main and Work values but allocates only Work values. Module and
request work use the same representation; quota ownership and the caller's
final action distinguish them.

A module initializer may publish its root into a building Main. A serving
request may only export or serialize its result and then discard Work.

## WorkView

Execution receives capabilities rather than a recursively owned world:

```rust
struct WorkView<'a> {
    main: &'a MainWorld,
    work: &'a mut WorkWorld,
}
```

The exact borrowing split may expose an immutable heap resolver internally,
but these rules are fixed:

- resolve handles, text, shapes, bytecode, and module roots from Main or Work;
- allocate objects, text, and shapes only in Work;
- never mutate Main during VM execution.

Standalone expression execution uses an empty Main and the same VM path.

## Stable values and storage tags

Runtime handles use fixed storage tags:

```text
Main
Work
```

No arbitrary WorldId is needed because a WorkView contains at most one world
of each tier. The compatibility name `PersistentValue` means a stable Main
root.

Object edges obey:

```text
Main object edges: Main only
Work object edges: Main or Work
```

## Publication

Publication is not a WorkView operation:

```text
execute with &Main and &mut Work
drop WorkView
publish with &mut building Main and &Work
```

Copying a root applies:

```text
Main reference -> preserve
Work reference -> copy or re-intern into Main
```

All requested roots share one forwarding context. Commit remains atomic: a
failed copy changes neither Main heap nor module registry. A published graph
is validated to contain no Work references.

Core module construction is a trusted Main-building path. It runs before any
module account and cannot import Work references.

## Module loading

Core imports resolve directly to roots already installed in Main. File modules
and static data modules initialize in fresh WorkWorld instances and publish to
Main. The dependency cache remains the module-registry authority for this RFC.

The root module's function and external roots retain Main links. A loaded
module owns a frozen Main for later sessions.

## Serving sessions

Each execution of the loaded root or a future handler creates a fresh
WorkWorld. The returned root is exported or serialized while Main and Work are
available, then Work is discarded. There is no request-to-Main publication
API. Stateful caches or process dictionaries remain explicit external-world
capabilities.

## Quotas

Trusted core installation is engine startup cost and outside module/session
accounts. Module initialization charges all Work allocation through its module
account, including values later copied to Main. Serving charges its Work
allocation through the session account. Publication does not refund requested
allocation.

## Representation scope

This RFC does not remove `BuiltinAtom`, unify Array and Tuple, define snapshots,
or add parallel module initialization. Those decisions are independent of the
two-tier ownership invariant.

## Rejected alternatives

### Separate BottomWorld for core values

It enables heap-level sharing across multiple Main worlds, but introduces a
third reference origin into every VM operation. Core values can be installed
once in each Main before accounting begins. Cross-application sharing can be
revisited with immutable Main snapshots if measurements justify it.

### World owns Option<Arc<World>> parent

Publishing into an Arc-owned parent requires interior mutability or persistent
snapshots and permits arbitrary world graphs that XL does not need.

### Heap IDs for every Work execution

Work references never cross execution boundaries. Fixed tier tags keep handles
compact and make invalid publication structurally detectable.

### Publish serving results back into Main

It would make requests interfere, require synchronization, and defeat arena
reclamation. Serving results cross the boundary through serialization.

## Deferred work

- immutable Main snapshots and compatibility fingerprints;
- parallel independent module initialization with serialized atomic publish;
- moving all module-loader state into MainWorld;
- direct streaming serialization from a request WorkView;
- measuring whether process-wide sharing of core values is worthwhile.

## Implementation plan

1. Rename storage ownership to Main and Work and teach views to resolve both.
2. Introduce MainWorld, WorkWorld, and temporary WorkView boundaries.
3. Install core modules directly into Main before module accounting.
4. Publish module roots by preserving Main references and relocating Work.
5. Seal Main after loading and remove mutation access from serving paths.
6. Add ownership, publication, failure atomicity, and session-isolation tests.

## Acceptance criteria

1. Every runtime handle, intern ID, and shape ID belongs to Main or Work.
2. VM execution reads Main and allocates only in Work.
3. Core modules are installed once in Main before module quota accounting.
4. Publication preserves Main references and atomically copies Work roots.
5. Published Main graphs contain no Work references.
6. Serving code cannot mutate or publish into frozen Main.
7. Repeated sessions use independent Work worlds and leave Main unchanged.
8. Existing locations, quotas, recursion, codecs, debug output, and CLI behavior
   remain compatible.

## Implementation result

Runtime storage tags are now exactly `Main` and `Work`. Module loading owns a
mutable `MainWorld`; completion consumes it into `FrozenMainWorld`, which is
the only stable world exposed to loaded-module sessions. VM execution returns
a `WorkWorld` and constructs a temporary `WorkView` for Main/Work resolution.
The lower-level `HeapView` remains an internal read-only handle resolver.

All core modules are installed into Main before module quota accounting starts.
JSON and XL module initialization allocate in Work and publish atomically into
Main. Publication preserves existing Main references, relocates reachable Work
objects, and rejects invalid Work handles without partially changing Main.
Session execution can export its result but has no mutable Main or publication
capability.

Tests cover Main-edge preservation, Work relocation, failure atomicity,
one-time dependency publication, core availability, and repeated sessions
leaving frozen Main allocation counts unchanged. `PersistentValue` remains as
a compatibility name for a stable Main root.
