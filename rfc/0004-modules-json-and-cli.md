# RFC 0004: Modules, JSON Data, and MVP CLI

- Status: Accepted for MVP
- Implementation: Complete

## Summary

This RFC introduces a closed static module graph, JSON data modules, an
explicit external JSON input boundary, and the command-line interface that ties
the MVP together.

## Import syntax

Imports are immutable top-level bindings:

```text
import config from "./config.json";
import defaults from "./defaults.xl";
```

Paths must be string literals and are resolved relative to the importing file.
The MVP supports `.xl` and `.json`. Dynamic paths, directory imports, package
names, and search paths are not supported.

Imports must appear with the other top-level declarations before the module's
result expression. Imports inside functions or nested blocks are rejected.

## Module values

Every `.xl` module exports exactly one immutable value: its final expression.
Bindings and type declarations are private to that module in the MVP.

A `.json` module exports its JSON root as an XL value:

```text
null          -> 'None
true          -> 'True
false         -> 'False
integer       -> Int, if representable as i64
non-integer   -> Float
string        -> String
array         -> Array
object        -> Dict
```

JSON object keys remain strings. Duplicate keys, invalid escapes, non-finite
numbers, and integers outside `i64` are errors. Dict canonicalization makes JSON
object expression order unobservable.

Imported values participate in structural type inference. A static JSON object
can therefore be checked against computed Type metadata without runtime IO.

## Closed module graph

The module loader canonicalizes paths, records every source/data dependency,
caches completed module values, and rejects import cycles with a diagnostic
that identifies the cycle.

Loading and evaluating a module uses a deterministic instruction budget per XL
module. Static imports are available to both tool-stage metadata evaluation and
program-stage execution as immutable constants.

The loader does not access the network, inspect environment variables, or load
native plugins.

## External JSON boundary

The `run` command may accept one genuine external JSON value:

```text
xl run program.xl --input request.json
xl run program.xl --input -
```

The first form reads a file and the second reads standard input. The decoded
value is bound as `input`. A program that does not request external input has no
`input` binding.

This boundary is not part of the static module graph. Its initial static type is
`Any`; programs recover guarantees explicitly:

```text
validate(Request, input)
```

The CLI does not implicitly validate or normalize external data.

## CLI

The workspace produces an `xl` binary with these commands:

```text
xl run <module.xl> [--input <file|->]
xl check <module.xl>
xl types <module.xl>
```

`run` loads, analyzes, and executes the root module, then prints its result.
Tool-stage or static errors prevent execution.

`check` loads and analyzes the full graph without intentionally executing the
root program-stage function. It prints `ok` and the dependency count on
success.

`types` prints deterministic lines for declared types, inferred bindings, and
the module result. This is a small structured analysis surface for human and
future LSP use; a machine JSON protocol is deferred.

All commands write diagnostics to standard error and return a nonzero exit code
on failure.

## Library API

The crate exposes:

- standalone JSON parsing from a named string;
- module loading/checking with an optional external binding map;
- the root module's bytecode, analysis, dependency list, and result execution.

Filesystem mutation and package resolution are outside the API.

## Deferred work

- YAML, TOML, text, and JSON Lines modules;
- package manifests, lock files, and remote dependencies;
- public named exports;
- incremental on-disk caches;
- a machine-readable diagnostic protocol and LSP server;
- streaming external input;
- effects and general runtime file access;
- source maps spanning imported modules.

## Implementation plan

Extend the lexer, AST, and parser for imports. Make analysis and compilation
accept resolved immutable external bindings. Implement a dependency-free JSON
parser, recursive module loader, CLI binary, examples, and integration tests.

## Acceptance criteria

1. JSON values map deterministically to XL values, including strict integer and
   duplicate-key errors.
2. JSON data modules participate in static structural annotation checks.
3. XL modules can import and compute with other XL module values.
4. Relative paths, dependency recording, caching, and cycle rejection work.
5. External JSON is available only when explicitly supplied as `input` and has
   static type `Any`.
6. `run`, `check`, and `types` succeed on an end-to-end example.
7. Tool-stage errors prevent `run` from executing the root program.
8. Module and CLI failures return useful diagnostics and nonzero status.
9. README instructions describe the MVP and its known limits.
10. Workspace tests, strict Clippy, and formatting checks pass.

## Implementation result

Implemented in the `xl` crate and binary. The dependency-free JSON parser,
closed recursive module loader, explicit `input` boundary, three CLI commands,
examples, and binary integration tests satisfy the acceptance criteria. The
README commands have been exercised end to end; workspace tests, strict Clippy,
and formatting checks pass.
