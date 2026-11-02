/**
 * The single Tauri IPC boundary for the frontend. Every backend command is
 * wrapped here with typed arguments and a typed result so components never
 * call `invoke` directly.
 */
import { invoke } from "@tauri-apps/api/core";
import type { RagQueryPayload, RagResponse } from "@/types/rag";
import type { EmbeddingOptions, UploadResponse, UserFiles } from "@/types/embed";
import type { ConnectionInfo, ToolPayload } from "@/types/mcp";
import type {
  ActiveJobsResponse,
  BackendStatus,
  CrawlResponse,
  DeleteResponse,
  EmbedWebsiteResponse,
  MessageResponse,
  MetadataValuesResponse,
  ServerResponse,
  ServersResponse,
  ToolResponse,
  WebsitesResponse,
} from "@/types/api";

export const getBackendStatus = () => invoke<BackendStatus>("get_backend_status");

export const ragQuery = (payload: RagQueryPayload) => invoke<RagResponse>("rag_query", { payload });

export const getMetadataValues = (collectionName: string, key: string) =>
  invoke<MetadataValuesResponse>("get_metadata_values", { collectionName, key });

export const uploadRepoZip = (paths: string[], embeddingOptions: EmbeddingOptions) =>
  invoke<UploadResponse>("upload_repo_zip", { paths, embeddingOptions });

export const uploadDocuments = (paths: string[], embeddingOptions: EmbeddingOptions) =>
  invoke<UploadResponse>("upload_documents", { paths, embeddingOptions });

export const uploadCodeFiles = (paths: string[], embeddingOptions: EmbeddingOptions) =>
  invoke<UploadResponse>("upload_code_files", { paths, embeddingOptions });

export const getActiveJobs = () => invoke<ActiveJobsResponse>("get_active_jobs");

/** Cancel a queued or running job and delete everything it produced. */
export const cancelJob = (jobId: string) => invoke<void>("cancel_job", { jobId });

export const getFiles = () => invoke<UserFiles>("get_files");

export const deleteRepo = (repoName: string) => invoke<DeleteResponse>("delete_repo", { repoName });

/** Delete one uploaded document by its `storage_object_id`. */
export const deleteDocument = (storageObjectId: string) =>
  invoke<DeleteResponse>("delete_document", { storageObjectId });

export const deleteGroup = (groupName: string) => invoke<DeleteResponse>("delete_group", { groupName });

export const clearUserCollection = (collectionName: string) =>
  invoke<DeleteResponse>("clear_user_collection", { collectionName });

export const getWebsites = () => invoke<WebsitesResponse>("get_websites");

export const deleteWebsite = (url: string) => invoke<DeleteResponse>("delete_website", { url });

export const deleteWebsiteGroup = (groupName: string) =>
  invoke<DeleteResponse>("delete_website_group", { groupName });

export const clearWebsites = () => invoke<DeleteResponse>("clear_websites");

export const getMcpServers = () => invoke<ServersResponse>("get_mcp_servers");

export const createMcpServer = (name: string, description?: string) =>
  invoke<ServerResponse>("create_mcp_server", { name, description });

export const getMcpServer = (serverId: string) => invoke<ServerResponse>("get_mcp_server", { serverId });

export const deleteMcpServer = (serverId: string) =>
  invoke<MessageResponse>("delete_mcp_server", { serverId });

export const createMcpTool = (serverId: string, toolData: ToolPayload) =>
  invoke<ToolResponse>("create_mcp_tool", { serverId, toolData });

export const updateMcpTool = (serverId: string, toolId: string, toolData: ToolPayload) =>
  invoke<ToolResponse>("update_mcp_tool", { serverId, toolId, toolData });

export const deleteMcpTool = (serverId: string, toolId: string) =>
  invoke<MessageResponse>("delete_mcp_tool", { serverId, toolId });

export const toggleMcpTool = (serverId: string, toolId: string, active: boolean) =>
  invoke<ToolResponse>("toggle_mcp_tool", { serverId, toolId, active });

export const getMcpConnectionInfo = (serverId: string) =>
  invoke<ConnectionInfo>("get_mcp_connection_info", { serverId });

export const crawlWebsite = (url: string, depth: number, sameDomainOnly: boolean, renderJavascript = false) =>
  invoke<CrawlResponse>("crawl_website", { url, depth, sameDomainOnly, renderJavascript });

export const cancelWebsiteCrawl = () => invoke<void>("cancel_website_crawl");

export const clearWebsiteCache = () => invoke<void>("clear_website_cache");

export const embedWebsite = (urls: string[], group: string, renderJavascript = false) =>
  invoke<EmbedWebsiteResponse>("embed_website", { urls, group, renderJavascript });
