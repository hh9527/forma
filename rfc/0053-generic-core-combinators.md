# RFC 0053: Generic core combinators

- Status: Implemented
- Depends on: RFC 0015, RFC 0016, RFC 0029, RFC 0048, RFC 0052, RFC 0054

## Summary

XL expands its core library with generic Option and Result combinators written
in ordinary XL, and sharpens several existing native contracts:

```xl
import options from "core:option";
import results from "core:result";

options.map('Some(1), fn(value) { value + 1 })
results.map('Ok(1), fn(value) { value + 1 })
```

`core:option` exports `map`, `flat_map`, `unwrap_or`, and `is_some`.
`core:result` retains its dynamic native `unwrap` boundary and adds `map`,
`map_err`, `unwrap_or`, and `is_ok`. The new functions use explicit
`TypeScheme` contracts and ordinary `def`, `fn`, and `match`; they add no VM
operation or runtime type argument.

Existing contracts also become precise where the runtime already guarantees a
relationship: Array filter predicates return Bool, debug observation preserves
its input type, and Dict pair enumeration exposes two-element Tuple items.

## Motivation

RFC 0052 made bidirectional checking authoritative for ordinary and generic
programs. The core Array module already exercises higher-order schemes, but
Option and Result values still require repeated ad hoc matches. Adding their
small, conventional combinator sets provides useful standard-library coverage
and tests whether ordinary XL definitions can serve as typed core library code
without privileged Rust implementations.

Some existing native declarations also discard relationships their
implementations guarantee. `debug.dbg` returns the exact value it receives;
`array.filter` accepts only Bool results; and `dict.pairs` always produces
`(String, value)` Tuples. Encoding those facts improves callers without
changing runtime behavior.

The codec boundary exposes a different problem. `codec.decode(metadata,
value)` chooses its result instance type from a runtime TypeMetadata value. XL
does not yet have singleton metadata types or a dependent relationship between
that value and a scheme parameter. This RFC therefore does not pretend that
`result.unwrap` can recover a statically known type from an `Any` codec result.

## Goals

1. add a reserved `core:option` module implemented entirely in XL;
2. provide generic Option `map`, `flat_map`, `unwrap_or`, and `is_some`;
3. extend `core:result` with generic `map`, `map_err`, `unwrap_or`, and `is_ok`;
4. retain the existing native `result.unwrap` dynamic boundary;
5. type Array filter predicates as Bool;
6. type debug observation as an identity relationship;
7. expose precise Dict pair collection shapes;
8. provide contract syntax for heterogeneous Tuple types;
9. propagate every new scheme through ordinary `ModuleInterface` data;
10. erase all generic parameters before runtime execution;
11. implement combinators with existing syntax, pattern matching, bytecode, and
    VM behavior.

## Non-goals

- interface, trait, capability, or associated-type systems;
- implicit generalization or higher-rank polymorphism;
- methods or member dispatch on Option and Result values;
- a dependent relationship between TypeMetadata values and instance types;
- statically typing codec decode and encode results;
- changing Result error propagation, runtime diagnostics, or VM unwinding;
- adding lazy evaluation, effects, or asynchronous combinators.

## Core Option module

`core:option` is a reserved module identity resolved through the existing core
module registry. It exports exactly four ordinary XL definitions:

```xl
def map:
    for(A, B) Fn(Option(A), Fn(A) -> B) -> Option(B)
= fn(option, function) {
    match option {
        'None => 'None,
        'Some(value) => 'Some(function(value)),
    }
};

def flat_map:
    for(A, B) Fn(Option(A), Fn(A) -> Option(B)) -> Option(B)
= fn(option, function) {
    match option {
        'None => 'None,
        'Some(value) => function(value),
    }
};

def unwrap_or:
    for(A) Fn(Option(A), A) -> A
= fn(option, fallback) {
    match option {
        'None => fallback,
        'Some(value) => value,
    }
};

def is_some:
    for(A) Fn(Option(A)) -> Bool
= fn(option) {
    match option {
        'None => 'False,
        'Some(_) => 'True,
    }
};
```

Callbacks run only for `Some`. `unwrap_or` is strict because XL arguments are
ordinary eagerly evaluated values. This RFC does not add a lazy fallback.

## Core Result module

`core:result` continues to export native `unwrap` and adds four XL definitions:

```xl
def map:
    for(A, E, B) Fn(Result(A, E), Fn(A) -> B) -> Result(B, E);

def map_err:
    for(A, E, F) Fn(Result(A, E), Fn(E) -> F) -> Result(A, F);

def unwrap_or:
    for(A, E) Fn(Result(A, E), A) -> A;

def is_ok:
    for(A, E) Fn(Result(A, E)) -> Bool;
```

Their implementations are ordinary matches. `map` calls its callback only for
`Ok`; `map_err` calls its callback only for `Err`; `unwrap_or` returns the Ok
payload or its strict fallback; and `is_ok` returns Bool.

The parameter order follows XL's existing normalized metadata constructor:
`Result(Ok, Err)`. It is intentionally not reversed to mirror another
language's library spelling.

The native declaration remains:

```xl
native unwrap: Fn(Any) -> Any;
```

