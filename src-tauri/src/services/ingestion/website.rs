//! Website crawler + section-aware scraper. Direct port of
//! `embedding/website_embedder/website_embedder.py`.
//!
//! Exposed entry points:
//! - [`crawl_website`]: BFS crawl, returns the list of visited URLs.
//! - [`scrape_website`]: fetch one page, returns `(title, sections)`.
//! - [`process_website`]: combine the two → produce `DocumentChunk`s ready
//!   for embedding.
//!
//! Default limits match the Python constants:
//! - `HEADERS`: a desktop UA
//! - `REQUEST_TIMEOUT = 3s`
//! - `CRAWL_DELAY = 100ms`
//! - `MERGE_MIN_TOKENS = 512`
//! - `MAX_PAGES = 200` (env-configurable via `WEBSITE_CRAWL_MAX_PAGES`)

use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value};
use text_splitter::TextSplitter;
use tokenizers::Tokenizer;
use url::Url;

use super::types::DocumentChunk;

/// Live crawl progress pushed to the UI while `crawl_website` runs.
#[derive(Debug, Clone, Serialize)]
pub struct CrawlProgressEvent {
    /// URL currently being fetched, or the URL just found / finished.
    pub url: String,
    /// `fetching` | `found` | `done`
    pub phase: String,
    /// Pages successfully crawled so far.
    pub found_count: usize,
}

/// Optional callback invoked from crawl workers (may be concurrent).
pub type CrawlProgressCallback = Arc<dyn Fn(CrawlProgressEvent) + Send + Sync>;

const REQUEST_TIMEOUT_SECS: u64 = 3;
const CRAWL_DELAY_MS: u64 = 100;
const MERGE_MIN_TOKENS: usize = 512;
const MIN_ANCHOR_LEN: usize = 4;

/// Domains that social sharing / analytics / app-store pages live on which
/// we refuse to crawl. Direct port of `IGNORED_DOMAINS`.
const IGNORED_DOMAINS: &[&str] = &[
    "facebook.com", "fb.com", "instagram.com", "threads.net", "twitter.com",
    "x.com", "t.co", "linkedin.com", "youtube.com", "youtu.be", "tiktok.com",
    "reddit.com", "redd.it", "snapchat.com", "pinterest.com", "pin.it",
    "tumblr.com", "twitch.tv", "discord.com", "discord.gg", "telegram.org",
    "t.me", "whatsapp.com", "wa.me", "weibo.com", "vk.com", "mail.google.com",
    "gmail.com", "outlook.live.com", "outlook.com", "mail.yahoo.com",
    "protonmail.com", "proton.me", "zendesk.com", "freshdesk.com",
    "freshservice.com", "helpscout.net", "helpscoutdocs.com", "intercom.com",
    "intercom.io", "statuspage.io", "atlassian.net", "google-analytics.com",
    "googletagmanager.com", "doubleclick.net", "facebook.net", "hotjar.com",
    "clarity.ms", "apps.apple.com", "play.google.com", "addthis.com",
    "sharethis.com", "disqus.com",
];

/// Tag names that emit a leaf text section.
const LEAF_TEXT_TAGS: &[&str] = &[
    "p", "li", "h3", "h4", "h5", "h6", "td", "th", "blockquote", "figcaption",
    "dd", "dt", "caption", "summary",
];

/// Maximum pages crawled in a single `crawl_website` call. Mirrors Python
/// `WEBSITE_CRAWL_MAX_PAGES` (default 200).
fn max_pages() -> usize {
    std::env::var("WEBSITE_CRAWL_MAX_PAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

/// Default headers applied to every outbound request.
fn headers() -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    let _ = map.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        ),
    );
    map
}

fn is_ignored_domain(host: &str) -> bool {
    let host = host.to_lowercase();
    for d in IGNORED_DOMAINS {
        if host == *d || host.ends_with(&format!(".{d}")) {
            return true;
        }
    }
    false
}

fn same_domain(url: &Url, base: &str) -> bool {
    if let Some(host) = url.host_str() {
        let h = host.to_lowercase();
        return h == base || h.ends_with(&format!(".{base}"));
    }
    false
}

