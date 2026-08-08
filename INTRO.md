# From Programmable Configuration to Trustworthy Plans: An Introduction to Forma

Configuration rarely remains "just a data tree." Once a system needs reuse,
external data, platform selection, validation, and command generation, it is
running a program. The real choice is no longer merely JSON versus YAML versus
a scripting language. It is:

> How can data representation, validation, transformation, and application
> meaning share one checkable and diagnosable model?

Forma is a language experiment around that question. It provides a closed,
pure, deterministic, resource-bounded world for data computation, then uses a
small number of explicit host entries to project results into real
applications. A Forma program may construct a process plan, build rule,
deployment object, or agent plan, but it does not itself download files, start
processes, or gain ambient access to the environment or network.

This introduction begins with a concrete application and then explains why
Forma chooses this boundary.

## A GCC Wrapper Beyond Dotslash

Consider a directly executable GCC wrapper. It must do more than download one
binary:

- GCC and the sysroot are separate packages, selected by host platform and
  compilation target respectively;
- `gcc`, `g++`, and `ar` share toolchain definitions and installation caches;
- the wrapper rejects conflicting arguments and adds `--sysroot`,
  `-ffile-prefix-map`, and `-fdebug-prefix-map` itself;
- download files and installation directories follow a complete deterministic
  policy rather than being guessed by an external runner;
- failures identify bad JSON data or the Forma rule that rejected it; and
- the complete execution plan is visible before any download or process start.

The repository contains an end-to-end example whose `gcc` entry is a thin
assembly module:

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
import "gcc-wrapper/toolchain.forma" { command };

export def exec: ExecFn = command("gcc", source);
```

There is no GCC-specific syntax here. Dependencies are static options, the
toolchain description is a JSON module, the wrapper is an ordinary Forma
module, and `ExecFn` is an ordinary type published by the host protocol.
`exec.capture-envs` does not inherit the whole environment; it allows the exec
entry to capture only `TARGET` and pass it to main as explicit request data.

The shared module first validates external data into domain types:

```forma
@struct type Package = {
    name: String,
    src: String,
    digest: String,
};

@struct type ToolchainSource = {
    compilers: Dict(Package),
    sysroots: Dict(Package),
};

def validated_source: Fn(Any) -> ToolchainSource = fn(raw) {
    match validate(ToolchainSource, raw) {
        'Ok(source) => source,
        'Err(error) => raise!(error),
    }
};
```

Ordinary functions then select packages, derive hash-addressed paths, and
rewrite arguments:

```forma
def install_dest = fn(settings, package, ty, strip) {
    let identity = `unpack-v1\n\{package.name}\n\{package.src}\n\{package.digest}\n\{ty}\n\{strip}`;
    `\{settings.install_prefix}/\{hash.sha256(identity)}`
};

def checked_compiler_args = fn(request, sysroot_dest) {
    let arguments = match argv.reject_option(request.args, "--sysroot") {
        'Ok(arguments) => arguments,
        'Err(error) => raise!(error),
    };
    let arguments = match argv.reject_option(arguments, "-ffile-prefix-map") {
        'Ok(arguments) => arguments,
        'Err(error) => raise!(error),
    };
    let arguments = match argv.reject_option(arguments, "-fdebug-prefix-map") {
        'Ok(arguments) => arguments,
        'Err(error) => raise!(error),
    };
    argv.prepend([
        `--sysroot=\{sysroot_dest}`,
        `-ffile-prefix-map=\{request.cwd}=.`,
        `-fdebug-prefix-map=\{request.cwd}=.`,
    ], arguments)
};
```

The final value is a concrete, JSON-encodable `ExecEnv`. It lists multiple
`Unpack` actions, every download file and installation destination, the
working directory, binary, arguments, and a `{clear, update}` environment
policy. The host does not expand variables, derive cache paths, or reinterpret
GCC policy.

The example can be run today:

```sh
TARGET=aarch64-linux-gnu \
  cargo run -p forma -- exec --dry-run \
  examples/gcc-wrapper/app/bin-src/gcc.forma -- \
  -c /workspace/hello.c -o /workspace/hello.o
