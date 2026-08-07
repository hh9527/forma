# RFC 0183: Grain and cardinality proof

- Status: Implemented
- Depends on: RFC 0182

## Summary

Make relationship cardinality part of the ontology and reject a dimension
when reaching its entity would expand the selected measure grain. A rejection
is derived from path lowering, not repeated as a measure/dimension Boolean
compatibility rule.

## Model

Relations are classified as `ManyToOne` or `OneToMany` in traversal direction.
The safe catalog contains grain-preserving many-to-one paths. A second catalog
contains known one-to-many reachability. For every target the planner reports
one of three outcomes:

- safely reachable, with a concrete join plan;
- reachable only through fan-out, requiring pre-aggregation or allocation;
- not reachable through the verified ontology.

The first implementation rejects the latter two. Policies that make fan-out
safe are explicit future capabilities, not implicit SQL behavior.

## Acceptance criteria

1. net revenue at Order grain accepts customer region;
2. units sold at OrderItem grain accepts product category and SKU;
3. net revenue plus category/SKU is rejected because Order-to-OrderItem is
   one-to-many, without a hard-coded measure/dimension check;
4. fan-out and missing-path diagnostics remain attached to authored dimensions;
5. failed proof exposes no SQL.

## Implementation result

`relations.forma` now records cardinality and returns `fanout` separately from
`missing`. Product dimensions use the same universal capability factory as
other dimensions. With net revenue selected, their lowering requirements are
valid but relational proof rejects the paths through Order-to-OrderItem. The
invalid fixture reports both dimensions at their authored values, alongside
the independent measure-count and unsupported EmployeeKind diagnostics.
