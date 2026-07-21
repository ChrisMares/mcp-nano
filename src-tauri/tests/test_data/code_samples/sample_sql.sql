-- Sample SQL file for tree-sitter chunker tests
-- Covers DDL, DML, views, indexes, functions, triggers across dialects

-- ============================================================
-- DDL: CREATE TABLE with constraints, foreign keys, defaults
-- ============================================================

CREATE TABLE dbo.customers (
    id INT PRIMARY KEY,
    first_name VARCHAR(100) NOT NULL,
    last_name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    is_active BOOLEAN DEFAULT true,
    dept_id INT,
    CONSTRAINT fk_department FOREIGN KEY (dept_id) REFERENCES departments(id)
);

CREATE TABLE orders (
    id SERIAL PRIMARY KEY,
    customer_id INT NOT NULL REFERENCES customers(id),
    total_amount DECIMAL(10, 2) NOT NULL,
    status VARCHAR(20) DEFAULT 'pending',
    ordered_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    shipped_at TIMESTAMP,
    CONSTRAINT chk_amount CHECK (total_amount >= 0)
);

CREATE TABLE order_items (
    id SERIAL PRIMARY KEY,
    order_id INT NOT NULL,
    product_id INT NOT NULL,
    quantity INT NOT NULL DEFAULT 1,
    unit_price DECIMAL(10, 2) NOT NULL,
    CONSTRAINT fk_order FOREIGN KEY (order_id) REFERENCES orders(id),
    CONSTRAINT fk_product FOREIGN KEY (product_id) REFERENCES products(id)
);

-- ============================================================
-- DDL: ALTER TABLE
-- ============================================================

ALTER TABLE customers ADD COLUMN phone VARCHAR(20);

-- ============================================================
-- DDL: CREATE INDEX
-- ============================================================

CREATE INDEX idx_customer_email ON customers(email);

CREATE UNIQUE INDEX idx_order_customer ON orders(customer_id, ordered_at);

-- ============================================================
-- DDL: CREATE VIEW
-- ============================================================

CREATE VIEW active_customers AS
SELECT c.id, c.first_name, c.last_name, c.email
FROM customers c
WHERE c.is_active = true;

CREATE VIEW order_summary AS
SELECT
    o.id AS order_id,
    c.first_name || ' ' || c.last_name AS customer_name,
    o.total_amount,
    o.status,
    o.ordered_at
FROM orders o
JOIN customers c ON o.customer_id = c.id;

-- ============================================================
-- DDL: CREATE FUNCTION (PostgreSQL style)
-- ============================================================

CREATE FUNCTION get_customer_total(p_customer_id INT)
RETURNS DECIMAL(10, 2) AS $$
BEGIN
    RETURN (
        SELECT COALESCE(SUM(total_amount), 0)
        FROM orders
        WHERE customer_id = p_customer_id
    );
END;
$$ LANGUAGE plpgsql;

-- ============================================================
-- DDL: CREATE TRIGGER
-- ============================================================

CREATE TRIGGER trg_customer_updated
AFTER UPDATE ON customers
FOR EACH ROW
EXECUTE FUNCTION log_customer_change();

-- ============================================================
-- DML: INSERT
-- ============================================================

INSERT INTO customers (id, first_name, last_name, email)
VALUES (1, 'John', 'Doe', 'john.doe@example.com');

-- ============================================================
-- DML: SELECT with JOIN, WHERE, GROUP BY, ORDER BY
-- ============================================================

SELECT
    c.first_name,
    c.last_name,
    COUNT(o.id) AS order_count,
    SUM(o.total_amount) AS total_spent
FROM customers c
LEFT JOIN orders o ON c.id = o.customer_id
WHERE c.is_active = true
GROUP BY c.id, c.first_name, c.last_name
HAVING SUM(o.total_amount) > 100
ORDER BY total_spent DESC;

-- ============================================================
-- DML: UPDATE
-- ============================================================

UPDATE orders
SET status = 'shipped', shipped_at = CURRENT_TIMESTAMP
WHERE id = 42;

-- ============================================================
-- DML: DELETE
-- ============================================================

DELETE FROM order_items WHERE order_id IN (
    SELECT id FROM orders WHERE status = 'cancelled'
);

-- ============================================================
-- DDL: DROP TABLE
-- ============================================================

DROP TABLE IF EXISTS temp_import_data;

-- ============================================================
-- DDL: CREATE SEQUENCE
-- ============================================================

CREATE SEQUENCE order_number_seq
    START WITH 1000
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 10;
