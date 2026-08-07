# RFC 0164: Pinned GitHub dependency provider

- Status: Proposed
- Depends on: RFC 0059, RFC 0140, RFC 0157, RFC 0162, RFC 0163

> Phase note (August 2026): implementation is deferred. The RFC 0157
> GCC-wrapper milestone uses existing exact Path dependencies so acquisition,
> cache, authentication, and asynchronous provider engineering do not block
> validation of the application model. This proposal remains the intended
> publication direction but is not on the current phase's critical path.

## Summary

`crate.dependency` gains one exact external source form:

```forma
option "crate.dependency" {
    name: "gcc-wrapper",
    source: 'GithubRepo({
        repo: "hh9527/gcc-wrapper.forma",
        rev: "0123456789abcdef0123456789abcdef01234567",
    }),
};
```

The resolver delegates acquisition of the pinned repository to a Host
dependency provider. The provider returns a local immutable crate root; normal
Forma resolution then maps requests such as `gcc-wrapper/toolchain.forma` to
the deterministic logical module ID with that same request spelling.

This RFC defines no network implementation. Tests and embedding Hosts inject a
provider, while the default Host reports that the exact dependency is not
available.

## Motivation

Path dependencies are useful during development but cannot describe the
published GCC-wrapper entry. Letting Forma evaluation clone a repository would
break the closed-world boundary. Hard-coding cache paths into module IDs would
make diagnostics and identity machine-dependent.

An exact provider boundary separates these concerns:

```text
(GithubRepo repo, rev) -> immutable physical crate root
dependency name + package-relative path -> logical ModuleId
```

The first mapping belongs to the Host. The second remains the existing module
resolver's deterministic, containment-checked operation.

## Specification

`repo` is exactly two non-empty GitHub path segments, `owner/repository`, with
ASCII letters, digits, `.`, `_`, and `-`. It has no scheme, host, query,
fragment, `.git` suffix normalization, or case rewriting.

`rev` is a lowercase hexadecimal full object identifier of 40 or 64 digits.
Branches, tags, abbreviated hashes, version ranges, and symbolic revisions are
rejected before provider invocation.

Dependency `name` keeps the existing module-name rules and must be unique.
Two names may intentionally point to the same exact source while retaining
distinct logical crate identities. A name cannot silently change source within
one root.

## Provider boundary

`forma-core` exposes an object-safe, thread-safe provider interface receiving a
validated owned specification and returning a crate root or a structured
failure. `EngineBuilder` installs at most one provider. An Engine without one
can still load roots using built-ins and Path dependencies, but rejects a
`GithubRepo` dependency with its option source location.

The provider may use a cache or network according to Host policy. Its returned
path is canonicalized and must be a directory. All subsequent imports use the
ordinary crate-boundary and private/native module rules. Provider errors may
describe acquisition, but credentials and physical cache paths do not become
logical module IDs or Forma values.

Providers receive cancellation in a later RFC when acquisition becomes async.
This phase performs only immediate injected resolution and must not add a
blocking public-network operation to module loading.

## Identity and cache key

The provider key is the byte-exact pair `(repo, rev)`. The dependency's public
module identity remains its manifest key, for example
`gcc-wrapper/toolchain.forma`. The resolved physical root is implementation
state and may differ across machines.

A Host cache may derive a directory from a domain-separated digest of the
provider kind, repository, and revision. This RFC does not standardize the
physical cache layout because no built-in downloader is added.

## Goals

1. express an immutable published dependency in an entry module;
2. keep acquisition outside Forma evaluation and VM bytecode;
3. keep logical IDs stable across cache locations;
4. reuse all existing containment, suffix, format, and crate-private rules;
5. permit deterministic network-free end-to-end fixtures through injection.

## Non-goals

- cloning, fetching, authentication, retries, mirrors, or offline cache UX;
- GitHub archives, releases, Git trees, submodules, or Git LFS;
- transitive dependency solving, lockfile generation, updates, or version
  selection;
- accepting branches, tags, short revisions, URLs, or arbitrary Git hosts;
- exposing provider or cache objects to Forma code;
- changing the canonical module-ID grammar.

## Acceptance criteria

1. a valid `GithubRepo` option invokes an installed provider exactly once per
   exact source in one resolver construction;
2. provider-backed modules resolve under the dependency key and support
   package-relative imports;
3. two physical roots for the same injected source produce the same logical
   module IDs in independent sessions;
4. malformed repo/rev values fail at the authored option before invocation;
5. absent-provider and provider failures remain sourced resolver diagnostics;
6. returned files cannot escape the provider crate root through lexical paths
   or symlinks;
7. private and native module access rules match Path dependencies;
8. tests perform no public network access;
9. existing Path dependencies and `forma-deps.json` behavior remain intact;
10. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. add validated `GithubRepoSpec` and a dependency-provider interface;
2. carry an optional provider from `EngineBuilder` into resolver construction;
3. extend embedded `crate.dependency` validation with `GithubRepo`;
4. memoize exact provider results during root resolver construction;
5. canonicalize and validate returned crate roots before registration;
6. add injected local fixtures for identity, containment, diagnostics, and
   coexistence with Path dependencies.

## Stopping rules

Work returns to discussion if this requires network access in tests, a provider
call during Forma evaluation, physical paths in public module IDs, floating
revisions, or a general package solver.
