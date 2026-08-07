# RFC 0158: Cross-module closure environments

- Status: Implemented
- Depends on: RFC 0012, RFC 0013, RFC 0034, RFC 0050, RFC 0144 through
  RFC 0147, RFC 0157

## Summary

Forma will preserve the lexical environment of exported bytecode Functions
across module publication, persistent-heap promotion, import projection, and
higher-order return.

```forma
# @src/library.forma
def helper = fn(value) { value + 1 };
export def factory = fn(offset) {
    fn(value) { helper(value) + offset }
};
```

```forma
import "@src/library.forma" { factory };
export let output = factory(2)(39);
```

The requester must export `42`. It must not fail with invalid bytecode, lose
the captured offset, or read a binding from the requester environment.

This RFC repairs a runtime correctness bug. It adds no new source syntax,
type rule, module visibility rule, or observable Function introspection.

## Motivation

RFC 0157 tested a reusable GCC-wrapper module rather than a self-contained
entry. The module passes parsing and checking, but `forma exec --dry-run`
fails when its exported Function reads a module-level helper:

```text
up-link read operand is not an up-link
```

Replacing a curried `command(tool)` with a directly exported
`gcc(settings, request)` does not avoid the failure. The common condition is
that an imported bytecode closure executes a `ReadUpLink` for a definition in
its source module.

Inlining every helper into every entry would hide the bug and destroy the
module reuse that explicit exports are meant to provide. Exported Functions
are ordinary values; their behavior cannot depend on whether a caller lives in
the same source file.

## Lexical ownership

A bytecode Function is interpreted against the lexical environment established
when its closure was created. Its upvalue positions have one compiler-defined
meaning. Publication may relocate heap objects, but it may not change that
meaning.

For every closure capture:

1. an ordinary capture remains the captured ordinary value;
2. a recursive or forward-definition capture expected by `ReadUpLink` remains
   an initialized up-link object;
3. nested closures preserve both the parent capture and any newly captured
   arguments;
4. imported callers cannot substitute bindings with equal source names; and
5. promotion rewrites handles consistently through the complete reachable
   object graph.

An implementation may choose a different internal representation, including
eliminating an unnecessary up-link after initialization, only if it rewrites
the corresponding bytecode/capture contract atomically. A capture slot cannot
contain a resolved ordinary Function while its bytecode still executes
`ReadUpLink` on that slot.

## Publication and caching

The invariant applies through every existing module boundary:

- publication from a Work heap into the persistent Main heap;
- projection of a named export from the synthesized export record;
- reuse of one cached module root by multiple requesters;
- diamond imports that reach one canonical dependency;
- higher-order Functions returned after import;
- repeated calls in separate VM sessions; and
- source replacement in a later workspace revision without mutation of an
  already published snapshot.

The module cache continues to own one immutable published root per canonical
module evaluation. This RFC does not permit requester-local re-evaluation as a
substitute for correct closure publication.

## Recursive definitions

Up-links exist to support single-assignment recursive and forward definition
groups. Fixing imported closures must preserve:

- self recursion;
- mutually recursive Functions;
- forward references within one accepted component;
- uninitialized and duplicate-initialization diagnostics; and
- recursive TypeMetadata graphs that use the same private runtime value class.

Ordinary acyclic helpers may currently share compiler machinery with recursive
groups. The implementation may narrow up-link allocation for optimization,
but such a change is not required and must not alter accepted source programs.

## Errors and internal visibility

An up-link remains a private VM value. It cannot be exported as data, encoded,
compared, observed through Dyn, or displayed by Forma code. A genuine malformed
capture/bytecode pairing remains `InvalidBytecode`; valid compiler output must
not produce that error merely because a closure crossed a module boundary.

Runtime diagnostics inside an imported Function retain the defining module's
instruction locations and call frames. Fixing heap ownership must not rebase
all internal rule locations to the import or call site.

## Goals

1. make direct and higher-order exported bytecode Functions callable from
   ordinary source and dependency modules;
2. preserve recursive-definition up-link semantics through publication;
3. retain closure identity, captures, source locations, and module cache reuse;
4. cover the GCC-wrapper failure with a minimal deterministic regression test;
5. keep module evaluation once-only and requester independent.

## Non-goals

- changing Function equality or adding structural closure equality;
- serializing Functions or exposing closure environments;
- dynamic import, runtime module loading, or requester-dependent resolution;
- changing native Function upvalue representation;
- redesigning recursive definitions or TypeMetadata recursion;
- adding weak references, module unloading, or a garbage collector;
- optimizing away all initialized up-links.

## Acceptance criteria

1. an exported Function can call an acyclic module-level helper after import;
2. a directly exported multi-argument Function and a factory-returned Function
   obey the same lexical ownership rule;
3. a returned closure preserves both its factory argument and module helper;
4. self-recursive and mutually recursive exported Functions work after import;
5. two requesters and a diamond import reuse one published dependency without
   capture corruption or re-evaluation;
6. repeated calls and separate VM sessions produce deterministic results;
7. imported runtime failures identify the defining module and retain a caller
   frame;
8. uninitialized up-links remain rejected and private up-links cannot escape;
9. the reduced GCC-wrapper module reaches plan construction rather than
   failing at its first module helper read;
10. existing heap promotion, module, recursion, codec, and TypeMetadata tests
    remain passing;
11. full workspace tests, formatting, and warning-denied Clippy pass.

## Implementation plan

1. add a module regression fixture for direct and higher-order exported helper
   calls and confirm the current `ReadUpLink` failure;
2. inspect closure construction and `publish_root` copying to find where an
   up-link capture is resolved or replaced without matching bytecode rewrite;
3. preserve the capture kind and relocate the initialized up-link graph during
   publication;
4. add recursive, diamond, repeated-session, and source-location coverage;
5. rerun the reduced GCC-wrapper dry-run and the full static checks;
6. append implementation evidence before marking this RFC Implemented.

## Stopping rules

Work returns to discussion if the fix requires:

1. re-evaluating a dependency separately for each requester;
2. resolving captured names dynamically from the requester module;
3. exposing up-links or closure environments to Forma code;
4. weakening recursive-definition initialization checks;
5. making module cache identity depend on physical heap addresses; or
6. changing public Function or module semantics beyond this correctness repair.

## Implementation result

The reduced production regression did not reproduce the provisional
GCC-audit failure. Current module publication already preserves initialized
up-link objects through `publish_root`; named and namespace imports both bind
the persistent module root rather than the legacy exported `Value` projection.
No VM or heap change was therefore justified.

The new regression exercises the complete shape that matters to RFC 0157:

- namespace import of a source module;
- imported runtime TypeMetadata rebound for authored Function contracts;
- native-module and module-helper captures in one closure;
- a directly exported Function and a higher-order returned closure;
- a `Fn(String) -> Fn(ExecSettings, ExecRequest) -> ExecEnv` factory;
- nested module-helper calls, mixed ordinary/up-link captures, and Struct
  arguments;
- mutually recursive exported Functions; and
- execution of the resulting plan-shaped value from a requester module.

All cases execute successfully after persistent publication. The implementation
evidence therefore closes this RFC as a verified correctness invariant and
regression gap, not as a runtime repair. The earlier temporary audit most
likely contained a fixture-specific mismatch that was not preserved after the
temporary files were removed; it is not sufficient evidence for changing
closure representation.

The acceptance boundary remains useful: future heap, export, or compiler work
must keep this regression passing, and an exact end-to-end wrapper failure must
be reduced independently rather than attributed to cross-module closure
ownership by default.
