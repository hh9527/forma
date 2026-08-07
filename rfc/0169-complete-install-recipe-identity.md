# RFC 0169: Complete install recipe identity

- Status: Implemented
- Depends on: RFC 0166, RFC 0168

## Summary

Executable plans will derive an installation destination from the complete
logical installation recipe. The GCC-wrapper fixture will hash a canonical
record containing:

```text
package name
source URL
integrity digest
unpack type
strip count
```

Changing any installation input changes `dest`. Physical cache prefixes and
the derived download `file` path are excluded from the identity.

## Motivation

The current fixture hashes only package name, source URL, and digest. Two
actions using the same archive with different unpack types or strip counts
therefore collide at one installation destination. The external executor must
not reinterpret this collision or decide whether an existing installation is
compatible.

Forma already computes the whole plan. Its pure plan layer should provide an
address that uniquely represents the installation recipe the executor is
asked to realize.

## Canonical recipe

For this phase the wrapper constructs the exact newline-delimited identity:

```text
unpack-v1
<package.name>
<package.src>
<package.digest>
<unpack-type>
<strip>
```

The leading version/domain tag prevents this encoding from being confused
with the earlier package-only identity and leaves room for future install
variants. `dest` is:

```text
settings.install_prefix + "/" + sha256(identity)
```

The recipe contains logical inputs only. `settings.install_prefix`,
`settings.download_prefix`, and the derived `file` path are deployment
locations, so moving a cache does not change the recipe hash. The source URL
remains included even when a digest is present because it is part of the
authored acquisition recipe.

The encoding is local to the user-space GCC wrapper. This RFC does not add a
VM instruction or make one global installation identity mandatory for every
application.

## Non-goals

- hashing physical cache prefixes or final paths;
- hashing archive bytes after download;
- adding real installation, locking, or cache validation;
- defining identities for every future `Install` variant;
- introducing a generic canonical-value serializer.

## Acceptance criteria

1. package name, source, digest, unpack type, and strip all affect `dest`;
2. download and install prefixes do not affect the hash suffix;
3. the encoding carries an explicit version/domain tag;
4. `file` remains independently addressed by source URL;
5. GCC, g++, and ar continue to share identical compiler installations when
   their complete compiler recipes match;
6. repeated dry-runs remain byte-identical and perform no cache effect;
7. focused identity tests plus full workspace tests and warning-denied Clippy
   pass.

## Implementation plan

1. make unpack type and strip explicit wrapper recipe inputs;
2. compute `dest` from the canonical complete recipe;
3. add tests proving every logical field changes the suffix and prefixes do
   not;
4. update end-to-end output expectations and record implementation evidence.

## Stopping rules

Work returns to discussion if completion requires an effect, hashing physical
cache locations, or prematurely standardizing a universal install-addressing
protocol.

## Implementation result

Implemented in August 2026. The GCC wrapper now shares explicit `ty` and
`strip` bindings between destination calculation and the emitted `Unpack`, so
the hashed recipe cannot drift from the action. Its versioned identity covers
package name, source URL, digest, unpack type, and strip count in the specified
order.

The CLI fixture locks the canonical GCC and sysroot suffixes, verifies those
suffixes remain unchanged under a relocated cache root, and confirms ar reuses
the identical compiler installation recipe. Download files remain independently
addressed by source URL. Repeated dry-runs remain effect-free and deterministic.
