# RFC 0054: First-class tagged values

- Status: Proposed
- Depends on: RFC 0001, RFC 0002, RFC 0029, RFC 0052

## Summary

XL introduces `Tagged` as the dedicated value and type shape for an Atom tag
carrying exactly one payload:

```xl
'None                    // Atom('None)
'Some(42)                // Tagged('Some, 42)
'Ok({name: "Ada"})       // Tagged('Ok, {name: "Ada"})
```

An Atom remains a zero-payload value and is also a first-class unary tag
constructor. Calling an Atom with one argument produces a Tagged value with
the same tag:

```xl
let Some = 'Some;
Some(42);
arrays.map([1, 2], Some);
```

Enum contracts map each tag to either no payload or one payload type. A
zero-payload variant accepts the corresponding Atom; a payload variant accepts
the corresponding Tagged value. Option, Result, Bool, pattern matching,
validation, codecs, and native Result boundaries migrate from the previous
Atom-or-two-element-Tuple convention to this direct representation.

This RFC does not merge or remove Array and Tuple. Ordinary Tuple values and
patterns retain their existing meaning, but no longer encode Enum payloads.

## Motivation

XL's Enum metadata already describes variants directly:

```text
Enum {
    None: no payload,
    Some: Int,
}
```

Its runtime convention is indirect:

```xl
'None
('Some, 42)
```

Every Enum consumer must recognize a generic Tuple of length two, verify that
its first item is an Atom, look up the tag, and then interpret the second item
as payload. Pattern matching uses Tuple patterns rather than Enum structure,
payload bindings lose type information, diagnostics mention Tuple mechanics,
and constructors require wrapper closures:

```xl
arrays.map(values, fn(value) { ('Some, value) })
```

Tagged makes the runtime value graph match the TypeMetadata graph. It also
gives Atom a coherent focused role: an interned tag identity, a zero-payload
variant value, and a unary constructor for the corresponding one-payload
shape.

## Goals

1. add a dedicated Tagged runtime and legacy value containing one Atom tag and
   one payload value;
2. add `Tagged(tag, payload)` to static descriptors and type graphs;
3. make every Atom value callable with exactly one argument to construct a
   Tagged value;
4. preserve Atom constructor behavior through aliases and higher-order calls;
5. parse `'Tag(expression)` through ordinary call syntax and synthesize its
   precise Tagged type;
6. add `'Tag(pattern)` as a dedicated tagged pattern;
7. infer tagged-pattern payload bindings from the matched Enum or Tagged type;
8. define Enum assignability and validation directly over Atom and Tagged;
9. migrate Bool, Option, Result, codecs, validation, and native boundaries;
10. expose canonical Tagged TypeMetadata;
11. preserve zero-payload Atoms as allocation-free immediate values;
12. keep schemes, Tagged payload types, and constructor function views erased
    from the runtime calling ABI beyond Atom call dispatch.

## Non-goals

- merging, replacing, or removing Array and Tuple;
- nominal Enum ownership or globally unique variant names;
- globally fixing an Atom's payload arity;
- more than one direct Tagged payload;
- interface, trait, callable protocol, or associated-type machinery;
- automatically generating lexical bindings from Enum metadata;
- exhaustive pattern analysis or general flow-sensitive narrowing;
- retaining the old payload-Tuple representation as a compatibility format.

## Value model

The public value distinction is:

```text
Atom(tag)
Tagged {
    tag: Atom,
    payload: Value,
}
```

Tagged always has a payload. It does not contain `Option<Value>`. The absence
of a payload is represented by the Atom itself:

```text
'None       = Atom(None)
'Some(42)   = Tagged { tag: Some, payload: 42 }
```

Zero-payload Atoms remain immediate runtime values. Tagged is a dedicated heap
object containing a compact tag identity and one rich payload edge. It is not
represented as an ordinary Tuple and cannot be observed through Tuple indexing
or Tuple patterns.

Tagged preserves the payload's rich source location. The Tagged root uses the
constructor call origin. Heap copy, promotion, equality, debug formatting,
quota charging, and legacy import/export treat the payload as one ordinary
reachable edge.

## Atom constructors

Every Atom value is callable with exactly one argument:

```text
call(Atom(tag), payload) = Tagged(tag, payload)
```

This property belongs to the runtime value, not only to an Atom literal AST.
Aliases and imported or computed Atom values therefore retain it:

```xl
let constructor = 'Some;
constructor(1);
```

Calling an Atom with zero or more than one argument is an arity error. Calling
a Tagged value is a non-callable-value error. A Tagged payload may itself be a
Tagged value without special treatment.

