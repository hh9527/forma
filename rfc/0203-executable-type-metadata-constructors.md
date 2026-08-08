# RFC 0203: Executable TypeMetadata constructors

- Status: Implemented
- Depends on: RFC 0051, RFC 0055, RFC 0192, RFC 0202

## Summary

Verify that the ontology experiment can construct model types with ordinary
Telora functions before proposing a new parameterized-type mechanism.

Two forms are exercised:

```telora
def Maybe:
    for(T) Fn(TypeOf(T)) -> TypeOf(Option(T)) =
    fn(inner) { Option(inner) };

def Capability:
    for(Id, Input, Output)
    Fn(TypeOf(Id), TypeOf(Input), TypeOf(Output)) -> Type =
    fn(Id, Input, Output) {
    struct('None, {
        id: Id,
        lower: Fn(Input) -> Option(Output),
    })
    };
```

The first preserves the precise witness of a composition over a built-in type
family. The second preserves precise input witnesses while widening its
currently unnameable user-family result to `Type`. It computes a concrete
structural type from model-supplied metadata; a consuming declaration evaluates
it at tool stage and retains the complete represented type.

## Acceptance criteria

1. a user function maps `TypeOf(T)` to `TypeOf(Option(T))` without native code;
2. an exported metadata function builds a capability record from three model
   types;
3. another crate instantiates the generated type in a declaration;
4. functions stored in the generated record receive and return the instantiated
   concrete types;
5. a mismatched capability implementation fails strict checking; and
6. no ontology-specific analyzer, VM, or Host behavior is added.

## Implementation result

`ontology-method/types.telora` exports precise `Maybe` and `Many` compositions
plus input-precise structural `Requirement` and `Capability` metadata
constructors. The
`ontology-construction` example imports them through a Path dependency,
instantiates closed Entity and Measure types, constructs a typed lowering
function, and executes it successfully.

The invalid fixture returns Entity where the generated capability requires
String and is rejected by the ordinary contract checker.

## Exact boundary

The following attempted signature is not currently available:

```telora
def Capability:
    for(Id, Input, Output)
    Fn(TypeOf(Id), TypeOf(Input), TypeOf(Output))
        -> TypeOf(CapabilityShape(Id, Input, Output)) =
    CapabilityShape;
```

While evaluating the generic scheme, `CapabilityShape` is not a type binding
available to the annotation evaluator. User-defined metadata functions can
therefore create concrete instantiated types, but cannot yet name their own
result as a precise reusable type family in another generic scheme.

This does not block the ontology experiment. Concrete models can instantiate a
generated type once, and shared higher-order functions can remain generic over
that complete type while receiving typed selectors and constructors. RFC 0204
must test that pattern before requesting a language extension.

The boundary is narrower than “Telora lacks parameterized type constructors”:
executable construction works, precise built-in-family composition works, and
the missing piece is a statically nameable user-defined family witness.