/// Canonical form used for visited/seen/results identity.
/// Strips fragment and trailing slashes (except `scheme://host` root stays valid).
fn canonical_url(url: &Url) -> String {
    let mut u = url.clone();
    u.set_fragment(None);
    let mut s = u.to_string();
    while s.ends_with('/') {
        s.pop();
    }
    s
}

/// True when the last path segment looks like a file (`name.ext`).
fn path_looks_like_file(url: &Url) -> bool {
    let path = url.path();
    let last = path.rsplit('/').next().unwrap_or("");
    !last.is_empty() && last.contains('.')
}

/// URL string used for the actual HTTP GET. Directory-like paths keep a
/// trailing slash so relative joins on the response and strict static
/// servers (`/docs/` only) both work.
fn fetch_url_string(url: &Url) -> String {
    if path_looks_like_file(url) || url.path().ends_with('/') {
        return url.to_string();
    }
    let mut u = url.clone();
    let path = u.path().to_string();
    if path.is_empty() {
        u.set_path("/");
    } else {
        u.set_path(&format!("{path}/"));
    }
    u.to_string()
}

/// Resolve `href` against the document base URL, then canonicalize.
///
/// Important: `base` must be the document URL *as the browser sees it*
/// (typically `response.url()` after redirects, which preserves a trailing
/// slash on directory indexes). Joining relative paths like `symbols/X.html`
/// against `https://example.com/api` (no slash) incorrectly yields
/// `https://example.com/symbols/X.html` per URL RFC 3986.
pub fn normalize_url(href: &str, base: &Url) -> Option<String> {
    let joined = base.join(href).ok()?;
    if !matches!(joined.scheme(), "http" | "https") {
        return None;
    }
    Some(canonical_url(&joined))
}

