# RFC 0157: Reusable executable-plan applications

- Status: Implemented
- Depends on: RFC 0057 through RFC 0059, RFC 0062, RFC 0063, RFC 0100,
  RFC 0113 through RFC 0117, RFC 0140 through RFC 0147

## Summary

Forma will make one realistic executable-plan application work end to end:
a thin GCC wrapper entry locks ordinary dependencies, imports toolchain data
and reusable Forma logic, and exports a typed pure `ExecFn` that `forma exec
--dry-run` can evaluate and validate.

```forma
#!/usr/bin/env -S forma exec --dry-run --

option "crate.dependency" {
    name: "gcc-toolchain-define",
    source: 'Path({path: "../gcc-toolchain-define"}),
};
option "crate.dependency" {
    name: "gcc-wrapper",
    source: 'Path({path: "../gcc-wrapper"}),
};
option "exec.capture-envs" ["TARGET"];

import "std/rt-types/exec.forma" { ExecFn };
import "gcc-toolchain-define/source.json" as source;
import "gcc-wrapper/toolchain.forma" { wrap_gcc };

export def exec: ExecFn = wrap_gcc(source);
```

This is an umbrella RFC. Child RFCs will verify cross-module Function
execution, establish ordinary runtime-protocol type modules, add literal-only
top-level Host options, use exact local dependency resolution, fill the
small Dict/argv library gaps exposed by the wrapper, and finally validate the
complete source-data-to-canonical-plan path.

The phase ends at a strict, deterministic dry-run. It does not download or
unpack toolchain archives and does not launch a process.

## Motivation

Forma already has most isolated ingredients required by an executable-plan
application: static data modules, explicit imports and exports, ordinary pure
Functions, structural types, hashing, immutable Array/Dict combinators, a
typed executable protocol, and a Host adapter that prints canonical JSON.

The GCC wrapper thought experiment combines them into a real requirement:

1. select a Host-specific GCC package and a TARGET-specific sysroot;
2. plan both installations without coupling their cache identities;
3. reuse one wrapper implementation for gcc, g++, and ar;
4. rewrite argv with sysroot and deterministic source/debug prefix maps;
5. diagnose malformed source data and invalid Host input at their origins;
6. produce a complete `ExecEnv` before any external effect occurs.

Trying this program exposed more useful priorities than adding isolated
syntax. The initial temporary audit reported an exported-closure up-link
failure, so the first child must reduce and verify that correctness boundary
before changing the VM. Dict lookup cannot yet express a domain-specific
missing TARGET error. Exact Path dependencies are sufficient to validate this
phase without coupling it to remote acquisition. The stable exec protocol also remains
coupled to its current module placement rather than an explicit runtime-types
surface.

This phase treats those findings as one application-driven correctness and
composition milestone.

## Architectural boundary

The target preserves three distinct layers:

```text
fixed dependency data
    -> pure Forma validation and transformation
    -> typed executable plan
    -> Host dry-run
```

`std/rt-types/exec.forma` describes values crossing the Host boundary. Merely
importing `ExecFn` grants no effect capability. The user selects `forma exec`;
that Host supplies `ExecSettings` and `ExecRequest`, calls the explicit `exec`
export, validates the resulting `ExecEnv`, and prints a canonical plan.

`option "crate.dependency"` is static Host input, not a runtime Forma expression
that performs network access. The resolver reads it before module evaluation,
obtains a fixed module graph, and then applies ordinary deterministic import
resolution. Application code cannot add dependencies during evaluation.

The wrapper remains an ordinary module. GCC package selection, decoding,
validation, hashing, and argv rewriting do not become VM instructions, new
effects, or a toolchain-specific DSL.

## Phase sequence

The planned child sequence is:

1. RFC 0158: verify and specify promoted cross-module Function environments,
   including exported higher-order closures and module-level helper reads;
2. RFC 0159: define the `std/rt-types/exec.forma` protocol surface and the
   `ExecFn` alias while keeping effect interpretation in the Host adapter;
3. RFC 0162: replace `@@manifest` with scoped ordered option actions,
   including the initial path dependency and exact format consumers;
4. RFC 0163: define explicit Host environment capture for executable entries,
   including deterministic request construction and cache identity;
5. RFC 0165: add the minimum type-preserving Dict lookup and argv-rewrite
   combinators needed for explicit input validation and conflict handling;
6. RFC 0166: add the end-to-end GCC-wrapper fixture with injected Path
   dependencies, source-data blame,
   canonical dry-run output, cancellation/quota coverage, and documentation
   evidence.

