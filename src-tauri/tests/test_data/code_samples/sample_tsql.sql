-- T-SQL (MS SQL Server) sample for dialect coverage testing

CREATE TABLE dbo.Employees (
    EmployeeID INT IDENTITY(1,1) PRIMARY KEY,
    FirstName NVARCHAR(50) NOT NULL,
    LastName NVARCHAR(50) NOT NULL,
    Email NVARCHAR(255),
    HireDate DATETIME2 DEFAULT GETDATE(),
    Salary MONEY,
    DepartmentID INT,
    CONSTRAINT FK_Employee_Dept FOREIGN KEY (DepartmentID) REFERENCES dbo.Departments(DepartmentID)
);

CREATE VIEW dbo.ActiveEmployees AS
SELECT EmployeeID, FirstName, LastName, Email
FROM dbo.Employees
WHERE HireDate >= '2020-01-01';

CREATE INDEX IX_Employee_Email ON dbo.Employees(Email);

ALTER TABLE dbo.Employees ADD PhoneNumber NVARCHAR(20);

INSERT INTO dbo.Employees (FirstName, LastName, Email, Salary)
VALUES (N'Jane', N'Smith', N'jane.smith@corp.com', 75000.00);

SELECT
    e.FirstName,
    e.LastName,
    e.Salary,
    d.DepartmentName
FROM dbo.Employees e
JOIN dbo.Departments d ON e.DepartmentID = d.DepartmentID
WHERE e.Salary > 50000
ORDER BY e.LastName;

UPDATE dbo.Employees
SET Salary = Salary * 1.05
WHERE DepartmentID = 3;

DELETE FROM dbo.Employees WHERE EmployeeID = 999;

DROP TABLE IF EXISTS dbo.TempImport;