```

The current adapter prints the plan but does not download or execute it. This
is not a missing final line in the demo. It is a deliberate authority boundary:
deterministic computation has finished, while real effects still require host
authorization.

## Why Existing Approaches Do Not Fully Occupy This Space

Forma does not claim that other approaches "cannot program." The difference
is what each approach makes fundamental and how many models must jointly
explain a result as the application grows.

### Data formats, schemas, and data tools

JSON provides a clear, stable, universally consumable data boundary. YAML,
TOML, and KDL make different tradeoffs for human authoring and structural
expression. They are excellent inputs and outputs, but do not define reuse,
conditional selection, or transformation.

JSON Schema adds a shared contract but does not compute how data is produced.
`JSON + jq/jaq` adds powerful querying, filtering, and composition; for an
ad-hoc transformation it is often the most direct tool. As an application
grows, however, domain types may live in a schema, transformation in filters,
dependencies and invocation in scripts, and the final protocol in host code.
Errors tend to describe the current JSON value or filter rather than one
relationship spanning source data, rejecting rule, generation step, and host
contract.

Forma keeps direct data transformation while placing static data, types,
functions, modules, provenance, and final protocols in one semantic model.
JSON, TOML, and YAML are modules in Forma rather than opaque blobs stripped of
locations.

### General-purpose languages

Python and JavaScript offer open dynamic computation and mature ecosystems.
Python annotations and TypeScript catch many interface mistakes while
deliberately retaining dynamic escape hatches. A configuration framework built
on them must still define what may be observed, how dependencies are fixed,
how resources and caching are bounded, and what dry-run means.

Proof-oriented languages can establish properties much stronger than Forma's,
at the cost of bringing configuration into termination and proof engineering.
Forma does not seek general theorem proving. It permits recursion and gives the
host a finite boundary through deterministic fuel, stack, call-depth, and
allocation quotas.

Scheme offers another important reference: "code is data" enables remarkable
language-building power, but static analysis must then understand code created
by expansion or execution. Forma adopts a narrower idea:

> Types are data, but code is not arbitrarily generated executable data.

This retains part of the metaprogramming value while keeping module
dependencies, name binding, and the executable function set known before
evaluation.

### Programmable configuration DSLs

CUE, KCL, and Nickel are closest to Forma's problem domain. They demonstrate
that constraints, merging, contracts, and programmable configuration deserve
purpose-built models. Forma's distinction is not a larger syntax checklist;
it attempts to reduce domain policy to ordinary data and ordinary functions.

Unification, contract application, and field merging are valuable DSL
semantics. Each specialized rule also brings its own composition and failure
model for users and tools to understand. Forma explores whether "types as
metadata + functions + a few controlled bridges" can support parsing, codecs,
schemas, display, and Eq/Hash without turning each into another language
mechanism.

This is a different allocation of complexity, not a universal replacement
claim. It must be tested by cross-module, cross-source applications such as the
GCC wrapper rather than isolated syntax examples.

## Forma's Core Model

### Closed, pure, and bounded

The code, static data, and dependency graph available to main are fixed before
evaluation. Values are immutable, functions have no external effects, and
there is no runtime `eval` or arbitrary dynamic import. Recursion is permitted,
but the host bounds fuel, stack, call depth, and allocation for every run.

"Closed" does not mean every input is a compile-time constant. Arguments,
selected environment variables, and platform information may enter, but only
after a host-selected entry materializes them explicitly. The world affecting
one computation is therefore realistic yet enumerable, cacheable, and
auditable.

### Types are ordinary programmable metadata

A Forma type declaration produces canonical immutable data. A type constructor
can be an ordinary pure function:

```forma
def Maybe: for(A) Fn(TypeOf(A)) -> TypeOf(Option(A)) = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

`TypeOf(A)` relates a metadata witness to the `A` it describes. One definition
can therefore drive static checking, runtime validation, codecs, schemas,
string parsing/display, and documentation tools instead of being copied into a
schema for every layer.

Ordinary Forma code can also interpret erased `TypeDesc` values. When such an
interpreter needs a statically typed calling surface, `interpreter!` provides a
narrow bridge:

```forma
def my_show: for(A) Fn(TypeOf(A)) -> Fn(A) -> Result(String, BlameError)
    = interpreter!(show_dyn);
```

