# Forma

> **Forma is the new name of the XL language.** The project was renamed after
> being published to its remote repository; see [rfc/0060](rfc/0060-rename-xl-to-forma.md)
> for the rename decision and its scope. Historical RFC documents keep their
> original XL wording as records of the design process.

Forma is an experimental general-purpose language with an immutable dynamic
bytecode runtime and a closed-world, two-stage type metadata model. Read
[VISION.md](VISION.md) for the design thesis and [rfc/](rfc/) for the design
sequence. 中文文档见 [README.zh.md](README.zh.md)。

The central hypothesis: in a sufficiently closed world, types are ordinary
immutable metadata, and higher-order type operations are ordinary pure
functions evaluated by a toolchain-hosted instance of the same language
runtime.

What the language demonstrates today:

- Rust-like expression syntax with `fn(args) { ... }` closures and `|>`;
- immutable Dict, Array, Tuple, Atom, and first-class `'Tag(payload)` values;
- pattern matching with payload type propagation;
- explicit single-assignment recursion (`decl`/`def`) with proper tail calls;
- ordinary functions that compute Type metadata in a tool-stage VM;
- structural annotations, runtime validation, and derived JSON codecs from the
  same metadata;
- normalized `@struct`/`@enum`/`@union` models with flat value attributes;
- explicit prenex generic contracts (`for(A, B)`) on native, `decl`, and `def`
  bindings, with erased runtime representation;
- `Type` and `TypeOf(A)` metadata witnesses, so `decode(User, input)` has type
  `Result(User, BlameError)`;
- crate-relative module identities (`@src/...`, dependency aliases, `@bim/std/...`)
  in a closed dependency graph;
- a language server with diagnostics, hover, definition, references, and
  conservative completion, built on recoverable semantic snapshots;
- dry-run executable plans through `forma exec --dry-run`.

## Try it

```sh
cargo run -p forma -- check examples/mvp/main.forma
cargo run -p forma -- run examples/mvp/external.forma --input examples/mvp/request.json
cargo run -p forma -- show examples/mvp/main.forma
cargo run -p forma-lsp -- --help
```

## Syntax snapshot

```xl
@struct
type User = {
    name: String,
    age: Int,
    nickname: Option(String),
};

let user: User = imported_user;
validate(User, user)
```

`value |> f` is exactly equivalent to `f(value)`. Explicit call sections use
`\(` and placeholders to construct ordinary closures:

```text
transform\(_, option)
// equivalent to fn(value) { transform(value, option) }

reorder\(_1, fixed, _0)
// equivalent to fn(a, b) { reorder(b, fixed, a) }
```

Bare placeholders create parameters in source order. Indexed placeholders may
reorder or reuse parameters and must form a continuous range from `_0`.

Collections are ordinary imported functions from built-in modules:

```xl
import arrays from "@bim/std/array";
import dicts from "@bim/std/dict";

[1, 2, 3]
    |> arrays.map\(_, fn(value) { value + 1 })
    |> arrays.filter\(_, fn(value) { 2 < value })
```

Derived codecs make the external-data boundary explicit and typed:

```xl
import data from "./abc.json";
import User from "./User.xl";
import result from "@bim/std/result";
import json from "@bim/std/json";

let user = data |> User.decode |> result.unwrap;
// user : User
user |> User.encode |> json.stringify_pretty(2)
```

See `examples/codec`. Enum values are first-class: `'None` is an Atom and
`'Some("Ada")` is a Tagged value. Every Atom is a unary constructor, so
`arrays.map([1, 2], 'Some)` works directly.

## Type metadata

Type declarations evaluate to canonical Dict/Array/Atom metadata that ordinary
Forma expressions can construct. Generic metadata functions are ordinary
annotated definitions:

```xl
def Maybe: Fn(Type) -> Type = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

Built-in validation and codec contracts carry witnesses, so instance types
survive the boundary:

```xl
native decode: for(A) Fn(TypeOf(A), Any) -> Result(A, BlameError);
native unwrap: for(A, E) Fn(Result(A, E)) -> A;
```

## Execution worlds

Runtime ownership has two fixed tiers. Built-in modules and initialized module
exports live in `MainWorld`; each module initialization or serving call
allocates in a fresh `WorkWorld`. VM execution reads Main but writes only
Work. Module publication copies reachable Work values into Main atomically.
After loading, Main is frozen; serving results are exported directly and Work
is discarded, so repeated sessions cannot mutate the loaded application.

## Tooling

The `forma-lsp` binary provides an asynchronous language server: diagnostics
with cross-file blame labels, hover over computed Type metadata, definition
and reference navigation, and conservative completion over module exports and
Struct fields. The same immutable workspace snapshot backs `forma show`,
tests, and the LSP adapter; incomplete source still yields recovered syntax,
definitions, and explicit fact states instead of guessed types.

## Executable plans

A Forma module can evaluate to a pure plan function. The host supplies
immutable invocation inputs and the module computes the concrete result:

```xl
#!/usr/bin/env -S forma exec --dry-run

fn(settings, request) {
    {
        install: [],
        command: "python3",
        args: request.args,
        env: request.env,
        cwd: request.cwd,
    }
}
```

`forma exec --dry-run tool.xl -- arg1 arg2` prints the canonical JSON plan.
Every deterministic decision lives in Forma; the host performs no download,
installation, or process creation in this phase.

## Current limits

Forma has no effects, package acquisition beyond path dependencies, YAML/TOML
parsers, traits, narrowing, or production garbage collector. Static inference
deliberately degrades to explicit unavailability or `Any` rather than guessing.
See the deferred-work sections of individual RFCs for the honest list.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
