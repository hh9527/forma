# RFC 0173: Exec as a synthetic entry

- Status: Implemented
- Depends on: RFC 0159, RFC 0167 through RFC 0172

## Summary

`forma exec --dry-run` will load and execute a generated `@entry` module
instead of invoking `@main.exec` and interpreting `ExecEnv` directly in Rust.
The entry imports `@main`, the authoritative exec protocol, injected runtime
and option modules, and ordinary codec/JSON libraries. It checks `main.exec`,
constructs Host input, invokes the function, encodes two concrete payloads, and
exports them as Strings.

The CLI prints one envelope without understanding either payload:

```json
{"install": [...], "exec_opts": {...}}
```

## Generated adapter

The conceptual source is:

```forma
import "@main" as main;
import "std/codec" as codec;
import "std/json" as json;
import "std/rt-types/exec.forma" {
    ExecFn,
    ExecSettings,
    ExecRequest,
    ExecEnvironment,
    Install,
};
import "std/rt.native.forma" as rt;
import "std/opts.priv.forma" as opts;

@struct type ExecOpts = {
    cwd: Option(String),
    bin: String,
    args: Array(String),
    env: ExecEnvironment,
};

let checked: ExecFn = main.exec;
let settings: ExecSettings = rt.settings;
let request: ExecRequest = {
    args: rt.args,
    env: rt.env,
    cwd: rt.cwd,
};
let output = checked(settings, request);

def encode = fn(ty, value) {
    match codec.encode(ty, value) {
        'Ok(encoded) => json.stringify(encoded),
        'Err(error) => reraise!(error),
    }
};

export let install = encode(Array(Install), output.install);
export let exec_opts = encode(ExecOpts, {
    cwd: output.cwd,
    bin: output.bin,
    args: output.args,
    env: output.env,
});
```

The actual helper may be duplicated if generic local inference cannot express
it without widening. The typed assignment and codec witnesses are mandatory.

## Host preparation

The CLI first loads `@main` far enough to obtain and validate its immediate
option actions. It snapshots arguments, cwd, selected environment values,
platform, download prefix, and install prefix. It generates:

- `std/rt.native.forma` with typed closed literals for those snapshots;
- `std/opts.priv.forma` with every parsed option action in source order.

The synthetic load then compiles `@main` in the entry graph. This phase may
perform a duplicate frontend load for option extraction; eliminating that
work is an optimization, not permission to merge or reread options during
evaluation.

Captured environment names are deduplicated in first-seen order. Missing names
remain absent. Capture controls only `ExecRequest.env`; target process policy
still comes exclusively from returned `ExecEnv.env`.

## Typed boundary

`let checked: ExecFn = main.exec` is the authoritative Host contract check.
Structurally equivalent aliases and fully inferred equivalent Functions pass.
Wrong arity, parameter types, or result types fail during entry analysis before
invocation. The generated request/settings assignments independently validate
Host snapshots against the same source-owned protocol.

The Host no longer checks only runtime arity or manually traverses `ExecEnv`.
It defensively requires the completed entry export record to contain exactly
two String payloads before printing them.

## Output and atomicity

Both payloads are produced through `std/codec` and `std/json`. A codec failure
is reraised with structured blame. Explicit module publication means neither
payload becomes visible unless all entry initialization succeeds.

The CLI envelope is assembled from already encoded JSON fragments. It does not
parse, reorder, normalize, or derive fields. The external executor may consume
the two channels separately in a future effectful mode.

## Removal

The Rust helpers that construct `ExecSettings`/`ExecRequest`, invoke the entry
Function, validate `ExecEnv`, enumerate `Install`, interpret environment
policy, and write canonical nested JSON are removed. Narrow Host snapshot,
literal-source generation, two-String export validation, and final envelope
assembly remain.

## Non-goals

- actual execution or installation;
- external entry file selection in the CLI;
- eliminating the preliminary option load;
- migrating run/build modes in this RFC;
- changing the exec protocol fields;
- parsing entry-produced JSON back into Rust values.

## Acceptance criteria

1. ExecFn compatibility is checked by ordinary entry source before invocation;
2. equivalent aliases pass and incompatible annotations fail pre-invocation;
3. request/settings are constructed and typed inside entry source;
4. repeated options reach the injected option module in order;
5. capture selection affects request env but not output env implicitly;
6. install and exec options are encoded by Forma codec/JSON modules;
7. codec/reraise diagnostics retain source and rule provenance;
8. entry publication is atomic across both payloads;
9. Rust contains no complete ExecEnv/Install serializer or direct exec invoke;
10. old malformed shapes fail through typed entry/codec diagnostics;
11. GCC-wrapper plans and deterministic recipe identities remain intact;
12. repeated dry-runs are byte-identical and create no cache;
13. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. expose parsed option actions for trusted adapter generation;
2. generate runtime/options literal modules and the exec entry source;
3. execute entry and validate its two String exports;
4. remove direct invocation and manual canonical ExecEnv serialization;
5. migrate adversarial CLI tests and record implementation evidence.

## Stopping rules

Work returns to discussion if completion requires weakening ExecFn checking,
letting Rust reinterpret plan fields, partial publication, exposing private
inputs to main, or bypassing Forma codecs.

## Implementation result

Implemented in `forma exec --dry-run`. The CLI performs a preliminary load for
option discovery, freezes closed runtime and ordered option snapshots as two
entry-only source modules, and compiles the generated adapter through the
ordinary resolver, checker, compiler, VM, codec, and JSON modules. The adapter
assigns `main.exec` to `ExecFn`, constructs typed settings/request values,
invokes it, and exports encoded `install` and `exec_opts` Strings.

The previous Rust `ExecEnv`/`Install` traversal and canonical serializer were
removed. Rust now knows only how to prepare narrow snapshots, reject malformed
adapter exports defensively, and assemble the two already encoded fragments
into one envelope. Entry analysis failures gain a primary diagnostic at the
real top-level `@main.exec` definition while retaining the detailed `@entry`
contract failure.

CLI coverage demonstrates that inferred functions and a separately named,
structurally equivalent `MyExecFn` pass; non-functions, wrong results, unknown
variants, missing install fields, and malformed environment/cwd values fail
before stdout publication. The GCC-wrapper fixture retains dual-source JSON
provenance, deterministic recipe identities, install `file`/`dest` values,
target-selected sysroots, command-line prefix rewriting, and explicit process
environment policy.
