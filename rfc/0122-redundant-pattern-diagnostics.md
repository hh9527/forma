# RFC 0122: Redundant pattern diagnostics

- Status: Implemented
- Depends on: RFC 0121

## Summary

Forma rejects match arms that are certainly unreachable under the shared
typed pattern analysis:

```forma
match option {
    'Some(value) => use(value),
    'Some(_) => fallback, # unreachable: 'Some already covered
    'None => none,
}
```

The initial analysis recognizes three proofs:

1. an earlier irrefutable pattern catches every possible value;
2. an earlier arm wholly covers every Enum variant the current arm can select;
3. earlier arms together wholly cover a closed Enum before a later catch-all.

These proofs use authored arm order and never reorder evaluation.

## Possible and whole coverage

RFC 0119's whole-variant coverage says which variants an arm completely
handles. This RFC also records possible outer variants: `'Some(1)` can select
only `Some`, even though it does not wholly cover `Some`.

An arm is redundant when all its possible variants were wholly covered by
earlier arms. Thus `'Some(1)` after `'Some(_)` is unreachable, but a second
`'Some(2)` after `'Some(1)` is not diagnosed because neither partial payload
pattern wholly covers the variant.

Wildcard and binding patterns can select every variant and are irrefutable.
Tuple and Struct patterns may be irrefutable for their known structural types.
For open or unknown types, only a syntactic wildcard or binding establishes a
catch-all proof.

## Diagnostics

The error points at the unreachable arm pattern and identifies either the
already covered variant names or the prior catch-all condition. Diagnostics
are deterministic and are emitted only for certainty; absence of a diagnostic
does not claim that an arm is useful.

Exhaustiveness and redundancy are complementary. Exhaustiveness compares the
final whole coverage with the Enum domain. Redundancy compares each arm with
coverage accumulated strictly before it.

## Acceptance criteria

1. every arm after a wildcard or binding catch-all is rejected;
2. a repeated unit variant is rejected;
3. a whole Tagged variant followed by another pattern for that tag is rejected;
4. two different refutable payload patterns are not falsely rejected;
5. a catch-all after complete prior Enum coverage is rejected;
6. an irrefutable known Tuple or Struct pattern makes later arms unreachable;
7. unknown types receive only syntactically certain catch-all diagnostics;
8. diagnostics point at the redundant arm and name stable coverage evidence;
9. match evaluation order and bytecode do not change; and
10. full tests and strict static checks pass.

## Non-goals

- complete nested-pattern usefulness analysis;
- combining literal, range, or structural constraints;
- warnings for stylistic overlap that remains reachable;
- automatic arm deletion or reordering; or
- changing the conservative exhaustiveness boundary.

## Implementation result

Typed pattern analysis now records possible outer Enum variants separately
from whole-variant coverage. Match inference compares each arm's possible set
with only the whole coverage accumulated before that arm, and separately tracks
whether prior arms cover every value through an irrefutable pattern or a
complete closed Enum.

The implementation diagnoses arms after catch-alls, repeated unit variants,
payload patterns after an irrefutable Tagged variant, catch-alls after complete
Enum coverage, and arms after irrefutable known Struct/Tuple patterns. It leaves
distinct refutable payload literals alone. Known-incompatible patterns are also
certainly unreachable; their diagnostic recursively points at the smallest
incompatible child while the VM's defensive no-match path remains tested
through an Any scrutinee. Focused tests cover every proof and the existing
arm-order join test now uses two reachable Int literals rather than deliberately
placing an arm after a catch-all. The full core suite and strict Clippy pass
with no bytecode or evaluation-order changes.
