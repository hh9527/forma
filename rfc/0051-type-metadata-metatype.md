# RFC 0051: TypeMetadata metatype

- Status: Implemented
- Depends on: RFC 0003, RFC 0022, RFC 0041, RFC 0050

## Summary

XL introduces `Type` as the static type of valid TypeMetadata values and gives
the built-in TypeMetadata constructors explicit static contracts:

```xl
Int                  // Type
Array(Int)           // Type
Fn(Int) -> String    // Type

def Maybe: Fn(Type) -> Type = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

`Type` is a metatype in the ordinary static type graph. It is not a quantified
parameter, a type scheme, a runtime kind argument, or the type represented by a
metadata value. A type declaration therefore has two related facts:

```text
User                 : Type
declared type User   = Struct(...)
```

TypeMetadata construction remains an ordinary, erased tool-stage computation.

## Motivation

XL already represents contracts as runtime-shaped TypeMetadata and evaluates
type declarations at tool stage. Constructors such as `Array`, `Tuple`, `Fn`,
`Option`, and `Result` are callable values, but static inference currently sees
them as `Any`. User-defined metadata constructors consequently cannot state
their real contract:

```xl
def Maybe = fn(Item) { Option(Item) };
```

The missing fact is not another instance type variable. `Item` above is a
value containing type metadata. Its static type should be `Type`, just as a
string value has static type `String`.

Without that distinction, tooling conflates the type represented by a metadata
value with the type of the metadata value itself, constructor composition is
unchecked, and malformed metadata is discovered only after tool-stage
execution.

## Goals

1. represent `Type` explicitly in descriptors, type graphs, workspace graphs,
   semantic facts, displays, assignability, and diagnostics;
2. expose `Type` itself as valid TypeMetadata in the tool-stage prelude;
3. assign `Type` to primitive and declared TypeMetadata values;
4. give built-in metadata constructors static function contracts;
5. allow user-defined annotated functions such as `Fn(Type) -> Type`;
6. propagate expected `Type` into type declaration expressions;
7. validate a runtime value against `Type` by decoding it as TypeMetadata;
8. keep the represented instance descriptor in the existing `declared_types`
   map independently of the binding's metatype;
9. preserve module schemes for exported typed metadata constructors;
10. retain the current tool-stage evaluator, metadata protocol, and VM ABI.

## Non-goals

- a general kind system or kind polymorphism;
- `Type : Type` as a foundational calculus claim;
- parameterized `type Name(A) = ...` syntax;
- treating `for(A) ...` or `TypeScheme` as a `Type` value;
- implicit generalization of metadata constructors;
- proving arbitrary user functions return valid metadata without executing
  them at tool stage;
- changing the runtime representation of TypeMetadata records;
- first-class reflection from every runtime instance type to metadata.

## Core distinction

The following entities are separate:

```text
TypeDescriptor::Type        static type of metadata values
TypeDescriptor::Int         static type of integer values
Int metadata value          runtime/tool-stage value representing Int
TypeScheme(for(A), body)    quantified static binding contract
Bound(A)                    rigid instance type inside a scheme body
```

Consequently:

```text
Int                         : Type
Array                       : Fn(Type) -> Type
Array(Int)                  : Type
Fn                          : Fn(Array(Type), Type) -> Type internally
Fn(Int) -> String           : Type in surface contract syntax
for(A) Fn(A) -> A           : TypeScheme, not Type
```

`Type` has a TypeMetadata encoding so it can appear in ordinary contracts:

```text
{kind: 'Type}
```

This encoding denotes the metatype. It does not claim that arbitrary metadata
records are instances of the type they represent.

## Static prelude

The tool-stage value prelude gains a parallel static environment. At minimum it
contains:

```text
Type, Any, Int, Float, String, Bytes, Bool : Type
Atom                                      : Fn(Any) -> Type
Array                                     : Fn(Type) -> Type
Tuple                                     : Fn(Array(Type)) -> Type
Fn                                        : Fn(Array(Type), Type) -> Type
Struct, Enum, Union                       : Fn(Any) -> Type
Option                                    : Fn(Type) -> Type
Result                                    : Fn(Type, Type) -> Type
validate                                  : Fn(Type, Any) -> Any
```

`Atom` and the model constructors retain `Any` inputs in this RFC because XL
does not yet have precise static row, atom-name, or heterogeneous metadata
collection types. Their results are nevertheless known to be `Type`.

The surface `Fn(A, B) -> C` grammar continues to lower to the existing internal
call `Fn([A, B], C)`, matching the constructor's static contract.

## Type declarations

For:

```xl
type User = Struct({name: String});
```

analysis records both:

- the expression and binding `User` have static type `Type`;
- `declared_types["User"]` contains the represented Struct descriptor.

References to `User` in a contract are still evaluated to its metadata value
and decoded as the represented instance type. References to `User` in an
ordinary expression or hover query report `Type`.

A type declaration expression is inferred with expected type `Type`. Static
constructor mismatches are diagnosed through normal bidirectional checking.
The expression is still evaluated and decoded; this remains the authoritative
validation for computations containing `Any` or otherwise imprecise inputs.

## User-defined constructors

Metadata constructors use the ordinary annotated-definition mechanism:

```xl
def PairWithString: Fn(Type) -> Type = fn(Item) {
    Tuple([Item, String])
};

type Entry = PairWithString(Int);
```

The function body is checked with `Item : Type`. Calling a built-in constructor
with a non-Type argument is a static error when the argument's type is known:

```xl
def Broken: Fn(Type) -> Type = fn(Item) {
    Array(1)
};
```

Returning a known non-Type value also fails the rigid annotated-definition
check. Dynamic `Any` remains compatible by existing language policy, so final
tool-stage decoding is still required.

An exported annotated constructor retains `Fn(Type) -> Type` in the existing
`ModuleInterface`; no new runtime module representation is introduced.

## Runtime validation

`validate(Type, value)` succeeds exactly when `value` decodes under the
existing TypeMetadata protocol, including transparent attributed wrappers and
linked recursive metadata. It fails with the decoder's structural path when
metadata is malformed.

`TypeDescriptor::Type::to_value` emits `{kind: 'Type}`. Metadata decoding
accepts that kind, and ordinary instance validation handles it by invoking the
same metadata decoder on the candidate value. Bound and inference variables
remain unavailable as ordinary runtime metadata except for the existing
internal bound encoding used while evaluating schemes.

## Modules and semantic queries

No new module payload is needed. Generic and monomorphic annotated constructor
schemes already travel through `ModuleInterface`. Workspace type graphs add a
`Type` node so hover, CLI type output, completion detail, and cross-module facts
can render the metatype without substituting the represented instance type.

The represented descriptor remains available through `declared_types` and the
existing type-name graph. Consumers must choose explicitly between asking for
the binding's static type and asking what a type declaration represents.

## Diagnostics

- known constructor argument mismatches use ordinary call-site diagnostics;
- a known non-Type constructor result points to the annotated definition body;
- malformed computed metadata retains the decoder path and type declaration
  location;
- `Type` validation failures identify the malformed metadata path;
- cancellation continues through existing inference and tool-stage checkpoints.

## Implementation plan

1. add `Type` to descriptor, local graph, workspace graph, displays,
   assignability, serialization, and validation;
2. add the `Type` metadata value to the tool-stage prelude;
3. build a parallel static prelude with contracts for metadata constants and
   constructors;
4. seed ordinary and generic inference environments from that static prelude;
5. record type binding/expression facts as `Type` while preserving represented
   descriptors in `declared_types`;
6. infer type declaration right-hand sides with expected `Type`;
7. type representative built-in and user-defined constructor compositions;
8. verify module interfaces, semantic display, runtime metadata validation,
   diagnostics, and cancellation;
9. run workspace tests, strict Clippy, formatting, and diff checks.

## Acceptance criteria

1. `Type`, primitive metadata constants, and constructed metadata report
   static type `Type`;
2. built-in constructor signatures are data-backed and visible to inference;
3. an annotated `Fn(Type) -> Type` constructor checks and drives a `type`
   declaration;
4. known non-Type constructor arguments and results are rejected statically;
5. malformed values fail runtime/tool-stage `Type` validation precisely;
6. type declaration binding facts and represented descriptors remain distinct;
7. exported constructor schemes work across modules;
8. existing instance contracts, generic functions, metadata decorators,
   recursive types, codecs, and schema generation remain unchanged;
9. workspace tests and strict static checks pass.

## Deferred work

- parameterized type declaration syntax;
- kinds above `Type` and higher-kinded parameters;
- precise static types for Struct/Enum/Union metadata input shapes;
- metadata constructor purity or totality guarantees;
- first-class `TypeScheme` values and higher-rank polymorphism.

## Implementation result

Implemented `Type` across `TypeDescriptor`, the local recursive `TypeGraph`,
the workspace graph, semantic displays, metadata serialization and both owned
and heap-backed decoders. `validate(Type, value)` delegates to the authoritative
metadata decoder, including nested path diagnostics.

Analysis now starts with a parallel static prelude for metadata constants and
constructors. Type declaration names and right-hand expressions have static
type `Type`, while their represented instance descriptors remain in
`declared_types`. Expected `Type` flows into constructor calls and metadata
dictionary candidates; known non-Type arguments and results are rejected, and
computed metadata still undergoes final tool-stage decoding.

`ModuleInterface` retains existing generic schemes plus zero-parameter schemes
whose body contains `Type`, as well as directly exported type bindings. This
preserves typed metadata constructors and values across modules without
changing the established dynamic boundary behavior of unrelated monomorphic
core functions.

Tests cover metadata round trips, constructor and declaration facts, bad
arguments and results, canonical metadata dictionaries, authoritative `Type`
validation, represented-type separation, and cross-module constructors. The
final workspace run passed 190 core tests with one manual benchmark ignored, 9
CLI tests, and 19 LSP tests. Strict Clippy, formatting, and whitespace
validation also pass.

## Rejected alternatives

### Treat metadata values as `Any`

This preserves execution but prevents constructor contracts, composition
checking, and accurate semantic facts. `Any` should represent known dynamic
behavior, not a missing metatype.

### Give `Int` static type `Int`

The runtime value bound to `Int` is metadata, not an integer. The represented
instance type belongs in `declared_types`; assigning the value itself `Int`
conflates two levels and makes `Array(Int)` impossible to type coherently.

### Introduce a full kind system first

XL currently needs one concrete fact: whether a tool-stage value is valid type
metadata. General kind abstraction, higher-kinded variables, and kind inference
would add substantial machinery without improving the immediate constructor
workflow.

### Make `Type` validation shallow

Checking only `{kind: ...}` would accept malformed nested metadata and disagree
with type declarations. Reusing the authoritative decoder keeps one protocol.
