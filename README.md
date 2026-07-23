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

## Current limits

The MVP has no effects, package manager, LSP server, traits, HKT, YAML/TOML
modules, normalization protocol, or production garbage collector.
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
