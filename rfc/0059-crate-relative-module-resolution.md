# RFC 0059: Crate-relative module resolution

- Status: Accepted
- Depends on: RFC 0004, RFC 0044, RFC 0057, RFC 0058

## Summary

Separate source import locators from deterministic resolved module identities.
Every resolution is performed against an immutable resolver snapshot:

```text
resolve(requester: ModuleId, request: ImportRequest) -> ModuleId
```

Physical source acquisition is a later operation:

```text
locate(module: ModuleId) -> SourceLocation
```

Neither relative filesystem locations nor checkout cache paths are module
identities.

## Crate layout

The working crate has this conventional layout:

```text
xl-deps.json
src/
  path/to/module.xl
bin-src/
  command-name.xl
```

Files below `src/` are importable working-crate modules. Selecting a file below
`bin-src/` as an executable entry maps it to the unique graph root `@main`; the
`bin-src` directory and source filename do not enter its module identity.

`@main` is observable by graph, diagnostic, debug, and tooling APIs but is not
searchable or importable. `bin-src` files cannot import one another. Entries use
`@src/...` or root-relative `./...` requests to import reusable program modules;
the physical `bin-src` directory is intentionally not part of the graph.

## Resolved module identities

One `ModuleId` model represents requesters and results:

```text
@main                 selected graph entry
@src/model/user.xl    working-crate source module
models/model/user.xl  dependency-crate source module
@bim/std/array        built-in module
```

The corresponding owners are `Main`, `Working`, `Dependency(alias,
resolution)`, and `Builtin`. A dependency's visible crate name is the key under
which its parent manifest mounts it, not a name embedded in its repository.
The exact dependency resolution remains part of equality and cache identity
even though stable diagnostics show only the alias.

The same physical source may therefore be `@src/model.xl` while its repository
is the working crate and `parser/model.xl` when another crate mounts it as
`parser`. Physical source text may be shared; resolved module evaluation may
not be shared across those owners.

## Import requests

Source import strings are parsed as follows:

```text
./path/to/a.json       relative to the requester directory
../../path/to/a.json   relative, without crossing the current crate root
models/path/to/a.json  absolute in manifest dependency `models`
@src/path/to/a.json    absolute in the requester's current crate
@bim/std/array         absolute in the built-in module space
```

`@src` is contextual request syntax. From a working-crate requester it resolves
to `@src/...`; from a requester owned by dependency `models` it resolves to
`models/...`. It never gives a dependency access to the consuming working
crate.

Relative and `@src` imports preserve owner. A bare first component changes
owner through the root dependency table. `@bim` selects the built-in owner.
There is no import spelling for `@main`.

Examples:

```text
resolve(@main, "@src/a.xl")              = @src/a.xl
resolve(@src/foo/b.xl, "../a.xl")        = @src/a.xl
resolve(parser/foo/b.xl, "../a.xl")      = parser/a.xl
resolve(parser/foo.xl, "@src/model.xl")  = parser/model.xl
resolve(@src/foo.xl, "parser/model.xl")  = parser/model.xl
resolve(parser/foo.xl, "@bim/std/array") = @bim/std/array
```

Empty paths, unknown dependency aliases, attempts to address `@main`, and paths
that lexically escape their owner root are errors. After physical lookup,
symlink targets must remain inside the corresponding crate source root.

## Built-in modules

The public built-in namespace is `@bim`. Existing `core:*` spellings are
replaced rather than retained as aliases:

```text
core:array       -> @bim/std/array
core:attributes  -> @bim/std/attributes
core:codec       -> @bim/std/codec
core:debug       -> @bim/std/debug
core:dict        -> @bim/std/dict
core:hash        -> @bim/std/hash
core:json        -> @bim/std/json
core:option      -> @bim/std/option
core:result      -> @bim/std/result
```

The implementation of a built-in module may combine XL and native functions;
that detail does not affect its identity.

## Manifest and publication

The independent `xl-deps.json` remains the development configuration. A future
entry-only declaration embeds its publishable literal subset:

```xl
$manifest {
    name: "command-name",
    dependencies: {
        parser: {git: "https://example/parser.git", rev: "<commit>"},
    },
};
```

`$manifest` contains only JSON-compatible immediate data and is decoded by the
same configuration model as `xl-deps.json`. It may occur only in an entry and
does not evaluate in the VM. A packaging command will project development path
dependencies to pinned publication dependencies and combine the manifest with
one `bin-src` entry in a file without an `.xl` extension.

Git acquisition, embedded module bundles, stdin roots, HTTP roots, and actual
effectful `xl exec` remain later `locate`, package, and execution work. They do
not change the resolution contract in this RFC.

## Implementation plan

1. represent main, working, dependency, and built-in modules in one logical ID;
2. map the selected root to `@main` and `src/` modules to `@src/...`;
3. parse relative, bare dependency, `@src`, and `@bim` import requests;
4. retain physical paths only on resolved source records used by `locate`;
5. migrate built-in and dependency imports to the new public spelling;
6. enforce lexical and physical crate boundaries and the unimportable root;
7. update module graphs, tooling projections, tests, and RFC 0057 terminology.

## Acceptance criteria

1. requester and result are deterministic `ModuleId` values;
2. working source IDs do not expose absolute filesystem paths;
3. dependency IDs use manifest aliases and do not expose checkout paths;
4. `@src` resolves against the requester's owner;
5. relative imports cannot cross a crate root;
6. `@main` cannot be produced by import resolution;
7. built-in imports and identities use only `@bim/std/...`;
8. `src`, `bin-src`, path dependencies, JSON formats, symlink containment,
   workspace recovery, semantic queries, LSP, and execution tests pass.

## Resolver implementation result

Commit `6e09f3d` implements the deterministic resolver core. One public
`ModuleId` now represents `@main`, working-crate `@src/...`, dependency alias,
and `@bim/...` identities. Physical paths live on resolved source records rather
than module IDs. The selected root maps to `@main`; a conventional `src/`
directory is the working source root, with a compatibility fallback for
standalone files that have no crate layout.

Imports now support owner-preserving relative paths, contextual `@src/...`,
bare dependency aliases, and `@bim/std/...`. Lexical normalization and physical
symlink containment enforce crate boundaries. `@main` is rejected as an import
target, and cycles remain detectable among ordinary source modules. Existing
`core:` and `deps:` public spellings were removed from implementation and test
fixtures rather than retained as aliases.

The implementation migrates workspace and semantic graph projection to logical
IDs, adds crate-layout and contextual-`@src` tests, and passes the complete
workspace suite and strict Clippy checks. The embedded `$manifest`, pinned git
location, packaging, stdin, and HTTP phases remain pending, so this RFC remains
Accepted rather than Implemented.
