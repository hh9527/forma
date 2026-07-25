# RFC 0057: Canonical module identities and JSON string boundaries

- Status: Implemented
- Depends on: RFC 0004, RFC 0020, RFC 0035, RFC 0044, RFC 0045, RFC 0055, RFC 0056

## Amendment

The first accepted text rendered source modules as
`local:///path/file.json#json` and reserved equivalent HTTP fragments. Further
design established that format is resolution metadata, not resource identity.
This amendment removes format fragments and introduces a dependency namespace
resolved by a workspace `xl-deps.json` manifest.

Canonical module identities are now:

```text
core:json
local:///absolute/path/data.json
deps://models/domain/user.xl
```

Each identity resolves to exactly one format within a workspace snapshot.
Standard extensions determine almost every format. A manifest may provide a
rare exact override; individual imports do not carry format or target-type
parameters.

## Summary

XL assigns every resolved module one canonical logical identity. Core modules,
workspace-owned local modules, and externally owned dependency modules occupy
disjoint namespaces:

```text
core:<name>
local:///absolute/path
deps://<dependency>/<relative-path>
```

Resolution also produces the physical source and `ModuleFormat`, but neither
is recovered later by parsing the display String:

```rust
struct ResolvedModule {
    id: ResolvedModuleId,
    format: ModuleFormat,
    source: PhysicalSource,
}
```

Local import literals continue to use relative paths. Standard extensions
select XL, JSON, and future TOML/YAML parsers. Import remains an untyped,
once-only module dependency. Applications cache a typed projection by defining
an ordinary wrapper XL module.

For dynamic String input, `core:json` adds:

```xl
json.parse: Fn(String) -> Result(Any, BlameError)
json.decode: for(A) Fn(TypeOf(A), String) -> Result(A, BlameError)
```

## Motivation

The strict loader, recoverable workspace, and overlay owner currently use
related but scattered canonical paths, fallback joins, and formatted String
keys. The behavior is mostly once-only already, but ownership and dependency
resolution are implicit.

`local:` and `deps:` separate workspace source from external dependency source.
A dependency may be acquired from a sibling directory, Git, a registry, or a
future HTTP transport without exposing a machine-specific cache path in module
identity or diagnostics.

Typed imports are deliberately unnecessary. A target such as
`Array(models.User)` is a contextual HIR expression containing References, not
a flat cache-key name. Ordinary wrapper modules already provide explicit,
correct decode-result reuse through once-only module evaluation:

```xl
import raw from "./users.json";
import codec from "core:codec";
import result from "core:result";
import models from "deps://models/models.xl";

{users: result.unwrap(codec.decode(Array(models.User), raw))}
```

## Identity model

The semantic identity is:

```rust
enum ResolvedModuleId {
    Core(CoreModuleName),
    Local(ResolvedLocalPath),
    Dependency {
        dependency: ResolvedDependencyId,
        path: DependencyPath,
    },
}

enum ModuleFormat {
    Xl,
    Json,
    Toml,
    Yaml,
}
```

`WorkspaceModuleId(u32)` remains compact and snapshot-local. It may change when
a graph is rebuilt and is never a persistent cache key. Resolved identity is
used for equality, cycles, deterministic sorting, source sharing, and module
caching.

Within one workspace snapshot:

```text
ResolvedModuleId -> exactly one ModuleFormat
```

A conflicting format configuration is an error rather than a second module
projection. Manifest, overlay, or dependency-resolution changes create a new
workspace revision, so snapshot-local caches cannot reuse the old mapping.

## Core modules

Core capabilities retain their stable compact names:

```text
core:array
core:codec
core:json
```

Unknown core names remain recoverable graph nodes and strict load errors.

## Local resolution

An ordinary import remains:

```xl
import data from "../data/users.json";
```

Resolution:

1. resolves relative paths against the importing local or dependency module;
2. converts local targets to absolute paths;
3. uses filesystem canonicalization for existing files, including symlinks;
4. uses lexical `.`/`..` normalization for missing and overlay-only files;
5. rejects non-UTF-8 local identities;
6. determines format from the exact lowercase standard extension;
7. constructs a structured `ResolvedModule`.

Canonical display uses a `local:` URI:

```text
local:///workspace/data/users.json
```

Strict loading reports missing targets as I/O failures. Recoverable workspace
construction retains the normalized unavailable identity. Identity cannot
change within one snapshot; filesystem changes are observed by the next
revision.

The extension table is:

```text
.xl    -> Xl
.json  -> Json
.toml  -> Toml
.yaml  -> Yaml
.yml   -> Yaml
```

