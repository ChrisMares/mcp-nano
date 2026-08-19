import { invoke } from "@tauri-apps/api/core";
import type { RagQueryPayload, RagResponse } from "@/types/rag";
import type {
  EmbedJob,
  EmbeddingOptions,
  JobStatus,
  UploadResponse,
  UserFiles,
  WebsiteItem,
} from "@/types/embed";
import type {
  ConnectionInfo,
  McpServer,
  ToolDefinition,
  ToolPayload,
} from "@/types/mcp";

export interface MetadataValuesResponse {
  values: string[];
}

export interface ActiveJobsResponse {
  jobs: EmbedJob[];
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

export interface ModelStatus {
  role: string;
  model: string;
  device: string;
  cpu_reason: string | null;
}

export async function getBackendStatus(): Promise<BackendStatus> {
  return invoke("get_backend_status");
}

export async function ragQuery(payload: RagQueryPayload): Promise<RagResponse> {
  return invoke("rag_query", { payload });
}

export async function getMetadataValues(
  collectionName: string,
  key: string
): Promise<MetadataValuesResponse> {
  return invoke("get_metadata_values", { collectionName, key });
}

export async function uploadRepoZip(
  paths: string[],
  embeddingOptions: EmbeddingOptions
): Promise<UploadResponse> {
  return invoke("upload_repo_zip", {
    paths,
    embeddingOptions,
  });
}

export async function uploadDocuments(
  paths: string[],
  embeddingOptions: EmbeddingOptions
): Promise<UploadResponse> {
  return invoke("upload_documents", {
    paths,
    embeddingOptions,
  });
}

export async function uploadCodeFiles(
  paths: string[],
  embeddingOptions: EmbeddingOptions
): Promise<UploadResponse> {
  return invoke("upload_code_files", {
    paths,
    embeddingOptions,
  });
}

export async function getActiveJobs(): Promise<ActiveJobsResponse> {
  return invoke("get_active_jobs");
}

export async function getJobStatus(jobId: string): Promise<JobStatus> {
  return invoke("get_job_status", { jobId });
}

export async function getFiles(): Promise<UserFiles> {
  return invoke("get_files");
}

export async function deleteRepo(repoName: string): Promise<DeleteResponse> {
  return invoke("delete_repo", { repoName });
}

export async function deleteDocument(filename: string): Promise<DeleteResponse> {
  return invoke("delete_document", { filename });
}

export async function deleteGroup(groupName: string): Promise<DeleteResponse> {
  return invoke("delete_group", { groupName });
}

export async function clearUserCollection(
  collectionName: string
): Promise<DeleteResponse> {
  return invoke("clear_user_collection", { collectionName });
}

export async function getWebsites(): Promise<WebsitesResponse> {
  return invoke("get_websites");
}

export async function deleteWebsite(url: string): Promise<DeleteResponse> {
  return invoke("delete_website", { url });
}

export async function deleteWebsiteGroup(
  groupName: string
): Promise<DeleteResponse> {
  return invoke("delete_website_group", { groupName });
}

export async function clearWebsites(): Promise<DeleteResponse> {
  return invoke("clear_websites");
}

export async function getMcpServers(): Promise<ServersResponse> {
  return invoke("get_mcp_servers");
}

export async function createMcpServer(
  name: string,
  description?: string
): Promise<ServerResponse> {
  return invoke("create_mcp_server", { name, description });
}

export async function getMcpServer(serverId: string): Promise<ServerResponse> {
  return invoke("get_mcp_server", { serverId });
}

export async function deleteMcpServer(
  serverId: string
): Promise<MessageResponse> {
  return invoke("delete_mcp_server", { serverId });
}

export async function createMcpTool(
  serverId: string,
  toolData: ToolPayload
): Promise<ToolResponse> {
  return invoke("create_mcp_tool", { serverId, toolData });
}

export async function updateMcpTool(
  serverId: string,
  toolId: string,
  toolData: ToolPayload
): Promise<ToolResponse> {
  return invoke("update_mcp_tool", { serverId, toolId, toolData });
}

export async function deleteMcpTool(
  serverId: string,
  toolId: string
): Promise<MessageResponse> {
  return invoke("delete_mcp_tool", { serverId, toolId });
}

export async function toggleMcpTool(
  serverId: string,
  toolId: string,
  active: boolean
): Promise<ToolResponse> {
  return invoke("toggle_mcp_tool", { serverId, toolId, active });
}

export async function getMcpConnectionInfo(
  serverId: string
): Promise<ConnectionInfo> {
  return invoke("get_mcp_connection_info", { serverId });
}

export async function crawlWebsite(
  url: string,
  depth: number,
  sameDomainOnly: boolean,
  renderJavascript = false,
): Promise<CrawlResponse> {
  return invoke("crawl_website", { url, depth, sameDomainOnly, renderJavascript });
}

export async function cancelWebsiteCrawl(): Promise<void> {
  return invoke("cancel_website_crawl");
}

export async function embedWebsite(
  urls: string[],
  group: string,
  renderJavascript = false,
): Promise<EmbedWebsiteResponse> {
  return invoke("embed_website", { urls, group, renderJavascript });
}