/// BFS-crawl a website starting from `start_url`, returning the list of
/// discovered HTML URLs. Direct port of `crawl_website`.
///
/// Uses a tokio task set capped at 10 concurrent fetches, with a 100ms
/// inter-fetch delay. Skips non-HTML responses, ignored domains, and (when
/// `same_domain_only`) off-domain pages.
///
/// When `on_progress` is set, emits `fetching` / `found` / `done` events so
/// the UI can show the live URL list (same idea as zip file embedding status).
pub async fn crawl_website(
    start_url: &str,
    depth: usize,
    same_domain_only: bool,
    on_progress: Option<CrawlProgressCallback>,
) -> anyhow::Result<Vec<String>> {
    let start = Url::parse(start_url.trim())
        .map_err(|e| anyhow::anyhow!("invalid URL {start_url:?}: {e}"))?;
    let base_domain = start
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("no domain in {start_url}"))?
        .to_lowercase();
    let start_key = canonical_url(&start);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .default_headers(headers())
        .build()?;
    let max_pages = max_pages();

    let visited: std::sync::Arc<tokio::sync::Mutex<HashSet<String>>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(HashSet::new()));
    let seen: std::sync::Arc<tokio::sync::Mutex<HashSet<String>>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(HashSet::from([start_key.clone()])));
    let results: std::sync::Arc<tokio::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));

    // Queue stores the fetch URL string (directory paths keep trailing slash) + depth.
    let queue: std::sync::Arc<tokio::sync::Mutex<VecDeque<(String, usize)>>> = std::sync::Arc::new(
        tokio::sync::Mutex::new(VecDeque::from([(fetch_url_string(&start), 0)])),
    );

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
    let mut handles = Vec::new();

    loop {
        let item = {
            let mut q = queue.lock().await;
            q.pop_front()
        };
        let Some((url_str, cur_depth)) = item else {
            // No items in queue; check if any tasks still running, otherwise
            // we're done.
            let active = semaphore.available_permits();
            if active == 10 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };

        let visit_key = {
            if let Ok(u) = Url::parse(&url_str) {
                canonical_url(&u)
            } else {
                url_str.trim_end_matches('/').to_string()
            }
        };
        {
            let mut v = visited.lock().await;
            if v.contains(&visit_key) {
                continue;
            }
            if v.len() >= max_pages {
                break;
            }
            v.insert(visit_key.clone());
        }

        tokio::time::sleep(Duration::from_millis(CRAWL_DELAY_MS)).await;

        let permit = semaphore.clone().acquire_owned().await?;
        let client = client.clone();
        let queue = queue.clone();
        let seen = seen.clone();
        let results = results.clone();
        let base_domain_clone = base_domain.clone();
        let on_progress = on_progress.clone();

        if let Some(cb) = on_progress.as_ref() {
            let found_count = results.lock().await.len();
            cb(CrawlProgressEvent {
                url: url_str.clone(),
                phase: "fetching".into(),
                found_count,
            });
        }

        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let resp = client.get(&url_str).send().await;
            let resp = match resp {
                Ok(r) => r,
                Err(_) => return,
            };
            if !resp.status().is_success() {
                return;
            }
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            if !content_type.contains("text/html") {
                return;
            }
            // Final URL after redirects is the correct join base (preserves
            // trailing slash on directory indexes like /api/ → /api/).
            let document_base = resp.url().clone();
            let result_key = canonical_url(&document_base);
            let body = match resp.text().await {
                Ok(t) => t,
                Err(_) => return,
            };

            let found_count = {
                let mut r = results.lock().await;
                if !r.iter().any(|u| u == &result_key) {
                    r.push(result_key.clone());
                }
                r.len()
            };
            if let Some(cb) = on_progress.as_ref() {
                cb(CrawlProgressEvent {
                    url: result_key,
                    phase: "found".into(),
                    found_count,
                });
            }

            if cur_depth < depth {
                let found: Vec<String> = {
                    let doc = scraper::Html::parse_document(&body);
                    let mut out: Vec<String> = Vec::new();
                    let sel = scraper::Selector::parse("a[href]").unwrap();
                    for el in doc.select(&sel) {
                        if let Some(href) = el.value().attr("href") {
                            if let Some(nu) = normalize_url(href, &document_base) {
                                if let Ok(parsed) = Url::parse(&nu) {
                                    let nu_host = parsed.host_str().unwrap_or("").to_lowercase();
                                    if is_ignored_domain(&nu_host) {
                                        continue;
                                    }
                                    if same_domain_only && !same_domain(&parsed, &base_domain_clone)
                                    {
                                        continue;
                                    }
                                }
                                out.push(nu);
                            }
                        }
                    }
                    out
                };
                let mut s = seen.lock().await;
                let mut q = queue.lock().await;
                for nu in found {
                    if !s.contains(&nu) {
                        s.insert(nu.clone());
                        let fetch = Url::parse(&nu)
                            .map(|u| fetch_url_string(&u))
                            .unwrap_or(nu);
                        q.push_back((fetch, cur_depth + 1));
                    }
                }
            }
        }));
    }

    for h in handles {
        let _ = h.await;
    }
    let r = results.lock().await;
    let urls = r.clone();
    if let Some(cb) = on_progress.as_ref() {
        cb(CrawlProgressEvent {
            url: String::new(),
            phase: "done".into(),
            found_count: urls.len(),
        });
    }
    Ok(urls)
}

/// One scraped section of an HTML page. Sections are keyed by the heading
/// path at the point of extraction (`h1 > h2`), and carry the leaf text +
/// pre-block code samples + anchor links discovered inside.
#[derive(Debug, Clone, Default)]
pub struct WebSection {
    pub heading_path: Vec<String>,
    pub content: String,
    pub code_blocks: Vec<CodeBlock>,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone)]
pub struct CodeBlock {
    pub raw: String,
    pub language: String,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub href: String,
    pub anchor_text: String,
}

/// Fetch + sectionize a single page. Direct port of `scrape_website`.
pub async fn scrape_website(url: &str) -> anyhow::Result<(String, Vec<WebSection>)> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .default_headers(headers())
        .build()?;
    let resp = client.get(url).send().await.map_err(|e| anyhow::anyhow!("fetching {url}: {e}"))?;
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if !status.is_success() {
        anyhow::bail!("failed to load page: HTTP {status}");
    }
    if !content_type.contains("text/html") {
        anyhow::bail!("page is not HTML (Content-Type: {content_type})");
    }
    let body = resp.text().await.map_err(|e| anyhow::anyhow!("reading body: {e}"))?;
    Ok(scrape_html(&body))
}

