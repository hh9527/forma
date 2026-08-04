# Forma

> Forma was formerly known as XL. The full design history is recorded in
> [rfc/](rfc/).

**Forma is an experimental language built around one question: what does a
language become when types are ordinary data?**

Configuration languages have explored several mature approaches. CUE
organizes constraints around unification; Dhall uses strong normalization to
establish a decidable execution boundary; Starlark provides controlled
general-purpose computation; and Nickel makes contracts and configuration
merging central abstractions. Each choice solves real problems while placing
the corresponding domain concepts in the language itself.

Forma takes a different position: **domain semantics should live in data, and
the language should provide computation**. Configuration, validation,
normalization, codecs, and schema generation are not language features. They
are ordinary functions operating on ordinary data.

## Three interlocking bets

### One language, two stages

Forma has a tool stage and a program stage, but both share one value model,
one bytecode VM, and one evaluation semantics. There is no separate type-level
language, macro language, or second compile-time evaluator. The tool stage
runs ordinary Forma code under fuel and resource quotas, deterministically and
with cacheable results.

### Types as metadata

A type declaration evaluates to ordinary Forma data rather than an internal
structure accessible only to the compiler:

```forma
def Maybe: for(A) Fn(TypeOf(A)) -> TypeOf(Option(A)) = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

`Maybe` is not a type operator. It is an ordinary function, called normally at
the tool stage, that returns ordinary data. The type checker does not
reimplement its body; it interprets the resulting data. Consequently:

- **Types can be printed**: `debug.dbg(User)` displays the type itself because
  it is a value.
- **Types can be transformed by functions**: `Partial(User)` and `Array(User)`
  are function calls.
- **Types retain what they describe**: with `TypeOf(A)`,
  `decode(User, input)` has type `Result(User, BlameError)`, not
  `Result(Any, Any)`. The instance type survives the boundary.

### A closed world

Module paths are statically known, dependencies are fixed, runtime `eval` is
absent, and external data enters through explicit boundaries. The closed world
is leverage: compile-time evaluation can be reproducible, tooling can
enumerate all code, and running someone else's configuration inside a host
process can be bounded by default. Every execution has independent fuel,
stack, allocation, and depth quotas; failure discards its work atomically
without mutating the shared world.

## What follows from these ideas

**Text makes evaluation visible.** Double-quoted and raw forms are inert
Strings; evaluated concatenation uses backticks:

```forma
let literal = "ordinary\nString";
let raw = r##"quotes " and \slashes stay unchanged"##;
let message = `hello \{name}`;
let continued = "first \
    second"; # "first second"
```

Raw Strings accept up to 255 matching `#` delimiters. Escaped Strings and
concat text support Rust-style ASCII and Unicode scalar escapes. This keeps
plain data visibly separate from expressions while making embedded source and
multiline text practical.

**Serde, without macros.** Decorators are ordinary functions, attributes are
ordinary data, and codecs are ordinary interpreters of metadata:

```forma
import json from "@bim/std/json";

@json.rename_all('CamelCase)
@struct
type User = {
    user_id: Int,
    @json.default('None)
    nickname: Option(String),
};
```

Field renaming, defaults, flattening, and skip conditions are library
functions that annotate metadata. Encoding and decoding share one plan, and
JSON Schema is generated from that same plan. The language itself has no
special knowledge of this vocabulary.

**Errors that point to both sides.** Source locations travel with values
through imports, transformations, and codec normalization. A validation error
can report:

```text
user.json:1:21: expected Int
  User.forma:3:47: contract rule declared here
```

The error identifies both the data location and the rule location.
`BlameError` preserves both sources directly, without a codec-specific
diagnostic structure separate from the value model.

**Executable plans.** Forma can express a more capable form of DotSlash. An
entry module is an ordinary pure function,
`Fn(ExecSettings, ExecRequest) -> ExecEnv`: the host explicitly supplies the
platform, install prefix, environment, arguments, and working
directory, and the function returns a fully concrete execution plan. It can
select multiple artifacts, for example installing a platform-specific
interpreter separately from a platform-independent runtime; derive stable
installation locations with `hash.sha256`; and construct search
paths, library paths, and environment variables.

Command-line rewriting belongs to the same pure computation. A gcc or rustc
launcher can add a sysroot and platform-specific search paths, inject
`source-prefix-map`, and rewrite user-supplied source arguments after artifact
locations are known. The returned `ExecEnv` already contains the final binary,
arguments, environment, and paths. The host expands no templates, substitutes
no variables, and reinterprets no policy. There is no special context module
or launcher syntax here, only explicit parameters, ordinary functions, and
typed data; the connection between the closed world and the effectful
host stays narrow.

The effect boundary can be expressed as a set of ordinary Forma types.
Installation methods form an extensible enum, while `ExecEnv` contains only
the final values consumed by the effect layer:

```forma
@enum type UnpackType = {
    TarGzip: 'None,
    Tar: 'None,
};

@struct type UnpackOpt = {
    dest: String,
    ty: UnpackType,
    src: String,
    strip: Int,
    digest: Option(String),
};

@enum type Install = {
    Unpack: UnpackOpt,
};

@struct type ExecEnv = {
    install: Array(Install),
    cwd: Option(String),
    bin: String,
    args: Array(String),
    env: Dict(String),
};
```

`Dict(String)` describes dynamic keys whose values are all Strings, unlike a
Struct with a fixed field set. The complete protocol uses existing
TypeMetadata and remains ordinary data; it adds no special installation
statement or command-line rewriting syntax.

For example, a reproducible gcc launch plan can be written in full:

