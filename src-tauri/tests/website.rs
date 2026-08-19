//! Integration tests for the website crawler + section-aware scraper.
//!
//! - Offline fixture tests exercise `scrape_html` / helpers.
//! - Local HTTP server tests exercise `crawl_website` link discovery and the
//!   directory-trailing-slash join fix without external network.
//! - Live GoJS tests crawl/scrape/embed https://gojs.net/latest/api/ (network).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::response::Html;
use axum::routing::get;
use axum::Router;
use mcp_nano_lib::services::ingestion::website;
use serde_json::Map;
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;

mod common;

fn website_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test_data/website")
}

#[test]
fn scrape_html_extracts_title_and_sections() {
    let sample =
        std::fs::read_to_string(website_dir().join("section_packing_sample.html")).unwrap();
    let (title, sections) = website::scrape_html(&sample);
    assert_eq!(title, "Packing Sample");
    assert!(!sections.is_empty(), "expected at least one section");

    let heading_paths: Vec<String> = sections
        .iter()
        .map(|s| s.heading_path.join(" > "))
        .collect();
    assert!(
        heading_paths.iter().any(|p| p == "Guide"),
        "expected Guide heading: {heading_paths:?}"
    );
    for sub in [
        "Intro",
        "Install",
        "Configure",
        "Execute",
        "Validate",
        "Troubleshoot",
    ] {
        assert!(
            heading_paths.iter().any(|p| p == &format!("Guide > {sub}")),
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
    let chunks: Vec<String> = (0..6).map(|i| format!("chunk-{i} with padding")).collect();
    let out = website::merge_small_chunks(chunks, 1);
    assert!(!out.is_empty(), "expected at least one merged chunk");
}

/// Serve a tiny multi-page site that mirrors the GoJS layout: index lives at a
/// directory URL (`/api/`) and links to relative `symbols/*.html` pages.
async fn spawn_dir_site() -> (String, oneshot::Sender<()>) {
    let index = Html(
        r#"<!doctype html><html><head><title>API Index</title></head>
        <body>
          <h1>API</h1>
          <p>Index of symbols.</p>
          <a href="symbols/Adornment.html">Adornment</a>
          <a href="symbols/Diagram.html">Diagram</a>
          <a href="symbols/Node.html">Node</a>
          <a href="../learn/">Learn</a>
        </body></html>"#,
    );
    let adornment = Html(
        r#"<!doctype html><html><head><title>Adornment</title></head>
        <body><h1>Adornment</h1><p>An Adornment is a special Part used for selection handles.</p></body></html>"#,
    );
    let diagram = Html(
        r#"<!doctype html><html><head><title>Diagram</title></head>
        <body><h1>Diagram</h1><p>A Diagram is a surface for displaying and editing nodes and links.</p></body></html>"#,
    );
    let node = Html(
        r#"<!doctype html><html><head><title>Node</title></head>
        <body><h1>Node</h1><p>A Node is a Part that may have ports and connections.</p></body></html>"#,
    );

    let app = Router::new()
        .route("/latest/api/", get(move || async move { index }))
        .route(
            "/latest/api/symbols/Adornment.html",
            get(move || async move { adornment }),
        )
        .route(
            "/latest/api/symbols/Diagram.html",
            get(move || async move { diagram }),
        )
        .route(
            "/latest/api/symbols/Node.html",
            get(move || async move { node }),
        )
        .route(
            "/latest/learn",
            get(|| async {
                Html("<html><body><h1>Learn</h1><p>Learning materials.</p></body></html>")
            }),
        )
        .route(
            "/latest/learn/",
            get(|| async {
                Html("<html><body><h1>Learn</h1><p>Learning materials.</p></body></html>")
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });
    (format!("http://{addr}"), tx)
}

/// Serve pages listed only in a nested sitemap. The index page intentionally
/// contains no links, proving sitemap discovery supplements HTML BFS crawling.
async fn spawn_sitemap_site() -> (String, oneshot::Sender<()>) {
    let index = Html("<html><body><h1>Index</h1></body></html>");
    let guide = Html("<html><body><h1>Guide</h1><p>Sitemap-only guide.</p></body></html>");
    let reference =
        Html("<html><body><h1>Reference</h1><p>Sitemap-only reference.</p></body></html>");
    let installation =
        Html("<html><body><h1>Installation</h1><p>Extensionless sitemap page.</p></body></html>");
    let blog = Html("<html><body><h1>Blog</h1><p>Blog content.</p></body></html>");
    let docs_guide =
        Html("<html><body><h1>Docs Guide</h1><p>Documentation content.</p></body></html>");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let sitemap_index = format!(
        r#"<?xml version="1.0"?><sitemapindex><sitemap><loc>{origin}/pages.xml</loc></sitemap></sitemapindex>"#
    );
    let pages_sitemap = format!(
        r#"<?xml version="1.0"?><urlset><url><loc>{origin}/guide.html</loc></url><url><loc>{origin}/reference.html</loc></url><url><loc>{origin}/get-started-installation</loc></url></urlset>"#
    );
    let secondary_sitemap = format!(
        r#"<?xml version="1.0"?><urlset><url><loc>{origin}/blog.html</loc></url><url><loc>{origin}/docs/guide.html</loc></url></urlset>"#
    );
    let app = Router::new()
        .route("/", get(move || async move { index }))
        .route("/guide.html", get(move || async move { guide }))
        .route("/reference.html", get(move || async move { reference }))
        .route(
            "/get-started-installation",
            get(move || async move { installation }),
        )
        .route(
            "/sitemap.xml",
            get(move || {
                let sitemap_index = sitemap_index.clone();
                async move { sitemap_index }
            }),
        )
        .route(
            "/pages.xml",
            get(move || {
                let pages_sitemap = pages_sitemap.clone();
                async move { pages_sitemap }
            }),
        )
        .route(
            "/secondary.xml",
            get(move || {
                let secondary_sitemap = secondary_sitemap.clone();
                async move { secondary_sitemap }
            }),
        )
        .route(
            "/robots.txt",
            get(|| async { "Sitemap: /sitemap.xml\nSitemap: /secondary.xml\n" }),
        )
        .route("/blog.html", get(move || async move { blog }))
        .route("/docs/guide.html", get(move || async move { docs_guide }));
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });
    (origin, tx)
}

#[tokio::test]
async fn crawl_website_resolves_relative_links_from_directory_index() {
    let (origin, shutdown) = spawn_dir_site().await;
    let start = format!("{origin}/latest/api/");
    // same_domain_only=false: the `../learn/` link intentionally leaves the
    // /latest/api/ path scope, which the scope filter would drop.
    let urls = website::crawl_website(&start, 1, false, None)
        .await
        .expect("crawl");
    let _ = shutdown.send(());

    let set: HashSet<_> = urls.iter().cloned().collect();
    assert!(
        set.iter().any(|u| u.ends_with("/latest/api")),
        "missing index in {urls:?}"
    );
    for sym in ["Adornment", "Diagram", "Node"] {
        let suffix = format!("/latest/api/symbols/{sym}.html");
        assert!(
            set.iter().any(|u| u.ends_with(&suffix)),
            "missing {suffix} in {urls:?}"
        );
    }
    assert!(
        set.iter().any(|u| u.ends_with("/latest/learn")),
        "missing learn page in {urls:?}"
    );
    assert!(
        urls.len() >= 5,
        "expected index + 3 symbols + learn, got {} ({urls:?})",
        urls.len()
    );
}

#[tokio::test]
async fn crawl_website_discovers_urls_from_nested_sitemap() {
    let (origin, shutdown) = spawn_sitemap_site().await;
    let urls = website::crawl_website(&origin, 0, true, None)
        .await
        .expect("crawl");
    let _ = shutdown.send(());

    let set: HashSet<_> = urls.iter().cloned().collect();
    for path in ["guide.html", "reference.html", "get-started-installation"] {
        let expected = format!("{origin}/{path}");
        assert!(set.contains(&expected), "missing {expected} in {urls:?}");
    }
    assert!(set.contains(&format!("{origin}/blog.html")));
}

#[tokio::test]
async fn crawl_website_current_site_section_excludes_sibling_paths() {
    let (origin, shutdown) = spawn_sitemap_site().await;
    let start = format!("{origin}/docs/");
    let urls = website::crawl_website(&start, 0, true, None)
        .await
        .expect("crawl");
    let _ = shutdown.send(());

    assert!(urls.iter().any(|url| url.ends_with("/docs/guide.html")));
    assert!(!urls.iter().any(|url| url.ends_with("/blog.html")));
}

#[tokio::test]
async fn crawl_website_stops_when_cancelled() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new().route(
        "/",
        get(|| async {
            sleep(Duration::from_secs(5)).await;
            Html("<html><body><h1>Slow page</h1></body></html>")
        }),
    );
    let (shutdown, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    let cancellation = CancellationToken::new();
    let start = format!("http://{addr}/");
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        website::crawl_website_with_options_and_cancellation(
            &start,
            1,
            true,
            false,
            Some(task_cancellation),
            None,
        )
        .await
    });
    sleep(Duration::from_millis(100)).await;
    cancellation.cancel();

    let result = task.await.unwrap();
    let _ = shutdown.send(());
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

