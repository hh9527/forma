# B2B commerce reporting domain

This fixture is a bounded experiment for `discuss/intelligent-reporting-intent-compiler.md`.
It is deliberately small enough to inspect while retaining business semantics
that cannot be recovered from SQL storage types alone.

## Physical tables

1. `regions`: a country/region hierarchy.
2. `organizations`: tenant and customer organizations, located in a region.
3. `employees`: employees belonging to an organization.
4. `customers`: purchasing accounts belonging to an organization.
5. `categories`: product-category hierarchy.
6. `products`: products belonging to a category.
7. `orders`: orders placed by customers.
8. `order_items`: product lines within an order.
9. `payments`: payment attempts for an order.
10. `refunds`: refunds against a captured payment.

## Semantic identities

SQLite stores every ID and enum code as `INTEGER`, but the reporting ontology
assigns distinct meanings:

```text
RegionId, OrganizationId, EmployeeId, CustomerId,
CategoryId, ProductId, OrderId, OrderItemId, PaymentId, RefundId

OrganizationKind, EmployeeKind, OrderStatus, PaymentStatus
```

Equal storage representation does not authorize comparison or joins. For
example, `organizations.id = employees.org_id` is meaningful because both
denote `OrganizationId`; `organizations.kind = employees.kind` is not, even
though both are integers.

## Relationships

```text
Region(parent) 1 -> N Region(child)
Region 1 -> N Organization
Organization 1 -> N Employee
Organization 1 -> N Customer
Customer 1 -> N Order
Order 1 -> N OrderItem
Category(parent) 1 -> N Category(child)
Category 1 -> N Product
Product 1 -> N OrderItem
Order 1 -> N Payment
Payment 1 -> N Refund
```

## Measures and grain

### `net_revenue`

```text
grain: Order
value: Money(CNY)
definition: captured payments - successful refunds
dimensions: order_month, customer_region, organization_kind
```

Payment and refund facts are pre-aggregated per order before joining. This
prevents payment/refund fan-out. `product_category` is intentionally illegal:
allocating order-level refunds to lines requires a business policy that this
ontology does not define.

### `units_sold`

```text
grain: OrderItem
value: Quantity
definition: sum(order_items.quantity) for non-cancelled orders
dimensions: order_month, customer_region, product_category
```

`organization_kind` is omitted to keep the first compiler surface small,
although a future ontology could authorize it.

The two measures cannot currently appear in one report because the experiment
does not define a verified Order-to-OrderItem grain alignment policy.

## Dimensions and drill hierarchy

```text
time:      order_month
geography: customer_region -> (future child-region drill)
customer:  organization_kind
product:   product_category -> parent category
```

Only the first-level dimensions are lowered in the initial experiment. The
hierarchy records why drill is an ontology operation rather than merely adding
another `GROUP BY`.

## Initial report intents

Accepted:

```text
net_revenue by order_month and customer_region
units_sold by order_month and product_category
```

Rejected in one compilation:

```text
unknown measure "revenue"
net_revenue grouped by product_category
render field not selected (reserved for the next experiment)
```

The first implementation lowers only to SQL. Rendering, authorization,
catalog injection, dynamic relation search, and diagnostic accumulation are
explicitly outside this slice.

