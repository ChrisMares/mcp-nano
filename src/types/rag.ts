export interface RagQueryPayload {
  collection: string;
  query: string;
  limit?: number;
  show_documents?: boolean;
  where?: Record<string, unknown>;
}

export interface RagResult {
  id: string;
  document?: string;
  metadata: Record<string, unknown>;
  score: number;
}

export interface RagResponse {
  results: RagResult[];
  total_count: number;
  user_query: string;
}
