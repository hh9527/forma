# RFC 0067: Conservative YAML 1.2 static data modules

- Status: Implemented
- Depends on: RFC 0057, RFC 0066

## Summary

Forma imports `.yaml` and `.yml` as immutable static data modules using an own
lossless frontend and the shared static-data publication pipeline. The initial
language deliberately implements a deterministic subset of YAML 1.2 rather
than YAML's full extensibility surface.

The semantic contract is YAML 1.2 Core Schema, String mapping keys, one
document, and immutable alias expansion. YAML 1.1 timestamp and Boolean
spellings are not inferred.

## Supported surface

- indentation-based mappings and sequences;
- flow mappings and sequences;
- plain, single-quoted, and double-quoted scalars;
- null, Boolean, integer, and floating Core Schema scalars;
- literal and folded block scalars with indentation and chomping indicators;
- anchors and backward aliases expanded into immutable values.

Mappings reject duplicate keys. Aliases do not retain runtime identity and are
bounded by depth and total expansion work. Unknown or cyclic references fail
without publishing a partial value.

## Deliberate rejections

- multiple-document streams;
- non-String mapping keys;
- explicit or custom tags;
- the YAML merge key `<<`;
- forward aliases;
- tabs used for indentation;
- YAML 1.1 implicit values such as `yes`, `on`, or timestamps.

Rejecting these forms keeps source interpretation deterministic and prevents a
configuration file from silently acquiring application-specific constructors.
Quoted text always remains String.

## Values and provenance

YAML null lowers to `'None`; Core Schema Booleans, integers, and floats lower to
their Forma scalar counterparts. Sequences become Arrays and mappings become
canonical Dicts. Plain timestamps are Strings.

Every scalar, sequence item, mapping key, and container records its source
location. An alias value is attributed to the alias use while its expanded
children retain their defining locations, allowing codec blame to identify the
effective data and its origin.

## Acceptance criteria

1. `.yaml` and `.yml` resolve and publish through the static-data pipeline;
2. the CST reconstructs the original source exactly;
3. Core Schema inference does not use YAML 1.1 legacy rules or timestamps;
4. block and flow collections lower to immutable Forma values;
5. block scalar folding, chomping, and indentation are honored;
6. duplicate keys, custom tags, merge keys, and document streams are rejected;
7. aliases expand deterministically with cycle and resource bounds;
8. strict and recoverable workspaces agree on values and diagnostics.

## Implementation result

The YAML frontend tokenizes complete physical lines into a lossless Lelwel CST;
the indentation-aware lowerer consumes the same `DocumentText` through its
rope chunks. It implements block and flow
collections, Core Schema scalars, quoted Strings, block scalars, anchors, and
bounded aliases while recording shared value provenance.

`ModuleFormat::Yaml` is enabled in resolution, strict loading, and recoverable
workspace snapshots as `WorkspaceModuleKind::Yaml`. Invalid YAML retains source
and diagnostics but publishes no guessed value.
