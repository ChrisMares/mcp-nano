//! SQL chunker. Direct port of `chunk_code_sql.py`.
//!
//! Multi-phase pipeline:
//! 1. tree-sitter extracts what it can parse (Phase 1 of the Python code).
//! 2. Each chunk is regex re-split to recover DDL the tree-sitter fragment
//!    may have buried inside an ERROR node (Phase 2).
//! 3. The remainder text is regex-split too (Phase 3).
//! 4. ALTER TABLE chunks are merged per `object_name` (Phase 4).
//! 5. Junk chunks with unrecognized statement types are filtered (Phase 5).

use std::collections::HashSet;

use regex::Regex;
use tree_sitter::Node;

use super::helpers::{build_remainder_code, generate_chunk_id, node_text, byte_slice_text};
use crate::services::ingestion::types::{CodeChunk, CodeChunkKind, SqlFields};

// Tree-sitter statement types that get their own chunk (Phase 1).
const STATEMENT_NODE_TYPES: &[&str] = &[
    "create_table",
    "create_view",
    "create_index",
    "create_function",
    "create_procedure",
    "create_trigger",
    "create_type",
    "create_sequence",
    "create_schema",
    "create_database",
    "create_extension",
    "create_role",
    "create_policy",
    "create_materialized_view",
    "alter_table",
    "alter_view",
    "alter_index",
    "alter_schema",
    "alter_sequence",
    "alter_type",
    "alter_role",
    "alter_policy",
    "alter_database",
    "alter_materialized_view",
    "drop_table",
    "drop_view",
    "drop_index",
    "drop_function",
    "drop_procedure",
    "drop_type",
    "drop_extension",
    "drop_sequence",
    "drop_schema",
    "drop_database",
    "drop_role",
    "drop_materialized_view",
    "insert",
    "select",
    "update",
    "delete",
    "merge",
    "grant",
    "revoke",
    "transaction",
];

// Regex pattern that finds a position where a SQL DDL object begins.
//
// The Python original uses a zero-width lookahead (`(?=(?:CREATE ... |ALTER
// ... |...)`) so the regex consumes only `^|\n\s*` and the matched position
// aligns with the DDL keyword. The Rust `regex` crate doesn't support
// lookahead, so we restructure the pattern to actually consume the DDL
// keyword and use capture group 2 (`m.get(2)`) to recover the true DDL
// start byte. Capture group 1 is the leading whitespace; group 2 is the
// DDL prefix itself.
fn sql_object_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?im)(?:^|\n)(\s*)((?:CREATE\s+(?:OR\s+REPLACE\s+)?(?:PUBLIC\s+)?(?:MATERIALIZED\s+VIEW|MATERIALIZED VIEW|TABLE|VIEW|INDEX|FUNCTION|PROCEDURE|TRIGGER|TYPE\s+BODY|TYPE|SEQUENCE|SCHEMA|DATABASE|EXTENSION|ROLE|POLICY|TABLESPACE|SYNONYM|PACKAGE\s+BODY|PACKAGE)\b|CREATE\s+(?:VIRTUAL\s+)?TABLE\b|CREATE\s+PROCEDURE\S|ALTER\s+TABLE|DROP\s+(?:TABLE|VIEW|FUNCTION|PROCEDURE|INDEX|TYPE|SEQUENCE|EXTENSION|SCHEMA|TRIGGER)\b|GRANT\s|REVOKE\s))",
        )
        .expect("sql_object_re must compile")
    })
}

const DOLLAR_TAG_RE_RAW: &str = r"\$[A-Za-z_]*\$";

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    collect_references(&root, source, &mut deps, &mut seen);
    deps
}

