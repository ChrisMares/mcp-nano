-- Oracle SQL/PL-SQL dialect sample for SQL chunker stress testing
-- Covers: PACKAGE/PACKAGE BODY, object types, PL/SQL IS/AS BEGIN..END,
-- GRANT/REVOKE, TABLESPACE, SYNONYM, sequences, / terminators, double-quoted identifiers

-- ============================================================
-- CREATE TABLESPACE
-- ============================================================

CREATE TABLESPACE app_data
    DATAFILE '/u01/oradata/mydb/app_data01.dbf'
    SIZE 500M
    AUTOEXTEND ON NEXT 100M MAXSIZE 2G
    EXTENT MANAGEMENT LOCAL
    SEGMENT SPACE MANAGEMENT AUTO;

-- ============================================================
-- CREATE SEQUENCE (Oracle-style with NOCACHE, CYCLE)
-- ============================================================

CREATE SEQUENCE hr.employee_seq
    START WITH 1000
    INCREMENT BY 1
    MINVALUE 1000
    MAXVALUE 999999
    NOCYCLE
    NOCACHE
    ORDER;
/

CREATE SEQUENCE hr.audit_seq
    START WITH 1
    INCREMENT BY 1
    CYCLE
    CACHE 20;
/

-- ============================================================
-- CREATE TABLE with Oracle-specific features
-- ============================================================

CREATE TABLE hr."Employees" (
    employee_id    NUMBER(10)    DEFAULT hr.employee_seq.NEXTVAL PRIMARY KEY,
    first_name     VARCHAR2(50)  NOT NULL,
    last_name      VARCHAR2(50)  NOT NULL,
    email          VARCHAR2(100) UNIQUE,
    hire_date      DATE          DEFAULT SYSDATE,
    salary         NUMBER(10,2)  NOT NULL,
    department_id  NUMBER(5),
    manager_id     NUMBER(10),
    status         VARCHAR2(20)  DEFAULT 'ACTIVE'
        CONSTRAINT chk_emp_status CHECK (status IN ('ACTIVE','INACTIVE','TERMINATED')),
    CONSTRAINT fk_emp_dept FOREIGN KEY (department_id)
        REFERENCES hr.departments(department_id),
    CONSTRAINT fk_emp_mgr FOREIGN KEY (manager_id)
        REFERENCES hr."Employees"(employee_id)
) TABLESPACE app_data;
/

-- ============================================================
-- CREATE OR REPLACE TYPE (object type)
-- ============================================================

CREATE OR REPLACE TYPE hr.address_obj AS OBJECT (
    street    VARCHAR2(200),
    city      VARCHAR2(100),
    state     VARCHAR2(50),
    zip_code  VARCHAR2(20),
    MEMBER FUNCTION formatted_address RETURN VARCHAR2
);
/

CREATE OR REPLACE TYPE BODY hr.address_obj AS
    MEMBER FUNCTION formatted_address RETURN VARCHAR2 IS
    BEGIN
        RETURN street || CHR(10)
            || city || ', ' || state || ' ' || zip_code;
    END formatted_address;
END;
/

-- ============================================================
-- CREATE SYNONYM
-- ============================================================

CREATE OR REPLACE PUBLIC SYNONYM employees FOR hr."Employees";
CREATE OR REPLACE SYNONYM emp_seq FOR hr.employee_seq;

-- ============================================================
-- CREATE OR REPLACE PACKAGE (specification)
-- ============================================================

CREATE OR REPLACE PACKAGE hr.emp_mgmt AS
    -- Public constants
    c_max_salary     CONSTANT NUMBER := 500000;
    c_min_salary     CONSTANT NUMBER := 30000;

    -- Exceptions
    e_salary_exceeded EXCEPTION;
    e_employee_not_found EXCEPTION;
    PRAGMA EXCEPTION_INIT(e_salary_exceeded, -20001);
    PRAGMA EXCEPTION_INIT(e_employee_not_found, -20002);

    -- Public procedures and functions
    FUNCTION hire_employee(
        p_first_name   IN VARCHAR2,
        p_last_name    IN VARCHAR2,
        p_email        IN VARCHAR2,
        p_salary       IN NUMBER,
        p_dept_id      IN NUMBER
    ) RETURN NUMBER;

    PROCEDURE terminate_employee(p_emp_id IN NUMBER);

    PROCEDURE adjust_salary(
        p_emp_id       IN NUMBER,
        p_new_salary   IN NUMBER
    );

    FUNCTION get_department_headcount(p_dept_id IN NUMBER) RETURN NUMBER;
END emp_mgmt;
/

-- ============================================================
-- CREATE OR REPLACE PACKAGE BODY
-- ============================================================

