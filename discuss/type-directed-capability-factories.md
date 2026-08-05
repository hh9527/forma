# Type-directed capability factories

- Stage: Discussion
- Scope: explicit `Eq`/`Hash`-like capabilities derived from `TypeOf(A)`
- Related: RFC 0048, RFC 0051, RFC 0052, RFC 0055, RFC 0085

## Question

Can Forma express most useful constrained-generic behavior without traits by
using a type object to construct an ordinary, statically typed capability?

The motivating shape is:

```forma
native eq_fn: for(A) Fn(TypeOf(A)) -> Fn(A, A) -> Bool;

@struct type User = {
    id: Int,
    name: String,
};

def EqUser: Fn(User, User) -> Bool = eq_fn(User);
```

`User` is both the declared type name in contracts and its runtime metadata
value in expression position. `TypeOf(A)` ties that value to the static type
parameter `A`. The factory returns an ordinary monomorphic closure whose
parameter types are now known.

This document explores that model. It does not propose syntax, a standard
library API, automatic derivation, or an implementation.

## Core observation

Forma already has the connection that a trait system would otherwise need to
reconstruct indirectly:

```text
TypeOf(A) value -> inspectable metadata describing A
A         type  -> static parameter used in the returned Function
```

A declaration can preserve the relationship across a native or ordinary
Forma boundary:

```forma
native eq_fn: for(A) Fn(TypeOf(A)) -> Fn(A, A) -> Bool;
native compare_fn: for(A) Fn(TypeOf(A)) -> Fn(A, A) -> Ordering;
```

Calling `eq_fn(User)` instantiates `A = User`. No global table is searched and
no implementation is selected from ambient scope. The result is just a value:
it can be named, passed, captured, exported, or replaced explicitly.

Conceptually this is explicit dictionary construction, but Forma need not
expose a dictionary or introduce a privileged capability category when one
Function is sufficient.

## What the type object proves

`TypeOf(A)` proves one precise fact:

> this metadata value denotes the same `A` that appears in the static
> signature.

It does not by itself prove:

- that equality, ordering, hashing, encoding, or another operation exists;
- that a derived operation is total;
- that an operation obeys algebraic laws;
- that metadata attributes contain a valid user implementation; or
- that two independently created closures have equal runtime identity.

Those are contracts of the individual factory. Treating `TypeOf(A)` as a
universal capability witness would merely hide the same questions that traits
make explicit.

## `Eq` as the favorable case

Forma already defines structural equality over its runtime values, including
opaque function identity. A type-directed equality factory can therefore be
total if it follows that authoritative equality relation:

```forma
native eq_fn: for(A) Fn(TypeOf(A)) -> Fn(A, A) -> Bool;
```

The metadata can still be useful even if runtime equality is already generic:

- it produces an exactly typed callback for Array/Dict combinators;
- it can precompute a structural comparison plan;
- it can reject malformed user-computed TypeMetadata at construction time;
- it can select field policies encoded as ordinary metadata attributes; and
- it keeps the chosen policy explicit at the call site.

Example use:

```forma
native contains_by: for(A) Fn(
    Array(A),
    A,
    Fn(A, A) -> Bool,
) -> Bool;

def EqUser: Fn(User, User) -> Bool = eq_fn(User);

contains_by(users, requested, EqUser)
```

The standard library may offer the direct convenience form later:

```forma
contains_by(users, requested, eq_fn(User))
```

Callable-shape inference from RFC 0085 makes wrappers around these functions
usable without introducing implicit resolution.

## `Hash` is not automatically the same problem

A useful hash must at minimum satisfy:

```text
EqA(left, right) == 'True  =>  HashA(left) == HashA(right)
```

Forma must also decide whether `Hash` means:

1. an in-process table hash, allowed to depend on opaque function identity;
2. a deterministic content hash, stable across executions and platforms; or
3. a cryptographic digest over a canonical external representation.

These contracts differ for functions, native capabilities, recursive values,
cycles, metadata carrying executable values, and future external handles.
Consequently this declaration is valid only if Forma intentionally defines a
total hash for every `A`:

```forma
native hash_fn: for(A) Fn(TypeOf(A)) -> Fn(A) -> Bytes;
```

If deterministic hashing is unavailable for some types, `TypeOf(A)` alone
cannot statically express that restriction today. Honest alternatives include:

```forma
# Validate derivability when the factory is evaluated.
native try_hash_fn: for(A) Fn(
    TypeOf(A),
) -> Result(Fn(A) -> Bytes, BlameError);

# Ask the caller to provide the policy explicitly.
native hash_with: for(A) Fn(
    A,
    Fn(A) -> Bytes,
) -> Bytes;
```

Returning `Result` is less convenient than `eq_fn(User)`, but it accurately
represents a partial derivation. Inventing a hidden constraint solely to remove
that `Result` would prematurely recreate trait bounds.

## Derivation and customization are separate

The default operation may be derived structurally from metadata:

```forma
def EqUser = eq_fn(User);
```

A domain-specific operation should remain an ordinary explicit value:

```forma
def EqUserById: Fn(User, User) -> Bool = fn(left, right) {
    left.id == right.id
};
```

Both values have the same type and can be passed to the same combinator. The
language does not need to decide which one is the unique implementation for
`User`.

This avoids coherence and orphan rules. A module can export several useful
policies with descriptive names:

```forma
{
    structural: eq_fn(User),
    by_id: EqUserById,
    by_name: EqUserByName,
}
```