fn collect_references<'a>(
    node: &Node<'a>,
    source: &[u8],
    deps: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    if node.kind() == "object_reference" {
        if node.parent().is_some() {
            if let Some(prev) = prev_named_sibling(node) {
                if prev.kind() == "keyword_references" {
                    if let Some(name) = extract_object_name(node, source) {
                        if seen.insert(name.clone()) {
                            deps.push(name);
                        }
                    }
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_references(&child, source, deps, seen);
    }
}

fn prev_named_sibling<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let parent = node.parent()?;
    let mut cursor = parent.walk();
    let mut prev: Option<Node<'a>> = None;
    for child in parent.children(&mut cursor) {
        if child == *node {
            return prev;
        }
        if child.is_named() {
            prev = Some(child);
        }
    }
    None
}

fn strip_brackets(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_object_name<'a>(obj_ref: &Node<'a>, source: &[u8]) -> Option<String> {
    let mut cursor = obj_ref.walk();
    let identifiers: Vec<Node<'a>> = obj_ref
        .children(&mut cursor)
        .filter(|c| c.kind() == "identifier")
        .collect();
    if identifiers.is_empty() {
        let raw = strip_brackets(&node_text(obj_ref, source).trim());
        return if raw.is_empty() { None } else { Some(raw) };
    }
    Some(
        identifiers
            .iter()
            .map(|n| strip_brackets(&node_text(n, source)))
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn extract_schema_and_name<'a>(obj_ref: &Node<'a>, source: &[u8]) -> (Option<String>, Option<String>) {
    let mut cursor = obj_ref.walk();
    let identifiers: Vec<Node<'a>> = obj_ref
        .children(&mut cursor)
        .filter(|c| c.kind() == "identifier")
        .collect();
    if identifiers.len() >= 2 {
        let schema = strip_brackets(&node_text(&identifiers[0], source));
        let name = strip_brackets(&node_text(identifiers.last().unwrap(), source));
        (Some(schema), Some(name))
    } else if identifiers.len() == 1 {
        (None, Some(strip_brackets(&node_text(&identifiers[0], source))))
    } else {
        let raw = strip_brackets(&node_text(obj_ref, source).trim());
        if raw.is_empty() {
            (None, None)
        } else {
            (None, Some(raw))
        }
    }
}

fn get_statement_label(node_type: &str) -> String {
    node_type.replace('_', " ").to_uppercase()
}

pub fn extract_chunks<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
) -> Vec<CodeChunk> {
    extract_chunks_with_db(node, source, file_path, deps, None)
}

pub fn extract_chunks_with_db<'a>(
    node: Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    database_name: Option<&str>,
) -> Vec<CodeChunk> {
    let mut raw_chunks: Vec<CodeChunk> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut error_node_ids: HashSet<String> = HashSet::new();
    extract_chunks_impl(
        &node,
        source,
        file_path,
        deps,
        &mut raw_chunks,
        &mut spans,
        &mut error_node_ids,
        database_name,
    );

    // Phase 2: regex re-split each chunk to recover DDL buried inside.
    let mut resplit: Vec<CodeChunk> = Vec::new();
    for chunk in raw_chunks {
        let is_error = error_node_ids.contains(&chunk.id);
        let sub = regex_split_sql_text(&chunk.code, file_path, deps, database_name);
        if !sub.is_empty() {
            resplit.extend(sub);
        } else if is_error {
            // Drop junk ERROR-node chunk.
        } else {
            resplit.push(chunk);
        }
    }

    // Phase 3: regex-split the remainder (tree-sitter didn't touch this).
    let remainder_code = build_remainder_code(source, &spans);
    if !remainder_code.trim().is_empty() {
        resplit.extend(regex_split_sql_text(&remainder_code, file_path, deps, database_name));
    }

    // Phase 4: merge ALTER TABLE chunks by object_name.
    let merged = merge_alter_table_chunks(resplit, file_path, deps, database_name);

    // Phase 5: filter junk chunks.
    filter_junk_chunks(merged)
}

