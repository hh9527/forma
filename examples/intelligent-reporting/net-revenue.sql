WITH payment_by_order AS (
  SELECT order_id, SUM(amount_cents) AS captured_cents
  FROM payments WHERE status = 2 GROUP BY order_id
), refund_by_order AS (
  SELECT p.order_id, SUM(r.amount_cents) AS refunded_cents
  FROM refunds r JOIN payments p ON p.id = r.payment_id
  WHERE r.status = 2 GROUP BY p.order_id
)
SELECT substr(o.ordered_at, 1, 7) AS order_month,
       r.name AS customer_region,
       SUM(COALESCE(p.captured_cents, 0) - COALESCE(f.refunded_cents, 0)) AS net_revenue_cents
FROM orders o
JOIN customers c ON c.id = o.customer_id
JOIN organizations org ON org.id = c.org_id
JOIN regions r ON r.id = org.region_id
LEFT JOIN payment_by_order p ON p.order_id = o.id
LEFT JOIN refund_by_order f ON f.order_id = o.id
WHERE o.status = 2
GROUP BY order_month, customer_region
ORDER BY order_month, customer_region;

