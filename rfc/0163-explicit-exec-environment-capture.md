# RFC 0163: Explicit exec environment capture

- Status: Implemented
- Depends on: RFC 0062, RFC 0113, RFC 0157, RFC 0159, RFC 0162

## Summary

An executable root declares the process environment names supplied through
`ExecRequest.env`:

```forma
option "exec.capture-envs" ["TARGET", "SDKROOT"];
```

`forma exec` will pass only declared names that exist in the Host environment.
The option is an ordered, repeatable, immediate action. It does not read the
environment during Forma evaluation and does not grant a general Host effect.

## Motivation

The current adapter copies every UTF-8 process environment entry into
`ExecRequest.env`. That makes a pure entry depend on undeclared ambient state,
needlessly exposes secrets, and prevents a future evaluator cache from knowing
which Host inputs belong in its key.

The GCC-wrapper needs `TARGET`, but `TARGET` is not a universal exec protocol
field. Explicit capture keeps the stable request shape general while making
this application's external inputs visible next to its dependency lock data.

## Semantics

Each payload is an `Array(String)`. Actions are processed in source order and
their names are deduplicated by first occurrence. An empty Array is valid.
Duplicate names within or across actions are harmless and do not duplicate a
Dict field.

For every effective name, the Host performs one lookup while constructing the
request:

- an existing UTF-8 value becomes a `String` field;
- an absent variable produces no field;
- a present non-UTF-8 value is a Host error naming the variable;
- undeclared variables are neither inspected nor copied.

The resulting Dict remains canonically ordered by the ordinary Dict runtime.
Forma code handles required-variable absence explicitly with `dict.get` and
`Option`/`Result`; capture itself does not turn absence into an empty String or
a generic Host failure.

`exec.capture-envs` is allowed only in `@main`. Other commands may load a root
containing it, but only `forma exec` interprets it. Imported modules cannot
expand their caller's ambient authority.

## Core/Host boundary

Parsing and immediate-value validation remain in `forma-core`. A successfully
loaded root exposes its validated ordered option actions to its embedding
Host. This is read-only module metadata, separate from evaluated exports.

The CLI consumes that metadata after module loading and before invocation. It
must not reopen or independently parse the source file: doing so would diverge
for overlays, future non-file roots, diagnostics, and option validation.

The captured input set and captured values are sufficient input material for
a future execution cache key. This RFC does not add that cache.

## Goals

1. make every environment input to an exec entry explicit in source;
2. prevent accidental disclosure of unrelated process environment values;
3. preserve `ExecRequest` as a general protocol rather than adding `TARGET`;
4. give embedding Hosts validated option metadata without source re-parsing;
5. leave missing-variable policy in deterministic user-space Forma code.

## Non-goals

- environment mutation, fallback values, wildcard capture, or prefix matching;
- capturing files, stdin, time, randomness, credentials, or platform probes;
- exposing an `env.get` Function to Forma evaluation;
- adding an execution-result cache;
- making options available as ordinary Forma values;
- changing argv or current-working-directory capture in this RFC.

## Acceptance criteria

1. without `exec.capture-envs`, `ExecRequest.env` is empty;
2. declared existing variables are passed and undeclared variables are absent;
3. repeated actions and duplicate names have deterministic first-seen meaning;
4. malformed payloads report their option source location before evaluation;
5. the option is rejected outside `@main`;
6. non-UTF-8 errors mention only a declared variable;
7. dry-run output reveals exactly the captured environment Dict returned by
   the entry;
8. `LoadedModule` exposes validated options without exposing mutable parser
   state or requiring the CLI to parse source again;
9. existing run, build, check, show, and LSP behavior remains deterministic;
10. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. retain validated immediate root option actions in `LoadedModule`;
2. add a narrow read-only option query suitable for embedding Hosts;
3. validate and fold `exec.capture-envs` in the exec CLI adapter;
4. change request construction from ambient enumeration to named lookup;
5. cover secrecy, absence, duplicates, malformed values, and root-only scope;
6. update the GCC-wrapper fixture and documentation.

## Stopping rules

Work returns to discussion if implementation requires runtime evaluation of an
option, a Forma-visible environment capability, import-dependent option
values, or a command-specific field added to the shared `ExecRequest` type.

## Implementation result

Implemented in August 2026. `LoadedModule` now retains validated immediate
option values and exposes a read-only keyed iterator to embedding Hosts. The
exec adapter folds all `exec.capture-envs` actions, deduplicates names by first
occurrence, and performs named Host lookups instead of enumerating the ambient
environment. No declaration therefore produces an empty request environment.

Validation remains in `forma-core` and labels the authored option. CLI tests
cover repeated actions, missing names, undeclared secret exclusion, canonical
dry-run output, and malformed payloads. Module tests cover metadata exposure
and rejection from imported modules. The VM and standard library gained no
environment capability.
