# XL

XL is an experimental general-purpose language with an immutable dynamic
bytecode runtime and a closed-world, two-stage type metadata model. Read
[VISION.md](VISION.md) for the design thesis and [rfc/](rfc/) for the accepted
MVP sequence.

The current MVP demonstrates:

- Rust-like expression syntax with `fn(args) { ... }` closures and `|>`;
- immutable Dict, Array, Tuple, Atom, and primitive values;
- Erlang-style tagged tuples and pattern matching;
- explicit single-assignment recursion with proper tail calls;
- pure Array and Dict transformations through explicit core modules;
- ordinary functions that compute Type metadata in a tool-stage VM;
- structural annotations and runtime validation from the same metadata;
- `.xl` and `.json` modules in a closed dependency graph;
- declarative `native name: contract;` host bindings for analyzable core interfaces;
- contextual Python-style decorators as ordinary RHS function transformations;
- explicit external JSON input through the CLI.
- Logos + Lelwel lossless CSTs shared by compilation and future editor tooling;
- byte-range diagnostics and source locations carried by runtime values.

## Try it

```sh
cargo run -p xl -- check examples/mvp/main.xl
cargo run -p xl -- types examples/mvp/main.xl
cargo run -p xl -- run examples/mvp/main.xl
cargo run -p xl -- run examples/mvp/external.xl --input examples/mvp/request.json
```

The core example defines `Optional` as an ordinary function over Type metadata,
checks an imported JSON value against a computed structural type, and uses the
same metadata with `validate` at runtime.

## Syntax snapshot

```text
fn Optional(item) {
    Union([
        Atom('None),
        Tuple([Atom('Some), item]),
    ])
}

type User = Struct({
    name: String,
    age: Int,
    nickname: Optional(String),
});

let user: User = imported_user;
validate(User, user)
```

`value |> f` is exactly equivalent to `f(value)`. Calls on the right retain
their ordinary meaning, so `value |> factory(arg)` means
`factory(arg)(value)`. Explicit call sections use `\(` and placeholders to
construct ordinary closures:

```text
transform\(_, option)
// equivalent to fn(value) { transform(value, option) }

reorder\(_1, fixed, _0)
// equivalent to fn(a, b) { reorder(b, fixed, a) }
```

Bare placeholders create parameters in source order. Indexed placeholders may
reorder or reuse parameters and must form a continuous range from `_0`.

Array operations are ordinary imported functions:

```text
import arrays from "core:array";

[1, 2, 3]
    |> arrays.map\(_, fn(value) { value + 1 })
    |> arrays.filter\(_, fn(value) { 2 < value })
```

The initial module exports `length`, `map`, `filter`, `flat_map`, and `fold`.

Dict enumeration and construction use `core:dict`:

```text
import dicts from "core:dict";

let entries = dicts.pairs({ z: 3, a: 1 });
let rebuilt = dicts.from_pairs(entries);
dicts.merge(rebuilt, { a: 10 })
```

`keys`, `values`, and `pairs` use deterministic field order. `from_pairs`
rejects duplicate keys, while the shallow `merge(left, right)` gives precedence
to the right Dict.

Debug observation is also an ordinary core module:

```text
import debug from "core:debug";

value
    |> debug.dbg_with\("loaded", _)
    |> transform
    |> debug.dbg
```

`dbg(value)` and `dbg_with(label, value)` emit bounded representations to the
host observer and return the exact input value. The CLI writes debug events to
stderr, leaving the final program value on stdout. Source-reflection forms such
as `file!()`, `line!()`, and `dbg!()` are intentionally deferred.

Derived codecs make the external-data boundary explicit:

```text
import data from "./abc.json";
import User from "./User.xl";
import result from "core:result";
import json from "core:json";

let user = data |> User.decode |> result.unwrap;
user |> User.encode |> json.stringify_pretty(2)
```

See `examples/codec`. `User.decode` normalizes the standard Option metadata
shape, including missing and null fields, while `User.encode` returns a strict
JSON-domain value. JSON serialization is deterministic and rejects XL-only
values that have not crossed an explicit codec.

Type declarations evaluate to the same canonical Dict/Array/Atom metadata that
ordinary XL expressions can construct. Built-in constructors validate and wrap
their rich XL arguments; they do not create a privileged schema object. Codec
failures are ordinary `('Err, {message, data, rule})` values until
`result.unwrap` renders data and contract-rule source labels.

## Execution worlds

Runtime ownership has two fixed tiers. Engine core modules and initialized
module exports live in `MainWorld`; each module initialization or serving call
allocates in a fresh `WorkWorld`. VM execution reads Main but writes only Work.
Module publication copies reachable Work values into Main while preserving
existing Main references. After loading, Main is frozen; serving results are
exported directly and Work is discarded, so repeated sessions cannot mutate
the loaded application.

## Native bindings

Core interfaces are XL source declarations rather than Dict values assembled
by Rust. For example, an interface may declare
`native map: fn(Array(Any), fn(Any) -> Any) -> Array(Any);` and export `map`
through an ordinary module Dict. The declaration contract and source location
are available to parsing and analysis; the host registry supplies only the
`NativeFunction`. Linking produces an ordinary `Func`, so native and bytecode
calls share the same VM ABI. User-provided native registries are not exposed
yet.

## Decorators

Decorators are domain-neutral syntax for transforming the RHS of a type or
Dict field. The compiler supplies only a syntax-derived ordinary-data context:

```xl
@optional
type UserId = Int;
// optional({ kind: 'Type, name: "UserId" }, Int)

{
    @json.rename("type")
    ty: Int,
}
// json.rename("type")({ kind: 'Field, name: "ty" }, Int)
```

Stacked decorators use Python nesting order. They may transform values,
validate them, or adopt a library convention such as flat `WithAttributes`;
the language assigns no attribute keys or model-specific meaning. Original
decorator syntax and locations remain available in the semantic AST.

## Attributed values

`core:attributes` provides a flat ordinary-data convention for model metadata:

```xl
import attributes from "core:attributes";

let rename = fn(name) {
    fn(ctx, value) {
        value |> attributes.add({ "core:json.rename": name })
    }
};
```

The module exports `normalize`, `add`, `get`, `has`, `all`, and `strip`.
Nested wrappers are flattened and later additions win. Wrapped TypeMetadata is
transparent to checking, validation, and derived codecs, while constructors
such as `Struct` preserve the raw wrappers for generators and future LSP views.

## Current limits

The MVP has no effects, package manager, LSP server, traits, HKT, YAML/TOML
modules, domain model library, or production garbage collector.
Function signatures are dynamically checked and static inference deliberately
falls back to `Any` where the focused checker has no precise model.

## Parsing substrate

Both XL and JSON use Logos lexers and Lelwel-generated resilient parsers. Their
owned lossless CSTs preserve trivia and byte ranges, while `SourceDatabase`
converts ranges to display positions. Parsing always reparses the complete
file. JSON lowering can additionally return a compatibility provenance side
table through `parse_json_with_provenance`. Module loading attaches those
locations to compact rich runtime values, so nested data locations survive
ordinary evaluation, heap promotion, and codec normalization.

Tooling and module loaders use `parse_registered` and `parse_json_registered`
with one shared `SourceDatabase`. These APIs retain all recovered diagnostics;
the older `parse` and `parse_json` entry points remain fail-fast compatibility
wrappers. Semantic AST nodes are lowered directly from CST rules and carry
source spans.

Run all verification with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
