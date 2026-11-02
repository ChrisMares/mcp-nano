//! Shared helpers used by all per-language code chunkers. Direct port of
//! `src/embedding/code_embedder/chunk_helpers.py`.
//!
//! - [`strip_sql_comments`] / [`strip_sql_boilerplate`] normalize SQL text
//!   before it's parsed by tree-sitter so comment syntax doesn't trip up the
//!   grammar.
//! - [`merge_spans`] / [`build_remainder_code`] / [`make_remainder_chunk`]
//!   emit the trailing "file_remainder" chunk after every named declaration
//!   has been extracted.
//! - [`generate_chunk_id`] mirrors the Python `<basename>-<id>-<8hex>` id
//!   scheme so chunk ids stay readable in Qdrant payloads.

use std::collections::HashSet;
use std::path::Path;

use regex::Regex;
use tree_sitter::Node;

use crate::services::ingestion::types::{
    normalize_code, now_iso, CodeChunk, CodeChunkKind, MemberFields,
};

/// Strip only the SQL comment styles (`--` line, `/* */` block) while
/// preserving dollar-quoted (`$$ ... $$`, `$tag$ ... $tag$`) and
/// single/double-quoted strings. Direct port of `chunk_helpers.strip_sql_comments`.
pub fn strip_sql_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];

        // Dollar-quoted strings: $$ or $tag$ ... matching close tag.
        if c == '$' && i + 1 < chars.len() {
            if let Some(tag_len) = match_dollar_tag(&chars, i) {
                let tag = &chars[i..i + tag_len];
                if let Some(close) = find_subseq(&chars, tag, i + tag_len) {
                    out.extend_from_slice(&chars[i..close + tag_len]);
                    i = close + tag_len;
                    continue;
                } else {
                    // No closing tag — preserve rest verbatim.
                    out.extend_from_slice(&chars[i..]);
                    return collapse_blank_runs(&out.into_iter().collect::<String>());
                }
            }
        }

        // Single-quoted strings and double-quoted identifiers.
        if c == '\'' || c == '"' {
            let j = quoted_end(&chars, i);
            out.extend_from_slice(&chars[i..j]);
            i = j;
            continue;
        }

        // Block comments: /* ... */ (nestable).
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            out.push('\n');
            continue;
        }

        // Line comments: --
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        out.push(c);
        i += 1;
    }

    collapse_blank_runs(&out.into_iter().collect::<String>())
}

