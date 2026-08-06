# RFC 0137: Bootstrap prelude artifact

- Status: Implemented
- Depends on: RFC 0136

## Summary

Forma replaces independent calls that assemble runtime prelude values, static
types, and generic schemes with one explicit bootstrap artifact:

```rust
struct BootstrapPrelude {
    values: BTreeMap<String, Value>,
    types: HashMap<String, TypeDescriptor>,
    schemes: HashMap<String, TypeScheme>,
}
```

The artifact is created for the active tool VM and passed as one unit into HIR
resolution and type analysis. Partial/recoverable analysis uses the same
constructor and projects the values and visible names it needs.

## World ownership

`Value` and `PersistentValue` are owned by a VM heap or `MainWorld`; they cannot
be cached globally on `Engine` without also moving heap ownership to `Engine`.
This RFC does not make that unrelated architectural change.

The bootstrap artifact is therefore a deterministic, reconstructible artifact
for an analysis world. RFC 0138 will execute `core/prelude` once in each
`MainWorld` and freeze its root there. All modules in that world reuse the
frozen result.

## Boundary

This RFC is behavior-preserving. It does not yet move `struct`, `enum`,
`union`, or `validate`; it makes their current duplication visible inside one
owned structure so RFC 0138 can remove them atomically.

The constructor must validate that every generic scheme refers to a runtime
and static binding. Runtime-only hidden implementation bindings are explicitly
exempt and remain inaccessible to HIR name resolution.

## Acceptance criteria

1. Strict and recoverable analysis construct one `BootstrapPrelude` rather
   than independently calling three prelude helpers.
2. HIR visible names, tool values, static types, and schemes are projections
   of that artifact.
3. Public scheme names have corresponding runtime and static entries.
4. Hidden implementation bindings are not HIR-visible.
5. Existing inference, module, LSP, and runtime behavior remains unchanged.

## Implementation result

Strict and recoverable analysis now construct `BootstrapPrelude` and project
HIR-visible names, tool values, static types, and generic schemes from that
owned artifact. Its constructor checks scheme projection consistency in debug
builds. Hidden `\0forma_pack_dyn` remains available to internal lowering but is
no longer offered as an HIR-visible prelude name.

The old independent entry points have become private artifact builders. This
keeps behavior stable while giving RFC 0138 one owned boundary from which to
remove the ordinary callable capabilities.
