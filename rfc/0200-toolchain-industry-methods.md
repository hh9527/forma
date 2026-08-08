# RFC 0200: Toolchain industry methods

- Status: Implemented
- Depends on: RFC 0198

## Summary

Extract toolchain-industry package selection and deterministic archive
preparation from the concrete GCC wrapper. Keep GCC-specific target policy,
compiler arguments, tools, and executable-plan assembly in the GCC model.

The industry module defines a typed `Package`, a `PreparedPackage` carrying its
stable destination and inert `Install` action, required catalog selection, and
the versioned archive identity calculation.

## Boundary

`toolchain-method` may know about package sources, digests, cache prefixes,
archive unpack policy, and the ordinary exec protocol. It may not know about:

- gcc, g++, or ar;
- TARGET names or Host platform keys;
- sysroot and prefix-map argument syntax;
- which tools need which packages; or
- application environment policy.

The GCC model owns those choices and consumes the prepared package value.

## Acceptance criteria

1. the industry module contains no GCC tool or target string;
2. the concrete source model remains a typed catalog of compiler and sysroot
   packages;
3. gcc/g++ still produce two installs and ar one;
4. canonical install and download hashes remain byte-identical;
5. argv rewriting and missing TARGET diagnostics remain GCC-owned;
6. malformed JSON retains source and authored type-rule provenance;
7. the cross-industry module is reused rather than copied.

## Implementation result

`examples/toolchain-method/src/toolchain.telora` owns `Package`, catalog
selection, and deterministic archive preparation. The GCC model imports those
types and values while retaining source decoding, platform/TARGET mapping,
tool branching, authoritative argv rejection, and `ExecEnv` construction.

The canonical CLI regression keeps both established install hashes and output
shape. This is visibly useful reuse, but it is an industry abstraction rather
than evidence that analytics and toolchains share package semantics.