/// Index just past the quoted run opened by `chars[start]`, where a doubled
/// quote (`''` or `""`) is an escaped quote. Unterminated runs end at EOF.
fn quoted_end(chars: &[char], start: usize) -> usize {
    let quote = chars[start];
    let mut j = start + 1;
    while j < chars.len() {
        if chars[j] == quote {
            if j + 1 < chars.len() && chars[j + 1] == quote {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    j
}

/// Matches an opening dollar-quote tag starting at `i`.
/// Returns `Some(len)` if `chars[i..]` starts with `$$` or `$<A-Za-z_>*$`.
fn match_dollar_tag(chars: &[char], i: usize) -> Option<usize> {
    if chars.get(i)? != &'$' {
        return None;
    }
    let mut j = i + 1;
    let len = chars.len();
    while j < len {
        let c = chars[j];
        if c == '$' {
            return Some(j - i + 1);
        }
        if !c.is_ascii_alphabetic() && c != '_' {
            return None;
        }
        j += 1;
    }
    None
}

fn find_subseq(haystack: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let limit = haystack.len() - needle.len();
    let mut i = from;
    while i <= limit {
        if haystack[i..i + needle.len()] == *needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Strip SQL boilerplate: GO batch separators, `SET ANSI_NULLS/QUOTED_IDENTIFIER`,
/// `USE [database]` (extracted separately), and T-SQL bracket-quoted
/// identifiers `[name] -> name`. Returns `(cleaned_text, database_name)`.
/// Direct port of `chunk_helpers.strip_sql_boilerplate`.
pub fn strip_sql_boilerplate(text: &str) -> (String, Option<String>) {
    static BRACKET_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = BRACKET_RE.get_or_init(|| Regex::new(r"\[([^\]]*)\]").unwrap());

    let mut cleaned: Vec<String> = Vec::new();
    let mut database: Option<String> = None;
    static USE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let use_re = USE_RE
        .get_or_init(|| Regex::new(r"(?i)^\ufeff?\s*USE\s+\[?([^\]\s;]+)\]?\s*;?\s*$").unwrap());

    for line in text.split('\n') {
        let stripped = line.trim().trim_start_matches('\u{feff}').to_uppercase();
        if stripped == "GO" || stripped == "/" {
            continue;
        }
        if matches!(
            stripped.as_str(),
            "SET ANSI_NULLS ON"
                | "SET ANSI_NULLS OFF"
                | "SET QUOTED_IDENTIFIER ON"
                | "SET QUOTED_IDENTIFIER OFF"
        ) {
            continue;
        }
        if let Some(c) = use_re.captures(line.trim()) {
            database = Some(c.get(1).unwrap().as_str().to_string());
            continue;
        }
        cleaned.push(line.to_string());
    }
    let joined = cleaned.join("\n");
    let result = re.replace_all(&joined, |c: &regex::Captures| {
        c.get(1).unwrap().as_str().to_string()
    });
    (result.to_string(), database)
}

/// Generate a chunk id of the form `<basename>-<identifier>-<8hex>`. Mirrors
/// `chunk_helpers.generate_chunk_id`. Uses the first 8 characters of a fresh
/// UUID v4 (alphanumeric ASCII) as the suffix — same uniqueness guarantees
/// as the Python `random.choices` 8-char pool, without pulling a new dep.
pub fn generate_chunk_id(file_path: &str, identifier: &str) -> String {
    let basename = file_basename(file_path);
    let suffix = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    format!("{basename}-{identifier}-{suffix}")
}

/// Final component of `file_path`, or the whole string when it has none.
pub fn file_basename(file_path: &str) -> String {
    Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_path)
        .to_string()
}

/// Build a chunk of `file_path` with an id derived from `identifier`
/// (see [`generate_chunk_id`]), an empty repo name, and the current time.
pub fn new_chunk(
    file_path: &str,
    identifier: &str,
    code: String,
    chunk_type: &str,
    deps: &[String],
    kind: CodeChunkKind,
) -> CodeChunk {
    CodeChunk {
        id: generate_chunk_id(file_path, identifier),
        repo_name: String::new(),
        file_name: file_basename(file_path),
        code,
        r#type: chunk_type.to_string(),
        dependencies: deps.to_vec(),
        created_at: now_iso(),
        kind,
    }
}

/// Chunk for the C++/Java/Rust member layout, identified by its function
/// name, else its class name, else `Unknown`.
pub fn member_chunk(
    file_path: &str,
    fields: MemberFields,
    code: String,
    chunk_type: &str,
    deps: &[String],
    kind: fn(MemberFields) -> CodeChunkKind,
) -> CodeChunk {
    let identifier = fields
        .function_name
        .clone()
        .or_else(|| fields.class_name.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    new_chunk(file_path, &identifier, code, chunk_type, deps, kind(fields))
}

/// Run `extract` to collect chunks and the byte spans they cover, then
/// append the [`make_remainder_chunk`] for everything left over.
pub fn chunks_with_remainder<F>(
    source: &[u8],
    file_path: &str,
    deps: &[String],
    extract: F,
) -> Vec<CodeChunk>
where
    F: FnOnce(&mut Vec<CodeChunk>, &mut Vec<(usize, usize)>),
{
    let mut chunks = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    extract(&mut chunks, &mut spans);
    if let Some(r) = make_remainder_chunk(source, &spans, file_path, deps) {
        chunks.push(r);
    }
    chunks
}

/// Pre-order DFS over a tree-sitter node, left-to-right. Used by dependency
/// extractors so discovery order matches Python's recursive `visit`.
pub fn walk_preorder<'a, F>(root: Node<'a>, mut visit: F)
where
    F: FnMut(Node<'a>),
{
    let mut stack: Vec<Node<'a>> = vec![root];
    while let Some(node) = stack.pop() {
        visit(node);
        let mut cursor = node.walk();
        let children: Vec<Node<'a>> = node.children(&mut cursor).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }
}

/// Walk `root` in pre-order, letting `visit` push the dependencies each node
/// declares, and return them de-duplicated in first-seen order.
pub fn collect_dependencies<'a, F>(root: Node<'a>, mut visit: F) -> Vec<String>
where
    F: FnMut(Node<'a>, &mut Vec<String>),
{
    let mut found: Vec<String> = Vec::new();
    walk_preorder(root, |node| visit(node, &mut found));
    let mut seen = HashSet::new();
    let mut deps = Vec::new();
    for dep in found {
        if seen.insert(dep.clone()) {
            deps.push(dep);
        }
    }
    deps
}

/// Text of `node`'s `field` child, untrimmed.
pub fn field_text<'a>(node: &Node<'a>, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| node_text(&n, source))
}

/// First direct child of `node` whose kind is `kind`.
pub fn find_child_of_kind<'a>(node: &Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == kind);
    found
}

/// True if any direct child of `node` has a kind in `kinds`.
pub fn has_child_of_kind<'a>(node: &Node<'a>, kinds: &[&str]) -> bool {
    let mut cursor = node.walk();
    let found = node
        .children(&mut cursor)
        .any(|c| kinds.contains(&c.kind()));
    found
}

/// Name of the nearest ancestor whose kind is in `kinds`: its `name` field,
/// else its first named child whose kind is in `fallback_kinds`. Ancestors
/// that match but carry neither are skipped.
pub fn nearest_ancestor_name<'a>(
    node: &Node<'a>,
    source: &[u8],
    kinds: &[&str],
    fallback_kinds: &[&str],
) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if kinds.contains(&parent.kind()) {
            if let Some(name) = parent.child_by_field_name("name") {
                return Some(node_text(&name, source));
            }
            let mut cursor = parent.walk();
            for child in parent.named_children(&mut cursor) {
                if fallback_kinds.contains(&child.kind()) {
                    return Some(node_text(&child, source));
                }
            }
        }
        current = parent.parent();
    }
    None
}

