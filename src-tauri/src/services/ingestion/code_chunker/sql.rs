//! SQL chunker. Direct port of `chunk_code_sql.py`.
//!
//! Multi-phase pipeline:
//! 1. tree-sitter extracts what it can parse (Phase 1 of the Python code).
//! 2. Each chunk is regex re-split to recover DDL the tree-sitter fragment
//!    may have buried inside an ERROR node (Phase 2).
//! 3. The remainder text is regex-split too (Phase 3).
//! 4. ALTER TABLE chunks are merged per `object_name` (Phase 4).
//! 5. Junk chunks with unrecognized statement types are filtered (Phase 5).

use regex::Regex;
use tree_sitter::Node;

use super::helpers::{
    build_remainder_code, byte_slice_text, collect_dependencies, find_child_of_kind, new_chunk,
    node_text,
};
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

fn dollar_tag_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$[A-Za-z_]*\$").expect("dollar tag regex must compile"))
}

pub fn extract_dependencies<'a>(root: Node<'a>, source: &[u8]) -> Vec<String> {
    collect_dependencies(root, |node, deps| {
        let follows_references = node.kind() == "object_reference"
            && prev_named_sibling(&node).is_some_and(|prev| prev.kind() == "keyword_references");
        if follows_references {
            if let Some(name) = extract_object_name(&node, source) {
                deps.push(name);
            }
        }
    })
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

fn identifier_children<'a>(obj_ref: &Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = obj_ref.walk();
    let identifiers = obj_ref
        .children(&mut cursor)
        .filter(|c| c.kind() == "identifier")
        .collect();
    identifiers
}

/// Whole text of `obj_ref` without brackets, or `None` when empty.
fn raw_object_name<'a>(obj_ref: &Node<'a>, source: &[u8]) -> Option<String> {
    let raw = strip_brackets(&node_text(obj_ref, source));
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

fn extract_object_name<'a>(obj_ref: &Node<'a>, source: &[u8]) -> Option<String> {
    let identifiers = identifier_children(obj_ref);
    if identifiers.is_empty() {
        return raw_object_name(obj_ref, source);
    }
    Some(
        identifiers
            .iter()
            .map(|n| strip_brackets(&node_text(n, source)))
            .collect::<Vec<_>>()
            .join("."),
    )
}

