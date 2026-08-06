# Forma

> Forma was formerly known as XL. Its design history is recorded in
> [rfc/](rfc/).

**Forma is an experimental language for programmable data transformation and
validation in a closed, pure, deterministic, and source-aware world.**

It asks:

> What is the smallest language that can provide general data computation,
> finite execution boundaries, and first-class diagnostics and feedback?

Forma sits between static configuration and general-purpose scripting. Static
formats are inspectable but limited; scripting languages are programmable but
often open, effectful, difficult to reproduce, and weak at explaining the
origin of transformed data. A sandbox with fuel and an API allowlist can bound
a script, but it does not by itself provide an authoritative semantic model,
cross-data provenance, recoverable analysis, or precise editor feedback.

Forma treats those requirements as one design problem.

## The Core Model

### Ordinary computation over ordinary data

Configuration, validation, normalization, migration, codecs, schema
generation, and plan construction are not language features. They are ordinary
pure functions over immutable values.

Forma supplies functions, closures, recursion, pattern matching, modules, and a
small runtime data model. Domain policies such as merge, defaults, precedence,
and encoding live in libraries where they can be inspected, replaced, and
composed.

### A closed and bounded world

Module paths are statically known, dependencies are fixed, runtime `eval` is
absent, and genuine runtime input enters through explicit host values. Forma,
JSON, YAML, and TOML files participate in the same immutable module graph.

Forma permits recursion, but every execution has independent fuel, stack, call
depth, and allocation quotas. An execution deterministically produces a value
or a structured resource failure within its configured boundary. Failed work
is discarded atomically rather than partially published into the persistent
world.

### Diagnostics are first-class

Source locations travel with values through imports, transformations, metadata,
and codec normalization. A validation failure can identify both the data and
the rule that rejected it:

```text
user.yaml:4:8: expected Int
  User.forma:3:10: requirement declared here
```

JSON, YAML, and TOML files in the workspace are first-class source modules, not
opaque external blobs. They retain syntax diagnostics and field-level
provenance and participate in dependency and workspace analysis.

Incomplete Forma source still provides useful navigation, types, and
diagnostics. Semantic facts distinguish known values from explicit `Any`,
unknown information, conflicts, dependency blocking, and tool-stage
incomputability. Completion does not invent structure to appear helpful.

This feedback model is part of the language experiment, not an editor added
after execution works.

### Types are programmable metadata

A type declaration evaluates to canonical ordinary Forma data:

```forma
def Maybe: for(A) Fn(TypeOf(A)) -> TypeOf(Option(A)) = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

`Maybe` is an ordinary pure function evaluated by the same VM used for program
code. The type checker interprets its result rather than reimplementing the
function in a hidden type-level language.

The same metadata can drive static checking, LSP information, runtime
validation, normalization, codecs, documentation, schema generation, and
user-space interpreters. `TypeOf(A)` preserves the relationship between a
metadata witness and the values it describes; the narrow `Dyn` and
`interpreter!(...)` boundary supports heterogeneous interpretation without an
unchecked cast.

Types are central to Forma, but they serve the larger goal: programmable data
rules with authoritative, source-aware feedback.

## The Host Owns Effects

Forma has no authority over the external world. A host supplies explicit
ordinary inputs and decides whether an ordinary output has external meaning:

```text
external world
    -> host input snapshot
    -> closed Forma computation
    -> output value
    -> host validation and authorization
    -> external world
```

There is no universal Forma action ABI. A process launcher, build system,
Kubernetes controller, or agent runtime defines its own types and interprets
only the values it recognizes. Permissions, IO, retries, transactions, clocks,
and observation remain host concerns.

`forma run`, `forma exec`, and `forma build` are concrete host adapters, not a
language-level effect system. Today the exec and build adapters validate and
print canonical plans without performing their described effects.

## What This Enables

### Codecs and schemas without language magic

Decorators are functions, attributes are data, and codecs are metadata
interpreters:

```forma
import "std/json" as json;