fn extract_chunks_impl<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    chunks: &mut Vec<CodeChunk>,
    spans: &mut Vec<(usize, usize)>,
    error_node_ids: &mut HashSet<String>,
    database_name: Option<&str>,
) {
    if node.kind() == "statement" {
        // The first *named* child of a `statement` node is the actual DDL/DML
        // construct (create_table, alter_table, ...). Using `child(0)`
        // would also surface anonymous sibling nodes (whitespace, comments).
        let inner = node.named_child(0);
        if let Some(inner) = inner {
            let inner_kind = inner.kind();
            if STATEMENT_NODE_TYPES.contains(&inner_kind) {
                emit_chunk(node, &inner, source, file_path, deps, database_name, chunks);
                spans.push((node.start_byte(), node.end_byte()));
                extend_span_past_semicolons(node, spans);
                return;
            }
        }
    }

    // ERROR at root level (T-SQL procedures with @ params, Oracle PL/SQL blocks).
    if node.kind() == "ERROR" {
        if let Some(parent) = node.parent() {
            if parent.kind() == "program" {
                let code = node_text(&node, source);
                let stmt_type = infer_statement_type(&code);
                let (schema, obj_name) = infer_schema_and_object_name(&code);
                let id = generate_chunk_id(file_path, &obj_name.clone().unwrap_or("unknown".to_string()));
                chunks.push(CodeChunk {
                    id: id.clone(),
                    repo_name: String::new(),
                    file_name: std::path::Path::new(file_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(file_path)
                        .to_string(),
                    code,
                    r#type: "sql_statement".to_string(),
                    dependencies: deps.to_vec(),
                    created_at: crate::services::ingestion::types::now_iso(),
                    kind: CodeChunkKind::Sql(SqlFields {
                        statement_type: Some(stmt_type),
                        object_name: obj_name,
                        schema_name: schema,
                        database_name: database_name.map(|s| s.to_string()),
                    }),
                });
                error_node_ids.insert(id);
                spans.push((node.start_byte(), node.end_byte()));
                return;
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_chunks_impl(&child, source, file_path, deps, chunks, spans, error_node_ids, database_name);
    }
}

fn emit_chunk<'a>(
    stmt_node: &Node<'a>,
    inner_node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    database_name: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    // Include trailing ';' in the chunk text.
    let mut end_byte = stmt_node.end_byte();
    if let Some(next_sib) = stmt_node.next_sibling() {
        if next_sib.kind() == ";" {
            end_byte = next_sib.end_byte();
        }
    }
    let code = byte_slice_text(source, stmt_node.start_byte(), end_byte);

    let mut schema_name: Option<String> = None;
    let mut object_name: Option<String> = None;
    let mut cursor = inner_node.walk();
    if let Some(obj_ref) = inner_node
        .children(&mut cursor)
        .find(|c| c.kind() == "object_reference")
    {
        let (s, n) = extract_schema_and_name(&obj_ref, source);
        schema_name = s;
        object_name = n;
    }

    let id = generate_chunk_id(file_path, &object_name.clone().unwrap_or_else(|| inner_node.kind().to_string()));
    chunks.push(CodeChunk {
        id,
        repo_name: String::new(),
        file_name: std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file_path)
            .to_string(),
        code,
        r#type: "sql_statement".to_string(),
        dependencies: deps.to_vec(),
        created_at: crate::services::ingestion::types::now_iso(),
        kind: CodeChunkKind::Sql(SqlFields {
            statement_type: Some(get_statement_label(inner_node.kind())),
            object_name,
            schema_name,
            database_name: database_name.map(|s| s.to_string()),
        }),
    });
}

fn extend_span_past_semicolons<'a>(node: &Node<'a>, spans: &mut Vec<(usize, usize)>) {
    if let Some(next_sib) = node.next_sibling() {
        if next_sib.kind() == ";" {
            if let Some(last) = spans.last_mut() {
                last.1 = next_sib.end_byte();
            }
        }
    }
}