The call site chooses semantics through ordinary dependency flow.

## Metadata attributes as policy input

Forma's type objects already carry normalized attributes. A factory may read
declarative attributes to configure structural derivation, much as codecs read
JSON attributes today.

This remains simpler when attributes are data:

```forma
@hash.ignore transient_cache: Any
```

Storing arbitrary executable capability values inside TypeMetadata is a much
larger step. It raises initialization order, closure identity, persistence,
cross-module promotion, serialization, and recursive metadata questions. The
initial model should prefer declarative attributes plus a known factory, while
custom executable policies remain ordinary module exports.

## Factory purity and caching

Given the same normalized type metadata and factory version, construction
should be deterministic and side-effect free. That permits caching by
authoritative metadata identity or structural metadata value.

Caching is an implementation choice, not observable capability identity.
Forma compares functions by opaque identity, so callers must not rely on:

```forma
eq_fn(User) == eq_fn(User)
```

being true unless a future API explicitly promises interning. Behavioral laws,
not closure identity, define the capability.

The factory may precompute a plan once and return a closure capturing that
plan. This mirrors codec/schema planning while keeping invocation typed and
ordinary.

## Recursive types

Recursive TypeMetadata can produce a recursive capability plan, but factory
construction must avoid eagerly expanding metadata forever. The existing
codec/schema machinery suggests the appropriate implementation shape:

1. allocate or identify a plan node for the current metadata root;
2. memoize it before descending;
3. connect recursive references to the existing node; and
4. seal the plan before returning the typed closure.

Runtime cyclic values are a separate question. Structural equality already
defines cycle behavior; a future hash contract must explicitly define it rather
than inherit an accidental traversal limit.

## Capability bundles

One Function needs no new type form. Some algorithms need related operations,
for example equality and hashing that must share one policy:

```text
EqHash(A) = {
    equal: Fn(A, A) -> Bool,
    hash: Fn(A) -> Bytes,
}
```

Forma cannot currently write this schematic Struct as an ordinary parameterized
data declaration. There are three progressively larger options:

1. pass the Functions as separate generic parameters;
2. return a Tuple of Functions when positional access remains tolerable; or
3. design parameterized data types such as `EqHash(A)` in a separate phase.

The first option should be tested in real standard-library APIs before the
third is considered. Parameterized data types affect metadata construction,
recursive types, module interfaces, codecs, display, and inference; they must
not enter as incidental ergonomics for two callbacks.

## Relationship to traits

The factory model provides several benefits commonly associated with traits:

- an operation is connected statically to `A`;
- generic combinators receive correctly typed callbacks;
- structural defaults can be derived;
- user policies can be named and reused; and
- native implementations can expose trusted generic contracts.

It deliberately does not provide:

- implicit lookup;
- a globally preferred implementation;
- coherence or orphan rules;
- method syntax based on receiver type;
- associated types;
- conditional instance chains such as `Eq(A) => Eq(Array(A))`; or
- compile-time proof that every factory is defined for a bounded `A`.

Those omissions are useful until real code demonstrates that explicit
construction and value passing are insufficient.

## Candidate experiments

Before proposing an RFC, implement or prototype these APIs using existing
language concepts wherever possible:

1. `eq_fn(TypeOf(A)) -> Fn(A, A) -> Bool` for scalar, Array, Struct, Enum, and
   recursive metadata;
2. `contains_by` and `unique_by` using an explicitly passed equality Function;
3. `compare_fn` returning a Tagged `Ordering` and an Array `sort_by` consumer;
4. `try_hash_fn` with a precise definition of deterministic hashable metadata;
5. separate equality/hash Functions passed to a small Dict-like algorithm; and
6. one custom policy that intentionally differs from structural derivation.

The experiments should measure semantic friction, not just syntax length:

- Can inference recover every `A` without annotations?
- Where does rank-1 monomorphism force an explicit contract?
- Is repeated factory construction awkward or merely cacheable?
- Do two separately passed operations drift into inconsistent policies?
- Does a named bundle solve a real correctness problem?
- Which metadata categories make derivation partial?

## Provisional direction

The promising default is:

```forma
native eq_fn: for(A) Fn(TypeOf(A)) -> Fn(A, A) -> Bool;
```

with explicit capability values passed to ordinary generic combinators. This
extends Forma's existing type-object model instead of adding implicit instance
resolution.

`Hash` should not copy the same signature until its determinism, supported
metadata domain, cycle policy, and equality law are defined. Partial factories
should return `Result` rather than claim a capability for every `A`.

Parameterized capability bundles remain a possible later motivation for
parameterized data types, not evidence that Forma currently needs traits.

## Open questions

1. Should structural `eq_fn` be native, ordinary Forma code over metadata, or
   a thin typed wrapper around the VM's authoritative equality?
2. Is `Bool` the final public result, or should equality remain the normalized
   `'True`/`'False` Enum representation only through its alias?
3. What exact contract should the name `Hash` imply: process-local,
   deterministic, or cryptographic?
4. Should a partial factory fail during tool-stage construction or return an
   ordinary `Result` value to runtime code?
5. Which declarative metadata attributes are legitimate inputs to derivation?
6. Can separate callback arguments express every near-term standard-library
   algorithm without parameterized Struct types?
7. When a factory result crosses a module interface, is its explicit contract
   enough, or do we need a named capability type for tooling ergonomics?