@json.rename_all('CamelCase)
@struct
type User = {
    user_id: Int,
    @json.default('None)
    nickname: Option(String),
};
```

Field renaming, defaults, flattening, and skip policies are library-defined
metadata. Encoding and decoding share one plan, and JSON Schema is generated
from that same plan.

### Deterministic executable and output plans

An executable entry is an ordinary function:

```text
Fn(ExecSettings, ExecRequest) -> ExecEnv
```

The host supplies the platform, install prefix, environment, arguments, and
working directory. Forma returns a fully concrete value containing artifacts,
paths, binary, arguments, and environment. The host expands no templates and
reinterprets no policy.

A build entry similarly returns `Fn() -> build.OutputPlan`. The adapter
validates normalized relative paths, rejects duplicate targets, and emits
canonical JSON. Text generation uses ordinary strings and functions rather
than a second template language.

### Static data as source

JSON, TOML, and YAML modules enter the same immutable graph as Forma code. TOML
temporal categories retain distinct tagged representations. YAML follows the
1.2 Core Schema conservatively: legacy implicit booleans and timestamps remain
Strings, mapping keys must be Strings, and custom tags and merge keys are
rejected. Ambiguous format behavior is rejected or delegated to explicit
library policy.

### Conservative local polymorphism

An unannotated closure-valued `let` can infer a rank-1 scheme:

```forma
let identity = fn(value) { value };
(identity(1), identity("text")) # (Int, String)
```

Inference is intentionally bounded. Aliases instantiate once, recursive groups
remain monomorphic without an explicit contract, and numeric constraints are
not erased into unconstrained parameters. Forma prefers an explicit unknown or
diagnostic over unstable inferred precision.

## Agentic Systems

Machine-generated programs make Forma's constraints more valuable. Generation
is cheap; trustworthy feedback and controlled external meaning are not.

Forma can act as a typed, source-aware IR for plans. An agent generates or
modifies a pure program; Forma returns a complete plan value that a host can
validate, compare, review, sign, or reject before any effect occurs. The plan's
action vocabulary remains ordinary host-defined data.

Forma can also define one pure step of a host-driven loop:

```text
Context x State x Observation
    -> Result(LoopDecision(State, Plan, Output), BlameError)
```

The host owns observation, persistence, time, effects, retries, approvals, and
the overall loop budget. Forma computes one deterministic, finitely bounded
transition. Its diagnostics can point back to generated Forma, a JSON/YAML/TOML
source value, and the rule that rejected it, creating a precise repair and
audit loop.

These uses require no Agent-specific syntax and grant Forma no additional
authority.

## Design Tradeoffs

- **Compared with CUE:** Forma does not make unification the foundational
  semantics of constraints and composition. Policies are explicit functions
  over data.
- **Compared with Dhall:** both value pure, reproducible computation. Dhall
  guarantees normalization; Forma permits recursion and supplies deterministic
  fuel and resource boundaries.
- **Compared with Starlark:** both support controlled hosted computation. Forma
  additionally makes programmable type metadata, source provenance, partial
  semantic facts, and editor feedback part of the core experiment.
- **Compared with Nickel:** Nickel makes contracts, merging, and priorities
  central configuration mechanisms. Forma keeps such policies in replaceable
  libraries.
- **Compared with a sandboxed scripting language:** Forma is not only bounded.
  It unifies static data, transformation code, rules, runtime validation, and
  tooling in one source-aware semantic model.

Forma does not eliminate complexity. It tries to place domain complexity in
ordinary libraries and data while keeping the trusted language semantics small
and consistent.

## Current Boundaries

Forma is experimental. It has no language-level effects, ambient IO, dynamic
imports, general package acquisition, traits, or type narrowing. Hosts may
provide narrow adapters, but effects are not a deferred part of the language.

The project has now demonstrated the central vertical path, including computed
and recursive type metadata, derived codecs and schemas, recoverable workspace
semantics, a language server, bounded rank-1 inference, safe dynamic
observation, and user-space reference Equality and Show interpreters. It has
not yet demonstrated production-scale hosts, long-term compatibility, or broad
external use.

Likely application domains include reusable configuration packages, build and
toolchain planning, continuous reconciliation, policy-driven data pipelines,
typed Agent plans, and host-driven Agent loops.

## Try It

```sh
cargo run -p forma -- check examples/mvp/main.forma
cargo run -p forma -- run examples/mvp/external.forma --input examples/mvp/request.json
cargo run -p forma -- show examples/mvp/main.forma
cargo run -p forma -- lsp
```

## Documentation

- [VISION.md](VISION.md): the design thesis and feature admission rule
- [rfc/](rfc/): numbered design documents with acceptance evidence
- [README.zh.md](README.zh.md): Chinese introduction

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
