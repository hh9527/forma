# RFC 0161: Explicit private module suffixes

- Status: Implemented
- Depends on: RFC 0116, RFC 0134, RFC 0160

## Summary

Forma replaces the special `.forma-sys` suffix with explicit visibility and
authority suffixes:

```text
model.forma          public across crates
model.priv.forma     crate-private source
codec.native.forma   crate-private privileged source
data.priv.json       crate-private data
```

A penultimate `.priv` suffix makes any supported source or data module
crate-private. A `.native.forma` suffix makes Forma source crate-private and
grants package-system authority. `.native` is reserved for Forma source;
`name.native.json` and equivalent data paths are rejected.

## Motivation

Visibility is not specific to Forma source. Packages also need private JSON,
TOML, and YAML inputs, so encoding privacy in `.forma-sys` cannot describe the
general rule.

Privacy and native authority are also different properties. `model.priv.forma`
should hide an implementation without granting Host linkage privileges.
`regex.native.forma` needs both privacy and the authority to declare registered
native symbols. The distinct suffix makes that narrow source-only privilege
visible at the declaration boundary.

## Resolution rules

A module is private when its final logical file name ends in `.priv.<format>`
or `.native.forma`. The check uses the normalized logical path inside its crate,
not its absolute filesystem path. Directory names do not implicitly change
visibility.

Private modules:

- may be imported with relative or owner-absolute requests from the same
  crate;
- cannot be selected through a dependency request from another crate;
- cannot be used as `@main`;
- retain deterministic ordinary module IDs.

Other supported files are public and may be imported through their dependency
name.

## Authority

For package files, `PackageSystem` authority is granted only when all of these
conditions hold:

1. the module format is Forma;
2. the final file name ends with `.native.forma`.

`data.priv.json` is private but ordinary data. A `.priv.forma` file does not
receive native authority.

`RuntimeSystem` authority remains determined by the runtime's built-in
registration. Embedded built-ins use `.native.forma` when their source contains
native declarations, but their public logical request such as `std/array` is
independent of that physical file name.

## Native declarations

`native type` and `native` Function declarations remain legal only under
`PackageSystem` or `RuntimeSystem` authority. Renaming an ordinary file does
not manufacture a Host implementation: module loading still verifies every
declaration against the registered native module contract.

## Goals

1. express crate privacy explicitly and uniformly for source and data modules;
2. separate ordinary privacy from native declaration authority;
3. make visibility apparent at each module file;
4. keep built-in logical IDs independent from embedded physical names;
5. remove the `.forma-sys` format exception completely.

## Non-goals

- item-level visibility modifiers;
- friend crates or selective package exports;
- granting native authority to data modules;
- directory-level privacy or package export lists;
- deriving built-in authority from `std/` or `core/` prefixes;
- retaining `.forma-sys` compatibility;
- changing historical RFC text.

## Acceptance criteria

1. `x.priv.forma` and `x.priv.json` resolve inside their owner crate;
2. dependency imports of those paths are rejected as private;
3. public dependency modules remain importable;
4. `x.native.forma` receives `PackageSystem` authority and accepts registered
   native declarations;
5. `x.priv.forma` does not receive native authority and `x.native.json` is
   rejected;
6. private files cannot be root modules;
7. `.forma-sys` is no longer a recognized module format;
8. embedded privileged modules use `name.native.forma` physical files while
   keeping their public registered requests;
9. formatting, workspace tests, and warning-denied Clippy pass.

## Implementation plan

1. replace suffix checks with normalized private-module and privileged-source
   predicates;
2. migrate resolver errors and coverage to `.priv` and `.native` paths;
3. rename embedded privileged module files and update their inclusions;
4. update active diagnostics and documentation;
5. record implementation results and mark this RFC Implemented.

## Implementation result

The resolver now treats `.priv.<format>` and `.native.forma` as crate-private,
rejects private roots and cross-crate requests, and reserves `.native` against
non-Forma formats even when a manifest format override exists. Only
`.native.forma` package files receive `PackageSystem` authority.

All embedded privileged sources use `.native.forma` physical names. Their
registered `core/...` and `std/...` requests and runtime-system authority remain
unchanged. Resolver and module-loading tests cover public, private data,
private ordinary source, privileged source, invalid native data, and native
registry enforcement.
