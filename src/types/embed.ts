export interface EmbedJob {
  job_id: string;
  status: string;
  progress_percentage: number;
  file_name: string | null;
  created_at: string | null;
  queue_position?: number;
  total_in_queue?: number;
}

export interface JobStatus {
  id: number;
  job_id: string;
  status: string;
  created_at: string | null;
  updated_at: string | null;
  result: string | null;
  error_message: string | null;
  progress_percentage: number;
  file_name: string | null;
  storage_object_id: string | null;
  task_name: string | null;
  task_params: string | null;
}

export interface EmbeddingOptions {
  collection: string;
  repo_name?: string;
  group?: string;
  metadata: Record<string, unknown>;
}

export interface UploadJobEntry {
  filename: string;
  job_id: string;
  collection: string;
  status: string;
}

export interface UploadResponse {
  message: string;
  jobs: UploadJobEntry[];
  errors: string[];
}

interface RepoItem {
  repo_name: string;
  created_at: string | null;
}

export interface DocItem {
  filename: string;
  file_type: string | null;
  size_bytes: number | null;
  created_at: string | null;
  group: string;
}

export interface UserFiles {
  repos: RepoItem[];
  documents: DocItem[];
}

export interface WebsiteItem {
  url: string;
  group: string;
  chunk_count: number;
  embedded_at: string;
}

export interface UrlTreeNode {
  segment: string;
  fullPath: string;
  depth: number;
  children: UrlTreeNode[];
  url: string | null;
}

export interface FileMetadataResponse {
  repos: { repo_name: string }[];
  documents: { filename: string }[];
}
