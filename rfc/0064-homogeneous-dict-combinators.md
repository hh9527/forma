# RFC 0064: Homogeneous Dict combinators

- Status: Implemented
- Depends on: RFC 0053, RFC 0061, RFC 0063

## Summary

`@bim/std/dict` gains three generic operations whose contracts preserve the
item type of `Dict(T)`:

```forma
native map_values: for(A, B) Fn(Dict(A), Fn(A) -> B) -> Dict(B);
native filter: for(A) Fn(Dict(A), Fn(A) -> Bool) -> Dict(A);
native fold: for(A, B) Fn(Dict(A), B, Fn(B, String, A) -> B) -> B;
```

The existing heterogeneous `keys`, `values`, `pairs`, `from_pairs`, and
`merge` functions remain unchanged.

## Motivation

RFC 0061 introduced precise homogeneous Dict metadata but deliberately did not
weaken the older heterogeneous Dict API. Replacing those functions with
`Dict(A)` contracts would reject Struct-shaped values whose fields have
different types.

New operations can be precise without overloading old names. `map_values`,
`filter`, and `fold` are useful for environment maps, labels, headers, lookup
tables, and decoded dynamic objects. Their input explicitly requires a
homogeneous Dict, so `Dict(A)` supplies all generic evidence needed by the
current bidirectional checker.

## Semantics

Iteration follows canonical key order, matching the immutable Dict shape and
codec behavior.

- `map_values` invokes the callback once per value and retains every key;
- `filter` retains entries whose callback returns `True`;
- `fold` invokes its callback as `(accumulator, key, value)` and returns the
  final accumulator;
- empty inputs return an empty `Dict(B)`, an empty `Dict(A)`, and the initial
  accumulator, respectively.

Callbacks receive values with their original source locations. Created Dict
containers use the call location while retained and transformed values preserve
their value locations.

## Compatibility

This RFC adds functions and does not alter the contracts or runtime behavior of
the five existing Dict operations. A heterogeneous Struct-shaped Dict remains
valid for those operations. It is rejected by a homogeneous combinator unless
all fields are assignable to one inferred item type or an expected `Dict(T)`
provides that type.

No overload resolution is introduced. The distinction is visible in operation
names and module-interface data.

## Execution and resource semantics

All three functions use resumable native continuations. Callback calls consume
ordinary VM fuel and share allocation and depth limits. `filter` requires a
canonical Bool result. Output allocation is charged incrementally and failure
does not publish a partial Dict.

## Non-goals

- changing legacy heterogeneous operations;
- overloads, row polymorphism, value unions, traits, or associated types;
- mutable insertion, deletion, or in-place updates;
- key-transforming map, because transformed String keys require an explicit
  duplicate-key policy;
- implicit widening of unannotated Struct literals.

## Acceptance criteria

1. exported schemes exactly preserve the relationships above;
2. `map_values` transforms `Dict(A)` to `Dict(B)`;
3. `filter` retains `Dict(A)` and rejects non-Bool callback results;
4. `fold` exposes String keys and typed values in canonical order;
5. callbacks execute exactly once per visited entry and nest normally;
6. empty Dict behavior is defined and typed;
7. legacy heterogeneous functions and tests remain unchanged;
8. workspace observation and hover retain `Dict<T>` results;
9. fuel, allocation, stack, source trace, and cancellation behavior match Array
   combinators;
10. workspace tests and strict static checks pass.

## Rejected alternatives

### Retype the existing functions

This repeats the incompatible RFC 0061 implementation attempt and removes
valid heterogeneous behavior.

### Add overload resolution

Three independent new operations do not justify a language-wide dispatch
mechanism. Overloads would also make the static choice depend on distinctions
that currently share one runtime Dict representation.

### Pass an explicit TypeMetadata witness

The `Dict(A)` argument already carries sufficient static evidence. A redundant
runtime witness would complicate calls and incorrectly suggest that callbacks
need runtime type inspection.

## Implementation result

The three contracts are exported from `@bim/std/dict` exactly as specified.
Their VM implementations use a dedicated resumable continuation over the
canonical Dict shape. `map_values` retains keys while transforming item types,
`filter` retains both key and located value, and `fold` supplies each String key
between the accumulator and typed value.

Output entries are allocation-charged as callback results arrive and the final
Dict is published only after successful completion. Fold key Strings share the
same quota account. Callback failures include the native combinator frame, and
non-Bool filter results remain explicit runtime errors when static knowledge is
`Any`.

Vertical tests observe `Dict<String>` and `Dict<Int>` result types, canonical
order, empty behavior, nested callback execution, rigid predicate contracts,
heterogeneous input rejection, and located callback traces. The five legacy
heterogeneous operations and their existing tests are unchanged.