/// Trim a type annotation and drop its leading `:` (`": u32"` -> `"u32"`).
pub fn normalize_type_text(text: &str) -> String {
    let s = text.trim();
    match s.strip_prefix(':') {
        Some(stripped) => stripped.trim().to_string(),
        None => s.to_string(),
    }
}

/// Merge overlapping/adjacent byte spans. Mirrors `chunk_helpers.merge_spans`.
pub fn merge_spans(spans: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if spans.is_empty() {
        return Vec::new();
    }
    let mut sorted = spans.to_vec();
    sorted.sort_by_key(|s| s.0);
    let mut merged: Vec<(usize, usize)> = vec![sorted[0]];
    for &(start, end) in sorted.iter().skip(1) {
        let last = merged.last_mut().unwrap();
        if start <= last.1 {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

/// Build the file-remainder text by removing the byte spans of every chunk
/// that's already been extracted. Mirrors `chunk_helpers.build_remainder_code`.
pub fn build_remainder_code(source_bytes: &[u8], spans: &[(usize, usize)]) -> String {
    if spans.is_empty() {
        return String::from_utf8_lossy(source_bytes).into_owned();
    }
    let len = source_bytes.len();
    let merged = merge_spans(spans);
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut last_end = 0usize;
    for &(start, end) in &merged {
        let start = start.min(len);
        let end = end.min(len);
        if start < last_end {
            last_end = last_end.max(end);
            continue;
        }
        if start > last_end {
            parts.push(&source_bytes[last_end..start]);
        }
        last_end = last_end.max(end);
    }
    if last_end < len {
        parts.push(&source_bytes[last_end..]);
    }
    let combined: Vec<u8> = parts.concat();
    String::from_utf8_lossy(&combined).into_owned()
}

/// Build a `file_remainder` generic chunk from the leftover source bytes.
/// Mirrors `chunk_helpers.make_remainder_chunk` — returns `None` if the
/// remainder is empty after trimming.
pub fn make_remainder_chunk(
    source_bytes: &[u8],
    spans: &[(usize, usize)],
    file_path: &str,
    dependencies: &[String],
) -> Option<CodeChunk> {
    let code = build_remainder_code(source_bytes, spans);
    if code.trim().is_empty() {
        return None;
    }
    Some(CodeChunk {
        id: uuid::Uuid::new_v4().to_string(),
        repo_name: String::new(),
        file_name: file_basename(file_path),
        code: normalize_code(&code),
        r#type: "file_remainder".to_string(),
        dependencies: dependencies.to_vec(),
        created_at: now_iso(),
        kind: CodeChunkKind::Generic,
    })
}

/// Lossy UTF-8 decode of `source[start..end]`. Returns empty string when the
/// range is invalid (never panics on OOB tree-sitter spans).
pub fn byte_slice_text(source: &[u8], start: usize, end: usize) -> String {
    if start > end || end > source.len() {
        return String::new();
    }
    String::from_utf8_lossy(&source[start..end]).into_owned()
}

/// Decode a tree-sitter node's text as UTF-8 (lossy). Mirrors `node_text`.
pub fn node_text<'a>(node: &Node<'a>, source_bytes: &[u8]) -> String {
    byte_slice_text(source_bytes, node.start_byte(), node.end_byte())
}

/// For JS/TS `variable_declarator` nodes, expand the removal span to the
/// enclosing `lexical_declaration`/`variable_declaration` when the declarator
/// is the only one in that declaration. Mirrors `chunk_helpers.get_declarator_removal_span`.
pub fn get_declarator_removal_span<'a>(declarator: &Node<'a>) -> (usize, usize) {
    if let Some(parent) = declarator.parent() {
        let parent_type = parent.kind();
        if parent_type == "lexical_declaration" || parent_type == "variable_declaration" {
            let mut count = 0;
            let mut cursor = parent.walk();
            for child in parent.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    count += 1;
                }
            }
            if count == 1 {
                let mut outer = parent;
                if parent_type == "variable_declaration" {
                    if let Some(gp) = outer.parent() {
                        if gp.kind() == "lexical_declaration" {
                            outer = gp;
                        }
                    }
                }
                return (outer.start_byte(), outer.end_byte());
            }
        }
    }
    (declarator.start_byte(), declarator.end_byte())
}

