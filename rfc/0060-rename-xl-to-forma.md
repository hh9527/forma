# RFC 0060: Rename XL to Forma

- Status: Accepted
- Implementation: Complete

## Summary

The XL language is renamed **Forma**. This RFC records the naming decision,
its exact scope, and the transition rules for documentation, code, and
user-facing identifiers. Language semantics, module resolution, and every
accepted RFC's technical content are unchanged.

## Motivation

`XL` was a placeholder name chosen for the initial experiment. The project has
since grown a complete two-stage type metadata model, a module system, and a
language server, and has been published to its remote repository. Before an
external audience forms, the language needs a stable, searchable,
self-identifying name.

**Forma** (Latin: shape, form) reflects the language's central concerns:
canonical shapes of immutable data, normalized models, and the form of type
metadata as ordinary values.

## Naming decisions

| Layer | Old | New |
| --- | --- | --- |
| Language name | XL | Forma |
| Compiler crate and CLI binary | `xl` | `forma` |
| Language server crate and binary | `xl-lsp` | `forma-lsp` |
| Source file extension | `.xl` | `.forma` |
| Dependency manifest | `xl-deps.json` | `forma-deps.json` |
| LSP language ID | `xl` | `forma` |
| Cache environment variable | `XL_CACHE_HOME` | `FORMA_CACHE_HOME` |
| Built-in module namespace | `@bim/std/...` | unchanged |

The rename is a vocabulary change, not a semantic change. Resolved module
identity, canonical TypeMetadata, bytecode, quotas, and diagnostics are
unaffected except for human-facing names and paths.

No transition aliases are kept anywhere: the project is pre-user, and previous
cleanups (RFC 0028, RFC 0050, RFC 0059) established that one canonical
spelling immediately is cheaper than carrying compatibility layers.

## Documentation policy

- `VISION.md`, `README.md`, and the new `README.zh.md` use the new name
  immediately.
- **Historical RFC documents are not rewritten.** They record the design
  process under the old name and remain accurate as history. RFC 0001 through
  RFC 0059 therefore keep their original XL wording.
- The RFC index in `rfc/README.md` appends new entries under the new name
  without editing prior titles.
- Active examples and fixtures migrate with the code rename.

## Rejected alternatives

### Keep the name XL

XL is nondescript, effectively unsearchable, and reads as an abbreviation
without a referent. Renaming before external adoption is the cheapest this
decision will ever be.

### Rename historical RFC documents

Rewriting history would destroy the record of when and why decisions were
made, and would falsely imply that features such as tagged values and
crate-relative resolution were designed under the new name. Historical
documents stay unchanged; the rename is itself recorded as an RFC.

### Accept `.xl` sources and `xl-deps.json` during a transition

A compatibility window would keep two spellings alive in the resolver, the
manifest loader, tests, and user documentation for an audience that does not
yet exist. The repository migrates in one change instead.

## Deferred work

- none for the rename itself; packaging and acquisition RFCs continue under
  the new vocabulary.

## Implementation result

Implemented in one change sequence:

- `VISION.md` renamed; `README.md` rewritten with a rename notice and updated
  to current syntax; a Chinese `README.zh.md` added.
- Crates renamed on the filesystem and in `Cargo.toml`: `crates/xl` to
  `crates/forma` and `crates/xl-lsp` to `crates/forma-lsp`, with package and
  binary names `forma` and `forma-lsp`.
- The module resolver accepts only `.forma` sources; the language module
  format is `forma`; the syntax-grammar directory and generated-parser output
  moved to `syntax/forma`.
- Dependency manifests are `forma-deps.json`; the cache variable is
  `FORMA_CACHE_HOME`; CLI usage text, shebang examples, the LSP language ID,
  and all test fixtures use the new spellings.
- Historical RFC documents are unchanged except for this RFC and the index.

Verification: `cargo build --workspace`, `cargo test --workspace` (all unit,
CLI, and LSP suites green), `cargo clippy --workspace --all-targets -- -D
warnings`, and `cargo fmt --all -- --check` pass.