/// Sectionize an HTML body without fetching it. Direct port of `_walk_dom`.
///
/// Iterates `h1, h2, pre, p, li, h3, h4, h5, h6, td, th, blockquote,
/// figcaption, dd, dt, caption, summary` elements in document order,
/// maintaining a small walker state keyed on the most recent h1/h2.
pub fn scrape_html(html: &str) -> (String, Vec<WebSection>) {
    let document = scraper::Html::parse_document(html);
    let title_selector = scraper::Selector::parse("title").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    // Find <main> or <article> or <body>; otherwise nothing.
    let container_sel = scraper::Selector::parse("main, article, body").unwrap();
    let Some(container) = document.select(&container_sel).next() else {
        return (title, Vec::new());
    };

    // Combined selector for everything we walk: h1/h2 boundaries + leaf text
    // tags + pre code blocks.
    let combined_sel_str = format!(
        "h1, h2, pre, {}",
        LEAF_TEXT_TAGS.join(", ")
    );
    let combined_sel = scraper::Selector::parse(&combined_sel_str).unwrap();
    let links_sel = scraper::Selector::parse("a[href]").unwrap();

    let mut current_h1 = String::new();
    let mut current_h2 = String::new();
    let mut raw: Vec<RawSection> = Vec::new();

    for elem in container.select(&combined_sel) {
        if is_in_noise(&elem) || has_selected_text_descendant(&elem, &combined_sel) {
            continue;
        }
        let tag = elem.value().name();
        if tag == "h1" {
            current_h1 = elem.text().collect::<String>().trim().to_string();
            current_h2 = String::new();
            raw.push(RawSection::with(&current_h1, &current_h2));
            continue;
        }
        if tag == "h2" {
            current_h2 = elem.text().collect::<String>().trim().to_string();
            raw.push(RawSection::with(&current_h1, &current_h2));
            continue;
        }
        if raw.is_empty() {
            raw.push(RawSection::with(&current_h1, &current_h2));
        }
        let last = raw.last_mut().unwrap();

        if tag == "pre" {
            let raw_text: String = elem.text().collect();
            let language = detect_code_language(&elem);
            last.code_blocks.push((raw_text, language));
            continue;
        }

        // leaf-text tag: collect text and any nested anchor links.
        let text: String = elem
            .text()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !text.is_empty() {
            last.content.push_str(&text);
            last.content.push('\n');
        }
        for a in elem.select(&links_sel) {
            let anchor_text = a.text().collect::<String>().trim().to_string();
            if anchor_text.len() < MIN_ANCHOR_LEN {
                continue;
            }
            if let Some(href) = a.value().attr("href") {
                let href = href.trim();
                if !href.is_empty() && href != "#" {
                    last.links.push((href.to_string(), anchor_text));
                }
            }
        }
    }

    let mut sections: Vec<WebSection> = Vec::new();
    for s in raw {
        let content = s.content.trim().to_string();
        if content.is_empty() && s.code_blocks.is_empty() {
            continue;
        }
        let mut heading_path: Vec<String> = Vec::new();
        if !s.h1.is_empty() {
            heading_path.push(s.h1.clone());
        }
        if !s.h2.is_empty() {
            heading_path.push(s.h2.clone());
        }
        if heading_path.is_empty() && !title.is_empty() {
            heading_path.push(title.clone());
        }
        sections.push(WebSection {
            heading_path,
            content,
            code_blocks: s.code_blocks.into_iter().map(|(raw, lang)| CodeBlock { raw, language: lang }).collect(),
            links: s.links.into_iter().map(|(h, a)| Link { href: h, anchor_text: a }).collect(),
        });
    }

    (title, sections)
}

#[derive(Default)]
struct RawSection {
    h1: String,
    h2: String,
    content: String,
    code_blocks: Vec<(String, String)>,
    links: Vec<(String, String)>,
}

impl RawSection {
    fn with(h1: &str, h2: &str) -> Self {
        Self {
            h1: h1.to_string(),
            h2: h2.to_string(),
            content: String::new(),
            code_blocks: Vec::new(),
            links: Vec::new(),
        }
    }
}

fn is_in_noise(elem: &scraper::ElementRef<'_>) -> bool {
    let mut current = Some(*elem);
    while let Some(node) = current {
        if matches!(node.value().name(), "script" | "style" | "nav" | "footer" | "header" | "aside") {
            return true;
        }
        current = node.parent().and_then(scraper::ElementRef::wrap);
    }
    false
}

