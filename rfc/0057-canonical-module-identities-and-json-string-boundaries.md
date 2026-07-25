# RFC 0057: Canonical module identities and JSON string boundaries

- Status: Accepted
- Depends on: RFC 0004, RFC 0020, RFC 0035, RFC 0044, RFC 0045, RFC 0055, RFC 0056

## Summary

XL assigns every resolved module one canonical, self-describing identity:

```text
core:json
local:///absolute/path/model.xl#xl
local:///absolute/path/data.json#json
```

The source URI identifies bytes or text. The mandatory fragment identifies the
format used to interpret that source. Local import literals continue to use
ordinary relative paths; the resolver derives the format exclusively from a
small standard extension table and emits the canonical identity.

Import remains an untyped, once-only module dependency. Applications that want
a cached typed projection define an ordinary XL wrapper module and rely on
existing once-only module evaluation:

```xl
import raw from "./users.json";
import codec from "core:codec";
import result from "core:result";
import models from "./models.xl";

{users: result.unwrap(codec.decode(Array(models.User), raw))}
```

For dynamic String input, `core:json` adds `parse` and `decode` Result
boundaries:

```xl
json.parse: Fn(String) -> Result(Any, BlameError)
json.decode: for(A) Fn(TypeOf(A), String) -> Result(A, BlameError)
```

## Motivation

The strict loader, recoverable workspace, and overlay owner currently use
related but scattered `PathBuf`, filesystem canonicalization, fallback joins,
and formatted String keys. The behavior is mostly once-only already, but the
identity protocol is implicit and cannot be extended to non-file sources
without changing every layer.

A canonical module identity makes caching and graph equality explicit. It also
separates source acquisition from interpretation:

```text
source identity: local:///absolute/path/data.json
module identity: local:///absolute/path/data.json#json
```

This distinction anticipates future HTTP sources without enabling network
imports now. One fetched source could later admit a statically selected format:

```text
https://example.com/config#json
https://example.com/config#yaml
```

Typed imports are deliberately unnecessary. A type annotation is an HIR
expression containing contextual References, not a flat cache-key name.
Ordinary wrapper modules already provide explicit, correct decode-result reuse
without introducing a second projection graph.

## Resolved module identity

The semantic model is:

```rust
enum ResolvedModuleId {
    Core(CoreModuleName),
    Source {
        source: ResolvedSourceId,
        format: ModuleFormat,
    },
}

enum ResolvedSourceId {
    Local(ResolvedLocalPath),
    // Reserved, not implemented: Http(CanonicalUrl),
}

enum ModuleFormat {
    Xl,
    Json,
    Toml,
    Yaml,
}
```

The canonical display forms are:

```text
core:<canonical-name>
<canonical-source-uri>#<canonical-format-name>
```

Canonical format names are lowercase `xl`, `json`, `toml`, and `yaml`.
`.yaml` and `.yml` both normalize to `#yaml`.

`WorkspaceModuleId(u32)` remains a compact, snapshot-local projection. It may
change when a graph is rebuilt and must never be used as a persistent cache
identity. `ResolvedModuleId` is the equality, cycle, sorting, source-sharing,
and module-cache identity.

## Local resolution

An ordinary local import remains:

```xl
import data from "../data/users.json";
```

Resolution performs these steps:

1. reject empty, non-UTF-8, unknown-extension, and unsupported-format targets;
2. resolve a relative path against the importing local module's directory;
3. convert the path to an absolute path;
4. for an existing target, use filesystem canonicalization, including symlink
   resolution;
5. for a missing target or an overlay-only document, lexically normalize `.`
   and `..` without consulting nonexistent filesystem components;
6. classify the final standard extension;
7. construct the canonical source URI and append the canonical format fragment.

Strict loading still reports a missing target as an I/O failure. Recoverable
workspace construction retains the normalized unavailable module identity.
Identity is immutable within one workspace snapshot. If creating a file or
changing a symlink changes its filesystem-canonical identity, the next
revision rebuilds the graph with the new identity.

The first implementation recognizes only lowercase extensions:

```text
.xl    -> #xl
.json  -> #json
```

`.toml`, `.yaml`, and `.yml` are reserved mappings but remain unsupported until
their parsers exist. Uppercase variants are not aliases. No format is inferred
from file contents or HTTP `Content-Type`.

Local paths display as `local:` URIs with an absolute path and percent encoding
where required:

```text
local:///workspace/data/users.json#json
```

The implementation stores structured paths and formats; it does not recover
them later by parsing its own display String. Platform-native path equality is
preserved internally. URI rendering is deterministic and lossless for
supported UTF-8 paths.

## Core identities

Core modules retain their compact, disjoint identities:

```text
core:array
core:codec
core:json
```

They do not gain `#xl`: a core identity selects an installed capability, not a
source interpreted by a format provider. Unknown core names remain graph nodes
in recoverable tooling and strict errors at execution boundaries.

## Source and module caching

The conceptual caches are:

```text
source_cache[ResolvedSourceId] -> immutable text/bytes
module_cache[ResolvedModuleId] -> parsed or evaluated module root
```

The first implementation may retain one module cache because every supported
local extension selects exactly one format. The identity split is still
observable and authoritative. Imports resolving to the same ID read, parse or
evaluate, promote, and publish exactly once per loader/snapshot build.

An overlay shadows disk text for the same local source identity. It does not
create a second module or alter the `#format` fragment. All source locations,
diagnostics, dependency edges, and module lookup use the same resolved ID.