This is not a macro system or `eval`. The interpretation algorithm remains an
ordinary function; the system only checks a fixed protocol between erased
parameters and the declared signature. Users can write general type-directed
capabilities without gaining arbitrary code generation.

### Provenance and blame travel through dataflow

When Forma reads JSON, TOML, or YAML, it retains field-level provenance rather
than only a final value. Locations travel through imports, validation, and
transformation; rules have origins as well. A failure can therefore report:

```text
source.json:12:9: expected String
  toolchain.forma:16:28: requirement declared here
```

`raise!` preserves this structure instead of replacing it with a new string.
The GCC fixture verifies that malformed toolchain data retains both JSON and
wrapper rule locations, while missing `TARGET` and conflicting arguments emit
no partial dry-run output.

The same model serves the LSP. Incomplete source need not collapse the entire
workspace into "no information": semantic facts distinguish known values,
explicit `Any`, unknowns, conflicts, and dependency blocking rather than
filling gaps with guesses.

### A lightweight connection between open and closed worlds

Main does not need IO or a general effect system. The host first selects a
trusted entry. Entry may observe controlled runtime information, inspect
options, and inject typed virtual modules, then explicitly initialize main:

```text
open host world
  -> pending ModuleHandle
  -> trusted entry prepares inputs and modules
  -> initialize_module (the freeze boundary)
  -> closed main computation
  -> type-checked export
  -> dry-run / host authorization / effects
```

Main cannot import the entry runtime directly. Injected modules belong to one
invocation, freeze at initialization, and cannot leak to another handle. Entry
obtains a main export first as `Dyn`, retaining its type scheme and source, and
then projects it authoritatively with `TypeOf(A)`. A bad `exec` signature is
rejected before invocation with both the main definition and entry protocol
check in the diagnostic.

Most of `forma exec` is consequently a replaceable Forma entry rather than
GCC or process schemas scattered through the CLI and VM. Future installer and
process APIs can remain entry-only while main stays an ordinary pure module.

## From One Wrapper to More Applications

The GCC wrapper follows one general data path:

```text
locked dependencies and host request
  -> external-data decoding and domain validation
  -> deterministic transformation and argument rewriting
  -> complete typed plan
  -> host authorization and interpretation
```

Changing the input and output protocol projects the same model elsewhere:

- **build rules:** produce an artifact DAG or `OutputPlan` from source and
  platform descriptions;
- **enhanced dotslash:** compose several packages, share installations,
  rewrite arguments, and dry-run before launch;
- **Helm charts and deployment plans:** validate multi-source data into
  reviewable Kubernetes objects;
- **agentic plan IR:** turn generated intent into a deterministic plan a host
  can compare, sign, approve, or reject; and
- **data migration and generation:** consume JSON/TOML/YAML and emit typed
  structured data or stable text.

Forma does not yet ship complete frameworks for these domains, production
package acquisition, real exec/install effects, or long-term compatibility
guarantees. What it has demonstrated is their shared vertical foundation:
data inputs, programmable type metadata, ordinary functional abstraction,
source-aware diagnostics, bounded evaluation, module reuse, and a controlled
host entry.

## What Forma Is Trying to Prove

Forma is not trying to be a smaller Python or to replace every configuration
DSL with another specialized constraint semantics. It tests a more specific
claim:

> Centering "types are data" can express data, constraints, and transformation
> with fewer, more uniform, and more explainable mechanisms, while retaining
> useful abstraction, reuse, diagnostics, and real application space.

The experiment fails if every application domain requires new VM instructions,
language-level effects, or compiler exceptions. It remains promising if new
domains primarily add ordinary Forma libraries, typed protocols, and narrow
host adapters while the core semantics stay small and coherent.

## Read and Run More

- [README.md](README.md) is the capability tour and quick-start entry;
- [VISION.md](VISION.md) states the design thesis and feature admission rule;
- [rfc/](rfc/) records incremental implementation and acceptance evidence; and
- [examples/gcc-wrapper/](examples/gcc-wrapper/) is the executable case used
  throughout this introduction.

```sh
cargo test --workspace

TARGET=aarch64-linux-gnu \
  cargo run -p forma -- exec --dry-run \
  examples/gcc-wrapper/app/bin-src/gcc.forma -- \
  -c hello.c -o hello.o
```
