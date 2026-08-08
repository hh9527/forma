# RFC 0191: Rename Forma to Telora

- Status: Implemented
- Supersedes the product name established by RFC 0060

## Summary

Rename the language from Forma to Telora. The previous name is already used by
an established Rust project in a related declarative rendering space, creating
avoidable package, command, search, and ecosystem ambiguity.

Telora has the recursive expansion:

> **TELORA Enables Lowering Objectives to Reliable Artifacts.**

The expansion describes the language boundary without embedding an application
model into the language itself. An objective may be configuration, an agent
intent, or another high-level plan. Domain libraries validate and lower it.
The resulting artifact may be data, SQL, an execution plan, generated files, or
another inert protocol value. A Host retains authority over effects.

## Product surface

The rename is complete and does not retain compatibility aliases:

| Previous | Current |
|---|---|
| Forma | Telora |
| `forma` | `telora` |
| `forma-core` | `telora-core` |
| `forma_core` | `telora_core` |
| `.forma` | `.telora` |
| `forma-deps.json` | `telora-deps.json` |
| `FORMA_CACHE_HOME` | `TELORA_CACHE_HOME` |
| cache namespace `forma/` | cache namespace `telora/` |

Private and native module suffixes compose with the new source extension:

```text
module.telora
module.priv.telora
module.native.telora
exec.entry.telora
```

Extensionless executable scripts remain valid and use the new CLI in their
shebang:

```text
#!/usr/bin/env -S telora exec --dry-run --
```

## Identity

Telora is a verified intent language between agents and the real world:

```text
explicit context + objective
    -> closed Telora computation
    -> validation and lowering by libraries
    -> reliable inert artifact or structured diagnostics
    -> Host authorization and effects
```

This positioning does not add Agent-specific syntax, make ontology a language
concept, or grant Telora external authority. It names the general boundary that
the existing language semantics already provide.

## History policy

RFCs 0001 through 0190 retain the terminology and paths that were current when
they were written. They are an append-only design record, not current product
documentation. README, VISION, INTRO, active discussions, implementation code,
tests, built-in modules, and examples use Telora exclusively after this RFC.

## Acceptance criteria

1. the workspace builds packages and a binary named `telora-core` and `telora`;
2. the resolver accepts `.telora` and rejects the removed `.forma` extension;
3. built-in modules and examples use `.telora` paths;
4. manifests, environment variables, cache paths, diagnostics, and help text use
   Telora names without compatibility aliases;
5. current English and Chinese documentation remain aligned;
6. historical RFCs remain unchanged;
7. the full workspace test suite passes after the migration.

## Implementation result

The crates, parser module, resolver format, CLI, embedded standard modules,
examples, environment boundary, cache namespace, and current documentation were
renamed together. Internal XL and Forma identifiers left by earlier migrations
were also removed. Git history remains the compatibility and provenance record;
the runtime and CLI expose only Telora.