No decode-result cache is added. Wrapper modules are the explicit memoization
unit for typed projections. Two independently written wrappers may decode the
same raw module twice; callers that require reuse import one shared wrapper.

## Future source schemes

HTTP and HTTPS identities are reserved design constraints, not enabled
capabilities:

```text
https://host/path/data.json#json
https://host/api/config#yaml
```

The fragment is a client-side format selector and is not sent in an HTTP
request. A future resolver will canonicalize scheme, host, default ports, dot
segments, and percent encoding while preserving query semantics. A standard
path extension may supply the fragment; an extensionless URL must provide it;
an explicit fragment and extension must agree.

The fragment namespace is reserved for module format. It is not a JSON Pointer
or YAML document selector. Resource projections require a separate future
syntax and identity component.

Network imports require a separate RFC covering reproducibility, redirects,
offline operation, quotas, timeouts, credentials, host policy, and snapshot
consistency.

## JSON String boundaries

`core:json` adds:

```xl
native parse: Fn(String) -> Result(Any, BlameError);
native decode: for(A) Fn(TypeOf(A), String) -> Result(A, BlameError);
```

`parse` maps valid JSON text to the same canonical JSON-shaped XL values used
by `.json` modules. A syntax failure returns:

```xl
'Err({
    message: String,
    data: source,
    rule: 'Json,
})
```

The error's data side retains the input String location. The initial message
contains the JSON parser's deterministic line, column, and expectation. No
synthetic file module or module-cache entry is created for a dynamic String.

`decode(T, source)` is semantically:

```xl
result.flat_map(json.parse(source), fn(value) {
    codec.decode(T, value)
})
```

It preserves one `BlameError` type across JSON syntax and TypeMetadata
mismatches. It does not read files, participate in the module graph, or promise
workspace-level memoization.

## Diagnostics

Diagnostics display the human source path at primary locations and may display
the canonical resolved module ID in graph/debug output. Equivalent import
spellings must resolve to one node and one root failure. Blocked dependents do
not duplicate a failed source diagnostic.

Format errors distinguish:

- missing extension;
- unknown extension;
- reserved but unsupported format;
- explicit future fragment conflicting with a standard extension.

`BlameError` remains reserved for content-versus-rule mismatches. Missing files,
permissions, and future transport failures are I/O failures.

## Non-goals

- HTTP or HTTPS fetching;
- TOML or YAML parsing;
- explicit `format=` or provider import syntax;
- typed imports, `as=Type`, or decode-result memoization;
- content sniffing or `Content-Type` format selection;
- package names, search paths, import maps, or version resolution;
- persistent caches across processes or workspace revisions;
- stable numeric workspace IDs;
- resource fragments or sub-document selection.

## Implementation plan

1. introduce structured resolved source, format, and module identity types;
2. centralize root, relative import, missing-target, and overlay resolution;
3. key strict and recoverable module graphs by resolved identity;
4. render canonical `local:///...#format` and stable core identities;
5. dispatch parsers from the resolved format rather than ad hoc extensions;
6. add native JSON String parsing with structured `BlameError` failures;
7. add typed JSON decode while reusing the existing codec transformation;
8. test alias paths, symlinks, missing nodes, overlays, format failures, cache
   reuse, JSON syntax blame, typed decode, and workspace determinism.

## Acceptance criteria

1. equivalent relative, dotted, absolute, and symlinked local imports resolve
   to one `ResolvedModuleId`;
2. canonical local IDs display as absolute `local:` URIs with mandatory
   lowercase format fragments;
3. `.yaml` and `.yml` reserve the same `#yaml` identity while reporting the
   format as unsupported in this implementation;
4. unknown, missing, and uppercase extensions are deterministic errors and are
   never content-sniffed;
5. strict loading, recoverable graph construction, overlays, cycles, sorting,
   and module lookup agree on identity;
6. the same JSON ID is read and parsed once per build and imported edges share
   one persistent root;
7. `json.parse(String)` returns JSON-shaped `Any` or `BlameError`;
8. `json.decode(T, String)` returns `Result(T, BlameError)` and preserves the
   precise `TypeOf(T)` relationship;
9. wrapper XL modules remain the only decoded-resource cache mechanism;
10. existing XL/JSON programs, CLI behavior, LSP snapshots, source locations,
    quotas, and cancellation remain valid;
11. workspace tests, formatting, clippy, and strict static checks pass.

## Rejected alternatives

### PathBuf alone is the permanent module identity

It is sufficient for today's local loader but conflates acquisition with
format and forces a later identity migration for non-file sources. The
structured ID retains native paths internally while defining the extensible
protocol now.

### Put `as=User` in the resolved URI

`User` is a contextual HIR Reference and the target may be an arbitrary type
expression such as `Array(models.User)`. It has no stable flat-name identity
before semantic resolution and TypeMetadata evaluation. Wrapper modules reuse
the existing reference, dependency, and once-only evaluation machinery.

### Let fragment override a local extension

Allowing `file.json#yaml` creates competing interpretations of one local file
and makes editor and build-tool behavior disagree. Local standard extensions
and fragments must agree; ordinary path imports simply derive the fragment.

### Infer HTTP format from Content-Type

It makes module identity unavailable until after a request and is frequently
misconfigured in practice. Future HTTP imports use standard path extensions or
an explicit format fragment.