CREATE OR REPLACE PACKAGE BODY hr.emp_mgmt AS
    -- Private package variable
    g_total_employees NUMBER := 0;

    FUNCTION hire_employee(
        p_first_name   IN VARCHAR2,
        p_last_name    IN VARCHAR2,
        p_email        IN VARCHAR2,
        p_salary       IN NUMBER,
        p_dept_id      IN NUMBER
    ) RETURN NUMBER IS
        v_emp_id NUMBER;
    BEGIN
        IF p_salary > c_max_salary THEN
            RAISE e_salary_exceeded;
        END IF;

        INSERT INTO hr."Employees" (first_name, last_name, email, salary, department_id)
        VALUES (p_first_name, p_last_name, p_email, p_salary, p_dept_id)
        RETURNING employee_id INTO v_emp_id;

        g_total_employees := g_total_employees + 1;
        COMMIT;
        RETURN v_emp_id;
    EXCEPTION
        WHEN DUP_VAL_ON_INDEX THEN
            RAISE_APPLICATION_ERROR(-20003, 'Employee email already exists: ' || p_email);
    END hire_employee;

    PROCEDURE terminate_employee(p_emp_id IN NUMBER) IS
        v_count NUMBER;
    BEGIN
        SELECT COUNT(*) INTO v_count
        FROM hr."Employees"
        WHERE employee_id = p_emp_id AND status = 'ACTIVE';

        IF v_count = 0 THEN
            RAISE e_employee_not_found;
        END IF;

        UPDATE hr."Employees"
        SET status = 'TERMINATED'
        WHERE employee_id = p_emp_id;

        g_total_employees := g_total_employees - 1;
        COMMIT;
    END terminate_employee;

    PROCEDURE adjust_salary(
        p_emp_id       IN NUMBER,
        p_new_salary   IN NUMBER
    ) IS
        v_old_salary NUMBER;
    BEGIN
        SELECT salary INTO v_old_salary
        FROM hr."Employees"
        WHERE employee_id = p_emp_id
        FOR UPDATE;

        IF p_new_salary > c_max_salary OR p_new_salary < c_min_salary THEN
            RAISE_APPLICATION_ERROR(-20004,
                'Salary ' || p_new_salary || ' outside allowed range ['
                || c_min_salary || ', ' || c_max_salary || ']');
        END IF;

        UPDATE hr."Employees"
        SET salary = p_new_salary
        WHERE employee_id = p_emp_id;

        INSERT INTO hr.salary_audit (employee_id, old_salary, new_salary, changed_by, changed_at)
        VALUES (p_emp_id, v_old_salary, p_new_salary, USER, SYSDATE);

        COMMIT;
    EXCEPTION
        WHEN NO_DATA_FOUND THEN
            RAISE e_employee_not_found;
    END adjust_salary;

    FUNCTION get_department_headcount(p_dept_id IN NUMBER) RETURN NUMBER IS
        v_count NUMBER;
    BEGIN
        SELECT COUNT(*) INTO v_count
        FROM hr."Employees"
        WHERE department_id = p_dept_id AND status = 'ACTIVE';
        RETURN v_count;
    END get_department_headcount;

END emp_mgmt;
/

-- ============================================================
-- Standalone PL/SQL procedure with IS/AS BEGIN...END
-- ============================================================

CREATE OR REPLACE PROCEDURE hr.generate_monthly_report(
    p_year   IN NUMBER,
    p_month  IN NUMBER
) AS
    v_start_date DATE;
    v_end_date   DATE;
    v_total      NUMBER(12,2);
    CURSOR c_departments IS
        SELECT department_id, department_name FROM hr.departments;
BEGIN
    v_start_date := TO_DATE(p_year || '-' || LPAD(p_month, 2, '0') || '-01', 'YYYY-MM-DD');
    v_end_date   := ADD_MONTHS(v_start_date, 1) - 1;

    FOR dept_rec IN c_departments LOOP
        SELECT NVL(SUM(salary), 0) INTO v_total
        FROM hr."Employees"
        WHERE department_id = dept_rec.department_id
          AND hire_date BETWEEN v_start_date AND v_end_date;

        DBMS_OUTPUT.PUT_LINE(
            dept_rec.department_name || ': ' || TO_CHAR(v_total, '$999,999.99')
        );
    END LOOP;
END generate_monthly_report;
/

-- ============================================================
-- GRANT / REVOKE
-- ============================================================

GRANT EXECUTE ON hr.emp_mgmt TO app_user;
GRANT SELECT ON hr."Employees" TO app_readonly;
GRANT SELECT, INSERT, UPDATE ON hr."Employees" TO app_user;
REVOKE DELETE ON hr."Employees" FROM app_user;

-- ============================================================
-- DML with Oracle-specific syntax
-- ============================================================

INSERT INTO hr."Employees" (first_name, last_name, email, salary, department_id)
VALUES ('Alice', 'Johnson', 'alice.j@company.com', 85000, 10);

SELECT e.first_name, e.last_name, d.department_name,
       RANK() OVER (PARTITION BY e.department_id ORDER BY e.salary DESC) AS salary_rank
FROM hr."Employees" e
JOIN hr.departments d ON e.department_id = d.department_id
WHERE e.status = 'ACTIVE'
ORDER BY d.department_name, salary_rank;