fn has_selected_text_descendant(elem: &scraper::ElementRef<'_>, selector: &scraper::Selector) -> bool {
    elem.select(selector).any(|child| child.id() != elem.id())
}

fn language_from_element(elem: &scraper::node::Element) -> Option<String> {
    elem.attr("class")
        .unwrap_or("")
        .split_whitespace()
        .find_map(|c| c.strip_prefix("language-").map(str::to_string))
}

fn detect_code_language(elem: &scraper::ElementRef<'_>) -> String {
    language_from_element(elem.value()).or_else(|| {
        scraper::Selector::parse("code")
            .ok()
            .and_then(|selector| elem.select(&selector).next())
            .and_then(|code| language_from_element(code.value()))
    }).unwrap_or_default()
}

/// Build a `website_key` JSON serialization used in the DocumentChunk
/// metadata. Mirrors `website_metadata.make_website_key`.
pub fn make_website_key(url: &str, group: &str, embedded_at: &str) -> String {
    serde_json::json!([url, group, embedded_at]).to_string()
}

/// Merge small chunks below `min_tokens` into bigger ones. Direct port of
/// `_merge_small_chunks`.
pub fn merge_small_chunks(chunks: Vec<String>, min_tokens: usize) -> Vec<String> {
    if chunks.is_empty() {
        return Vec::new();
    }
    let mut merged = Vec::new();
    let mut buffer: Vec<String> = Vec::new();
    for chunk in chunks {
        buffer.push(chunk.clone());
        let combined = buffer.join("\n");
        if combined.len() >= min_tokens * 4 {
            merged.push(combined);
            buffer.clear();
        }
    }
    if !buffer.is_empty() {
        merged.push(buffer.join("\n"));
    }
    merged
}

/// Build `DocumentChunk`s from a list of URLs. Fetches each, scrapes, and
/// produces prose + code chunks with prev/next id linkage.
/// Direct port of `process_website`.
pub async fn process_website(urls: &[String], metadata: &Map<String, Value>) -> anyhow::Result<Vec<DocumentChunk>> {
    process_website_with_splitter(urls, metadata, None).await
}