fn extract_schema_and_name<'a>(
    obj_ref: &Node<'a>,
    source: &[u8],
) -> (Option<String>, Option<String>) {
    let identifiers = identifier_children(obj_ref);
    if identifiers.len() >= 2 {
        let schema = strip_brackets(&node_text(&identifiers[0], source));
        let name = strip_brackets(&node_text(identifiers.last().unwrap(), source));
        (Some(schema), Some(name))
    } else if identifiers.len() == 1 {
        (
            None,
            Some(strip_brackets(&node_text(&identifiers[0], source))),
        )
    } else {
        (None, raw_object_name(obj_ref, source))
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
    database_name: Option<&str>,
) -> Vec<CodeChunk> {
    let mut raw_chunks: Vec<(CodeChunk, bool)> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    extract_chunks_impl(
        &node,
        source,
        file_path,
        deps,
        database_name,
        &mut raw_chunks,
        &mut spans,
    );

    // Phase 2: regex re-split each chunk to recover DDL buried inside. Junk
    // ERROR-node chunks with nothing recognizable are dropped.
    let mut resplit: Vec<CodeChunk> = Vec::new();
    for (chunk, is_error) in raw_chunks {
        let sub = regex_split_sql_text(&chunk.code, file_path, deps, database_name);
        if !sub.is_empty() {
            resplit.extend(sub);
        } else if !is_error {
            resplit.push(chunk);
        }
    }

    // Phase 3: regex-split the remainder (tree-sitter didn't touch this).
    let remainder_code = build_remainder_code(source, &spans);
    if !remainder_code.trim().is_empty() {
        resplit.extend(regex_split_sql_text(
            &remainder_code,
            file_path,
            deps,
            database_name,
        ));
    }

    // Phase 4: merge ALTER TABLE chunks by object_name.
    let merged = merge_alter_table_chunks(resplit, file_path, deps, database_name);

    // Phase 5: filter junk chunks.
    filter_junk_chunks(merged)
}

/// Collect statement chunks and root-level ERROR chunks (flagged `true`).
fn extract_chunks_impl<'a>(
    node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    database_name: Option<&str>,
    chunks: &mut Vec<(CodeChunk, bool)>,
    spans: &mut Vec<(usize, usize)>,
) {
    if node.kind() == "statement" {
        // The first *named* child of a `statement` node is the actual DDL/DML
        // construct (create_table, alter_table, ...). Using `child(0)`
        // would also surface anonymous sibling nodes (whitespace, comments).
        let inner = node.named_child(0);
        if let Some(inner) = inner {
            let inner_kind = inner.kind();
            if STATEMENT_NODE_TYPES.contains(&inner_kind) {
                let chunk = statement_chunk(node, &inner, source, file_path, deps, database_name);
                chunks.push((chunk, false));
                spans.push((node.start_byte(), statement_end(node)));
                return;
            }
        }
    }

    // ERROR at root level (T-SQL procedures with @ params, Oracle PL/SQL blocks).
    if node.kind() == "ERROR" {
        if let Some(parent) = node.parent() {
            if parent.kind() == "program" {
                let chunk = inferred_chunk(node_text(node, source), file_path, deps, database_name);
                chunks.push((chunk, true));
                spans.push((node.start_byte(), node.end_byte()));
                return;
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_chunks_impl(
            &child,
            source,
            file_path,
            deps,
            database_name,
            chunks,
            spans,
        );
    }
}

/// End byte of a statement, including a trailing `;` sibling.
fn statement_end<'a>(stmt_node: &Node<'a>) -> usize {
    match stmt_node.next_sibling() {
        Some(next_sib) if next_sib.kind() == ";" => next_sib.end_byte(),
        _ => stmt_node.end_byte(),
    }
}

fn statement_chunk<'a>(
    stmt_node: &Node<'a>,
    inner_node: &Node<'a>,
    source: &[u8],
    file_path: &str,
    deps: &[String],
    database_name: Option<&str>,
) -> CodeChunk {
    let code = byte_slice_text(source, stmt_node.start_byte(), statement_end(stmt_node));

    let mut schema_name: Option<String> = None;
    let mut object_name: Option<String> = None;
    if let Some(obj_ref) = find_child_of_kind(inner_node, "object_reference") {
        let (s, n) = extract_schema_and_name(&obj_ref, source);
        schema_name = s;
        object_name = n;
    }

    let identifier = object_name
        .clone()
        .unwrap_or_else(|| inner_node.kind().to_string());
    let fields = SqlFields {
        statement_type: Some(get_statement_label(inner_node.kind())),
        object_name,
        schema_name,
        database_name: database_name.map(str::to_string),
    };
    sql_chunk(file_path, &identifier, code, deps, fields)
}

fn sql_chunk(
    file_path: &str,
    identifier: &str,
    code: String,
    deps: &[String],
    fields: SqlFields,
) -> CodeChunk {
    new_chunk(
        file_path,
        identifier,
        code,
        "sql_statement",
        deps,
        CodeChunkKind::Sql(fields),
    )
}

/// Chunk whose statement type and object name are inferred from `code`.
fn inferred_chunk(
    code: String,
    file_path: &str,
    deps: &[String],
    database_name: Option<&str>,
) -> CodeChunk {
    let (schema_name, object_name) = infer_schema_and_object_name(&code);
    let identifier = object_name.clone().unwrap_or_else(|| "unknown".to_string());
    let fields = SqlFields {
        statement_type: Some(infer_statement_type(&code)),
        object_name,
        schema_name,
        database_name: database_name.map(str::to_string),
    };
    sql_chunk(file_path, &identifier, code, deps, fields)
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
    positions.sort();
    positions.dedup();

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
        chunks.push(inferred_chunk(
            segment.to_string(),
            file_path,
            deps,
            database_name,
        ));
    }
    chunks
}

