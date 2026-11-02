/**
 * Response shapes returned by Tauri commands that are not owned by a feature
 * type module. Each interface mirrors a serde struct in
 * `src-tauri/src/models/response/` (Rust `Option<T>` maps to `T | null`).
 */
import type { JobStatus, WebsiteItem } from "@/types/embed";
import type { McpServer, ToolDefinition } from "@/types/mcp";

export interface MetadataValuesResponse {
  values: string[];
}

export interface ActiveJobsResponse {
  jobs: JobStatus[];
  total_count: number;
}

export interface DeleteResponse {
  deleted: boolean;
}

export interface MessageResponse {
  message: string;
}

export interface ServersResponse {
  servers: McpServer[];
}

export interface ServerResponse {
  server: McpServer;
}

export interface ToolResponse {
  tool: ToolDefinition;
}

export interface WebsitesResponse {
  websites: WebsiteItem[];
}

export interface CrawlResponse {
  urls: string[];
  count: number;
}

export interface EmbedWebsiteResponse {
  job_id: string;
  url_count: number;
}

/** Mirrors `ModelStatusResponse`. */
export interface ModelStatus {
  role: string;
  model: string;
  device: string;
  cpu_reason: string | null;
}

/**
 * Mirrors `BackendStatusResponse`. Returned by `get_backend_status` and
 * carried in full by every `backend_status` event.
 */
export interface BackendStatus {
  qdrant_ready: boolean;
  qdrant_error: string | null;
  http_port: number | null;
  grpc_port: number | null;
  db_ready: boolean;
  embedders_ready: boolean;
  embedding_device: string | null;
  model_statuses: ModelStatus[];
  qdrant_storage_path: string | null;
  sqlite_path: string | null;
  logs_path: string | null;
  logs_size_bytes: number | null;
  worker_ready: boolean;
}
