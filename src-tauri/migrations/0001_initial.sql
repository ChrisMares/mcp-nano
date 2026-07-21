CREATE TABLE file_metadata (
    id INTEGER NOT NULL PRIMARY KEY,
    storage_object_id VARCHAR(36) NOT NULL UNIQUE,
    full_path TEXT NOT NULL,
    file_type VARCHAR(255),
    size_bytes INTEGER,
    repo_name VARCHAR(255),
    group_id VARCHAR(255),
    status VARCHAR(50) NOT NULL,
    error_message TEXT,
    created_at DATETIME,
    collection VARCHAR(50)
);

CREATE INDEX ix_file_metadata_status ON file_metadata (status);
CREATE INDEX idx_file_metadata_status_collection_created_at
    ON file_metadata (status, collection, created_at DESC);

CREATE TABLE job_status (
    id INTEGER NOT NULL PRIMARY KEY,
    job_id VARCHAR(36) NOT NULL UNIQUE,
    status VARCHAR(20) NOT NULL,
    created_at DATETIME,
    updated_at DATETIME,
    result TEXT,
    error_message TEXT,
    progress_percentage INTEGER,
    file_name VARCHAR(255),
    storage_object_id VARCHAR(255),
    task_name VARCHAR(100),
    task_params TEXT
);

CREATE TABLE mcp_servers (
    id VARCHAR(36) NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    active BOOLEAN NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

CREATE TABLE tool_definitions (
    id VARCHAR(36) NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    active BOOLEAN NOT NULL,
    mcp_server_id VARCHAR(36) NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY (mcp_server_id) REFERENCES mcp_servers (id)
);

CREATE INDEX ix_tool_definitions_mcp_server_id
    ON tool_definitions (mcp_server_id);

CREATE TABLE tool_code_search (
    id VARCHAR(36) NOT NULL PRIMARY KEY,
    tool_definition_id VARCHAR(36) NOT NULL,
    collection VARCHAR(100) NOT NULL,
    repo_names JSON NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY (tool_definition_id) REFERENCES tool_definitions (id)
);

CREATE INDEX ix_tool_code_search_tool_definition_id
    ON tool_code_search (tool_definition_id);

CREATE TABLE tool_document_search (
    id VARCHAR(36) NOT NULL PRIMARY KEY,
    tool_definition_id VARCHAR(36) NOT NULL,
    collection VARCHAR(100) NOT NULL,
    group_ids JSON NOT NULL,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    FOREIGN KEY (tool_definition_id) REFERENCES tool_definitions (id)
);

CREATE INDEX ix_tool_document_search_tool_definition_id
    ON tool_document_search (tool_definition_id);