The language does not assign one global arity to a tag. Consequently
`'None(value)` is a valid structural Tagged value even though it is not an
instance of the standard `Option(A)`, whose `None` variant has no payload. A
nominal declaration system would be required to reject such construction
globally and is outside this RFC.

## Static types

Static descriptors add:

```text
Tagged {
    tag: Atom,
    payload: TypeDescriptor,
}
```

An Atom expression without a function expectation synthesizes its existing
singleton Atom type:

```text
'None : Atom('None)
```

Calling a value whose resolved type is `Atom(tag)` with one argument of type
`P` synthesizes:

```text
Tagged(tag, P)
```

In a unary function checking context, an Atom has a built-in constructor view:

```text
Atom(tag) <= Fn(P) -> R
    when Tagged(tag, P) <= R
```

This is a focused rule in the bidirectional checker, not a general callable
trait and not a runtime wrapper closure. It applies equally to literals,
aliases, parameters, and fields whose static type is a singleton Atom.

Without an expected result Enum, a higher-order use may infer the singleton
Tagged result itself:

```text
map([1], 'Some) : Array(Tagged('Some, Int))
```

That result is assignable to `Array(Option(Int))`. With an expected
`Array(Option(Int))`, expected-result flow checks the constructor directly
against the Option variant contract.

## Enum relationship

Enum metadata remains a mapping from names to optional payload contracts:

```text
Enum {
    A: None,
    B: Some(Int),
}
```

Its accepted values are exactly:

```text
Atom(A)
Tagged(B, payload) where payload <= Int
```

Accordingly:

```text
Atom(B)          is not assignable to the Enum
Tagged(A, Int)   is not assignable to the Enum
Tagged(B, Int)   is assignable to the Enum
```

Enums remain structural. The same `Tagged(B, Int)` may be assignable to every
Enum containing a compatible `B(Int)` variant. Tagged does not store an owning
Enum identity.

Bool continues to accept `Atom(True)` and `Atom(False)`. Option and Result use:

```text
Option(A) = Enum {
    None: no payload,
    Some: A,
}

Result(A, E) = Enum {
    Err: E,
    Ok: A,
}
```

The existing `Result(Ok, Err)` parameter order is unchanged.

## Patterns

Patterns add one direct form:

```xl
'Tag(pattern)
```

Examples:

```xl
match option {
    'None => fallback,
    'Some(value) => value,
}

match result {
    'Err({message: message}) => message,
    'Ok(value) => value,
}
```

The payload subpattern is required and singular. Existing `'Tag` matches only
an Atom with that tag. `'Tag(pattern)` matches only a Tagged value with that
tag and applies the nested pattern to its payload. Tuple patterns continue to
match only Tuple values.

When the scrutinee is a known Enum, the checker finds the selected payload
contract by tag and propagates it to the nested pattern. When it is a known
Tagged singleton, the checker uses its payload descriptor. Unknown or `Any`
scrutinees retain conservative `Any` pattern bindings. This RFC does not add
exhaustiveness checking.

## TypeMetadata

Tagged has canonical TypeMetadata:

```xl
Tagged('Some, Int)
```

with protocol form:

```text
{
    kind: 'Tagged,
    tag: 'Some,
    payload: Int,
}
```

The tool-stage prelude exposes:

```text
Tagged : Fn(Any, Type) -> Type
```

The constructor requires its tag argument to be an Atom at tool-stage runtime;
`Any` remains the focused static input because XL does not have a general
metatype for tag identities. Metadata decoding, encoding, display, validation,
local and workspace type graphs, and module semantic facts preserve the tag
and payload descriptor explicitly.

`Atom('Tag)` remains the exact zero-payload Atom TypeMetadata constructor. It
is not changed to mean either arity.

## Compiler and VM

Ordinary expression syntax already represents `'Tag(payload)` as a call. The
compiler emits the ordinary call operation. VM call dispatch adds one trusted
case for an Atom callee: require one argument, allocate one Tagged object, and
return it through the ordinary return target. Higher-order calls therefore use
the same path as direct construction and require no synthetic closure.

Tagged patterns lower to dedicated tag-test and payload-read operations. The
payload read is executed only after the tag test succeeds. Bytecode verification
checks registers normally; the VM reports malformed instruction use as an
internal bytecode error rather than treating Tagged as Tuple.

The legacy `Value` boundary adds a Tagged variant. Rich heap import/export,
persistent promotion, structural equality, and debug formatting preserve it
without converting through Tuple.

## Codecs and boundaries

Enum validation directly accepts Atom for zero-payload variants and Tagged for
payload variants. The previous two-element Tuple convention is removed.

Derived JSON codecs retain their external policies but produce and consume
Tagged internally:

- a unit Enum variant decodes to its Atom;
- a payload variant decodes to Tagged;
- encoding a payload variant requires Tagged;
- externally tagged and untagged JSON shapes are otherwise unchanged.

Native Result helpers return `'Ok(value)` and `'Err(error)`. `result.unwrap`
requires Tagged with tag `Ok` or `Err` and reads its payload directly. Contract
blame locations continue to use the rich payload and rule origins.

Debug rendering uses source-like forms:

```text
'None
'Some(42)
'Err("message")
```

Structural equality requires equal tags and structurally equal payloads. Atom
and Tagged are never equal, even when they use the same tag.

## Compatibility

This is an intentional source and value compatibility break:

```xl
('Some, value)  // remains an ordinary Tuple, not an Option value
'Some(value)    // canonical payload variant
```

All repository core modules, examples, tests, codec fixtures, and diagnostics
migrate in one implementation. Decoders do not accept both forms, because a
dual representation would preserve the ambiguity Tagged is intended to remove
and complicate future narrowing and exhaustiveness.

Stored external JSON is unaffected; only canonical XL runtime values and XL
source using payload variants change.

## Diagnostics

- Atom calls report an exact one-argument requirement;
- Tagged calls report that the value is not callable;
- tagged patterns report tag and payload-shape mismatches through normal match
  behavior;
- validation distinguishes an expected unit Atom from an expected payload
  Tagged variant;
- Enum codec errors name the expected tag and payload instead of Tuple length
  or Tuple tag position;
- cancellation remains checked through parsing, analysis, compilation, and
  workspace queries.

## Implementation plan

1. add legacy and rich Tagged values plus heap import, export, copy, equality,
   formatting, and allocation accounting;
2. add Atom call dispatch that constructs one Tagged payload;
3. add Tagged descriptors, graph nodes, metadata encoding, decoding, validation,
   display, and static prelude construction;
4. teach bidirectional inference to synthesize Atom calls and check Atom values
   through their unary constructor view;
5. parse and lower tagged patterns and add LIR/bytecode/VM operations;
6. propagate known Enum and Tagged payload types into pattern bindings;
7. migrate Option, Result, Bool consumers, native helpers, validation, and JSON
   codecs from payload Tuples;
8. migrate core source, examples, tests, displays, and diagnostics;
9. verify direct and aliased constructors, higher-order constructor use,
   structural Enum membership, pattern typing, codecs, promotion, equality,
   quotas, semantic facts, and cancellation;
10. run workspace tests, strict Clippy, formatting, and whitespace checks.

## Acceptance criteria

1. `'None` remains an allocation-free Atom and `'Some(1)` is a dedicated
   Tagged value;
2. an Atom alias remains a callable one-argument constructor;
3. an Atom can satisfy a compatible unary callback contract;
4. Atom construction synthesizes a precise Tagged payload type;
5. zero- and one-payload Enum variants reject the opposite value shape;
6. tagged patterns match and bind payloads with known static types;
7. Tuple values and Tuple patterns no longer participate in Enum semantics;
8. Tagged TypeMetadata round-trips and validates values;
9. Option, Result, Bool, derived codecs, and native Result boundaries use the
   new representation exclusively;
10. Tagged values survive heap promotion and legacy boundaries and compare
    structurally;
11. debug and user-facing values render as `'Tag(payload)`;
12. Array and Tuple semantics otherwise remain unchanged;
13. no interface, trait, nominal Enum, or associated type is introduced;
14. workspace tests and strict static checks pass.

## Deferred work

- exhaustive and redundant pattern diagnostics;
- flow-sensitive Enum narrowing outside match arms;
- nominal Enum declarations and globally scoped constructors;
- constructor sections for payload transformations beyond unary Atom calls;
- payloads with direct arity greater than one;
- Array and Tuple unification or removal.

## Rejected alternatives

### Make zero-payload values Tagged with an optional payload

That makes `'None` a new compound value, risks allocation for Bool and unit
variants, and forces a payloadless Tagged to be both a value and a callable
constructor. Atom already represents the zero-payload case exactly. Tagged is
simpler when its payload is mandatory.

### Keep payload variants as Tuples

The convention is the problem being solved. It loses variant identity in the
runtime kind, weakens pattern typing, duplicates validation, and prevents a
direct first-class constructor model.

### Give every tag one global arity

Structural Enums may reuse a tag with different contracts. A global registry
would introduce nominal ownership and ordering constraints without a declared
Enum namespace. Arity remains a property of each Enum variant contract.

### Rewrite Atom literals to synthetic closures

Literal-only rewriting fails aliases and computed Atom values, allocates or
generates unnecessary functions, and makes constructor behavior depend on
syntax rather than value semantics. Atom callability belongs in runtime call
dispatch and the static Atom descriptor rule.
