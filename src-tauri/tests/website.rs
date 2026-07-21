//! Integration tests for the website crawler + section-aware scraper.
//! Exercises `scrape_html` (the synchronous sectionizer) against the local
//! `section_packing_sample.html` fixture; the network-bound `crawl_website`
//! is intentionally not exercised here per the user's instruction.

use std::path::PathBuf;

use mcp_nano_lib::services::ingestion::website;

fn website_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_data/website")
}

#[test]
fn scrape_html_extracts_title_and_sections() {
    let sample = std::fs::read_to_string(website_dir().join("section_packing_sample.html")).unwrap();
    let (title, sections) = website::scrape_html(&sample);
    assert_eq!(title, "Packing Sample");
    assert!(!sections.is_empty(), "expected at least one section");

    // Verify h1/h2 boundaries create section breaks at expected positions.
    let heading_paths: Vec<String> = sections
        .iter()
        .map(|s| s.heading_path.join(" > "))
        .collect();
    // The first section inherits the h1 "Guide"; subsequent sections get
    // h1 + h2 headings.
    assert!(
        heading_paths.iter().any(|p| p == "Guide"),
        "expected Guide heading: {heading_paths:?}"
    );
    for sub in ["Intro", "Install", "Configure", "Execute", "Validate", "Troubleshoot"] {
        assert!(
            heading_paths
                .iter()
                .any(|p| p == &format!("Guide > {sub}")),
            "expected Guide > {sub} heading: {heading_paths:?}"
        );
    }
}

#[test]
fn scrape_html_collects_section_content() {
    let sample =
        std::fs::read_to_string(website_dir().join("section_packing_sample.html")).unwrap();
    let (_title, sections) = website::scrape_html(&sample);
    let intro = sections
        .iter()
        .find(|s| s.heading_path.iter().any(|h| h == "Intro"))
        .expect("missing Intro section");
    assert!(
        intro.content.contains("intro explains the purpose"),
        "Intro section content missing: {:?}",
        intro
    );
}

#[test]
fn make_website_key_serializes_compactly() {
    let key = website::make_website_key("https://example.com/", "docs", "2026-01-02T03:04:05Z");
    // Mirrors the Python JSON list `["url", "group", "embedded_at"]` with
    // no surrounding whitespace.
    assert_eq!(
        key,
        r#"["https://example.com/","docs","2026-01-02T03:04:05Z"]"#
    );
}

#[test]
fn merge_small_chunks_keeps_passthrough_for_single_chunk() {
    let out = website::merge_small_chunks(vec!["short".to_string()], 512);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0], "short");
}

#[test]
fn merge_small_chunks_combines_below_threshold() {
    // Each chunk is below the threshold (512 * 4 = 2048 chars). The first
    // few should be combined into a single chunk that reaches the limit,
    // leaving the rest as the final merge.
    let chunks: Vec<String> = (0..6).map(|i| format!("chunk-{i} with padding")).collect();
    let out = website::merge_small_chunks(chunks, 1);
    assert!(!out.is_empty(), "expected at least one merged chunk");
}