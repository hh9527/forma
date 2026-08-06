# RFC 0117: Separate the engine from the Forma application

- Status: Implemented
- Depends on: RFC 0046, RFC 0100, RFC 0114

## Summary

The workspace separates the embeddable language engine from the user-facing
application:

```text
forma -> forma-core
```

`forma-core` contains the parser, type system, evaluator, VM, module system,
workspace model, and public embedding APIs. `forma` contains the CLI and LSP
adapters and produces the single binary `forma`. The language server starts as
`forma lsp`; there is no separate `forma-lsp` package or executable.

The package names intentionally match their roles. `forma` is what users run
and distribute. `forma-core` is the complete language engine, rather than a
narrow VM package.

## Ownership

- `forma-core`: parsing, syntax, types, VM, native APIs, module resolution,
  `Engine`/`EngineBuilder`, evaluation, and semantic workspace queries;
- `forma`: argument parsing, process and filesystem integration, terminal
  rendering, JSON-RPC transport, document synchronization, cancellation,
  request scheduling, and LSP projection.

The application depends on `forma-core`; the engine never depends on the
application. In particular, Tokio, `async-lsp`, and LSP serialization remain
outside `forma-core`.

CLI and LSP live together because future tooling actions, such as dependency
synchronization, may be initiated from either interface. Such actions should
return structured results and remain independent of terminal or LSP
presentation. This RFC does not create an action abstraction before a real
shared action exists.

## Compatibility

The Cargo package and its binary are both named `forma`:

```toml
[package]
name = "forma"

[[bin]]
name = "forma"
path = "src/main.rs"
```

Installed command names and existing CLI behavior remain unchanged. Workspace
development commands use `cargo run -p forma -- ...`; editors launch the
language server with `forma lsp`.

## Acceptance criteria

1. `forma-core` has a library target and no binary target;
2. `forma` depends on `forma-core` and builds the sole `forma` binary;
3. `forma` owns both CLI and LSP adapters, with LSP available as `forma lsp`;
4. no `forma-cli` or `forma-lsp` package remains;
5. CLI and LSP tests retain their behavior;
6. README development commands use the final package and subcommand names;
7. `forma-core` has no application transport dependencies;
8. no language, native-module, CLI, or LSP protocol semantics change; and
9. full workspace tests and strict Clippy pass.

## Non-goals

- splitting parser, VM, types, or workspace internals into more packages;
- inventing shared tooling actions before they are needed;
- changing existing command options, output, or exit status; or
- introducing independent release or version policies.

## Implementation result

Renamed the embeddable library package and directory to `forma-core`. Renamed
the application package and directory to `forma`, preserving its same-named
binary and CLI integration tests. Moved the asynchronous LSP implementation
into the application as a dedicated module and exposed it through `forma lsp`.

The former `forma-lsp` package and executable were removed. The LSP transport
dependencies moved to `forma`, while `forma-core` remains synchronous at its
boundary and free of transport/runtime dependencies. README commands and a CLI
black-box test record the final entry point. No language-engine or protocol
behavior changed.
