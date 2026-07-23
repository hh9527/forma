# RFC 0023: Three-tier worlds and ephemeral execution

- Status: Accepted for implementation
- Depends on: RFC 0011, RFC 0012, RFC 0021, RFC 0022

## Summary

XL execution uses exactly three ownership tiers:

```text
BottomWorld  <-  MainWorld  <-  LocalWorld
```

BottomWorld contains immutable engine capabilities. MainWorld contains the
published result of one closed module graph and is frozen before serving.
LocalWorld is private to one module initialization or one serving request and
is discarded as a unit.

Worlds do not own parent pointers. A temporary WorkView grants one execution
read access to Bottom and Main plus mutable access to Local. Publication is a
separate phase after the WorkView borrow ends.

## Motivation

RFC 0012 established a persistent heap plus execution arena. Core modules are
currently imported into every persistent heap, and the VM models only
`current/background`. General recursive worlds with Arc parents would require
world IDs, interior mutability, and arbitrary graph rules that the target
module-loader and serving workflows do not need.

The actual lifecycle is fixed:

```text
module loading:
    read Bottom + already-published Main
    allocate in one module Local
    publish exports Local -> Main
    discard Local

serving:
    read frozen Bottom + Main
    allocate in one request Local
    serialize the result
    discard Local without publication
```

Encoding these tiers directly keeps the copy-collection boundary and removes
the need for a general heap tree.

## Worlds

### BottomWorld

BottomWorld is built by the engine and then frozen. It contains only values
without user SourceIds or session state:

- core native module exports;
- fixed native closures and immutable core data;
- eventually well-known interned Atoms and primitive Type metadata.

The first implementation may construct one shared BottomWorld per Engine or
loader boundary. Process-global interning is not required by this RFC; the
observable invariant is immutability and sharing below MainWorld.

### MainWorld

MainWorld is mutable only while a module graph is loading. Each successful
module publishes one exported root into it and the module registry records that
root exactly once. Dependencies read previously published roots from the same
MainWorld.

After the root module is compiled, MainWorld is sealed and exposed only through
read APIs. It may contain references to Main and Bottom, never Local.

### LocalWorld

LocalWorld owns one VM value stack's allocated graph and linked prototypes. It
may reference all three tiers but only allocate into Local. Module and request
locals use the same representation; quota ownership and the caller's final
action distinguish them.

## WorkView

Execution receives capabilities rather than a recursively owned world:

```rust
struct WorkView<'a> {
    bottom: &'a BottomWorld,
    main: &'a MainWorld,
    local: &'a mut LocalWorld,
}
```

The exact borrowing split may expose immutable and mutable heap views
internally, but the rules are fixed:

- resolve handles and interned text from Bottom, Main, or Local;
- resolve modules from Main then Bottom;
- allocate objects, text, and shapes only in Local;
- never mutate Bottom or Main during VM execution.

Standalone expression execution uses empty Bottom and Main worlds with the
same VM path.

## Stable values and storage tags

Runtime handles use a fixed storage tag:

```text
Bottom
Main
Local
```

No arbitrary WorldId is needed because one WorkView contains at most one world
of each tier. A stable module/core root may belong to Bottom or Main. The
existing PersistentValue concept is therefore generalized semantically to a
non-Local stable root, even if a compatibility name remains temporarily.

Values in each tier obey:

```text
Bottom object edges: Bottom only
Main object edges:   Bottom or Main
Local object edges:  Bottom, Main, or Local
```

## Publication

Publication is not a WorkView operation:

```text
execute with &Main and &mut Local
drop WorkView
publish with &mut Main and &Local
```

Copying a root applies:

```text
Bottom reference -> preserve
Main reference   -> preserve
Local reference  -> copy/re-intern into Main
```

All requested roots share one forwarding context. Commit remains atomic: a
failed copy changes neither Main heap nor module registry. A published graph is
validated to contain no Local references.

Bottom construction uses a separate trusted freeze path and cannot import
Main or Local references.

## Module loading

Core imports resolve from BottomWorld and are not republished into MainWorld.
File modules and static data modules initialize in fresh LocalWorld instances
and publish to MainWorld. The existing dependency cache remains the
module-registry authority for this RFC; moving its complete state into World is
an internal follow-up that does not alter the ownership model.

The root module's function and external roots retain Main/Bottom links. A
loaded module owns frozen Bottom and Main worlds for later sessions.

## Serving sessions

Each execution of the loaded root or future handler creates a fresh LocalWorld:

```text
WorkView(&Bottom, &Main, &mut request_local)
```

The returned root is exported or serialized while the view is available, then
the entire request local is discarded. There is no request-to-Main publication
API. Stateful caches or process dictionaries remain explicit external-world
capabilities.

## Quotas

Bottom allocation is engine startup cost and outside module/session accounts.
Module initialization charges all Local requests through its module account,
including work later copied to Main. Serving charges its request Local through
the session account. Publication does not refund requested allocation.

## Atom and sequence scope

This RFC creates the Bottom tier needed for well-known ordinary interned Atoms,
but does not remove BuiltinAtom yet. It also does not change the semantic or
physical distinction between Array and Tuple. Those representation decisions
can be made independently after the world invariants are tested.

## Rejected alternatives

### World owns Option<Arc<World>> parent

Publishing from a child into its Arc-owned parent requires interior mutability
or persistent snapshots. It also permits unnecessary arbitrary world graphs.

### Heap IDs for every local execution

Local references never cross execution boundaries. Fixed tier tags are enough
and keep handles compact.

### Publish serving results back into Main

It would make requests interfere, require synchronization, and defeat arena
reclamation. Serving results cross the boundary through serialization.

## Deferred work

- process-global or cross-Engine BottomWorld sharing;
- replacing BuiltinAtom with Bottom-interned well-known Atoms;
- snapshot serialization and Bottom compatibility fingerprints;
- parallel independent module initialization and serialized atomic publish;
- moving all module loader state into MainWorld;
- direct streaming serialization from a request WorkView.

## Implementation plan

1. Extend storage ownership to Bottom, Main, and Local and teach read views to
   resolve all three tiers.
2. Introduce BottomWorld, MainWorld, LocalWorld, and temporary WorkView
   wrappers without parent ownership.
3. Build core module roots once in BottomWorld and link imports directly to
   them.
4. Generalize publication to preserve Bottom/Main references and relocate only
   Local references into Main.
5. Run module initialization and loaded-module execution in fresh LocalWorlds.
6. Seal MainWorld at loader completion and remove mutation APIs from serving
   paths.
7. Add ownership, core-sharing, module publication, failure atomicity, and
   repeated-session isolation tests.

## Acceptance criteria

1. Every runtime handle, intern ID, and shape ID belongs to Bottom, Main, or
   Local.
2. VM execution can read Bottom/Main and allocates only in Local.
3. Core module imports resolve to Bottom roots and do not grow Main heap.
4. Module publication preserves Bottom/Main references and copies Local roots
   atomically into Main.
5. Published Main graphs contain no Local references.
6. Main is immutable after module loading completes.
7. Repeated loaded-module executions use independent Local worlds and leave
   Main counts unchanged.
8. Serving results are exported without publication.
9. Existing source locations, quota accounting, recursive functions, codecs,
   debug observation, and CLI behavior remain compatible.