/// `process_website` scrapes pages concurrently: 5 pages that each take 400ms
/// must finish well under the 2s a sequential loop would need, and chunks
/// must stay in input order.
#[tokio::test]
async fn process_website_scrapes_pages_concurrently() {
    async fn slow_page() -> Html<&'static str> {
        sleep(Duration::from_millis(400)).await;
        Html(
            "<html><head><title>Slow page</title></head>\
             <body><h1>Slow page</h1><p>Concurrent content.</p></body></html>",
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/p0", get(slow_page))
        .route("/p1", get(slow_page))
        .route("/p2", get(slow_page))
        .route("/p3", get(slow_page))
        .route("/p4", get(slow_page));
    let (shutdown, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    let urls: Vec<String> = (0..5).map(|n| format!("http://{addr}/p{n}")).collect();
    let started = std::time::Instant::now();
    let chunks = website::process_website(&urls, &Map::new())
        .await
        .expect("process website");
    let elapsed = started.elapsed();
    let _ = shutdown.send(());

    assert_eq!(chunks.len(), urls.len(), "expected one chunk per page");
    assert!(
        chunks
            .iter()
            .all(|c| c.content.contains("Concurrent content")),
        "unexpected chunk content"
    );
    for (chunk, url) in chunks.iter().zip(&urls) {
        assert_eq!(chunk.file_name, *url, "chunks must stay in input order");
    }
    assert!(
        elapsed < Duration::from_millis(1500),
        "scraping 5 x 400ms pages took {elapsed:?}; expected concurrent execution"
    );
}

#[tokio::test]
async fn crawl_website_gojs_api_discovers_symbol_pages() {
    std::env::set_var("WEBSITE_CRAWL_MAX_PAGES", "500");
    let start = "https://gojs.net/latest/api/";
    let urls = match website::crawl_website(start, 1, true, None).await {
        Ok(u) => u,
        Err(e) => {
            eprintln!("skipping live gojs crawl: {e:#}");
            return;
        }
    };

    let symbols: Vec<_> = urls
        .iter()
        .filter(|u| u.contains("/api/symbols/") && u.ends_with(".html"))
        .collect();
    assert!(
        symbols.len() >= 200,
        "expected ~225 GoJS symbol pages at depth=1, got {} total urls, {} symbols. sample={:?}",
        urls.len(),
        symbols.len(),
        urls.iter().take(15).collect::<Vec<_>>()
    );
    assert!(
        symbols.iter().any(|u| u.ends_with("/symbols/Diagram.html")),
        "missing Diagram.html in {symbols:?}"
    );
    assert!(
        symbols.iter().any(|u| u.ends_with("/symbols/Node.html")),
        "missing Node.html"
    );
    assert!(
        symbols
            .iter()
            .any(|u| u.ends_with("/symbols/Adornment.html")),
        "missing Adornment.html"
    );
}

#[tokio::test]
async fn scrape_website_gojs_diagram_has_content() {
    let url = "https://gojs.net/latest/api/symbols/Diagram.html";
    let (title, sections) = match website::scrape_website(url).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("skipping live gojs scrape: {e:#}");
            return;
        }
    };
    assert!(
        title.to_lowercase().contains("diagram"),
        "unexpected title: {title:?}"
    );
    assert!(!sections.is_empty(), "expected sections from Diagram page");
    let text: String = sections.iter().map(|s| s.content.as_str()).collect();
    assert!(
        text.to_lowercase().contains("diagram"),
        "Diagram page body missing expected text; sections={sections:?}"
    );
}

