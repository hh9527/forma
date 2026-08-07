# RFC 0165: Dict lookup and argv rewrite combinators

- Status: Implemented
- Depends on: RFC 0053, RFC 0097, RFC 0107, RFC 0157, RFC 0163

## Summary

Forma adds one missing type-preserving Dict primitive and one small pure argv
module:

```forma
import "std/dict" as dict;
import "std/argv" as argv;

dict.get(request.env, "TARGET") # Option(String)
argv.reject_option(request.args, "--sysroot")
# Result(Array(String), BlameError)
```

`std/argv` recognizes a long option in either `--name` or `--name=value` form.
It does not parse shell text, response files, short-option clusters, or a
tool-specific grammar.

## Motivation

Direct dynamic field access turns a missing TARGET into a low-level runtime
failure. The wrapper needs typed absence so user-space code can attach a
domain-specific diagnostic.

GCC argv rewriting also needs one explicit conflict policy. The wrapper will
inject its own `--sysroot` and source/debug prefix maps, so an authored helper
must reject user arguments that could override those deterministic values.
Array and String already provide the iteration and prefix operations; this RFC
only packages the repeated option-shape policy.

## Dict lookup

`std/dict` exports:

```forma
native get: for(A) Fn(Dict(A), String) -> Option(A);
```

An existing key returns `'Some(value)` without widening `A`. An absent key
returns `'None`. A non-Dict first argument remains a native boundary error;
well-typed Forma code cannot produce it.

The operation does not add a default, callback, insertion, or mutation API.
Applications choose their own missing-key error and blame anchor.

## Argv module

`std/argv` is an ordinary source-only module exporting:

```forma
matches_option: Fn(String, String) -> Bool;
contains_option: Fn(Array(String), String) -> Bool;
reject_option:
    Fn(Array(String), String) -> Result(Array(String), BlameError);
prepend: Fn(Array(String), Array(String)) -> Array(String);
```

For option name `--sysroot`, `matches_option` accepts exactly `--sysroot` and
Strings beginning with `--sysroot=`. It does not accept `--sysrooted`, split
the following argument, normalize spelling, or inspect response files.

`reject_option` returns the original immutable Array in `'Ok` when no match
exists. On the first matching argument it returns `'Err(blame!(argument,
message))`, preserving the Host request value as the diagnostic subject.
Validation Functions returning `Result` may chain several policies with `?`
before prepending authoritative arguments. The current `ExecFn` returns
`ExecEnv`, not `Result`; an executable wrapper must therefore handle the error
explicitly at that boundary, for example by converting its message to
`panic!`. This RFC does not silently change the Host protocol.

`prepend(prefix, arguments)` is equivalent to `array.concat([prefix,
arguments])`; the named operation makes rewrite intent explicit at call sites.
Neither operation mutates or reorders existing arguments.

## GCC-wrapper policy

The end-to-end fixture will reject these user-controlled options before adding
authoritative replacements:

- `--sysroot`;
- `-fdebug-prefix-map`;
- `-ffile-prefix-map`.

It then prepends deterministic values derived from the selected sysroot and
working directory. This RFC supplies mechanism, not a globally mandatory GCC
policy. The wrapper module owns the list and ordering.

## Goals

1. expose missing Dict keys as typed `Option` values;
2. preserve `Dict(A) -> Option(A)` through inference and module interfaces;
3. centralize the minimal long-option matching rule used by the wrapper;
4. permit early sourced conflict rejection before constructing `ExecEnv`;
5. keep argv rewriting immutable, deterministic, and ordinary Forma code.

## Non-goals

- a command-line parser, shell lexer, schema, mutable builder, or iterator;
- short-option clusters, option arity, `--` semantics, or response files;
- parsing GCC input/output modes, linker forwarding, `-I`, or `-L`;
- Dict insertion, removal, arbitrary key types, or ordered-map semantics;
- automatically rewriting every executable plan;
- moving GCC policy into the VM or exec Host.

## Acceptance criteria

1. `dict.get({A: 1}, "A")` is `'Some(1)` and missing lookup is `'None`;
2. generic and nested Dict values retain their exact element type;
3. lookup participates in check, show, hover, and imported interfaces without
   widening to `Any`;
4. option matching distinguishes `--x`, `--x=value`, and `--xyz`;
5. rejection reports the first conflicting argument as its blame subject;
6. prepend preserves prefix order and the complete original argv order;
7. the module composes with `?` in an exported validation Function, while an
   `ExecFn` handles the resulting error explicitly;
8. quota and invalid-native-argument behavior follow existing Dict/Array
   primitives;
9. no API performs effects or mutates an Array/Dict;
10. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. add `CoreDictFunction::Get` and its generic native declaration;
2. test runtime lookup, static type preservation, missing keys, and quotas;
3. add a reserved source-only `std/argv` module using existing combinators;
4. test exact matching, sourced rejection, prepend order, and `?` composition;
5. update the umbrella and GCC-wrapper thought experiment with accepted names.

## Stopping rules

Work returns to discussion if implementation needs mutable collections, a
general CLI grammar, dynamic return widening, or a Host/VM-specific GCC path.

## Implementation result

Implemented in August 2026. `std/dict.get` is a generic native primitive that
returns `Option(A)` and allocates only the `'Some` wrapper when a key exists.
`std/argv` is embedded source at reserved module ID 22; its matching,
rejection, and prepend behavior is authored in Forma over existing Array and
String combinators.

Supporting a source-only composed standard module closed one bootstrap gap:
built-in source modules may now namespace- or selectively import an earlier
registered built-in and receive its ordinary value, persistent root, and typed
interface. Registration order is explicit, missing earlier dependencies are
sourced errors, and no runtime loading or import cycle mechanism was added.

Tests cover exact and `=value` matching, prefix false positives, typed present
and missing Dict lookup, immutable prepend order, first-conflict blame data,
and `?` composition inside an exported Result-returning Function. The GCC
thought experiment now handles those Results explicitly at the non-Result
`ExecFn` boundary.
