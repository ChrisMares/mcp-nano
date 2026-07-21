//! Integration tests for the document loaders. Exercises each format
//! loader against a small committed fixture in `tests/test_data/documents/`.
//!
//! CHM is intentionally not implemented per user request. PDF is omitted
//! here because `pdf-extract` requires a real binary PDF fixture that's
//! hard to generate programmatically; the loader path is exercised on real
//! PDFs via the E2E `ingestion_pipeline.rs` test instead.

use std::path::PathBuf;

use mcp_nano_lib::services::ingestion::document_loaders;

fn doc_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_data/documents")
}

fn doc(file: &str) -> PathBuf {
    doc_dir().join(file)
}

#[test]
fn loads_markdown_as_text() {
    let chunks = document_loaders::load_document(&doc("sample.md")).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].doc_type, "md");
    assert!(chunks[0].content.contains("Markdown Sample"));
    assert!(chunks[0].content.contains("Bullet one"));
}

#[test]
fn loads_csv_one_chunk_per_row() {
    let chunks = document_loaders::load_document(&doc("sample.csv")).unwrap();
    assert_eq!(chunks.len(), 3);
    assert!(chunks[0].content.contains("Alice"));
    assert!(chunks[1].content.contains("Bob"));
    assert!(chunks[2].content.contains("Charlie"));
    for (i, c) in chunks.iter().enumerate() {
        assert_eq!(c.doc_type, "csv");
        assert_eq!(c.chunk_index, i as i64);
    }
}

#[test]
fn loads_json_pretty_printed() {
    let chunks = document_loaders::load_document(&doc("sample.json")).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].doc_type, "json");
    // Pretty-printed output keeps indentation; we get the keys back.
    assert!(chunks[0].content.contains("\"title\""));
    assert!(chunks[0].content.contains("\"items\""));
    assert!(chunks[0].content.contains("\"alpha\""));
}

#[test]
fn loads_xml_returns_text() {
    let chunks = document_loaders::load_document(&doc("sample.xml")).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].doc_type, "xml");
    assert!(chunks[0].content.contains("Sample XML"));
    assert!(chunks[0].content.contains("Alpha"));
    assert!(chunks[0].content.contains("Gamma"));
}

#[test]
fn loads_html_with_noise_tags_stripped() {
    let chunks = document_loaders::load_document(&doc("sample.html")).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].doc_type, "html");
    assert!(chunks[0].content.contains("Alpha Section"));
    assert!(chunks[0].content.contains("Body paragraph"));
    assert!(chunks[0].content.contains("a link"));
    // The header/nav/footer/script content shouldn't appear in the output.
    assert!(!chunks[0].content.contains("Skip me"));
    assert!(!chunks[0].content.contains("console.log"));
    assert!(!chunks[0].content.contains("margin:"));
}

#[test]
fn loads_docx_via_docx_lite() {
    let chunks = document_loaders::load_document(&doc("sample.docx")).unwrap();
    assert!(!chunks.is_empty());
    assert_eq!(chunks[0].doc_type, "docx");
    let combined: String = chunks.iter().map(|c| c.content.as_str()).collect();
    assert!(combined.contains("First paragraph of the sample docx."));
    assert!(combined.contains("Second paragraph with more text."));
}

#[test]
fn loads_xlsx_via_calamine() {
    let chunks = document_loaders::load_document(&doc("sample.xlsx")).unwrap();
    assert!(!chunks.is_empty());
    let combined: String = chunks.iter().map(|c| c.content.as_str()).collect();
    // The shared strings include "Alice", "Bob", "Charlie", "active".
    assert!(combined.contains("Alice"), "combined: {combined}");
    assert!(combined.contains("Charlie"));
}

#[test]
fn loads_odt_via_unzip_and_quick_xml() {
    let chunks = document_loaders::load_document(&doc("sample.odt")).unwrap();
    assert!(!chunks.is_empty());
    let combined: String = chunks.iter().map(|c| c.content.as_str()).collect();
    assert!(combined.contains("first paragraph of the sample ODT"));
    assert!(combined.contains("second paragraph"), "combined: {combined}");
}