fn regex_split_sql_text(
    text: &str,
    file_path: &str,
    deps: &[String],
    database_name: Option<&str>,
) -> Vec<CodeChunk> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let dollar_spans = find_dollar_quoted_spans(trimmed);
    let re = sql_object_re();
    // `find_iter` isn't enough because the position of interest is the
    // *DDL keyword* (capture group 2), not the start of the overall match
    // (which sits one char back on the `^|\n` boundary, or earlier on the
    // `\s*` prefix). Iterate `captures_iter` and pull `m.get(2)`.
    let mut positions: Vec<usize> = Vec::new();
    for caps in re.captures_iter(trimmed) {
        if let Some(m) = caps.get(2) {
            let p = m.start();
            // If the leading match sat on a `\n`, we've consumed the `\n` and
            // the DDL start is actually after the `\s*` whitespace. The
            // capture group 2 is the start of the DDL keyword text.
            if !inside_spans(p, &dollar_spans) {
                positions.push(p);
            }
        }
    }
    if positions.is_empty() {
        return Vec::new();
    }
    positions = positions.into_iter().collect::<HashSet<_>>().into_iter().collect();
    positions.sort();
    let file_basename = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path)
        .to_string();

    let mut chunks = Vec::new();
    for (i, start) in positions.iter().enumerate() {
        let end = if i + 1 < positions.len() {
            positions[i + 1]
        } else {
            trimmed.len()
        };
        let segment = trimmed[*start..end].trim();
        if segment.is_empty() {
            continue;
        }
        let stmt_type = infer_statement_type(segment);
        let (schema, obj_name) = infer_schema_and_object_name(segment);
        let id = generate_chunk_id(file_path, &obj_name.clone().unwrap_or("unknown".to_string()));
        chunks.push(CodeChunk {
            id,
            repo_name: String::new(),
            file_name: file_basename.clone(),
            code: segment.to_string(),
            r#type: "sql_statement".to_string(),
            dependencies: deps.to_vec(),
            created_at: crate::services::ingestion::types::now_iso(),
            kind: CodeChunkKind::Sql(SqlFields {
                statement_type: Some(stmt_type),
                object_name: obj_name,
                schema_name: schema,
                database_name: database_name.map(|s| s.to_string()),
            }),
        });
    }
    chunks
}

