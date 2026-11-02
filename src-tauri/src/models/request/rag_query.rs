//! Request body for RAG queries (Tauri `rag_query` and MCP tool calls).

use serde::Deserialize;

/// How a symbol lookup relates results to the requested symbol.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolRelation {
    /// Chunks that define the symbol.
    #[default]
    Definition,
    /// Chunks that call the symbol.
    References,
    /// Definitions of the functions the symbol calls.
    Callees,
    /// The type itself and the members (methods) defined inside it.
    Members,
}

#[derive(Debug, Default, Deserialize)]
pub struct RagQueryRequest {
    pub collection: String,
    /// Free-text query. Optional for symbol lookups, where it ranks results.
    #[serde(default)]
    pub query: String,
    pub limit: Option<i64>,
    pub show_documents: Option<bool>,
    #[serde(rename = "where", default)]
    pub where_clause: Option<serde_json::Value>,
    /// Function/type name for a symbol lookup instead of a text search.
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub relation: Option<SymbolRelation>,
    /// Also return definitions of functions called by the hits.
    #[serde(default)]
    pub expand: Option<bool>,
}
