# RFC 0063: Deterministic standard-library foundations

- Status: Accepted
- Depends on: RFC 0053, RFC 0062

## Summary

Forma expands the small standard library needed for pure plan computation.
The additions cover generic Array traversal, UTF-8 Strings, and deterministic
lexical paths:

```forma
import arrays from "@bim/std/array";
import paths from "@bim/std/path";
import strings from "@bim/std/string";
```

They are ordinary typed core-module functions. They perform no I/O, observe no
host filesystem state, and retain the existing VM fuel, allocation, trace, and
cancellation boundaries.

## Motivation

RFC 0062 demonstrated that Forma can compute a typed executable plan, but its
example still assembles common operations manually. Real launchers and build
plans need to combine argument fragments, inspect collections, construct paths,
and rewrite text. Those capabilities belong in reusable pure modules rather
than in the effectful host.

The goal is a coherent minimum, not an exhaustive mirror of another language's
standard library. Each operation has deterministic cross-platform semantics
and a precise contract expressible by the current type system.

## Array additions

`@bim/std/array` adds:

```forma
native concat: for(A) Fn(Array(Array(A))) -> Array(A);
native any: for(A) Fn(Array(A), Fn(A) -> Bool) -> Bool;
native all: for(A) Fn(Array(A), Fn(A) -> Bool) -> Bool;
native find: for(A) Fn(Array(A), Fn(A) -> Bool) -> Option(A);
```

`concat` preserves source order. `any` and `all` short-circuit. `find` returns
the first matching item. Predicates must produce canonical Bool atoms; dynamic
violations remain located runtime errors. Empty inputs produce `False`, `True`,
and `None`, respectively.

## String module

`@bim/std/string` exports:

```forma
native length: Fn(String) -> Int;
native join: Fn(Array(String), String) -> String;
native split: Fn(String, String) -> Array(String);
native starts_with: Fn(String, String) -> Bool;
native ends_with: Fn(String, String) -> Bool;
native contains: Fn(String, String) -> Bool;
native replace: Fn(String, String, String) -> String;
```

`length` counts Unicode scalar values, not UTF-8 bytes. `split` follows literal
substring splitting; an empty separator splits at Unicode scalar boundaries.
`replace` replaces every non-overlapping literal occurrence. All functions are
Unicode-safe and locale-independent.

## Lexical path module

`@bim/std/path` treats paths as portable POSIX-style strings and exports:

```forma
native join: Fn(Array(String)) -> String;
native normalize: Fn(String) -> String;
native parent: Fn(String) -> Option(String);
native file_name: Fn(String) -> Option(String);
```

The separator is always `/`. Normalization removes empty and `.` components,
resolves `..` without crossing an absolute root, and retains leading `..` for
relative paths. It does not expand `~`, environment variables, symlinks, drive
letters, or URI syntax. `join` concatenates components and then normalizes;
later absolute components restart the path.

`parent` and `file_name` operate on the normalized representation. A root has
no parent or file name. An empty relative path normalizes to `.` and likewise
has neither.

## Execution and resource semantics

Callback-based Array operations use resumable native continuations, so nested
Forma callbacks share the caller's fuel, allocation account, call-depth limit,
and trace. Predicate operations stop invoking callbacks as soon as their result
is determined.

Native String and path operations validate every argument before allocating an
output. New Strings and Arrays are charged by their logical output size.
Integer conversion for lengths is checked. No function reads process state or
the filesystem.

## Non-goals

- host-native path behavior or Windows drive/UNC parsing;
- URL parsing, shell quoting, globbing, or filesystem access;
- Unicode normalization, grapheme segmentation, case folding, or regex;
- mutable collections, iterators, lazy sequences, or parallel callbacks;
- collection traits or associated item types.

## Acceptance criteria

1. all functions have the contracts above in their exported interfaces;
2. Array result types preserve or transform generic parameters precisely;
3. `any`, `all`, and `find` short-circuit and reject non-Bool predicates;
4. String operations handle non-ASCII input without slicing invalid UTF-8;
5. path results are deterministic across host platforms;
6. normalization cannot cross an absolute root and retains relative `..`;
7. callback traces, fuel, allocation quotas, and nested calls remain bounded;
8. modules resolve without filesystem access;
9. workspace tests, strict Clippy, and formatting checks pass.

## Rejected alternatives

### Use the host path library

That would make the same Forma program compute different plans on different
hosts and would conflate lexical target paths with paths used by the compiler.

### Put these helpers in the executable host

Path construction and argument rewriting are deterministic policy. Moving them
across the effect boundary would contradict RFC 0062 and make dry-run output a
template rather than the final plan.

### Add a general collection interface first

Current generic schemes express these concrete operations. Traits and
associated types should be justified by abstractions that ordinary modules
cannot represent, not introduced as a prerequisite for a small useful library.
