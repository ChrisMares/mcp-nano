//! Document loaders for non-code uploads. Direct port of the per-extension
//! dispatch in `embed_document.load_document`.
//!
//! Supported formats:
//! - `.txt` / unknown — read as UTF-8 text.
//! - `.md` — read raw (text-splitter handles it).
//! - `.csv` — one chunk per row pair (mirrors langchain `CSVLoader`'s
//!   "row_num: <text>" output).
//! - `.json` — pretty-printed as a single chunk.
//! - `.xml` — `<tag>` text content joined with spaces.
//! - `.html` / `.htm` — body text via `scraper` (noise tags stripped).
//! - `.odt` — unzip + parse `content.xml` via `quick-xml`.
//! - `.xlsx` / `.xls` / `.xlsb` / `.ods` — via `calamine`.
//! - `.pdf` — `pdf-extract`.
//! - `.docx` / `.doc` — `docx-lite`.
//!
//! OCR (`.png`/`.jpg`/`.jpeg`) is intentionally skipped per the rewrite
//! plan; image files don't reach this loader when called from the
//! ingestion service.
//!
//! CHM is intentionally not implemented per user request.

use std::path::Path;

use serde_json::{Map, Value};

use super::types::DocumentChunk;

/// Load a single file into one or more `DocumentChunk`s. The caller is
/// responsible for splitting with the `text-splitter` if it returns a single
/// large chunk. Mirrors `embed_documents.load_document` (without the
/// per-page metadata stamped by `PyPDFLoader` — `pdf-extract` doesn't expose
/// page boundaries).
pub fn load_document(path: &Path) -> anyhow::Result<Vec<DocumentChunk>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let doc_type = if ext.is_empty() { "txt".to_string() } else { ext.clone() };

    match ext.as_str() {
        "txt" | "" => load_text(path, &file_name, &doc_type),
        "md" => load_text(path, &file_name, "md"),
        "csv" => load_csv(path, &file_name),
        "json" => load_json(path, &file_name),
        "xml" => load_xml(path, &file_name),
        "htm" | "html" => load_html(path, &file_name),
        "odt" => load_odt(path, &file_name),
        "xlsx" | "xls" | "xlsb" | "ods" => load_spreadsheet(path, &file_name, &doc_type),
        "pdf" => load_pdf(path, &file_name),
        "docx" | "doc" => load_docx(path, &file_name),
        other => {
            // Fallback: treat unknown extensions as text.
            load_text(path, &file_name, other)
        }
    }
}

fn one_chunk(file_name: &str, doc_type: &str, content: String) -> Vec<DocumentChunk> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    vec![DocumentChunk::new(uuid::Uuid::new_v4().to_string(), file_name, content, doc_type, 0)]
}

fn load_text(path: &Path, file_name: &str, doc_type: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(one_chunk(file_name, doc_type, text))
}

fn load_csv(path: &Path, file_name: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)?;
    let headers = reader.headers()?.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut chunks = Vec::new();
    let mut idx = 0i64;
    for record in reader.records() {
        let Ok(record) = record else { continue };
        let row: Vec<String> = record.iter().map(|s| s.to_string()).collect();
        let lines: Vec<String> = headers
            .iter()
            .enumerate()
            .zip(row.iter())
            .map(|((i, h), v)| format!("{i}: {h}: {v}"))
            .collect();
        let content = format!("row {}: \n{}", idx + 1, lines.join("\n"));
        let mut chunk = DocumentChunk::new(uuid::Uuid::new_v4().to_string(), file_name, content, "csv", idx);
        chunk.metadata.insert("row".into(), Value::Number((idx + 1).into()));
        chunks.push(chunk);
        idx += 1;
    }
    Ok(chunks)
}

fn load_json(path: &Path, file_name: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let bytes = std::fs::read(path)?;
    let value: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
        // If parsing fails, just store the raw text.
        Value::String(String::from_utf8_lossy(&bytes).into_owned())
    });
    let pretty = serde_json::to_string_pretty(&value)?;
    Ok(one_chunk(file_name, "json", pretty))
}

fn load_xml(path: &Path, file_name: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let bytes = std::fs::read(path)?;
    let text = extract_xml_text(&bytes);
    Ok(one_chunk(file_name, "xml", text))
}

fn extract_xml_text(bytes: &[u8]) -> String {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    // quick-xml 0.41 exposes `Reader::from_reader` (for `BufRead` types) or
    // `Reader::from_str` (for `&str`). Both are usable here; from_reader is
    // the path-of-least-resistance for `&[u8]` (a `&[u8]` implements
    // `BufRead`).
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut text = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(e)) => {
                let s = e.decode().map(|c| c.into_owned()).unwrap_or_default();
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(trimmed);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    text
}