Each child remains independently reviewable and may narrow its implementation
surface. It may not silently move effects into Forma evaluation or weaken the
shared acceptance criteria.

## Goals

1. execute exported Functions against the lexical module environment in which
   they were defined, across promotion and module-cache boundaries;
2. expose executable Host contracts as ordinary importable Forma type
   metadata with one convenient `ExecFn` name;
3. let a top-level entry carry literal, statically extractable Host options;
4. resolve fixed Path dependency names and package-internal paths to
   deterministic module identities;
5. let applications validate required Dict input and rewrite argv without
   dynamic-field failures or mutable state;
6. preserve source blame from imported toolchain data through user-space
   validation and wrapper transformation;
7. produce one complete canonical `ExecEnv` under strict bounded evaluation;
8. keep all acquisition and execution policy under Host authority.

## Non-goals

- downloading, verifying, unpacking, or installing GCC and sysroot archives;
- launching, replacing, or supervising an external process;
- physically merging a dependency closure into one source file;
- `forma exec URL`, multicall packaging, or executable publication tooling;
- runtime dependency loading, computed import paths, or Forma-visible network
  access;
- a general package solver, version ranges, floating branches, or registry
  discovery;
- shell parsing, a complete GCC driver parser, or response-file expansion;
- a generic effect or capability system;
- implicit imports, a global execution context, or effectful standard-library
  Functions;
- changing best-effort analysis so an effect Host can consume partial values.

## Cross-module Function correctness

An exported Function is a value whose lexical environment belongs to its
definition module. Publication, promotion, caching, import projection, and
higher-order return must preserve that environment:

```forma
# dependency/toolchain.forma
def helper = fn(value) { value + 1 };
export def factory = fn(offset) {
    fn(value) { helper(value) + offset }
};
```

Calling `factory(2)(39)` from another module must produce `42`. Directly
exporting a two-argument Function that calls `helper` must obey the same rule.
The fix must not special-case entry exports or copy dependency source into the
requester.

The child RFC must identify the ownership invariant violated by the current
up-link failure and cover persistent-root promotion, diamond imports, cached
modules, nested closures, repeated calls, and module disposal. A workaround
that merely inlines helpers is not acceptance evidence.

## Runtime protocol types

Host boundary contracts are ordinary data descriptions. The initial module
surface is conceptually:

```forma
export type ExecSettings = ...;
export type ExecRequest = ...;
export type ExecEnv = ...;
export type ExecFn = Fn(ExecSettings, ExecRequest) -> ExecEnv;
```

The exact legal alias mechanism must be settled by RFC 0159; this umbrella
does not use `ExecFn` to smuggle in parameterized type aliases. The accepted
surface must provide the authored contract shown by the target program and
must remain inspectable by check, show, hover, and module-interface queries.

The Host adapter retains authoritative validation. Constructing an `ExecEnv`
in another mode is harmless data computation; only `forma exec` interprets it
as an executable plan.

## Static top-level options

An option declaration is allowed only in the non-importable top-level module:

```forma
option "crate.dependency" {name: "dependency-name", source: ...};
```

Its payload is restricted to the same closed literal data accepted by the
embedded-manifest boundary. It cannot call a Function, reference a binding,
read an import, interpolate Host input, or depend on evaluation order. The
Host extracts and validates options before resolving ordinary imports.

RFC 0162 defines coexistence with `forma-deps.json` and removes the existing
`@@manifest`. Conflicts are deterministic and diagnostic; silent merging or
last-writer-wins behavior is not accepted.

Executable entries declare the Host environment inputs they consume:

```forma
option "exec.capture-envs" ["TARGET"];
```

The Host constructs `ExecRequest.env` from exactly those names. This option
does not read the process environment during Forma evaluation, and undeclared
variables cannot affect evaluation, dry-run output, or a future cache key.
Repeated actions preserve option order while the effective name set is
deduplicated deterministically. A captured name that is absent from the Host
environment remains absent from the Dict for explicit user-space validation.

## Fixed dependency resolution

The phase uses the existing exact local specification:

```forma
'Path({path: "../gcc-wrapper"})
```

The path is resolved relative to the project root before ordinary module
resolution. Dependency keys still produce deterministic logical identities,
and relative imports remain contained within their dependency crate. RFC 0164
retains the future pinned GitHub provider design, but neither its provider nor
network/cache behavior is required by this phase.

## Application combinators

The wrapper must not rely on `request.env.TARGET` producing a low-level missing
field error. A type-preserving lookup must expose absence explicitly, for
example:

```forma
dict.get(request.env, "TARGET") # Option(String)
```

The exact helper names belong to RFC 0165. Its scope is the minimum needed to
detect required input, reject or normalize conflicting `--sysroot` and prefix
map arguments, and build a new immutable argv. It does not introduce a generic
command-line grammar or mutable builder.

## Provenance and diagnostics

Imported JSON source retains field-level original provenance. Values computed
by `wrap_gcc` follow RFC 0102 generation and preservation rules. At minimum:

- malformed source structure identifies `source.json`;
- a rejected toolchain value identifies the relevant source value and authored
  validation rule;
- missing TARGET identifies the Host request and the wrapper requirement;
- an invalid final plan identifies the generated wrapper expression before the
  Host refuses it;
- propagated failures do not become executable partial plans.

The end-to-end child may expose a narrower Host-input provenance rendering if
the current request representation cannot address individual environment
entries. It must record that boundary rather than fabricate a file source.

## Shared acceptance criteria

1. a dependency module may export a Function that calls module-level helpers,
   and requesters can invoke it repeatedly without an up-link failure;
2. higher-order exported closures retain captured arguments and their defining
   module environment after promotion and caching;
3. `ExecFn` is available from `std/rt-types/exec.forma` and is observed
   consistently by check, show, hover, and `forma exec`;
4. top-level options accept closed literals and reject computed or import-
   dependent payloads before evaluation;
5. option/manifest conflicts and duplicate dependency keys produce stable
   source diagnostics;
6. exact Path dependency specifications resolve to canonical logical
   module IDs independent of physical cache paths;
7. relative imports cannot escape a dependency crate and dependencies cannot
   import the main-only entry;
8. required environment input is handled through an explicit typed absence or
   error path rather than a dynamic field failure;
9. gcc, g++, and ar reuse one wrapper module while selecting the correct GCC
   and sysroot install actions;
10. argv output injects deterministic sysroot and source/debug prefix maps and
    handles conflicting user options according to one documented policy;
11. malformed imported data, invalid Host input, and invalid generated plans
    retain distinct useful blame anchors;
12. `forma exec --dry-run` emits stable canonical JSON and performs no network,
    filesystem-installation, or process effect during the acceptance test;
13. strict execution quotas and cancellation cannot publish or execute a
    partial plan; and
14. full workspace tests, formatting, Clippy, and documentation checks pass.

## Stopping rules

Work returns to discussion if a child requires:

1. runtime `eval`, computed imports, or dependencies selected during Forma
   evaluation;
2. exposing filesystem cache paths as canonical module identity;
3. floating dependency resolution or a general version solver;
4. granting effects merely by importing a protocol type;
5. partial `ExecEnv` publication after a failed or cancelled computation;
6. a general effect system, mutable builder, trait system, or shell language;
7. weakening closure lexical ownership to make cross-module calls work;
8. giving imported JSON fabricated static types instead of decoding or
   validating it at an explicit boundary; or
9. making actual toolchain acquisition necessary for deterministic tests.

## Delivery discipline

Each child RFC receives its own proposal and implementation evidence. The
umbrella remains Proposed until the saved target entry, or a syntax-equivalent
fixture with injected exact dependencies, completes `forma exec --dry-run` and
the canonical plan demonstrates two independently selected install resources,
argv rewriting, module reuse, and sourced rejection paths.

If implementation evidence narrows `option`, `ExecFn`, or argv APIs,
this umbrella must be amended before it is marked Implemented. The introduction
may use the target program as implemented evidence only after this gate passes.

## Implementation result

The phase completed in August 2026 through RFC 0158, RFC 0159, RFC 0162,
RFC 0163, RFC 0165, RFC 0166, and RFC 0167. The saved gcc, g++, and ar entries
resolve exact Path dependencies, validate imported source data, share one
higher-order wrapper, capture only TARGET, and produce deterministic canonical
plans through the real `forma exec --dry-run` adapter.

The implementation materializes initialized definition captures before
exporting closures while preserving unresolved recursive up-links. Structured
validation and argv failures cross the strict `ExecFn` boundary through
`reraise!`, retaining their data/rule provenance instead of converting the
error to a String panic. The malformed-data fixture consequently identifies
both `source.json` and the authored rule in `toolchain.forma`.

The fixture performs no acquisition, installation, or process effect. Repeated
dry-runs are byte-identical, do not create the configured cache, and strict
quota failure cannot publish a partial plan. The umbrella's network,
packaging, and real-execution non-goals remain deferred.
