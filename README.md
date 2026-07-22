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
- ordinary functions that compute Type metadata in a tool-stage VM;
- structural annotations and runtime validation from the same metadata;
- `.xl` and `.json` modules in a closed dependency graph;
- explicit external JSON input through the CLI.
- Logos + Lelwel lossless CSTs shared by compilation and future editor tooling;
- byte-range diagnostics and path-addressable JSON source provenance.

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

`value |> f(arg)` passes `value` as the first argument. The built-in validator
uses the order `validate(type, value)`, so it is called directly; placeholders
for non-first pipeline insertion are deferred.

## Current limits

The MVP has no effects, package manager, LSP server, traits, HKT, YAML/TOML
modules, normalization protocol, or production garbage collector.
Function signatures are dynamically checked and static inference deliberately
falls back to `Any` where the focused checker has no precise model.

## Parsing substrate

Both XL and JSON use Logos lexers and Lelwel-generated resilient parsers. Their
owned lossless CSTs preserve trivia and byte ranges, while `SourceDatabase`
converts ranges to display positions. Parsing always reparses the complete
file. JSON lowering can additionally return a provenance side table through
`parse_json_with_provenance`; provenance is deliberately not stored in runtime
`Value` objects.

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