fn load_html(path: &Path, file_name: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let bytes = std::fs::read(path)?;
    let html = String::from_utf8_lossy(&bytes).into_owned();
    let document = scraper::Html::parse_document(&html);

    // Prefer <main> / <article> / <body>; otherwise the whole document.
    let container_sel = scraper::Selector::parse("main, article, body").unwrap();
    let container = document.select(&container_sel).next();

    // Walk leaf-text tags (paragraph-like and headings). Mirrors the
    // website crawler's leaf-text set, expanded with `code` for inline code
    // content so it's preserved.
    let leaf_sel = scraper::Selector::parse(
        "h1, h2, h3, h4, h5, h6, p, li, td, th, blockquote, figcaption, dd, dt, caption, summary, pre, code",
    )
    .unwrap();

    let mut text = String::new();
    if let Some(container) = container {
        for el in container.select(&leaf_sel) {
            // Skip if the element is *inside* a noise-tag subtree.
            let mut is_in_noise = false;
            let mut ancestor = el.parent();
            while let Some(p) = ancestor {
                if let scraper::node::Node::Element(e) = p.value() {
                    let n = e.name();
                    if matches!(n, "script" | "style" | "nav" | "footer" | "header" | "aside") {
                        is_in_noise = true;
                        break;
                    }
                }
                ancestor = p.parent();
            }
            if is_in_noise {
                continue;
            }
            let s: String = el.text().collect::<String>().trim().to_string();
            if s.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&s);
        }
    }
    Ok(one_chunk(file_name, "html", text))
}

fn load_odt(path: &Path, file_name: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let content_idx = archive
        .file_names()
        .position(|n| n == "content.xml")
        .ok_or_else(|| anyhow::anyhow!("odt missing content.xml"))?;
    let mut entry = archive.by_index(content_idx)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)?;
    let text = extract_xml_text(&bytes);
    Ok(one_chunk(file_name, "odt", text))
}

fn load_spreadsheet(path: &Path, file_name: &str, doc_type: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    use calamine::{open_workbook_auto, Data, Reader};

    let mut workbook = open_workbook_auto(path)
        .map_err(|e| anyhow::anyhow!("opening spreadsheet: {e}"))?;
    let mut chunks = Vec::new();
    let mut idx = 0i64;
    let sheet_names = workbook.sheet_names().to_vec();
    for sheet_name in sheet_names {
        let Ok(range) = workbook.worksheet_range(&sheet_name) else { continue };
        let mut rendered = String::new();
        rendered.push_str(&format!("Sheet: {sheet_name}\n"));
        let mut row_idx = 0usize;
        for row in range.rows() {
            let cells: Vec<String> = row
                .iter()
                .map(|c| match c {
                    Data::Int(i) => i.to_string(),
                    Data::Float(f) => f.to_string(),
                    Data::String(s) => s.clone(),
                    Data::DateTime(d) => d.to_string(),
                    Data::DurationIso(s) => s.clone(),
                    Data::DateTimeIso(s) => s.clone(),
                    Data::Bool(b) => b.to_string(),
                    Data::Error(e) => format!("ERROR:{e}"),
                    Data::Empty => String::new(),
                })
                .collect();
            rendered.push_str(&cells.join("\t"));
            rendered.push('\n');
            row_idx += 1;
        }
        if !rendered.trim().is_empty() {
            let mut chunk = DocumentChunk::new(uuid::Uuid::new_v4().to_string(), file_name, rendered, doc_type, idx);
            chunk.metadata.insert("sheet".into(), Value::String(sheet_name.clone()));
            chunks.push(chunk);
            idx += 1;
        }
    }
    Ok(chunks)
}

fn load_pdf(path: &Path, file_name: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| anyhow::anyhow!("pdf-extract failed: {e}"))?;
    Ok(one_chunk(file_name, "pdf", text))
}

fn load_docx(path: &Path, file_name: &str) -> anyhow::Result<Vec<DocumentChunk>> {
    let text = docx_lite::extract_text(path)
        .map_err(|e| anyhow::anyhow!("extracting docx: {e}"))?;
    Ok(one_chunk(file_name, "docx", text))
}

/// Convert a list of language-aware code chunks to `DocumentChunk`s using
/// the loaded text-splitter for further token-based sizing. Wraps
/// `code_chunker::code_chunks_to_document_chunks` so callers don't need to
/// import that submodule.
pub fn from_code_chunks(code_chunks: Vec<super::types::CodeChunk>) -> Vec<DocumentChunk> {
    super::code_chunker::code_chunks_to_document_chunks(code_chunks)
}

#[allow(dead_code)]
fn _unused_metadata(_m: &Map<String, Value>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_text_reads_utf8_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foo.txt");
        std::fs::write(&path, "hello world").unwrap();
        let chunks = load_document(&path).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "hello world");
        assert_eq!(chunks[0].doc_type, "txt");
    }

    #[test]
    fn load_csv_produces_one_chunk_per_row() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.csv");
        std::fs::write(&path, "name,age\nAlice,30\nBob,40").unwrap();
        let chunks = load_document(&path).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("Alice"));
        assert!(chunks[1].content.contains("Bob"));
        assert_eq!(chunks[0].doc_type, "csv");
    }

    #[test]
    fn load_json_pretty_prints() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        std::fs::write(&path, r#"{"a": 1, "b": 2}"#).unwrap();
        let chunks = load_document(&path).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("\"a\": 1"));
    }

    #[test]
    fn load_xml_strips_tags() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.xml");
        std::fs::write(&path, "<root><a>hello</a><b>world</b></root>").unwrap();
        let chunks = load_document(&path).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("hello"));
        assert!(chunks[0].content.contains("world"));
    }
}