# RFC 0099: Reference Show interpreter

- Status: Implemented
- Depends on: RFC 0096 through RFC 0098

## Summary

Forma adds an executable user-space `my_show` example that lifts a unary erased
interpreter through the generalized `interpreter` boundary:

```forma
def show_dyn: Fn(Dyn) -> Result(String, BlameError) = ...;

def my_show:
    for(A) Fn(TypeOf(A)) -> Fn(A) -> Result(String, BlameError) =
    interpreter(show_dyn);
```

The example recursively renders representative primitive and structural values
using only public TypeDesc/Dyn observers and standard combinators. It validates
the general mechanism; it is not the language's production Show contract.

## Placement and authority

The implementation lives at `examples/reference-show.forma`. It is ordinary
Forma source and imports only public `@bim/std` modules. No Show module, operator,
implicit capability, or formatting protocol is added to the standard library.

A future production Show may be native so it can provide a stable total format,
efficient escaping, cycle policy, and privileged support for opaque values. The
reference example neither constrains that design nor becomes an implicit
fallback.

## Supported rendering

The example supports:

- Int, Float, and String leaves;
- Atom, Tagged, and Enum tags and payloads;
- Array and Tuple elements;
- Struct and Dict fields in observer order;
- WithAttributes by following its logical descriptor child; and
- explicit Ref descriptors through public resolution.

Strings are quoted and escape backslash and quote. Arrays use `[a, b]`, Tuples
use `(a, b)`, records use `{name: value}`, and tagged values use `'Tag(payload)`.
Atom values omit parentheses.

Bytes, Function, Any, Never, Type, TypeOf, Union, Bound, Dyn, unresolved Ref,
and newly unhandled descriptor kinds return `Err(BlameError)`. Unsupported
values never fall back to debug output or native stringification.

## Recursion and failure

`show_dyn` follows each finite runtime value and its descriptor together. Array,
Tuple, and field rendering use `array.fold_control` so the first observer or
recursive failure returns immediately. Forma runtime values remain acyclic;
recursive type descriptors therefore do not require value-cycle detection.

Every failure names the reference `my_show` rule and retains the offending Dyn
package as blame data. This is an example-level contract, not a global error
format for future Show.

## Validation role

The example specifically proves:

1. unary `A` is packed as Dyn by RFC 0098;
2. a recursive user Function can consume that package through public observers;
3. the lifted result remains `Result(String, BlameError)` and contains no `A`;
4. explicit and witness-driven generic instantiation both work; and
5. failures cross the adapter unchanged.

At least one direct test compares `my_show(Int)(...)` and
`my_show[Int](Int)(...)`. Structural tests cover nested sequences, records, and
tagged values. An opaque Function demonstrates explicit blame.

## Goals

1. validate unary parameter-wise lifting with useful recursive behavior;
2. demonstrate that user-space interpreters are not equality-specific;
3. exercise public reflection and controlled folds together;
4. publish a readable reference example; and
5. close the RFC 0096 phase with implementation evidence.

## Non-goals

- standardizing Show syntax or exact production formatting;
- a native Show implementation or `show` operator;
- implicit formatting in string concatenation;
- custom per-type instances, traits, or capability lookup;
- pretty-print layout, width, indentation, or color;
- cyclic runtime value handling; or
- total rendering of opaque and metadata values.

## Acceptance criteria

1. `examples/reference-show.forma` uses only public Forma APIs;
2. `my_show` has the authored unary generic scheme;
3. primitive and nested structural examples return deterministic strings;
4. Atom and payload-bearing tags render distinctly;
5. explicit and inferred witness calls agree;
6. the first recursive failure propagates as BlameError;
7. Function and unsupported descriptor kinds do not silently stringify;
8. no core Show module, VM operation, or privileged observer is added;
9. umbrella RFC 0096 records the completed phase boundary; and
10. full workspace tests and strict Clippy pass.

## Implementation plan

1. write descriptor normalization, blame, escaping, and recursive render helpers;
2. implement sequence and field traversal with controlled folds;
3. lift `show_dyn` as `my_show` through unary `interpreter`;
4. add execution, scheme, nested-value, instantiation, and failure tests;
5. run the quality gate and record the implementation result; and
6. mark RFC 0096 Implemented with a phase summary.

## Implementation result

Added `examples/reference-show.forma` as an ordinary importable Forma module.
Its recursive `show_dyn` uses public TypeDesc normalization and Dyn observers,
`array.fold_control` for fail-fast sequence/field traversal, Array/String
combinators for assembly, and a small user-space quote/escape helper. The lifted
`my_show` has the proposed unary generic contract and uses the generalized
parameter-wise interpreter path.

The supported implementation renders Int, Float, String, Array, Tuple, Struct,
Dict, Atom, Tagged, Enum, attributes, and explicit recursive descriptor links.
Unsupported Function and descriptor domains return BlameError with rule
`my_show`; no debug stringification or native fallback occurs.

Execution tests import the example and cover inferred and explicit type
application, escaped strings, sequences, Tuples, records, nullary and payload
tags, and Function blame. The expected output is asserted exactly, including
delimiters and escaping. The example required no new observer, standard module,
VM operation, or compiler exception.

Full Forma tests pass with 292 passed and 1 ignored; all 13 CLI and 20 LSP tests
pass, and strict workspace Clippy reports no warnings.