fn find_dollar_quoted_spans(text: &str) -> Vec<(usize, usize)> {
    let re = dollar_tag_re();
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
    let mut groups: Vec<(String, Vec<CodeChunk>)> = Vec::new();

    for chunk in chunks {
        let key = match &chunk.kind {
            CodeChunkKind::Sql(f) if f.statement_type.as_deref() == Some("ALTER TABLE") => {
                f.object_name.clone().unwrap_or_default().to_lowercase()
            }
            _ => {
                result.push(chunk);
                continue;
            }
        };
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.push(chunk),
            None => groups.push((key, vec![chunk])),
        }
    }

    for (key, group) in groups {
        let mut codes: Vec<&str> = Vec::new();
        for chunk in &group {
            codes.push(&chunk.code);
        }
        let (object_name, schema_name) = match &group[0].kind {
            CodeChunkKind::Sql(f) => (f.object_name.clone(), f.schema_name.clone()),
            _ => (None, None),
        };
        let fields = SqlFields {
            statement_type: Some("ALTER TABLE".to_string()),
            object_name,
            schema_name,
            database_name: database_name.map(str::to_string),
        };
        result.push(sql_chunk(file_path, &key, codes.join("\n\n"), deps, fields));
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
    let normalized: String = code
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase();
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
                return keyword
                    .split(" IF ")
                    .next()
                    .unwrap_or(keyword)
                    .trim()
                    .to_string();
            }
            return keyword.to_string();
        }
    }
    // Match `CREATE PROCEDUREdbo.Name` no-space quirks.
    static CREATE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let create_re = CREATE_RE.get_or_init(|| {
        Regex::new(r"^CREATE\s+(?:TABLE|VIEW|PROCEDURE|FUNCTION|INDEX|TRIGGER)").unwrap()
    });
    if let Some(m) = create_re.find(&normalized) {
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
    static RES: std::sync::OnceLock<[Regex; 3]> = std::sync::OnceLock::new();
    let res = RES.get_or_init(|| {
        [
            Regex::new(r"(?i)^(?:GRANT|REVOKE)\s+.*?\bON\s+(\S+)").unwrap(),
            Regex::new(
                r"(?i)^(?:CREATE|ALTER|DROP)\s+(?:OR\s+REPLACE\s+)?POLICY(?:\s+IF\s+(?:NOT\s+)?EXISTS)?\s+(\S+)",
            )
            .unwrap(),
            Regex::new(
                r"(?i)^(?:CREATE|ALTER|DROP)\s+(?:OR\s+REPLACE\s+)?(?:PUBLIC\s+)?(?:UNIQUE\s+)?(?:MATERIALIZED\s+VIEW|VIRTUAL\s+TABLE|PACKAGE\s+BODY|TYPE\s+BODY|TABLESPACE|TABLE|VIEW|FUNCTION|PROCEDURE|INDEX|TRIGGER|TYPE|SEQUENCE|SCHEMA|DATABASE|EXTENSION|ROLE|SYNONYM|PACKAGE)(?:\s+IF\s+(?:NOT\s+)?EXISTS)?\s*(\S+)",
            )
            .unwrap(),
        ]
    });
    let stripped = code.trim();
    for re in res {
        if let Some(c) = re.captures(stripped) {
            let raw = c[1].split('(').next().unwrap_or("");
            return parse_qualified_name(raw.trim_end_matches([';', ',']));
        }
    }
    (None, None)
}

fn parse_qualified_name(raw: &str) -> (Option<String>, Option<String>) {
    let clean = raw.replace(['[', ']'], "");
    let parts: Vec<&str> = clean.split('.').collect();
    if parts.len() >= 2 {
        (
            Some(parts[0].to_string()),
            Some(parts.last().unwrap().to_string()),
        )
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