#[tokio::test]
async fn process_website_gojs_produces_chunks() {
    let urls = vec![
        "https://gojs.net/latest/api/".to_string(),
        "https://gojs.net/latest/api/symbols/Diagram.html".to_string(),
        "https://gojs.net/latest/api/symbols/Node.html".to_string(),
    ];
    let mut md = Map::new();
    md.insert("group".into(), serde_json::json!("gojs-api-test"));
    let chunks = match website::process_website(&urls, &md).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping live gojs process_website: {e:#}");
            return;
        }
    };
    assert!(
        chunks.len() >= 2,
        "expected chunks from gojs pages, got {}",
        chunks.len()
    );
    assert!(chunks.iter().all(|c| c.doc_type == "website"));
    assert!(chunks.iter().any(|c| {
        c.content.to_lowercase().contains("diagram")
            || c.metadata
                .get("page_title")
                .and_then(|v| v.as_str())
                .is_some_and(|t| t.to_lowercase().contains("diagram"))
    }));
}

/// Full crawl of GoJS API index (depth=1) then scrape+chunk+embed+upsert.
#[tokio::test]
async fn gojs_api_crawl_and_embed_e2e() {
    use common::{create_test_collection, load_embedders, models_dir, spawn_qdrant};
    use mcp_nano_lib::services::embedders::EncodeQuery;
    use mcp_nano_lib::services::ingestion_service::IngestionService;
    use mcp_nano_lib::services::qdrant_service::{Include, QdrantService};
    use mcp_nano_lib::worker::noop_progress;
    use qdrant_client::qdrant::{Condition, Filter};

    std::env::set_var("WEBSITE_CRAWL_MAX_PAGES", "500");

    let Some((qdrant_client, _guard)) = spawn_qdrant().await else {
        eprintln!("skipping: could not spawn qdrant");
        return;
    };
    let Some(embedders) = load_embedders() else {
        eprintln!("skipping: embedder models missing");
        return;
    };

    let collection = "general";
    create_test_collection(&qdrant_client, collection, embedders.dense.dim())
        .await
        .ok();

    let urls = website::crawl_website("https://gojs.net/latest/api/", 1, true, None)
        .await
        .expect("crawl gojs api");
    let symbol_count = urls
        .iter()
        .filter(|u| u.contains("/api/symbols/") && u.ends_with(".html"))
        .count();
    assert!(
        symbol_count >= 200,
        "crawl under-discovered symbol pages: {} symbols / {} urls",
        symbol_count,
        urls.len()
    );

    let qdrant_service = QdrantService::new(qdrant_client.clone());
    let ingest = IngestionService::new(embedders.clone(), qdrant_service, &models_dir())
        .expect("build ingestion service");

    let group = format!("gojs-api-itest-{}", std::process::id());

    // Embed a representative subset so the test finishes in reasonable time,
    // while still proving the crawl discovered the full link set above.
    let embed_urls: Vec<String> = {
        let mut v = vec!["https://gojs.net/latest/api/".to_string()];
        for name in ["Diagram", "Node", "Link", "GraphObject", "Animation"] {
            if let Some(u) = urls
                .iter()
                .find(|u| u.ends_with(&format!("/symbols/{name}.html")))
            {
                v.push(u.clone());
            }
        }
        for u in urls.iter().filter(|u| u.contains("/api/symbols/")).take(10) {
            if !v.contains(u) {
                v.push(u.clone());
            }
        }
        v
    };

    let result = ingest
        .process_website_embed(
            serde_json::json!({
                "urls": embed_urls,
                "group": group,
            }),
            noop_progress(),
        )
        .await
        .expect("embed gojs pages");
    assert!(
        result.to_lowercase().contains("embedded"),
        "unexpected result: {result}"
    );

    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let svc = QdrantService::new(qdrant_client.clone());
    let q_emb = embedders
        .dense
        .encode_query("GoJS Diagram class nodes and links")
        .expect("encode query");
    let filter = Filter::must([Condition::matches("group", group.clone())]);
    let hits = svc
        .query_items(
            collection,
            &q_emb,
            Some("GoJS Diagram class nodes and links"),
            5,
            Some(filter),
            Include::all(),
            Some(&embedders.bm25),
        )
        .await
        .expect("query");
    assert!(!hits.is_empty(), "expected searchable website vectors");
    assert!(
        hits.documents[0].iter().any(|d| {
            let l = d.to_lowercase();
            l.contains("diagram") || l.contains("node") || l.contains("gojs")
        }),
        "unexpected hit docs: {:?}",
        hits.documents[0]
    );

    let cleanup = Filter::must([Condition::matches("group", group)]);
    let _ = svc.delete_items(collection, None, Some(cleanup)).await;
}
