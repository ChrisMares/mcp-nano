-- Existing installs created name without UNIQUE; enforce uniqueness going forward.
CREATE UNIQUE INDEX IF NOT EXISTS ux_mcp_servers_name ON mcp_servers (name);