The first implementation parses only XL and JSON. TOML/YAML identities are
recognized but unavailable until their parsers exist. Uppercase variants are
not aliases. Content sniffing is forbidden.

## Dependency resolution

The workspace root may contain:

```text
xl-deps.json
```

The initial manifest supports path dependencies:

```json
{
  "dependencies": {
    "models": { "path": "../models" },
    "contracts": { "path": "vendor/contracts" }
  }
}
```

Source imports use logical identities:

```xl
import models from "deps://models/models.xl";
import schema from "deps://contracts/schema.json";
```

`deps://name/path` resolution:

1. requires `name` in the workspace manifest;
2. resolves the dependency root relative to the manifest directory;
3. canonicalizes the dependency root and records a snapshot resolution ID;
4. normalizes the module-relative path without allowing it to escape the root;
5. resolves symlinks and rejects a final physical target outside the dependency
   root;
6. derives format from an exact manifest override or standard extension;
7. retains the logical `deps:` identity in graphs and diagnostics rather than
   exposing the physical cache or checkout path.

Relative imports inside a dependency retain dependency ownership:

```text
deps://models/domain/user.xl
  + ../common.xl
  = deps://models/common.xl
```

A path dependency and workspace local path may physically reference the same
file but remain distinct logical ownership domains. The resolver may share
immutable source text internally; module identity does not silently cross the
ownership boundary.

The minimal optional override table is exact, not glob based:

```json
{
  "formats": {
    "deps://contracts/schema": "json"
  }
}
```

Override priority is:

```text
exact manifest override > standard extension > error
```

An override conflicting with a recognized extension is an error. Overrides are
primarily for extensionless external resources and are expected to be rare.

Git, registry, and remote acquisition are deferred. Future manifests may map a
dependency name to those transports while preserving `deps:` imports. HTTP is
therefore a dependency transport, not an import namespace required by source
code.

## Workspace root

The first implementation determines one workspace root from the root XL module:

- the nearest ancestor containing `xl-deps.json`, if present;
- otherwise the root module's parent directory.

Dependency imports without a manifest are errors. Nested dependency manifests
do not replace the root workspace resolution while building one graph.

Manifest contents participate in workspace revision input. Changing the
manifest invalidates the resolved graph and all snapshot-local module caches.
Persistent lockfiles and cross-process caches are deferred.

## Caching

The conceptual caches are:

```text
source_cache[PhysicalSourceIdentity] -> immutable text/bytes
module_cache[ResolvedModuleId]       -> parsed or evaluated root
```

The implementation may keep one module cache while supported resources map
one-to-one to physical source text. Imports resolving to the same module ID
read, parse or evaluate, promote, and publish exactly once per build.

An overlay shadows disk text for the same local identity. It does not create a
second module. Dependency overlays are deferred until editors have a concrete
need to address checked-out dependency documents through logical IDs.

No decode-result cache is added. Wrapper modules are the explicit memoization
unit. Two independently written wrappers may decode the same raw value twice;
callers requiring reuse import one shared wrapper.

## JSON String boundaries

`core:json` adds:

```xl
native parse: Fn(String) -> Result(Any, BlameError);
native decode: for(A) Fn(TypeOf(A), String) -> Result(A, BlameError);
```

`parse` maps valid JSON text to the same canonical JSON-shaped XL values used
by JSON modules. Syntax failure returns:

```xl
'Err({message: String, data: source, rule: 'Json})
```

`decode(T, source)` composes parse with `codec.decode(T, value)`. It preserves
one `BlameError` type across syntax and TypeMetadata mismatch, does not read a
file, creates no graph node, and promises no workspace-level memoization.

## Diagnostics and display

Human source excerpts retain readable physical paths. Graph/debug output uses
canonical logical IDs. Equivalent local spellings and equivalent dependency
imports resolve to one node and one root failure.

Format errors distinguish missing, unknown, reserved-but-unsupported, and
conflicting configured formats. `BlameError` remains content-versus-rule;
missing files, permissions, manifest errors, and future transport failures are
I/O or resolution errors.

## Non-goals

- Git, registry, HTTP, or HTTPS fetching;
- TOML or YAML parsing;
- per-import format/provider syntax;
- typed imports, `as=Type`, or decode-result memoization;
- content sniffing or `Content-Type` selection;
- package versions, lockfiles, import maps, or search paths;
- persistent caches across processes or revisions;
- dependency overlays;
- stable numeric workspace IDs.

## Implementation plan

