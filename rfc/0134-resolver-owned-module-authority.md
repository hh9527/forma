# RFC 0134: Resolver-owned module authority

- Status: Implemented
- Depends on: RFC 0048, RFC 0108, RFC 0116

## Summary

Module privilege is determined by resolver provenance rather than by a module
name prefix. Every resolved module carries one authority:

```text
Ordinary
PackageSystem
RuntimeSystem
```

The runtime supplies an exact built-in list to the resolver. A non-relative
request is looked up in that list before ordinary vtops and dependencies. A
hit has `RuntimeSystem` authority. Names such as `std/array` and `core/dyn` are
ordinary registered module IDs and do not grant authority by spelling.

Filesystem modules ending in `.forma-sys` have `PackageSystem` authority. They
may be imported directly only by modules with the same resolved crate identity
and may not be root modules. A dependency may expose a safe `.forma` facade,
but another crate cannot address its `.forma-sys` implementation directly.

`native fn` and `native type` declarations require system authority. Parsers
still preserve these declarations everywhere so tools can report a sourced
permission diagnostic; ordinary `.forma` modules are rejected before type
analysis. System authority does not itself grant IO or other Host capability:
native implementations remain explicitly registered by the runtime.

## Resolution

For a request from `requester`:

1. Relative requests resolve only inside the requester's crate.
2. Other non-special requests first use an exact built-in-list lookup.
3. A miss falls back to the existing vtop/dependency resolution rules.
4. A `.forma-sys` result is accepted only when requester and target have the
   same crate identity.

Resolved origin, not the source request, controls authority. Import aliases and
names beginning with `core/` or `std/` cannot manufacture privilege.

## Implementation result

Implemented in module IDs, resolver construction, normal loading, workspace
recovery, Host native registration, and native-declaration validation. Active
standard-library imports use `std/...`; the historical `@bim/...` RFC text is
unchanged.

Tests cover built-in precedence over a same-named vtop, runtime and package
authority, same-crate system imports, cross-crate denial, root denial, ordinary
native-declaration denial, and the distinct unregistered-system diagnostic.
