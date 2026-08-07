PRAGMA foreign_keys = ON;

CREATE TABLE regions (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES regions(id), name TEXT NOT NULL);
CREATE TABLE organizations (id INTEGER PRIMARY KEY, region_id INTEGER NOT NULL REFERENCES regions(id), kind INTEGER NOT NULL, name TEXT NOT NULL);
CREATE TABLE employees (id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES organizations(id), kind INTEGER NOT NULL, name TEXT NOT NULL);
CREATE TABLE customers (id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL REFERENCES organizations(id), name TEXT NOT NULL);
CREATE TABLE categories (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES categories(id), name TEXT NOT NULL);
CREATE TABLE products (id INTEGER PRIMARY KEY, category_id INTEGER NOT NULL REFERENCES categories(id), sku TEXT NOT NULL UNIQUE, name TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), ordered_at TEXT NOT NULL, status INTEGER NOT NULL);
CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders(id), product_id INTEGER NOT NULL REFERENCES products(id), quantity INTEGER NOT NULL, unit_price_cents INTEGER NOT NULL);
CREATE TABLE payments (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders(id), status INTEGER NOT NULL, amount_cents INTEGER NOT NULL);
CREATE TABLE refunds (id INTEGER PRIMARY KEY, payment_id INTEGER NOT NULL REFERENCES payments(id), status INTEGER NOT NULL, amount_cents INTEGER NOT NULL);

INSERT INTO regions VALUES (1, NULL, 'East'), (2, NULL, 'West');
INSERT INTO organizations VALUES (10, 1, 1, 'Acme'), (20, 2, 2, 'Globex');
INSERT INTO employees VALUES (100, 10, 1, 'Alice'), (200, 20, 2, 'Bob');
INSERT INTO customers VALUES (1000, 10, 'Acme Buying'), (2000, 20, 'Globex Buying');
INSERT INTO categories VALUES (10000, NULL, 'Hardware'), (11000, 10000, 'Keyboards'), (12000, 10000, 'Mice');
INSERT INTO products VALUES (100000, 11000, 'KB-1', 'Keyboard'), (200000, 12000, 'MS-1', 'Mouse');
INSERT INTO orders VALUES (1, 1000, '2026-01-10', 2), (2, 2000, '2026-02-12', 2), (3, 1000, '2026-02-20', 3);
INSERT INTO order_items VALUES (1, 1, 100000, 2, 5000), (2, 1, 200000, 1, 3000), (3, 2, 200000, 4, 3000), (4, 3, 100000, 1, 5000);
INSERT INTO payments VALUES (1, 1, 2, 13000), (2, 2, 2, 12000), (3, 3, 1, 5000);
INSERT INTO refunds VALUES (1, 1, 2, 3000), (2, 2, 1, 1000);