1. introduce structured module identity, format, and physical source types;
2. centralize local, root, missing-target, and overlay resolution;
3. discover the workspace root and parse minimal `xl-deps.json` path entries;
4. resolve `deps:` imports without leaking physical paths into logical IDs;
5. key strict and recoverable graphs by resolved identity;
6. dispatch parsers from resolved format metadata;
7. add native JSON String parsing with structured `BlameError` failures;
8. add typed JSON decode using the existing codec transformation;
9. test aliases, symlinks, dependencies, escape rejection, missing nodes,
   overlays, format failures, cache reuse, JSON blame, and determinism.

## Acceptance criteria

1. equivalent relative, dotted, absolute, and symlinked local imports resolve
   to one local ID;
2. canonical local IDs display as absolute `local:` URIs without format
   fragments;
3. format is precise resolver metadata derived from an exact override or
   standard lowercase extension;
4. unknown, missing, uppercase, conflicting, and unsupported formats are
   deterministic errors and never content-sniffed;
5. strict loading, recovery, overlays, cycles, sorting, and lookup agree on
   identity;
6. same-ID JSON imports parse once and share one persistent root;
7. minimal path dependencies resolve through `xl-deps.json` to stable `deps:`
   graph identities;
8. dependency paths and symlinks cannot escape their declared roots;
9. `json.parse(String)` returns JSON-shaped `Any` or `BlameError`;
10. `json.decode(T, String)` returns `Result(T, BlameError)` with precise
    `TypeOf(T)` propagation;
11. wrapper modules remain the only decoded-resource cache mechanism;
12. existing XL/JSON programs, CLI, LSP snapshots, locations, quotas, and
    cancellation remain valid;
13. workspace tests, formatting, clippy, and strict checks pass.

## Rejected alternatives

### Encode format in a URI fragment

It makes interpretation look like resource identity and adds syntax to every
resolved name even though standard extensions or one workspace manifest
already determine the format. Resolution metadata provides the same cache
correctness with simpler logical IDs.

### Direct HTTP imports

They expose transport policy in source imports and immediately require
reproducibility, security, offline, redirect, credential, and cache semantics.
`deps:` keeps source stable while the manifest and future lockfile own
acquisition.

### Put `as=User` in module identity

`User` is a contextual HIR Reference and targets may be arbitrary type
expressions. Wrapper modules reuse the existing semantic dependency and
once-only evaluation machinery.

### Infer format from content

Content sniffing makes graph identity depend on parsing and creates ambiguous
or platform-specific behavior. Exact configuration and standard extensions are
deterministic before reading content.

## Implementation result

Implemented in July 2026.

- `ResolvedModuleId` now distinguishes `core:`, canonical absolute `local:`,
  and logical `deps:` identities. `ResolvedModule` carries the selected
  `ModuleFormat`; local and dependency variants retain their resolved physical
  paths so loaders never reconstruct them from display strings.
- One `ModuleResolver` is created for a root graph and is shared by strict and
  recoverable loading. Cache keys, cycle detection, semantic import edges, and
  workspace module names use resolved IDs. File reads and overlays use the
  associated physical path.
- The nearest `xl-deps.json` supplies minimal path dependencies and exact
  format overrides. Dependency-relative imports preserve ownership, while
  lexical and resolved-symlink escapes are rejected.
- XL and JSON dispatch through resolver metadata. TOML and YAML extensions are
  recognized deterministically but remain unavailable as specified by the
  non-goals. Missing, unknown, uppercase, and conflicting formats fail without
  content sniffing.
- `core:json` exports `parse` and generic `decode` with the accepted
  `BlameError` contracts. Parsing uses the existing canonical JSON lowerer;
  typed decode reuses the existing codec transformation. Syntax failures keep
  the original String in `data` and use `'Json` as `rule`.
- JSON String parsing accounts for the materialized value and Result wrapper
  against the VM allocation quota. Query checkpoints and existing cancellation
  behavior remain unchanged.
- Tests cover canonical display, dotted aliases, exact extensions, path
  dependencies, dependency-relative imports, lexical and symlink escape
  rejection, JSON success/failure, typed propagation, and shared module roots.

Deferred items are exactly the RFC non-goals: TOML/YAML parsers, non-path
dependency acquisition, dependency overlays, lockfiles, and persistent caches.

## JSON manifest amendment

The initial implementation used `xl-deps.toml` and the external `toml` crate.
Before the manifest acquired compatibility constraints, the format was changed
to `xl-deps.json`. The dependency model and resolution semantics are unchanged.

The resolver parses the manifest with XL's own lossless JSON parser and lowers
the resulting canonical XL value. This avoids carrying an external TOML/Serde
stack solely for a small configuration surface and ensures manifest syntax
errors use the same parser behavior as JSON modules. A future native TOML
parser should be implemented once, with the same lexer/CST/diagnostic quality
as JSON, and then serve both `.toml` modules and any future TOML-facing API.
