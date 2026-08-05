# RFC 0117: Separate library, CLI, and LSP crates

- Status: Proposed
- Depends on: RFC 0046, RFC 0100, RFC 0114

## Summary

The workspace separates its three existing deployment units:

```text
forma-cli -> forma <- forma-lsp
```

`forma` becomes a library-only package containing the language engine,
frontends, type system, evaluator, module system, workspace model, and public
embedding APIs. `forma-cli` owns the user-facing executable, whose binary name
remains `forma`. `forma-lsp` remains the asynchronous language server package.

## Ownership

The crates own these boundaries:

- `forma`: parsing, syntax, types, VM, native APIs, module resolution,
  Engine/EngineBuilder, evaluation, and semantic workspace queries;
- `forma-cli`: argument parsing, process environment, stdin/stdout/stderr,
  filesystem-oriented commands, exec/build request construction, and CLI
  rendering policy;
- `forma-lsp`: JSON-RPC transport, document synchronization, cancellation,
  request scheduling, and LSP projection.

Both applications depend on `forma`; neither application depends on the other.
This RFC does not split parser, VM, types, or workspace internals into more
packages.

## Compatibility

The Cargo package containing the executable becomes `forma-cli`, while its
declared binary remains named `forma`:

```toml
[package]
name = "forma-cli"

[[bin]]
name = "forma"
path = "src/main.rs"
```

Installed command names and CLI behavior therefore remain unchanged. Workspace
development commands use `cargo run -p forma-cli -- ...`. CLI integration tests
move with the binary and continue to use `CARGO_BIN_EXE_forma`.

## Acceptance criteria

1. `forma` has a library target and no binary target;
2. `forma-cli` depends on `forma` and builds binary `forma`;
3. `forma-lsp` continues to depend only on `forma` for language behavior;
4. CLI integration tests belong to `forma-cli` and retain behavior;
5. README command examples use the new Cargo package name;
6. no language, CLI, LSP, or native-module semantics change;
7. package dependency direction contains no cycle; and
8. full workspace tests and strict Clippy pass.

## Non-goals

- splitting `forma` into parser, VM, types, or compiler packages;
- adding a shared application framework between CLI and LSP;
- changing command names, options, output, or exit status;
- changing public language-engine APIs; or
- independent release/version policy in this phase.