/// Strip the surrounding quote/backtick from a JS/TS import source string.
/// Mirrors `chunk_helpers.strip_js_import_source`.
pub fn strip_js_import_source(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0] as char;
        if (first == '\'' || first == '"' || first == '`') && value.ends_with(first) {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn collapse_blank_runs(s: &str) -> String {
    // Collapse 3+ newlines to 2.
    let mut out = String::with_capacity(s.len());
    let mut blanks = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            blanks += 1;
            if blanks <= 2 {
                out.push(ch);
            }
        } else {
            blanks = 0;
            out.push(ch);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_sql_comments_keeps_dollar_quoted() {
        let src =
            "CREATE FUNCTION f() RETURNS int AS $$\n-- inner comment\nSELECT 1\n$$ LANGUAGE sql;";
        let out = strip_sql_comments(src);
        assert!(out.contains("$$"));
        assert!(out.contains("SELECT 1"));
        // The dollar-quoted block body is preserved verbatim (including
        // the `-- inner comment` text), mirroring Python's behavior.
        assert!(out.contains("inner comment"));
    }

    #[test]
    fn strip_sql_comments_strips_outside_dollar_quotes() {
        let src =
            "-- header comment\nSELECT 1;\nCREATE FUNCTION f() RETURNS int AS $$$x$$ LANGUAGE sql;";
        let out = strip_sql_comments(src);
        assert!(!out.contains("header comment"));
    }

    #[test]
    fn strip_sql_boilerplate_extracts_database() {
        let src = "USE [mydb];\nGO\nSET ANSI_NULLS ON\nCREATE TABLE x (id int);";
        let (cleaned, db) = strip_sql_boilerplate(src);
        assert_eq!(db.as_deref(), Some("mydb"));
        assert!(!cleaned.contains("USE"));
        assert!(!cleaned.contains("GO"));
        assert!(!cleaned.contains("ANSI_NULLS"));
        assert!(cleaned.contains("CREATE TABLE x"));
    }

    #[test]
    fn strip_sql_boilerplate_strips_brackets() {
        let (cleaned, _) = strip_sql_boilerplate("SELECT [dbo].[x] FROM [t]");
        assert!(cleaned.contains("dbo.x"));
        assert!(cleaned.contains("FROM t"));
        assert!(!cleaned.contains('['));
    }

    #[test]
    fn merge_spans_combines_overlap() {
        let spans = vec![(0, 10), (5, 15), (20, 30), (25, 50)];
        let merged = merge_spans(&spans);
        assert_eq!(merged, vec![(0, 15), (20, 50)]);
    }

    #[test]
    fn build_remainder_keeps_uncovered_gaps() {
        let source = b"AAAAABBBBCCCCDDDD";
        let spans = vec![(5, 9), (13, 17)];
        let remainder = build_remainder_code(source, &spans);
        assert_eq!(remainder, "AAAAACCCC");
    }

    #[test]
    fn build_remainder_tolerates_out_of_bounds_spans() {
        let source = b"hello";
        let remainder = build_remainder_code(source, &[(0, 2), (100, 200), (3, 999)]);
        assert_eq!(remainder, "l");
    }

    #[test]
    fn byte_slice_text_never_panics_on_bad_ranges() {
        let src = b"abcdef";
        assert_eq!(byte_slice_text(src, 0, 3), "abc");
        assert_eq!(byte_slice_text(src, 3, 3), "");
        assert_eq!(byte_slice_text(src, 4, 2), "");
        assert_eq!(byte_slice_text(src, 0, 100), "");
        assert_eq!(byte_slice_text(src, 50, 60), "");
    }

    #[test]
    fn make_remainder_chunk_normalizes_and_skips_empty() {
        let empty: &[u8] = b"   \n\n   ";
        assert!(make_remainder_chunk(empty, &[], "f.txt", &[]).is_none());

        let src: &[u8] = b"left\n  \n\n\n   right\n";
        let chunk = make_remainder_chunk(src, &[], "f.txt", &[]).unwrap();
        assert_eq!(chunk.r#type, "file_remainder");
        assert!(chunk.code.contains("left"));
        assert!(chunk.code.contains("right"));
    }

    #[test]
    fn generate_chunk_id_format() {
        let id = generate_chunk_id("/a/b/f.txt", "my_func");
        assert!(id.starts_with("f.txt-my_func-"));
        assert_eq!(id.len(), "f.txt-my_func-".len() + 8);
    }

    #[test]
    fn strip_js_import_source_handles_quotes() {
        assert_eq!(strip_js_import_source("\"foo\""), "foo");
        assert_eq!(strip_js_import_source("'foo'"), "foo");
        assert_eq!(strip_js_import_source("`foo`"), "foo");
        assert_eq!(strip_js_import_source("foo"), "foo");
    }
}
