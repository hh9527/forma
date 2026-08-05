# RFC 0113: Stable native module and type slots

- Status: Proposed
- Depends on: RFC 0059, RFC 0109

## Summary

Forma makes native nominal identity independent of module installation and
declaration order. Core module specifications receive fixed reserved IDs, and
every native type declaration carries an explicit module-local slot:

```forma
native type HashState = @3;
```

The runtime identity remains the compact pair:

```text
NativeTypeId = (NativeModuleId, NativeTypeLocalId)
```

The numbers are trusted linker metadata, not Forma values. Reflection and
diagnostics continue to expose the qualified logical name
`@bim/std/hash#HashState`, never the numeric pair.

## Module ID space

NativeModuleId is an unsigned 32-bit ABI namespace. ID zero is invalid. Forma
reserves `1..=1023` for built-in modules. IDs `1024..=u32::MAX` are available
to an embedding Host registry.

Every built-in module has one explicit ID in the core registry. Reordering the
registry, adding another module, or loading only a subset cannot change an
existing ID. Duplicate, zero, or non-reserved built-in IDs are initialization
errors.

A future Host registry may assign IDs from the open range. A stable external
module must retain its assigned ID across processes before its types may be
used in persistent cache keys. Session-local allocation remains possible, but
such identities are only stable within that Host session. Forma source and
crate manifests cannot claim a NativeModuleId.

This RFC establishes the range and validation contract but does not add
dynamic native module loading.

## Local type slots

The grammar is:

```text
'native' 'type' Identifier '=' '@' Int ';'
```

The integer must fit `u32`. Each slot must be unique among the native type
declarations in its module. Declaration order has no semantic effect. A slot
is an ABI allocation: published modules must not reuse it for an unrelated
type.

Native implementations that require a type witness identify the same explicit
slot. Linking fails before type analysis when:

- a declaration has an invalid or duplicate slot;
- an implementation references an undeclared slot;
- a built-in module ID is invalid or duplicated; or
- the declaration and Host registry otherwise disagree.

The hidden closure upvalue remains the linked NativeType witness. Forma code
cannot construct a witness from `@3`, observe its ID, or use a slot expression
outside a native type declaration.

## Stability boundary

This RFC stabilizes nominal native leaves. It does not replace the existing
primary TypeNode variants or make arbitrary structural type intern IDs stable
across processes. Composite types such as `Array(HashState)` retain their
canonical graph structure; any future persistent TypeFingerprint is separate
from NativeTypeId.

Changing the qualified name while retaining the numeric pair is an ABI rename:
runtime equality still follows the pair, while reflection shows the new name.
The core registry must therefore review name and ID changes together. Cache
formats should carry an ABI version in addition to native IDs before they are
made persistent.

## Acceptance criteria

1. `native type T = @n;` is lossless in CST and lowers with exact source range;
2. the old order-derived `native type T;` form is rejected;
3. HashState uses an explicit non-zero local slot;
4. reordering native type declarations cannot change their identities;
5. duplicate and out-of-range local slots are diagnosed at their declarations;
6. native callbacks resolve witnesses by slot rather than declaration index;
7. every core module has a unique fixed ID in the reserved range;
8. reordering core module specifications cannot change native identity;
9. qualified reflection names and opaque behavior remain unchanged;
10. primary and structural type representations are not migrated; and
11. full workspace tests and strict Clippy pass.

## Implementation plan

1. extend the lossless grammar and AST lowering with explicit local slots;
2. add fixed reserved IDs and registry validation to core module specs;
3. link declarations and native callback witnesses through slot maps;
4. migrate HashState and fixtures from order-derived index zero;
5. add syntax, collision, missing-slot, reorder, and identity tests;
6. amend RFC 0109's implemented surface and record the result here.

## Non-goals

- unifying Int, String, or other primary types with native IDs;
- stable numeric IDs for arbitrary structural or interned type nodes;
- exposing native IDs to ordinary Forma expressions or reflection;
- dynamic libraries, plugin discovery, or package-native loading;
- persistence format or cross-version cache compatibility; or
- Host resources, finalizers, ownership, or invalidation.

