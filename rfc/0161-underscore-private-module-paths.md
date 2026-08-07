# RFC 0161: Underscore-private module paths

- Status: Proposed
- Depends on: RFC 0116, RFC 0134, RFC 0160

## Summary

Forma replaces the special `.forma-sys` suffix with two composable path
conventions:

```text
public.forma       public across crates
_private.forma     crate-private source
__native.forma     crate-private privileged source
_data.json         crate-private data
```

Any logical path segment beginning with `_` makes the module crate-private.
Only a `.forma` file whose file name begins with `__` receives package-system
authority and may contain native declarations.

## Motivation

Visibility is not specific to Forma source. Packages also need private JSON,
TOML, and YAML inputs, so encoding privacy in `.forma-sys` cannot describe the
general rule.

Privacy and native authority are also different properties. `_model.forma`
should hide an implementation without granting Host linkage privileges.
`__regex.forma` needs both privacy and the authority to declare registered
native symbols. One underscore expresses the common visibility boundary; the
second is a narrow source-only privilege marker.

## Resolution rules

A module path is private when any normalized logical path segment starts with
`_`. This makes a directory such as `_internal/` a private subtree. The check
uses the resolved logical path inside its crate, not unrelated underscore
segments in an absolute filesystem path.

Private modules:

- may be imported with relative or owner-absolute requests from the same
  crate;
- cannot be selected through a dependency request from another crate;
- cannot be used as `@main`;
- retain deterministic ordinary module IDs.

Public files contain no underscore-prefixed path segment and may be imported
through their dependency name.

## Authority

For package files, `PackageSystem` authority is granted only when all of these
conditions hold:

1. the module format is Forma;
2. the final file name starts with `__`;
3. the final file name ends with `.forma`.

`__data.json` is private but ordinary data. An underscore directory does not
grant authority to ordinary Forma files below it.

`RuntimeSystem` authority remains determined by the runtime's built-in
registration. Built-ins use the same privileged source discipline in their
embedded implementation files, but their public logical request such as
`std/array` is independent of that physical file name.

## Native declarations

`native type` and `native` Function declarations remain legal only under
`PackageSystem` or `RuntimeSystem` authority. Renaming an ordinary file does
not manufacture a Host implementation: module loading still verifies every
declaration against the registered native module contract.

## Goals

1. express crate privacy uniformly for source and data modules;
2. separate ordinary privacy from native declaration authority;
3. make private subtrees possible without new manifest configuration;
4. keep built-in logical IDs independent from embedded physical names;
5. remove the `.forma-sys` format exception completely.

## Non-goals

- item-level visibility modifiers;
- friend crates or selective package exports;
- granting native authority to data modules;
- deriving built-in authority from `std/` or `core/` prefixes;
- retaining `.forma-sys` compatibility;
- changing historical RFC text.

## Acceptance criteria

1. `_x.forma`, `_x.json`, and `_dir/x.forma` resolve inside their owner crate;
2. dependency imports of those paths are rejected as private;
3. public dependency modules remain importable;
4. `__x.forma` receives `PackageSystem` authority and accepts registered native
   declarations;
5. `_x.forma` and `__x.json` do not receive native authority;
6. private files cannot be root modules;
7. `.forma-sys` is no longer a recognized module format;
8. embedded privileged modules use `__name.forma` physical files while keeping
   their public registered requests;
9. formatting, workspace tests, and warning-denied Clippy pass.

## Implementation plan

1. replace suffix checks with normalized private-path and privileged-source
   predicates;
2. migrate resolver errors and coverage to `_` and `__` paths;
3. rename embedded privileged module files and update their inclusions;
4. update active diagnostics and documentation;
5. record implementation results and mark this RFC Implemented.