```forma
#!/usr/bin/env -S forma exec --dry-run

import arrays from "@bim/std/array";
import exec from "@bim/std/exec";
import hash from "@bim/std/hash";

type ExecSettings = exec.ExecSettings;
type ExecRequest = exec.ExecRequest;
type ExecEnv = exec.ExecEnv;

let main: Fn(ExecSettings, ExecRequest) -> ExecEnv = fn(settings, request) {
    let platform = `\{settings.platform.os}-\{settings.platform.arch}`;
    let toolchain_url = `https://example.invalid/gcc-\{platform}.tar.gz`;
    let sysroot_url = `https://example.invalid/sysroot-\{platform}.tar.gz`;
    let toolchain = `\{settings.install_prefix}/\{hash.sha256(`gcc:\{toolchain_url}:unpack-v1`)}`;
    let sysroot = `\{settings.install_prefix}/\{hash.sha256(`sysroot:\{sysroot_url}:unpack-v1`)}`;
    let args: Array(String) = arrays.flat_map([
        [
            `--sysroot=\{sysroot}`,
            `-isystem\{sysroot}/usr/include`,
            `-ffile-prefix-map=\{request.cwd}=.`,
        ],
        request.args,
    ], fn(part) { part });

    {
        install: [
            'Unpack({dest: toolchain, ty: 'TarGzip, src: toolchain_url, strip: 1, digest: 'None}),
            'Unpack({dest: sysroot, ty: 'TarGzip, src: sysroot_url, strip: 1, digest: 'None}),
        ],
        bin: `\{toolchain}/bin/gcc`,
        args: args,
        env: {FORMA_SYSROOT: sysroot},
        cwd: 'Some(request.cwd),
    }
};

main
```

`Unpack` carries its own `src`, so the protocol needs no separate download
action. The effect layer can fetch, cache, and verify the artifact from `src`
and its optional `digest`, then unpack it into the already determined `dest`.

Today, `forma exec --dry-run` only validates and prints the plan; it does not
download, install, or start a process. Even before the effect layer exists,
every deterministic decision can be reviewed, versioned, and tested in
isolation. A future host only needs to consume the concrete plan and perform
its effects. Build rules and Kubernetes reconciliation can use the same
boundary: **pure functions produce plans; the host performs effects**.

The same boundary now covers generated text. A module returning
`Fn() -> build.OutputPlan` can declare ordered `TextFile` artifacts, while
`forma build --dry-run` validates normalized relative paths, rejects duplicate
targets, and prints canonical JSON without writing anything. Raw Strings and
concat handle source text; `@bim/std/string` provides line splitting/joining,
indentation, explicit margin trimming, and trailing-newline normalization.
There is no separate template language or implicit layout state.

The standard library supports that pure side directly. Generic Array
operations compose and short-circuit typed collections; String operations are
UTF-8 safe; lexical path operations use deterministic `/` semantics rather
than the host filesystem; and homogeneous Dict combinators preserve
`Dict<T>` through value mapping and filtering. These are ordinary imported
functions with explicit contracts, not launcher magic.

JSON, TOML, and YAML files can enter the same immutable module graph as source code.
TOML tables become ordinary Dicts, while its four temporal scalar categories
remain distinct as validated Tagged String values through
`@bim/std/toml.DateTime`. YAML follows the 1.2 Core Schema conservatively:
legacy implicit Booleans and timestamps remain Strings, mapping keys must be
Strings, and custom tags and merge keys are rejected. Codec failures retain
both the external data location and the Forma type declaration that imposed
the requirement.

**A conservative language server.** Hover information comes from the same
metadata used by runtime validation. Completion does not invent structure
through `Any`. Incomplete source can still provide navigation and diagnostics
while distinguishing unknown, conflicting, and unevaluable states.

## Design tradeoffs

- **Compared with CUE**: Forma does not use unification as the foundational
  semantics for constraints and composition. Types are data; validation and
  composition policies are explicit functions.
- **Compared with Dhall**: both value pure computation and reproducible
  results. Dhall guarantees termination through strong normalization; Forma
  permits recursion and bounds execution with fuel and resource quotas.
- **Compared with Starlark**: both are suited to controlled execution inside a
  host. Forma additionally makes type metadata available to ordinary
  computation, with static tools and the runtime interpreting the same data.
- **Compared with Nickel**: Nickel makes contracts, merging, and priorities
  central configuration mechanisms. Forma favors expressing those policies as
  library functions that can be inspected, replaced, and composed.

Forma does not eliminate complexity. It tries to place domain complexity in
libraries and data, where users can read, replace, and extend it, while
keeping the language core small and consistent.

## Honest boundaries

Forma is experimental. Today it has no effect system, package acquisition
beyond path dependencies, traits, or type narrowing. Static
inference explicitly reports when it does not know instead of guessing. These
are deliberate boundaries: the project is testing the "types as metadata"
hypothesis deeply before expanding its scope. Seventy-six RFCs record the tradeoffs
at each step, including the rejected alternatives.

The intended use cases follow from those boundaries: **expressing build rules,
continuous reconciliation in Kubernetes operators, and reusable configuration
packages**. In each case, a host needs to execute externally supplied logic
deterministically and explain where both the data and the violated rule came
from.

## Try it

```sh
cargo run -p forma -- check examples/mvp/main.forma
cargo run -p forma -- run examples/mvp/external.forma --input examples/mvp/request.json
cargo run -p forma -- show examples/mvp/main.forma
cargo run -p forma-lsp -- --help
```

## Documentation

- [VISION.md](VISION.md): design thesis
- [rfc/](rfc/): sixty-five design documents, each with rejected alternatives and
  acceptance criteria
- [README.zh.md](README.zh.md): 中文

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
