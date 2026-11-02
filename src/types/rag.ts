/** RAG query request/response shapes; mirror `RagQueryRequest` and `RagResponse` in Rust. */

/** How a symbol lookup relates results to the symbol (mirrors Rust `SymbolRelation`). */
export type SymbolRelation = "definition" | "references" | "callees" | "members";

export interface RagQueryPayload {
  collection: string;
  /** Free-text query. Optional for symbol lookups, where it ranks results. */
  query: string;
  limit?: number;
  show_documents?: boolean;
  where?: Record<string, unknown>;
  /** Function or type name to look up instead of a text search (codebase only). */
  symbol?: string;
  relation?: SymbolRelation;
  /** Also return definitions of functions called by the hits, in `related`. */
  expand?: boolean;
}

export interface RagResult {
  id: string;
  /** Empty when the query was sent with `show_documents: false`. */
  document: string;
  metadata: Record<string, unknown>;
  score: number;
}

export interface RagResponse {
  results: RagResult[];
  /** Definitions of functions called by `results`; present only for `expand` requests. */
  related?: RagResult[];
  /** Hints about the request, e.g. how to qualify a symbol defined in several files. */
  notes?: string[];
  total_count: number;
  user_query: string;
}