`unwrap` is an explicit dynamic boundary: it returns an Ok payload and turns an
Err String or diagnostic Dict into a located runtime failure. Generalizing it
would require its input to statically expose a Result payload type. Existing
codec calls produce `Any`, so no such evidence exists yet.

## Refined native contracts

The Array filter declaration becomes:

```xl
native filter: for(A) Fn(Array(A), Fn(A) -> Bool) -> Array(A);
```

The runtime already accepts only `'True` and `'False`; known non-Bool callbacks
now fail statically while `Any` callbacks retain the runtime check.

Debug observation becomes type preserving:

```xl
native dbg: for(A) Fn(A) -> A;
native dbg_with: for(A) Fn(String, A) -> A;
```

Both implementations return the exact observed rich value. Separate direct
member accesses instantiate freshly; an ordinary local alias remains
monomorphic under RFC 0049.

Dict pair contracts become:

```xl
native pairs: Fn(Any) -> Array(Tuple(String, Any));
native from_pairs: Fn(Array(Tuple(String, Any))) -> Any;
```

XL still lacks a homogeneous Dict instance type, so Dict inputs and outputs
remain `Any`. The pair collection shape itself is exact and useful to Array
callbacks.

`Tuple(A, B)` in a contract is mechanically lowered to the existing metadata
expression `Tuple([A, B])`, just as `Fn(A, B) -> C` is lowered to the existing
`Fn([A, B], C)` metadata constructor call. This contract-only syntax accepts
zero or more item contracts and does not change the ordinary tool-stage
`Tuple` constructor's one-Array argument ABI.

## Static and runtime semantics

The new core definitions are loaded, analyzed, compiled, and published through
the same persistent module path as file modules and native-backed core modules.
Their explicit schemes are checked rigidly once and exported in the existing
`ModuleInterface`. Every direct imported member reference instantiates fresh
inference variables.

Pattern matching uses RFC 0052's bidirectional arm checking. The implementation
does not rely on exhaustiveness or flow-sensitive narrowing: pattern-bound
payloads may remain conservative internally, while the rigid function contract
checks every returned branch.

At runtime Option and Result remain their existing normalized Enum values.
Generic parameters and schemes are erased. The new combinators compile to
ordinary bytecode and invoke callbacks through ordinary calls; no native
continuation or ABI change is introduced.

## Diagnostics

- known callback input and result mismatches are reported at the call site;
- an Array filter callback with a known non-Bool result fails statically;
- a malformed dynamic Option or Result can still reach the existing runtime
  non-exhaustive-match error through `Any`;
- known malformed Dict pair shapes fail against the refined contract;
- dynamic Dict values retain the native runtime validation;
- cancellation continues through module analysis and bidirectional checking.

## Implementation plan

1. register `core:option` as a source-only core module;
2. add the four Option definitions and four Result definitions in core source;
3. retain only `unwrap` as a native Result export;
4. lower `Tuple(A, B)` contract syntax to the existing Array-backed metadata
   constructor call;
5. refine Array, debug, and Dict declarations without changing native code;
6. verify rigid definition checking and exported scheme data;
7. add execution and inferred-type tests for every combinator;
8. test fresh imported member instantiation, monomorphic aliases, callback
   failures, dynamic unwrap compatibility, and refined native contracts;
9. run workspace tests, strict Clippy, formatting, and whitespace checks.

## Acceptance criteria

1. `core:option` resolves without filesystem access and exports four functions;
2. all Option combinators execute correctly for `None` and `Some`;
3. all Result combinators execute correctly for `Err` and `Ok`;
4. callbacks execute only on the matching variant;
5. inferred result types preserve and transform scheme parameters correctly;
6. imported combinator members instantiate freshly while aliases remain
   monomorphic;
7. filter requires a Bool callback when its result is statically known;
8. debug calls preserve exact input types;
9. Dict pair callbacks observe `(String, Any)` item types;
10. `Tuple(A, B)` contracts round-trip losslessly and evaluate through the
    existing Tuple metadata constructor;
11. dynamic codec-to-unwrap pipelines retain their existing behavior;
12. VM bytecode, native ABI, and runtime Option/Result representation remain
    unchanged;
13. no interface, trait, or associated-type representation is introduced;
14. workspace tests and strict static checks pass.

## Deferred work

- dependent typing between TypeMetadata arguments and codec results;
- a statically typed `unwrap` once Result evidence survives external boundaries;
- lazy `unwrap_or_else` variants;
- richer Option and Result functions such as `and_then`, `or_else`, and folds;
- a homogeneous or row-polymorphic Dict type;
- collection interfaces and associated item types.

## Rejected alternatives

### Make every combinator native

Pattern matching and explicit generic definitions already express these
functions. Native implementations would duplicate semantics, enlarge the
trusted VM surface, and fail to exercise XL as its own standard-library
implementation language.

### Generalize dynamic `unwrap` immediately

An `Any` argument supplies no evidence for the Ok payload type. Allowing an
unresolved variable to become `Any` would weaken RFC 0052, while pretending the
TypeMetadata argument to `codec.decode` determines a chosen scheme variable
would be unsound without a data-backed relationship.

### Add methods or an iterable interface

XL has no method or interface model yet. Explicit imported functions remain
consistent with the existing Array, Dict, codec, and debug modules and avoid
prematurely entering trait or associated-type design.