/// Production website path. Uses the same tokenizer-backed splitter as local
/// documents, matching the Python TokenTextSplitter behavior.
pub async fn process_website_with_splitter(
    urls: &[String],
    metadata: &Map<String, Value>,
    splitter: Option<&TextSplitter<Tokenizer>>,
) -> anyhow::Result<Vec<DocumentChunk>> {
    let chunk_size: usize = std::env::var("DOC_CHUNK_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(768);
    let chunk_overlap: usize = std::env::var("DOC_CHUNK_OVERLAP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);

    let embedded_at = super::types::now_iso();
    let mut all_chunks: Vec<DocumentChunk> = Vec::new();

    for url in urls {
        let (page_title, sections) = match scrape_website(url).await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("failed to scrape {url}: {e:#}");
                continue;
            }
        };
        if sections.is_empty() {
            continue;
        }
        let page_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();

        // Build a link_map: anchor_text -> href (first occurrence wins).
        let mut link_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for section in &sections {
            for link in &section.links {
                link_map.entry(link.anchor_text.clone()).or_insert_with(|| link.href.clone());
            }
        }

        let mut base_md: Map<String, Value> = metadata.clone();
        base_md.insert("url".into(), Value::String(url.clone()));
        base_md.insert("doc_type".into(), Value::String("website".into()));
        base_md.insert("embedded_at".into(), Value::String(embedded_at.clone()));
        base_md.insert("page_id".into(), Value::String(page_id.clone()));
        base_md.insert("page_title".into(), Value::String(page_title.clone()));
        base_md.insert(
            "website_key".into(),
            Value::String(make_website_key(
                url,
                base_md
                    .get("group")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default"),
                &embedded_at,
            )),
        );

        // Build the full-page text (prose only) and split into chunks.
        let full_page_text: String = sections
            .iter()
            .map(|s| {
                if s.heading_path.is_empty() {
                    s.content.clone()
                } else {
                    format!("{}\n{}", s.heading_path.join(" > "), s.content)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prose_docs = splitter
            .map(|splitter| splitter.chunks(&full_page_text).map(str::to_string).collect())
            .unwrap_or_else(|| simple_chunk(&full_page_text, chunk_size, chunk_overlap));
        let prose_docs = if prose_docs.len() > 1 {
            merge_small_chunks(prose_docs, MERGE_MIN_TOKENS)
        } else {
            prose_docs
        };

        let mut page_chunks: Vec<DocumentChunk> = Vec::new();
        for (idx, c) in prose_docs.iter().enumerate() {
            let mut md = base_md.clone();
            md.insert("heading_path".into(), Value::String(page_title.clone()));
            md.insert("content_type".into(), Value::String("prose".into()));
            page_chunks.push(DocumentChunk {
                id: uuid::Uuid::new_v4().to_string(),
                file_name: url.clone(),
                content: c.clone(),
                doc_type: "website".to_string(),
                chunk_index: idx as i64,
                page: None,
                metadata: md,
                created_at: super::types::now_iso(),
            });
        }

        let mut code_entries: Vec<(String, String, String)> = Vec::new(); // (raw, lang, heading_path)
        for section in &sections {
            let hp = section.heading_path.join(" > ");
            for cb in &section.code_blocks {
                code_entries.push((cb.raw.clone(), cb.language.clone(), hp.clone()));
            }
        }
        for (cb_idx, (raw, lang, hp)) in code_entries.into_iter().enumerate() {
            let mut md = base_md.clone();
            md.insert("heading_path".into(), Value::String(hp));
            md.insert("content_type".into(), Value::String("code".into()));
            if !lang.is_empty() {
                md.insert("language".into(), Value::String(lang));
            }
            page_chunks.push(DocumentChunk {
                id: uuid::Uuid::new_v4().to_string(),
                file_name: url.clone(),
                content: raw,
                doc_type: "website".to_string(),
                chunk_index: prose_docs.len() as i64 + cb_idx as i64,
                page: None,
                metadata: md,
                created_at: super::types::now_iso(),
            });
        }

        if page_chunks.is_empty() {
            continue;
        }

        // Assign chunk_hash + prev/next ids + matched links.
        let total = page_chunks.len() as i64;
        let ids: Vec<String> = page_chunks.iter().map(|c| c.id.clone()).collect();
        use sha2::{Digest, Sha256};
        for (i, c) in page_chunks.iter_mut().enumerate() {
            let mut hasher = Sha256::new();
            hasher.update(c.content.as_bytes());
            let hash_hex = hasher.finalize();
            let chunk_hash = hash_hex.iter().take(8).map(|b| format!("{:02x}", b)).collect::<String>();
            c.metadata
                .insert("chunk_hash".into(), Value::String(chunk_hash));
            c.metadata.insert("total_chunks".into(), Value::Number(total.into()));
            c.metadata.insert(
                "prev_chunk_id".into(),
                if i > 0 {
                    Value::String(ids[i - 1].clone())
                } else {
                    Value::Null
                },
            );
            c.metadata.insert(
                "next_chunk_id".into(),
                if i < ids.len() - 1 {
                    Value::String(ids[i + 1].clone())
                } else {
                    Value::Null
                },
            );
            let matches: Vec<Value> = link_map
                .iter()
                .filter(|(anchor, _)| c.content.contains(anchor.as_str()))
                .map(|(anchor, href)| {
                    serde_json::json!({"href": href, "anchor_text": anchor})
                })
                .collect();
            c.metadata.insert("links".into(), Value::Array(matches));
        }

        all_chunks.extend(page_chunks);
    }

    Ok(all_chunks)
}

/// Approximate the Python `TokenTextSplitter` for website prose. Returns
/// chunks of roughly `chunk_size` "tokens" (approximated as chars / 4) with
/// the given overlap. The real `text-splitter` crate is configured in
/// `IngestionService` and applied during the embed stage; this keeps the
/// crawler decoupled from `Tokenizer` so it can run in tests without model
/// files.
fn simple_chunk(text: &str, chunk_size: usize, _chunk_overlap: usize) -> Vec<String> {
    if chunk_size == 0 {
        return vec![text.to_string()];
    }
    let target_chars = chunk_size * 4;
    if text.len() <= target_chars {
        return vec![text.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        // Never slice mid-codepoint (multibyte pages panicked here before).
        let mut end = (start + target_chars).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            // Single codepoint wider than the window: advance one char.
            end = start + 1;
            while !text.is_char_boundary(end) {
                end += 1;
            }
        }
        // Walk back to the last whitespace if we're mid-word.
        let mut e = end;
        while e > start && e < text.len() && !text.as_bytes()[e].is_ascii_whitespace() {
            e -= 1;
        }
        if e == start {
            e = end;
        }
        chunks.push(text[start..e].to_string());
        start = e;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_html_excludes_noise_and_does_not_duplicate_nested_text() {
        let html = r#"<html><head><title>Guide</title></head><body>
            <header><p>header noise</p></header><nav><p>nav noise</p></nav>
            <main><h1>Guide</h1><ul><li><p>keep once</p></li></ul>
            <pre><code class="language-python">print('hello')</code></pre></main>
            <footer><p>footer noise</p></footer></body></html>"#;
        let (_, sections) = scrape_html(html);
        let content = sections.iter().map(|section| section.content.as_str()).collect::<String>();
        assert!(content.contains("keep once"));
        assert_eq!(content.matches("keep once").count(), 1);
        assert!(!content.contains("header noise"));
        assert!(!content.contains("nav noise"));
        assert!(!content.contains("footer noise"));
        assert_eq!(sections[0].code_blocks[0].language, "python");
    }

    #[test]
    fn normalize_url_joins_relative_against_directory_base() {
        let base = Url::parse("https://gojs.net/latest/api/").unwrap();
        assert_eq!(
            normalize_url("symbols/Adornment.html", &base).as_deref(),
            Some("https://gojs.net/latest/api/symbols/Adornment.html")
        );
    }

    #[test]
    fn normalize_url_strips_fragment_and_trailing_slash() {
        let base = Url::parse("https://example.com/docs/").unwrap();
        assert_eq!(
            normalize_url("page#section", &base).as_deref(),
            Some("https://example.com/docs/page")
        );
        assert_eq!(
            normalize_url("../other/", &base).as_deref(),
            Some("https://example.com/other")
        );
    }

    #[test]
    fn normalize_url_rejects_non_http() {
        let base = Url::parse("https://example.com/").unwrap();
        assert!(normalize_url("mailto:a@b.com", &base).is_none());
        assert!(normalize_url("javascript:void(0)", &base).is_none());
    }

    #[test]
    fn normalize_url_absolute_path_from_directory() {
        let base = Url::parse("https://example.com/base").unwrap();
        assert_eq!(
            normalize_url("/page#section", &base).as_deref(),
            Some("https://example.com/page")
        );
    }

    #[test]
    fn simple_chunk_splits_on_whitespace_when_possible() {
        let text = "word ".repeat(2000); // 10_000 bytes > 768*4 target
        let chunks = simple_chunk(&text, 768, 50);
        assert!(chunks.len() > 1);
        let rejoined: String = chunks.concat();
        assert_eq!(rejoined, text);
        for c in &chunks {
            assert!(c.len() <= 768 * 4, "chunk over target: {}", c.len());
        }
    }

    #[test]
    fn simple_chunk_multibyte_text_does_not_panic_or_lose_content() {
        // CJK text has no ASCII whitespace: the old byte-slicing version
        // panicked on non-char-boundary slices (failing whole website jobs).
        let text = "日本語のテキスト。".repeat(1000);
        let chunks = simple_chunk(&text, 1, 0); // tiny window forces splits
        assert!(chunks.len() > 1);
        let rejoined: String = chunks.concat();
        assert_eq!(rejoined, text);
    }

    #[test]
    fn simple_chunk_emoji_does_not_panic() {
        let text = "🦀".repeat(5000); // 4-byte chars, no whitespace
        let chunks = simple_chunk(&text, 1, 0);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn simple_chunk_short_text_passthrough() {
        assert_eq!(simple_chunk("hello world", 768, 50), vec!["hello world"]);
        assert_eq!(simple_chunk("anything", 0, 0), vec!["anything"]);
    }
}
