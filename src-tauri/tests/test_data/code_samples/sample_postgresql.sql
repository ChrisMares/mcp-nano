-- PostgreSQL-specific dialect sample for SQL chunker stress testing
-- Covers: $$ dollar-quoting, PL/pgSQL, CREATE TYPE, EXTENSION, MATERIALIZED VIEW,
-- PARTITION BY, RLS policies, DO blocks, COMMENT ON, double-quoted identifiers

-- ============================================================
-- CREATE EXTENSION
-- ============================================================

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ============================================================
-- CREATE TYPE (enum and composite)
-- ============================================================

CREATE TYPE order_status AS ENUM ('pending', 'processing', 'shipped', 'delivered', 'cancelled');

CREATE TYPE address_type AS (
    street TEXT,
    city TEXT,
    state VARCHAR(2),
    zip VARCHAR(10)
);

-- ============================================================
-- CREATE TABLE with partitioning and double-quoted identifiers
-- ============================================================

CREATE TABLE "EventLog" (
    id UUID DEFAULT uuid_generate_v4(),
    "userId" BIGINT NOT NULL,
    event_type VARCHAR(50) NOT NULL,
    payload JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

CREATE TABLE "EventLog_2024_q1" PARTITION OF "EventLog"
    FOR VALUES FROM ('2024-01-01') TO ('2024-04-01');

CREATE TABLE "EventLog_2024_q2" PARTITION OF "EventLog"
    FOR VALUES FROM ('2024-04-01') TO ('2024-07-01');

CREATE INDEX idx_event_user ON "EventLog" ("userId", created_at DESC);

-- ============================================================
-- Row Level Security (RLS) policies
-- ============================================================

ALTER TABLE "EventLog" ENABLE ROW LEVEL SECURITY;

CREATE POLICY event_isolation_policy ON "EventLog"
    AS PERMISSIVE
    FOR ALL
    TO PUBLIC
    USING ("userId" = current_setting('app.current_user_id')::BIGINT);

CREATE POLICY event_insert_policy ON "EventLog"
    FOR INSERT
    WITH CHECK ("userId" = current_setting('app.current_user_id')::BIGINT);

-- ============================================================
-- CREATE OR REPLACE FUNCTION with $$ dollar-quoting and PL/pgSQL
-- ============================================================

CREATE OR REPLACE FUNCTION calculate_order_total(p_order_id BIGINT)
RETURNS NUMERIC(12, 2) AS $$
DECLARE
    v_total NUMERIC(12, 2);
    v_discount NUMERIC(5, 2);
    v_status order_status;
BEGIN
    SELECT status INTO v_status
    FROM orders WHERE id = p_order_id;

    IF v_status IS NULL THEN
        RAISE EXCEPTION 'Order % not found', p_order_id;
    END IF;

    SELECT COALESCE(SUM(quantity * unit_price), 0) INTO v_total
    FROM order_items
    WHERE order_id = p_order_id;

    -- Apply discount for large orders
    IF v_total > 500.00 THEN
        v_discount := v_total * 0.10;
    ELSIF v_total > 200.00 THEN
        v_discount := v_total * 0.05;
    ELSE
        v_discount := 0;
    END IF;

    RETURN v_total - v_discount;
END;
$$ LANGUAGE plpgsql STABLE;

-- ============================================================
-- Function with nested $$ dollar-quoting (uses $fn$ tag)
-- ============================================================

CREATE OR REPLACE FUNCTION create_audit_trigger(target_table TEXT)
RETURNS VOID AS $fn$
DECLARE
    trigger_name TEXT;
BEGIN
    trigger_name := target_table || '_audit_trig';

    EXECUTE format(
        $inner$
        CREATE OR REPLACE FUNCTION %I_audit_fn() RETURNS TRIGGER AS $body$
        BEGIN
            INSERT INTO audit_log (table_name, operation, old_data, new_data, changed_at)
            VALUES (TG_TABLE_NAME, TG_OP, row_to_json(OLD), row_to_json(NEW), NOW());
            RETURN NEW;
        END;
        $body$ LANGUAGE plpgsql;
        $inner$,
        target_table
    );

    EXECUTE format(
        'CREATE TRIGGER %I AFTER INSERT OR UPDATE OR DELETE ON %I
         FOR EACH ROW EXECUTE FUNCTION %I_audit_fn()',
        trigger_name, target_table, target_table
    );
END;
$fn$ LANGUAGE plpgsql;

-- ============================================================
-- MATERIALIZED VIEW with indexes
-- ============================================================

CREATE MATERIALIZED VIEW mv_monthly_revenue AS
SELECT
    date_trunc('month', o.ordered_at) AS month,
    c.id AS customer_id,
    c.first_name || ' ' || c.last_name AS customer_name,
    COUNT(o.id) AS order_count,
    SUM(o.total_amount) AS revenue
FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.status != 'cancelled'::order_status
GROUP BY date_trunc('month', o.ordered_at), c.id, c.first_name, c.last_name
WITH DATA;

CREATE UNIQUE INDEX idx_mv_revenue_month_cust ON mv_monthly_revenue (month, customer_id);

-- ============================================================
-- DO $$ anonymous code block
-- ============================================================

DO $$
DECLARE
    r RECORD;
    v_count INT := 0;
BEGIN
    FOR r IN
        SELECT table_name
        FROM information_schema.tables
        WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
    LOOP
        EXECUTE format('ANALYZE %I', r.table_name);
        v_count := v_count + 1;
    END LOOP;
    RAISE NOTICE 'Analyzed % tables', v_count;
END$$;

-- ============================================================
-- COMMENT ON statements
-- ============================================================

COMMENT ON TABLE "EventLog" IS 'Partitioned event log for user activity tracking';
COMMENT ON COLUMN "EventLog"."userId" IS 'References auth.users, enforced by RLS';
COMMENT ON FUNCTION calculate_order_total(BIGINT) IS 'Calculates total with tiered discounts';
COMMENT ON MATERIALIZED VIEW mv_monthly_revenue IS 'Pre-aggregated monthly revenue per customer';

-- ============================================================
-- CREATE SEQUENCE and DML
-- ============================================================

CREATE SEQUENCE IF NOT EXISTS invoice_number_seq
    START WITH 10000
    INCREMENT BY 1
    NO CYCLE;

INSERT INTO "EventLog" ("userId", event_type, payload)
VALUES (1, 'page_view', '{"path": "/dashboard", "duration_ms": 3200}'::JSONB);

REFRESH MATERIALIZED VIEW CONCURRENTLY mv_monthly_revenue;
