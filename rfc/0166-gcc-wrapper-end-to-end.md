# RFC 0166: GCC-wrapper end-to-end fixture

- Status: Proposed
- Depends on: RFC 0157 through RFC 0159, RFC 0162, RFC 0163, RFC 0165

## Summary

Forma will keep one network-free application fixture composed from three local
crates:

```text
app/bin-src/gcc.forma
gcc-toolchain-define/src/source.json
gcc-wrapper/src/toolchain.forma
```

The entry declares both crates with exact Path dependencies, captures TARGET,
imports raw toolchain data and reusable wrapper logic, and exports a typed
`ExecFn`. `forma exec --dry-run` must produce a canonical plan containing
independent GCC and sysroot installations plus deterministic argv rewrites.

## Application boundary

The source-data crate owns package descriptions. The wrapper crate owns
validation, host/target selection, cache identities, install actions, tool
selection, and argv policy. The entry owns dependency wiring, Host input
capture, and command selection.

No layer downloads, unpacks, or executes anything. URLs and digests are data;
the Host only validates and renders `ExecEnv`.

## Required behavior

For gcc and g++, the wrapper:

1. selects the compiler package from `ExecSettings.platform`;
2. obtains TARGET through typed `dict.get` and selects its sysroot;
3. derives independent install destinations from stable package identity;
4. rejects user `--sysroot`, `-ffile-prefix-map`, and
   `-fdebug-prefix-map` arguments;
5. prepends authoritative sysroot and deterministic prefix maps;
6. returns both install actions and the selected compiler binary.

The reusable module also supports ar, which uses the compiler package without
the sysroot or compiler-specific rewritten arguments. Thin command entries do
not duplicate selection or installation logic.

## Error boundary

Imported source data is explicitly validated before use. Invalid data must
identify the JSON source and authored type rule through existing validation
diagnostics. Missing TARGET and conflicting argv are Host-request failures.

`ExecFn` currently returns `ExecEnv`, not `Result`. The wrapper therefore
converts those two user-space `BlameError` paths into sourced `panic!` at the
entry computation boundary. This is an explicit protocol limitation, not an
implicit widening or partial plan. A later RFC may consider a Result-returning
Host protocol; this fixture does not change it.

## Goals

1. validate the complete closed-data-to-executable-plan composition path;
2. prove Path dependency identities and package-relative imports in a real app;
3. reuse one wrapper implementation across gcc, g++, and ar;
4. demonstrate explicit Host inputs and deterministic command-line rewriting;
5. preserve strict failure: no invalid or partial plan reaches the Host.

## Non-goals

- remote dependencies, downloads, archive extraction, or process execution;
- a complete GCC driver parser, response files, linker forwarding, or probing;
- publishing fixture packages or generating standalone scripts;
- changing `ExecFn`, dependency formats, or provenance semantics;
- benchmarking cache or execution performance.

## Acceptance criteria

1. the fixture uses only Path dependencies and performs no network access;
2. the entry uses `option "exec.capture-envs" ["TARGET"]` and `ExecFn`;
3. gcc output contains two independently addressed `Unpack` actions;
4. GCC package selection follows Host os/arch and sysroot selection follows
   TARGET independently;
5. canonical argv contains sysroot and both prefix maps before original args;
6. gcc and g++ share logic but select distinct binaries; ar omits sysroot;
7. missing TARGET and conflicting authoritative options fail before a plan is
   printed;
8. malformed package data fails with source/rule diagnostics;
9. repeated dry-runs produce byte-identical stdout and no cache directory;
10. strict quota failure cannot publish a partial plan;
11. the umbrella RFC records implementation evidence and remaining limits;
12. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. add test fixtures for source data, reusable wrapper, and thin entries;
2. invoke gcc/g++/ar through the real CLI dry-run adapter;
3. cover missing TARGET, conflicting argv, malformed JSON, and repeatability;
4. record canonical output evidence and mark RFC 0157/0166 Implemented.

## Stopping rules

Work returns to discussion if completion requires remote acquisition, real
effects, a general command-line parser, fabricated provenance, or weakening
the typed `ExecFn` boundary.

## Implementation progress

The executable application fixture and its success/failure CLI coverage were
implemented in August 2026. gcc and g++ produce two independent install
actions and deterministic rewritten argv; ar reuses the compiler package with
one install and no sysroot. Missing TARGET, conflicting authoritative options,
repeatability, and no-cache dry-run behavior are covered through the real CLI.

The fixture exposed two correctness gaps. Ready definition captures are now
materialized by the compiler while unresolved recursive links remain up-links;
the fixture and `std/argv` no longer need source-level workarounds. One gap
therefore remains and this RFC stays Proposed:

1. malformed imported JSON is rejected at the authored `validate` rule, but
   its original `source.json` provenance is lost after crossing the dependency
   and promoted-closure boundary. The diagnostic must eventually carry both
   anchors before acceptance criterion 8 is complete.

No partial plan is printed in either failure. RFC 0157 and this RFC stay
Proposed until the cross-module provenance issue is fixed.