fn find_dollar_quoted_spans(text: &str) -> Vec<(usize, usize)> {
    let re = Regex::new(DOLLAR_TAG_RE_RAW).unwrap();
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    // Index-based iteration over byte positions, but `text` ASCII by use-case.
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // Skip single-quoted strings so `$$` inside is ignored.
        if bytes[i] == b'\'' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\'' && i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'\'' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'$' {
            // Need to match the dollar-tag regex starting at byte i.
            if let Some(m) = re.find_at(text, i) {
                if m.start() == i {
                    let tag = m.as_str();
                    let body_start = i + tag.len();
                    if let Some(close) = text[tag.len() + i..].find(tag) {
                        let close_byte = body_start + close;
                        spans.push((body_start, close_byte));
                        i = close_byte + tag.len();
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    let _ = chars; // suppress unused warning when feature not enabled
    spans
}

fn inside_spans(pos: usize, spans: &[(usize, usize)]) -> bool {
    spans.iter().any(|(s, e)| *s <= pos && pos < *e)
}

fn merge_alter_table_chunks(
    chunks: Vec<CodeChunk>,
    file_path: &str,
    deps: &[String],
    database_name: Option<&str>,
) -> Vec<CodeChunk> {
    let mut result: Vec<CodeChunk> = Vec::new();
    let mut alter_groups: std::collections::HashMap<String, Vec<CodeChunk>> = std::collections::HashMap::new();
    let mut group_order: Vec<String> = Vec::new();

    for chunk in chunks {
        let is_alter_table = matches!(&chunk.kind, CodeChunkKind::Sql(f) if f.statement_type.as_deref() == Some("ALTER TABLE"));
        if is_alter_table {
            let key = chunk
                .kind_metadata()
                .get("object_name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_lowercase();
            if !group_order.contains(&key) {
                group_order.push(key.clone());
            }
            alter_groups.entry(key).or_default().push(chunk);
        } else {
            result.push(chunk);
        }
    }

    for key in group_order {
        if let Some(group) = alter_groups.remove(&key) {
            let merged_code = group
                .iter()
                .map(|c| c.code.clone())
                .collect::<Vec<_>>()
                .join("\n\n");
            let first = &group[0];
            let id = generate_chunk_id(file_path, &key);
            result.push(CodeChunk {
                id,
                repo_name: first.repo_name.clone(),
                file_name: first.file_name.clone(),
                code: merged_code,
                r#type: "sql_statement".to_string(),
                dependencies: deps.to_vec(),
                created_at: crate::services::ingestion::types::now_iso(),
                kind: CodeChunkKind::Sql(SqlFields {
                    statement_type: Some("ALTER TABLE".to_string()),
                    object_name: match &first.kind {
                        CodeChunkKind::Sql(f) => f.object_name.clone(),
                        _ => None,
                    },
                    schema_name: match &first.kind {
                        CodeChunkKind::Sql(f) => f.schema_name.clone(),
                        _ => None,
                    },
                    database_name: database_name.map(|s| s.to_string()),
                }),
            });
        }
    }
    result
}

const KNOWN_STATEMENT_PREFIXES: &[&str] = &[
    "CREATE TABLE",
    "CREATE VIRTUAL TABLE",
    "CREATE VIEW",
    "CREATE MATERIALIZED VIEW",
    "CREATE INDEX",
    "CREATE UNIQUE INDEX",
    "CREATE FUNCTION",
    "CREATE PROCEDURE",
    "CREATE TRIGGER",
    "CREATE TYPE",
    "CREATE TYPE BODY",
    "CREATE SEQUENCE",
    "CREATE SCHEMA",
    "CREATE DATABASE",
    "CREATE EXTENSION",
    "CREATE ROLE",
    "CREATE POLICY",
    "CREATE TABLESPACE",
    "CREATE SYNONYM",
    "CREATE PACKAGE",
    "CREATE PACKAGE BODY",
    "CREATE OR REPLACE",
    "ALTER TABLE",
    "ALTER VIEW",
    "ALTER INDEX",
    "ALTER SCHEMA",
    "DROP TABLE",
    "DROP VIEW",
    "DROP FUNCTION",
    "DROP PROCEDURE",
    "DROP INDEX",
    "DROP TYPE",
    "DROP SEQUENCE",
    "DROP EXTENSION",
    "DROP SCHEMA",
    "DROP TRIGGER",
    "INSERT",
    "SELECT",
    "UPDATE",
    "DELETE",
    "MERGE",
    "GRANT",
    "REVOKE",
];

fn filter_junk_chunks(chunks: Vec<CodeChunk>) -> Vec<CodeChunk> {
    chunks
        .into_iter()
        .filter(|c| {
            let stmt = match &c.kind {
                CodeChunkKind::Sql(f) => f.statement_type.as_deref().unwrap_or("").to_uppercase(),
                _ => return true,
            };
            if stmt.trim().is_empty() {
                return false;
            }
            KNOWN_STATEMENT_PREFIXES.iter().any(|p| stmt.starts_with(p))
        })
        .collect()
}

fn infer_statement_type(code: &str) -> String {
    let normalized: String = code.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase();
    let prefixes = [
        "CREATE OR REPLACE PACKAGE BODY",
        "CREATE OR REPLACE PACKAGE",
        "CREATE OR REPLACE PUBLIC SYNONYM",
        "CREATE OR REPLACE SYNONYM",
        "CREATE OR REPLACE TYPE BODY",
        "CREATE OR REPLACE TYPE",
        "CREATE OR REPLACE PROCEDURE",
        "CREATE OR REPLACE FUNCTION",
        "CREATE OR REPLACE TRIGGER",
        "CREATE MATERIALIZED VIEW",
        "CREATE VIRTUAL TABLE",
        "CREATE PACKAGE BODY",
        "CREATE PACKAGE",
        "CREATE TYPE BODY",
        "CREATE TABLESPACE",
        "CREATE UNIQUE INDEX",
        "CREATE PROCEDURE",
        "CREATE FUNCTION",
        "CREATE TABLE",
        "CREATE VIEW",
        "CREATE INDEX",
        "CREATE TRIGGER",
        "CREATE SEQUENCE",
        "CREATE TYPE",
        "CREATE SCHEMA",
        "CREATE DATABASE",
        "CREATE EXTENSION",
        "CREATE ROLE",
        "CREATE POLICY",
        "CREATE SYNONYM",
        "ALTER TABLE",
        "DROP TABLE IF EXISTS",
        "DROP TABLE",
        "DROP VIEW",
        "DROP FUNCTION",
        "DROP PROCEDURE",
        "DROP INDEX",
        "DROP TYPE",
        "DROP SEQUENCE",
        "DROP EXTENSION",
        "DROP SCHEMA",
        "DROP TRIGGER",
    ];
    for keyword in prefixes {
        if normalized.starts_with(keyword) {
            if keyword.contains(" IF EXISTS") || keyword.contains(" IF NOT EXISTS") {
                return keyword.split(" IF ").next().unwrap_or(keyword).trim().to_string();
            }
            return keyword.to_string();
        }
    }
    // Match `CREATE PROCEDUREdbo.Name` no-space quirks.
    if let Some(m) = Regex::new(
        r"^CREATE\s+(?:TABLE|VIEW|PROCEDURE|FUNCTION|INDEX|TRIGGER)",
    )
    .ok()
    .and_then(|r| r.find(&normalized))
    {
        return m.as_str().trim().to_string();
    }
    let first_token = normalized.split_whitespace().next().unwrap_or("");
    if matches!(
        first_token,
        "INSERT" | "SELECT" | "UPDATE" | "DELETE" | "MERGE" | "GRANT" | "REVOKE"
    ) {
        return first_token.to_string();
    }
    let tokens: Vec<&str> = normalized.split_whitespace().take(2).collect();
    if tokens.len() >= 2 {
        tokens.join(" ")
    } else if !tokens.is_empty() {
        tokens[0].to_string()
    } else {
        "UNKNOWN".to_string()
    }
}

fn infer_schema_and_object_name(code: &str) -> (Option<String>, Option<String>) {
    let stripped = code.trim();
    let grant_re = Regex::new(r"(?i)^(?:GRANT|REVOKE)\s+.*?\bON\s+(\S+)").unwrap();
    if let Some(c) = grant_re.captures(stripped) {
        let raw = c.get(1).unwrap().as_str().split('(').next().unwrap_or("").trim_end_matches(|c: char| c == ';' || c == ',');
        return parse_qualified_name(raw);
    }
    let policy_re = Regex::new(
        r"(?i)^(?:CREATE|ALTER|DROP)\s+(?:OR\s+REPLACE\s+)?POLICY(?:\s+IF\s+(?:NOT\s+)?EXISTS)?\s+(\S+)",
    )
    .unwrap();
    if let Some(c) = policy_re.captures(stripped) {
        let raw = c.get(1).unwrap().as_str().split('(').next().unwrap_or("").trim_end_matches(|c: char| c == ';' || c == ',');
        return parse_qualified_name(raw);
    }
    let general_re = Regex::new(
        r"(?i)^(?:CREATE|ALTER|DROP)\s+(?:OR\s+REPLACE\s+)?(?:PUBLIC\s+)?(?:UNIQUE\s+)?(?:MATERIALIZED\s+VIEW|VIRTUAL\s+TABLE|PACKAGE\s+BODY|TYPE\s+BODY|TABLESPACE|TABLE|VIEW|FUNCTION|PROCEDURE|INDEX|TRIGGER|TYPE|SEQUENCE|SCHEMA|DATABASE|EXTENSION|ROLE|SYNONYM|PACKAGE)(?:\s+IF\s+(?:NOT\s+)?EXISTS)?\s*(\S+)",
    )
    .unwrap();
    if let Some(c) = general_re.captures(stripped) {
        let raw = c.get(1).unwrap().as_str().split('(').next().unwrap_or("").trim_end_matches(|c: char| c == ';' || c == ',');
        return parse_qualified_name(raw);
    }
    (None, None)
}

fn parse_qualified_name(raw: &str) -> (Option<String>, Option<String>) {
    let clean = raw.replace('[', "").replace(']', "");
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() >= 2 {
        (Some(parts[0].to_string()), Some(parts.last().unwrap().to_string()))
    } else if let Some(single) = parts.first() {
        if single.is_empty() {
            (None, None)
        } else {
            (None, Some((*single).to_string()))
        }
    } else {
        (None, None)
    }
}