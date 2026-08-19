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
//! - `MAX_PAGES = 1000` (env-configurable via `WEBSITE_CRAWL_MAX_PAGES`)

use std::collections::HashSet;
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use futures_util::stream::StreamExt;
use serde::Serialize;
use serde_json::{Map, Value};
use text_splitter::TextSplitter;
use tokenizers::Tokenizer;
use tokio_util::sync::CancellationToken;
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
/// Concurrent page fetches during a crawl.
const CRAWL_CONCURRENCY: usize = 10;
const MERGE_MIN_TOKENS: usize = 512;
const MIN_ANCHOR_LEN: usize = 4;

/// Domains that social sharing / analytics / app-store pages live on which
/// the crawler intentionally skips.
const IGNORED_DOMAINS: &[&str] = &[
    "facebook.com",
    "fb.com",
    "instagram.com",
    "threads.net",
    "twitter.com",
    "x.com",
    "t.co",
    "linkedin.com",
    "youtube.com",
    "youtu.be",
    "tiktok.com",
    "reddit.com",
    "redd.it",
    "snapchat.com",
    "pinterest.com",
    "pin.it",
    "tumblr.com",
    "twitch.tv",
    "discord.com",
    "discord.gg",
    "telegram.org",
    "t.me",
    "whatsapp.com",
    "wa.me",
    "weibo.com",
    "vk.com",
    "mail.google.com",
    "gmail.com",
    "outlook.live.com",
    "outlook.com",
    "mail.yahoo.com",
    "protonmail.com",
    "proton.me",
    "zendesk.com",
    "freshdesk.com",
    "freshservice.com",
    "helpscout.net",
    "helpscoutdocs.com",
    "intercom.com",
    "intercom.io",
    "statuspage.io",
    "atlassian.net",
    "google-analytics.com",
    "googletagmanager.com",
    "doubleclick.net",
    "facebook.net",
    "hotjar.com",
    "clarity.ms",
    "apps.apple.com",
    "play.google.com",
    "addthis.com",
    "sharethis.com",
    "disqus.com",
];
/// Tag names that emit a leaf text section.
const LEAF_TEXT_TAGS: &[&str] = &[
    "p",
    "li",
    "h3",
    "h4",
    "h5",
    "h6",
    "td",
    "th",
    "blockquote",
    "figcaption",
    "dd",
    "dt",
    "caption",
    "summary",
];

/// Maximum pages fetched in a single crawl. Sitemap discovery is not limited
/// by this value, so a large sitemap remains fully selectable in the UI.
fn max_pages() -> usize {
    std::env::var("WEBSITE_CRAWL_MAX_PAGES")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(|value| if value == 0 { usize::MAX } else { value })
        .unwrap_or(1000)
}

fn max_sitemaps() -> usize {
    std::env::var("WEBSITE_CRAWL_MAX_SITEMAPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(512)
}

fn max_sitemap_urls() -> usize {
    std::env::var("WEBSITE_CRAWL_MAX_SITEMAP_URLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250_000)
}

/// Concurrent page scrapes during embedding. Browser renders dominate
/// CPU/memory, so they default lower than plain HTTP fetches.
/// `WEBSITE_SCRAPE_CONCURRENCY` overrides.
fn scrape_concurrency(render_javascript: bool) -> usize {
    std::env::var("WEBSITE_SCRAPE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(if render_javascript { 4 } else { 8 })
}

/// Default headers applied to every outbound request.
fn headers() -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    let _ = map.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"),
    );
    map
}

fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .default_headers(headers())
        .build()?)
}

fn browser_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("WEBSITE_BROWSER_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "msedge",
        "chrome",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.to_string_lossy().contains('/') || which_on_path(path))
}

fn which_on_path(command: &PathBuf) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(command);
        candidate.is_file()
            || (cfg!(windows)
                && directory
                    .join(format!("{}.exe", command.display()))
                    .is_file())
    })
}

async fn render_page(url: &str, browser: &PathBuf) -> anyhow::Result<String> {
    let profile = tempfile::tempdir().context("creating browser profile")?;
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(browser)
            .args([
                "--headless=new",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                "--no-sandbox",
                "--no-first-run",
                "--dump-dom",
                "--virtual-time-budget=10000",
            ])
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg(url)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("browser timed out after 30 seconds"))??;
    if !output.status.success() {
        anyhow::bail!("browser exited with status {}", output.status);
    }
    String::from_utf8(output.stdout).context("browser returned non-UTF-8 HTML")
}

fn same_scope(url: &Url, start: &Url) -> bool {
    if url.host_str().map(str::to_ascii_lowercase) != start.host_str().map(str::to_ascii_lowercase)
        || url.port_or_known_default() != start.port_or_known_default()
    {
        return false;
    }

    let start_path = start.path().trim_end_matches('/');
    if start_path.is_empty() {
        return true;
    }
    let url_path = url.path().trim_end_matches('/');
    url_path == start_path || url_path.starts_with(&format!("{start_path}/"))
}

fn is_ignored_domain(host: &str) -> bool {
    let host = host.to_lowercase();
    IGNORED_DOMAINS
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
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

fn origin_url(start: &Url) -> Url {
    let mut origin = start.clone();
    origin.set_path("/");
    origin.set_query(None);
    origin.set_fragment(None);
    origin
}

fn sitemap_seed_urls(start: &Url) -> Vec<Url> {
    let origin = origin_url(start);
    let mut seeds = vec![
        origin
            .join("sitemap.xml")
            .unwrap_or_else(|_| origin.clone()),
        origin
            .join("sitemap_index.xml")
            .unwrap_or_else(|_| origin.clone()),
    ];

    for name in ["sitemap.xml", "sitemap_index.xml"] {
        if let Ok(url) = start.join(name) {
            if !seeds
                .iter()
                .any(|candidate| canonical_url(candidate) == canonical_url(&url))
            {
                seeds.push(url);
            }
        }
    }
    seeds
}

fn sitemap_urls_from_robots(body: &[u8], base: &Url) -> Vec<Url> {
    let text = String::from_utf8_lossy(body);
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next()?.trim();
            let (key, value) = line.split_once(':')?;
            if !key.trim().eq_ignore_ascii_case("sitemap") {
                return None;
            }
            let value = value.trim();
            if value.is_empty() {
                return None;
            }
            base.join(value).ok()
        })
        .collect()
}

fn sitemap_urls_from_html(html: &str, base: &Url) -> Vec<Url> {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("link[href]").unwrap();
    document
        .select(&selector)
        .filter_map(|element| {
            let rel = element.value().attr("rel").unwrap_or_default();
            if !rel
                .split_ascii_whitespace()
                .any(|value| value.eq_ignore_ascii_case("sitemap"))
            {
                return None;
            }
            normalize_url(element.value().attr("href")?, base).and_then(|url| Url::parse(&url).ok())
        })
        .collect()
}

/// Parse a sitemap URL set or sitemap index, returning whether its locations
/// refer to nested sitemaps and the contained URLs.
fn parse_sitemap(xml: &[u8]) -> Option<(bool, Vec<String>)> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut is_index = false;
    let mut root = None;
    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut in_loc = false;
    let mut locations = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = event.local_name();
                if root.is_none() {
                    root = Some(name.as_ref().to_vec());
                    is_index = name.as_ref() == b"sitemapindex";
                }
                match name.as_ref() {
                    b"loc"
                        if matches!(stack.last().map(Vec::as_slice), Some(b"sitemap" | b"url")) =>
                    {
                        in_loc = true
                    }
                    b"link" if matches!(root.as_deref(), Some(b"rss") | Some(b"feed")) => {
                        in_loc = true
                    }
                    _ => {}
                }
                stack.push(name.as_ref().to_vec());
            }
            Ok(Event::Text(event)) if in_loc => {
                let location =
                    event
                        .decode()
                        .map_err(|error| error.to_string())
                        .and_then(|value| {
                            quick_xml::escape::unescape(value.as_ref())
                                .map_err(|error| error.to_string())
                                .map(|value| value.into_owned())
                        });
                if let Ok(location) = location {
                    let location = location.trim();
                    if !location.is_empty() {
                        locations.push(location.to_string());
                    }
                }
            }
            Ok(Event::End(event)) => {
                if matches!(event.local_name().as_ref(), b"loc" | b"link") {
                    in_loc = false;
                }
                let _ = stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    (!locations.is_empty()).then_some((is_index, locations))
}

/// Await `future` unless `cancellation` fires first.
async fn cancel_aware<F: Future>(
    future: F,
    cancellation: Option<&CancellationToken>,
) -> Option<F::Output> {
    match cancellation {
        Some(token) => tokio::select! {
            output = future => Some(output),
            _ = token.cancelled() => None,
        },
        None => Some(future.await),
    }
}

async fn request_response(
    client: &reqwest::Client,
    url: Url,
    cancellation: Option<&CancellationToken>,
) -> Option<reqwest::Response> {
    cancel_aware(client.get(url).send(), cancellation)
        .await
        .and_then(|response| response.ok())
}

async fn response_bytes(
    response: reqwest::Response,
    cancellation: Option<&CancellationToken>,
) -> Option<Vec<u8>> {
    cancel_aware(response.bytes(), cancellation)
        .await
        .and_then(|bytes| bytes.ok())
        .map(|bytes| bytes.to_vec())
}

async fn response_text(
    response: reqwest::Response,
    cancellation: Option<&CancellationToken>,
) -> Option<String> {
    cancel_aware(response.text(), cancellation)
        .await
        .and_then(|body| body.ok())
}

/// Fetch `url`, returning the body and the final (post-redirect) URL when
/// the response succeeds.
async fn fetch_body(
    client: &reqwest::Client,
    url: Url,
    cancellation: Option<&CancellationToken>,
) -> Option<(Vec<u8>, Url)> {
    let response = request_response(client, url, cancellation).await?;
    if !response.status().is_success() {
        return None;
    }
    let response_url = response.url().clone();
    let body = response_bytes(response, cancellation).await?;
    Some((body, response_url))
}

/// Fetch a page over HTTP, optionally rendering it with a headless browser.
///
/// With `browser` set, the rendered DOM replaces the HTTP body (keeping the
/// HTTP body when rendering fails), and an unsuccessful HTTP fetch falls
/// back to rendering alone.
async fn fetch_page(
    client: &reqwest::Client,
    url_str: &str,
    browser: Option<&PathBuf>,
    cancellation: Option<&CancellationToken>,
) -> Option<(String, Url, String)> {
    let response = cancel_aware(client.get(url_str).send(), cancellation).await;
    let (content_type, response_url, body) = match response {
        Some(Ok(resp)) if resp.status().is_success() => {
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let response_url = resp.url().clone();
            let body = response_text(resp, cancellation).await?;
            (content_type, response_url, body)
        }
        _ if browser.is_some() => (
            "text/html".to_string(),
            Url::parse(url_str).ok()?,
            String::new(),
        ),
        _ => return None,
    };
    let body = match browser {
        Some(browser) => match cancel_aware(render_page(url_str, browser), cancellation).await {
            Some(Ok(rendered)) => rendered,
            Some(Err(error)) => {
                tracing::debug!("JavaScript render failed for {url_str}: {error:#}");
                body
            }
            None => return None,
        },
        None => body,
    };
    Some((content_type, response_url, body))
}

async fn discover_sitemap_urls(
    client: &reqwest::Client,
    start: &Url,
    cancellation: Option<CancellationToken>,
) -> Vec<String> {
    let origin = origin_url(start);
    let mut pending: VecDeque<Url> = sitemap_seed_urls(start).into();

    if let Some((body, _)) = fetch_body(
        client,
        origin.join("robots.txt").unwrap_or_else(|_| origin.clone()),
        cancellation.as_ref(),
    )
    .await
    {
        for sitemap in sitemap_urls_from_robots(&body, &origin) {
            pending.push_back(sitemap);
        }
    }

    // Some sites advertise a sitemap only through the HTML head. This is a
    // discovery hint, not a replacement for robots.txt or conventional paths.
    if let Some((body, base)) = fetch_body(client, start.clone(), cancellation.as_ref()).await {
        if let Ok(html) = String::from_utf8(body) {
            for sitemap in sitemap_urls_from_html(&html, &base) {
                pending.push_back(sitemap);
            }
        }
    }

    let mut seen_sitemaps = HashSet::new();
    let mut urls = Vec::new();

    while let Some(sitemap) = pending.pop_front() {
        if cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            break;
        }
        let sitemap_key = canonical_url(&sitemap);
        if !seen_sitemaps.insert(sitemap_key) || seen_sitemaps.len() > max_sitemaps() {
            continue;
        }
        let Some((body, response_url)) =
            fetch_sitemap_body(client, sitemap, cancellation.as_ref()).await
        else {
            continue;
        };
        match parse_sitemap(&body) {
            Some((is_index, locations)) => {
                for location in locations {
                    let Some(url) = normalize_url(&location, &response_url)
                        .and_then(|value| Url::parse(&value).ok())
                    else {
                        continue;
                    };
                    if is_index {
                        pending.push_back(url);
                    } else if urls.len() < max_sitemap_urls() {
                        urls.push(canonical_url(&url));
                    }
                }
            }
            None => {
                for url in plain_text_sitemap_urls(&body, &response_url) {
                    if urls.len() >= max_sitemap_urls() {
                        break;
                    }
                    urls.push(url);
                }
            }
        }
    }

    urls.sort();
    urls.dedup();
    urls
}

/// Fetch + decode a single sitemap, enforcing the size cap and transparent
/// gzip decompression (`.xml.gz` files are served without Content-Encoding).
async fn fetch_sitemap_body(
    client: &reqwest::Client,
    url: Url,
    cancellation: Option<&CancellationToken>,
) -> Option<(Vec<u8>, Url)> {
    const MAX_SITEMAP_BYTES: usize = 52_428_800;
    let (body, response_url) = fetch_body(client, url, cancellation).await?;
    if body.len() > MAX_SITEMAP_BYTES {
        return None;
    }
    Some((gunzip(body)?, response_url))
}

fn gunzip(body: Vec<u8>) -> Option<Vec<u8>> {
    if !body.starts_with(&[0x1f, 0x8b]) {
        return Some(body);
    }
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(body.as_slice()), &mut decoded)
        .ok()?;
    Some(decoded)
}

/// Parse a plain-text sitemap: one URL per line, resolved against the
/// sitemap's own URL.
fn plain_text_sitemap_urls(body: &[u8], response_url: &Url) -> Vec<String> {
    String::from_utf8_lossy(body)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let url = normalize_url(line, response_url)?;
            Some(canonical_url(&Url::parse(&url).ok()?))
        })
        .collect()
}

/// Turn a user-entered website address into an absolute HTTP URL.
///
/// Browsers commonly accept `example.com` in an address bar, but the URL
/// parser and HTTP client require a scheme. HTTPS is the default for bare
/// addresses; explicit HTTP/HTTPS URLs are preserved.
pub fn normalize_website_url(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("website URL cannot be empty");
    }

    let candidate = if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = Url::parse(&candidate).map_err(|e| anyhow::anyhow!("invalid URL {raw:?}: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!(
            "unsupported URL scheme {:?}; use http or https",
            parsed.scheme()
        );
    }
    if parsed.host_str().is_none() {
        anyhow::bail!("website URL has no domain: {raw:?}");
    }
    Ok(parsed.to_string())
}

/// Resolve `href` against the document base URL and remove its fragment.
///
/// Important: `base` must be the document URL *as the browser sees it*
/// (typically `response.url()` after redirects, which preserves a trailing
/// slash on directory indexes). Joining relative paths like `symbols/X.html`
/// against `https://example.com/api` (no slash) incorrectly yields
/// `https://example.com/symbols/X.html` per URL RFC 3986.
pub fn normalize_url(href: &str, base: &Url) -> Option<String> {
    let mut joined = base.join(href).ok()?;
    if !matches!(joined.scheme(), "http" | "https") {
        return None;
    }
    joined.set_fragment(None);
    Some(joined.to_string())
}

fn is_html_response(content_type: &str, body: &str) -> bool {
    let content_type = content_type.to_ascii_lowercase();
    content_type.contains("text/html")
        || content_type.contains("application/xhtml+xml")
        || (content_type.is_empty()
            && body
                .trim_start()
                .get(..64)
                .map(|prefix| {
                    let prefix = prefix.to_ascii_lowercase();
                    prefix.contains("<html") || prefix.contains("<!doctype html")
                })
                .unwrap_or(false))
}

/// BFS-crawl a website starting from `start_url`, returning the list of
/// discovered page URLs.
///
/// Uses a tokio task set capped at `CRAWL_CONCURRENCY` concurrent fetches,
/// with a 100ms inter-fetch delay. Sitemap URLs are returned as soon as they
/// are discovered; `WEBSITE_CRAWL_MAX_PAGES` limits validation fetches only.
///
/// When `on_progress` is set, emits `fetching` / `found` / `done` events so
/// the UI can show the live URL list (same idea as zip file embedding status).
pub async fn crawl_website(
    start_url: &str,
    depth: usize,
    same_domain_only: bool,
    on_progress: Option<CrawlProgressCallback>,
) -> anyhow::Result<Vec<String>> {
    crawl_website_with_options_and_cancellation(
        start_url,
        depth,
        same_domain_only,
        false,
        None,
        on_progress,
    )
    .await
}

/// Shared state for one crawl run: options plus the synchronized queues and
/// sets the worker tasks mutate.
struct CrawlRun {
    client: reqwest::Client,
    start: Url,
    browser: Option<PathBuf>,
    cancellation: Option<CancellationToken>,
    on_progress: Option<CrawlProgressCallback>,
    depth: usize,
    same_domain_only: bool,
    max_pages: usize,
    visited: Arc<tokio::sync::Mutex<HashSet<String>>>,
    seen: Arc<tokio::sync::Mutex<HashSet<String>>>,
    results: Arc<tokio::sync::Mutex<Vec<String>>>,
    queue: Arc<tokio::sync::Mutex<VecDeque<(String, usize)>>>,
    sitemap_queue: Arc<tokio::sync::Mutex<VecDeque<(String, usize)>>>,
}

pub async fn crawl_website_with_options_and_cancellation(
    start_url: &str,
    depth: usize,
    same_domain_only: bool,
    render_javascript: bool,
    cancellation: Option<CancellationToken>,
    on_progress: Option<CrawlProgressCallback>,
) -> anyhow::Result<Vec<String>> {
    let normalized_start = normalize_website_url(start_url)?;
    let start = Url::parse(&normalized_start)?;
    let start_key = canonical_url(&start);

    let browser = if render_javascript {
        browser_path()
    } else {
        None
    };
    if render_javascript && browser.is_none() {
        tracing::warn!(
            "JavaScript rendering requested but no Chromium-family browser was found; using HTTP HTML"
        );
    }

    let client = http_client()?;
    let sitemap_urls = discover_sitemap_urls(&client, &start, cancellation.clone()).await;

    // The queue stores the fetch URL (directory paths keep their trailing
    // slash, which affects relative link resolution) + depth.
    let run = Arc::new(CrawlRun {
        client,
        start: start.clone(),
        browser,
        cancellation,
        on_progress,
        depth,
        same_domain_only,
        max_pages: max_pages(),
        visited: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
        seen: Arc::new(tokio::sync::Mutex::new(HashSet::from([start_key.clone()]))),
        results: Arc::new(tokio::sync::Mutex::new(vec![start_key])),
        queue: Arc::new(tokio::sync::Mutex::new(VecDeque::from([(start.to_string(), 0)]))),
        sitemap_queue: Arc::new(tokio::sync::Mutex::new(VecDeque::new())),
    });
    enqueue_sitemap_urls(&run, sitemap_urls).await;

    let semaphore = Arc::new(tokio::sync::Semaphore::new(CRAWL_CONCURRENCY));
    let mut handles = Vec::new();

    loop {
        if run.cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            break;
        }
        let item = run.queue.lock().await.pop_front();
        let item = if item.is_some() {
            item
        } else if semaphore.available_permits() == CRAWL_CONCURRENCY {
            run.sitemap_queue.lock().await.pop_front()
        } else {
            None
        };
        let Some((url_str, cur_depth)) = item else {
            // Nothing queued; done when no fetch is in flight either.
            if semaphore.available_permits() == CRAWL_CONCURRENCY
                && run.sitemap_queue.lock().await.is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        };

        let visit_key = Url::parse(&url_str)
            .map(|u| canonical_url(&u))
            .unwrap_or_else(|_| url_str.trim_end_matches('/').to_string());
        {
            let mut visited = run.visited.lock().await;
            if visited.contains(&visit_key) {
                continue;
            }
            if visited.len() >= run.max_pages {
                break;
            }
            visited.insert(visit_key);
        }

        tokio::time::sleep(Duration::from_millis(CRAWL_DELAY_MS)).await;

        let permit = semaphore.clone().acquire_owned().await?;
        if let Some(cb) = run.on_progress.as_ref() {
            cb(CrawlProgressEvent {
                url: url_str.clone(),
                phase: "fetching".into(),
                found_count: run.results.lock().await.len(),
            });
        }

        let task_run = run.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            crawl_page(task_run, url_str, cur_depth).await;
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
    if run.cancellation
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        anyhow::bail!("website crawl cancelled");
    }
    let urls = run.results.lock().await.clone();
    if let Some(cb) = run.on_progress.as_ref() {
        cb(CrawlProgressEvent {
            url: String::new(),
            phase: "done".into(),
            found_count: urls.len(),
        });
    }
    Ok(urls)
}

/// Queue sitemap-discovered URLs for later validation: they are only fetched
/// once the link-based crawl drains, so sitemap breadth never crowds out the
/// pages actually linked from the site.
async fn enqueue_sitemap_urls(run: &CrawlRun, sitemap_urls: Vec<String>) {
    let mut queue = VecDeque::new();
    let mut results = Vec::new();
    let mut seen = run.seen.lock().await;
    for sitemap_url in sitemap_urls {
        let Some(url) = normalize_url(&sitemap_url, &run.start) else {
            continue;
        };
        let Ok(parsed) = Url::parse(&url) else {
            continue;
        };
        if is_ignored_domain(parsed.host_str().unwrap_or_default()) {
            continue;
        }
        if run.same_domain_only && !same_scope(&parsed, &run.start) {
            continue;
        }
        let key = canonical_url(&parsed);
        if seen.insert(key.clone()) {
            queue.push_back((parsed.to_string(), 0));
            results.push(key);
        }
    }
    drop(seen);
    *run.sitemap_queue.lock().await = queue;
    run.results.lock().await.extend(results);
}

/// Fetch one page, record it in the results, and enqueue newly found links.
async fn crawl_page(run: Arc<CrawlRun>, url_str: String, cur_depth: usize) {
    if run.cancellation
        .as_ref()
        .is_some_and(CancellationToken::is_cancelled)
    {
        return;
    }
    let Some((content_type, response_url, body)) = fetch_page(
        &run.client,
        &url_str,
        run.browser.as_ref(),
        run.cancellation.as_ref(),
    )
    .await
    else {
        return;
    };
    // Use the final URL after redirects, unless the HTTP client has removed
    // a slash from the directory URL we requested. That slash changes how
    // relative links such as `../learn/` are resolved.
    let document_base = if url_str.ends_with('/') && !response_url.path().ends_with('/') {
        Url::parse(&url_str).unwrap_or(response_url)
    } else {
        response_url
    };
    let result_key = canonical_url(&document_base);
    if !is_html_response(&content_type, &body) {
        return;
    }

    let found_count = {
        let mut results = run.results.lock().await;
        if !results.iter().any(|u| u == &result_key) {
            results.push(result_key.clone());
        }
        results.len()
    };
    if let Some(cb) = run.on_progress.as_ref() {
        cb(CrawlProgressEvent {
            url: result_key,
            phase: "found".into(),
            found_count,
        });
    }

    if cur_depth >= run.depth {
        return;
    }
    let found = extract_page_links(&body, &document_base, run.same_domain_only, &run.start);
    let mut seen = run.seen.lock().await;
    let mut queue = run.queue.lock().await;
    for url in found {
        let Ok(parsed) = Url::parse(&url) else {
            continue;
        };
        let key = canonical_url(&parsed);
        if seen.insert(key.clone()) {
            queue.push_back((url, cur_depth + 1));
            run.results.lock().await.push(key);
        }
    }
}

/// Resolve a document's effective base URL (`<base href>` overrides the
/// response URL).
fn base_href(document: &scraper::Html, document_base: &Url) -> Url {
    scraper::Selector::parse("base[href]")
        .ok()
        .and_then(|selector| document.select(&selector).next())
        .and_then(|element| element.value().attr("href"))
        .and_then(|href| normalize_url(href, document_base))
        .and_then(|url| Url::parse(&url).ok())
        .unwrap_or_else(|| document_base.clone())
}

/// Collect outgoing page links from an HTML body, honoring scope and
/// ignored-domain rules.
fn extract_page_links(
    body: &str,
    document_base: &Url,
    same_domain_only: bool,
    scope_start: &Url,
) -> Vec<String> {
    let document = scraper::Html::parse_document(body);
    let link_base = base_href(&document, document_base);
    let selector = scraper::Selector::parse(
        "a[href], area[href], [data-href], [data-url], link[rel~='next'][href]",
    )
    .unwrap();
    document
        .select(&selector)
        .filter_map(|element| {
            let href = element
                .value()
                .attr("href")
                .or_else(|| element.value().attr("data-href"))
                .or_else(|| element.value().attr("data-url"))?;
            let url = normalize_url(href, &link_base)?;
            let parsed = Url::parse(&url).ok()?;
            if same_domain_only && !same_scope(&parsed, scope_start) {
                return None;
            }
            if is_ignored_domain(parsed.host_str().unwrap_or_default()) {
                return None;
            }
            Some(url)
        })
        .collect()
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

/// One scraped page: title, sections, and navigation links.
type ScrapedPage = (String, Vec<WebSection>, Vec<Link>);

/// Fetch + sectionize a single page. Direct port of `scrape_website`.
pub async fn scrape_website(url: &str) -> anyhow::Result<(String, Vec<WebSection>)> {
    let (title, sections, _) = scrape_page(&http_client()?, None, url).await?;
    Ok((title, sections))
}

/// Fetch one page and return `(title, sections, navigation links)`. With
/// `browser` set the page is rendered with it first; render errors propagate
/// (the crawl path uses [`fetch_page`] to fall back to the HTTP body).
async fn scrape_page(
    client: &reqwest::Client,
    browser: Option<&PathBuf>,
    url: &str,
) -> anyhow::Result<ScrapedPage> {
    let url = normalize_website_url(url)?;
    let response = client.get(&url).send().await;
    let (content_type, response_url, mut body) = match response {
        Ok(resp) if resp.status().is_success() => {
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let response_url = resp.url().clone();
            let body = resp
                .text()
                .await
                .map_err(|e| anyhow::anyhow!("reading body: {e}"))?;
            (content_type, response_url, body)
        }
        _ if browser.is_some() => ("text/html".to_string(), Url::parse(&url)?, String::new()),
        Ok(resp) => anyhow::bail!("failed to load page: HTTP {}", resp.status()),
        Err(error) => return Err(anyhow::anyhow!("fetching {url}: {error}")),
    };
    if let Some(browser) = browser {
        body = render_page(&url, browser).await?;
    }
    if !is_html_response(&content_type, &body) {
        anyhow::bail!("page is not HTML (Content-Type: {content_type})");
    }
    let (title, sections) = scrape_html_with_base(&body, Some(&response_url));
    let navigation = extract_navigation_links(&body, &response_url);
    Ok((title, sections, navigation))
}

/// Sectionize an HTML body without fetching it. Direct port of `_walk_dom`.
///
/// Iterates `h1, h2, pre, p, li, h3, h4, h5, h6, td, th, blockquote,
/// figcaption, dd, dt, caption, summary` elements in document order,
/// maintaining a small walker state keyed on the most recent h1/h2.
pub fn scrape_html(html: &str) -> (String, Vec<WebSection>) {
    scrape_html_with_base(html, None)
}

fn scrape_html_with_base(html: &str, base: Option<&Url>) -> (String, Vec<WebSection>) {
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
    let combined_sel_str = format!("h1, h2, pre, {}", LEAF_TEXT_TAGS.join(", "));
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
                    let href = base
                        .and_then(|base| normalize_url(href, base))
                        .unwrap_or_else(|| href.to_string());
                    last.links.push((href, anchor_text));
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
            code_blocks: s
                .code_blocks
                .into_iter()
                .map(|(raw, lang)| CodeBlock {
                    raw,
                    language: lang,
                })
                .collect(),
            links: s
                .links
                .into_iter()
                .map(|(h, a)| Link {
                    href: h,
                    anchor_text: a,
                })
                .collect(),
        });
    }

    (title, sections)
}

fn extract_navigation_links(html: &str, base: &Url) -> Vec<Link> {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse(
        "nav a[href], aside a[href], [role='navigation'] a[href], [data-nav] a[href]",
    )
    .unwrap();
    let mut links = Vec::new();
    let mut seen = HashSet::new();
    for element in document.select(&selector) {
        let anchor_text = element.text().collect::<String>().trim().to_string();
        let Some(href) = element.value().attr("href") else {
            continue;
        };
        if anchor_text.len() < MIN_ANCHOR_LEN {
            continue;
        }
        let Some(href) = normalize_url(href.trim(), base) else {
            continue;
        };
        if seen.insert((href.clone(), anchor_text.clone())) {
            links.push(Link { href, anchor_text });
        }
    }
    links
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
        if matches!(
            node.value().name(),
            "script" | "style" | "nav" | "footer" | "header" | "aside"
        ) {
            return true;
        }
        current = node.parent().and_then(scraper::ElementRef::wrap);
    }
    false
}

fn has_selected_text_descendant(
    elem: &scraper::ElementRef<'_>,
    selector: &scraper::Selector,
) -> bool {
    elem.select(selector).any(|child| child.id() != elem.id())
}

fn language_from_element(elem: &scraper::node::Element) -> Option<String> {
    elem.attr("class")
        .unwrap_or("")
        .split_whitespace()
        .find_map(|c| c.strip_prefix("language-").map(str::to_string))
}

fn detect_code_language(elem: &scraper::ElementRef<'_>) -> String {
    language_from_element(elem.value())
        .or_else(|| {
            scraper::Selector::parse("code")
                .ok()
                .and_then(|selector| elem.select(&selector).next())
                .and_then(|code| language_from_element(code.value()))
        })
        .unwrap_or_default()
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
pub async fn process_website(
    urls: &[String],
    metadata: &Map<String, Value>,
) -> anyhow::Result<Vec<DocumentChunk>> {
    process_website_with_splitter(urls, metadata, None, false).await
}

/// Production website path. Uses the same tokenizer-backed splitter as local
/// documents, matching the Python TokenTextSplitter behavior.
pub async fn process_website_with_splitter(
    urls: &[String],
    metadata: &Map<String, Value>,
    splitter: Option<&TextSplitter<Tokenizer>>,
    render_javascript: bool,
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

    let client = http_client()?;
    let browser = if render_javascript {
        browser_path()
    } else {
        None
    };
    if render_javascript && browser.is_none() {
        tracing::warn!(
            "JavaScript rendering requested but no Chromium-family browser was found; using HTTP HTML"
        );
    }

    // Scrape pages concurrently: a JavaScript render takes seconds per page,
    // so a sequential loop dominates the total embed time on large sites.
    // `buffered` keeps the chunk output in input order.
    let pages: Vec<Option<ScrapedPage>> =
        futures_util::stream::iter(urls.iter().cloned())
            .map(|url| {
                let client = client.clone();
                let browser = browser.clone();
                async move {
                    match scrape_page(&client, browser.as_ref(), &url).await {
                        Ok(page) => Some(page),
                        Err(e) => {
                            tracing::warn!("failed to scrape {url}: {e:#}");
                            None
                        }
                    }
                }
            })
            .buffered(scrape_concurrency(render_javascript))
            .collect()
            .await;

    for (url, page) in urls.iter().zip(pages) {
        let Some((page_title, sections, navigation_links)) = page else {
            continue;
        };
        if sections.is_empty() {
            continue;
        }
        let page_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();

        // Build a link_map: anchor_text -> href (first occurrence wins).
        let mut link_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for section in &sections {
            for link in &section.links {
                link_map
                    .entry(link.anchor_text.clone())
                    .or_insert_with(|| link.href.clone());
            }
        }
        for link in &navigation_links {
            link_map
                .entry(link.anchor_text.clone())
                .or_insert_with(|| link.href.clone());
        }

        let mut base_md: Map<String, Value> = metadata.clone();
        base_md.insert("url".into(), Value::String(url.clone()));
        base_md.insert("doc_type".into(), Value::String("website".into()));
        base_md.insert("embedded_at".into(), Value::String(embedded_at.clone()));
        base_md.insert("page_id".into(), Value::String(page_id.clone()));
        base_md.insert("page_title".into(), Value::String(page_title.clone()));
        base_md.insert(
            "navigation_links".into(),
            Value::Array(
                navigation_links
                    .iter()
                    .map(|link| {
                        serde_json::json!({
                            "href": link.href,
                            "anchor_text": link.anchor_text,
                        })
                    })
                    .collect(),
            ),
        );
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
            .map(|splitter| {
                splitter
                    .chunks(&full_page_text)
                    .map(str::to_string)
                    .collect()
            })
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
            let chunk_hash = hash_hex
                .iter()
                .take(8)
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            c.metadata
                .insert("chunk_hash".into(), Value::String(chunk_hash));
            c.metadata
                .insert("total_chunks".into(), Value::Number(total.into()));
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
                .map(|(anchor, href)| serde_json::json!({"href": href, "anchor_text": anchor}))
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
        let content = sections
            .iter()
            .map(|section| section.content.as_str())
            .collect::<String>();
        assert!(content.contains("keep once"));
        assert_eq!(content.matches("keep once").count(), 1);
        assert!(!content.contains("header noise"));
        assert!(!content.contains("nav noise"));
        assert!(!content.contains("footer noise"));
        assert_eq!(sections[0].code_blocks[0].language, "python");
    }

    #[test]
    fn normalize_website_url_adds_https_to_bare_domains() {
        assert_eq!(
            normalize_website_url("www.google.com").unwrap(),
            "https://www.google.com/"
        );
        assert_eq!(
            normalize_website_url("google.com/search?q=rust").unwrap(),
            "https://google.com/search?q=rust"
        );
    }

    #[test]
    fn normalize_website_url_preserves_http_and_rejects_invalid_schemes() {
        assert_eq!(
            normalize_website_url("http://localhost:3000/docs").unwrap(),
            "http://localhost:3000/docs"
        );
        assert!(normalize_website_url("ftp://example.com").is_err());
        assert!(normalize_website_url("   ").is_err());
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
    fn normalize_url_strips_fragment_and_preserves_trailing_slash() {
        let base = Url::parse("https://example.com/docs/").unwrap();
        assert_eq!(
            normalize_url("page#section", &base).as_deref(),
            Some("https://example.com/docs/page")
        );
        assert_eq!(
            normalize_url("../other/", &base).as_deref(),
            Some("https://example.com/other/")
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
    fn parse_sitemap_reads_url_sets_and_indexes() {
        let urls = parse_sitemap(
            br#"<?xml version="1.0"?><urlset><url><loc>https://example.com/guide</loc></url></urlset>"#,
        );
        assert_eq!(
            urls,
            Some((false, vec!["https://example.com/guide".to_string()]))
        );

        let index = parse_sitemap(
            br#"<?xml version="1.0"?><sitemapindex><sitemap><loc>https://example.com/pages.xml</loc></sitemap></sitemapindex>"#,
        );
        assert_eq!(
            index,
            Some((true, vec!["https://example.com/pages.xml".to_string()]))
        );
    }

    #[test]
    fn parse_sitemap_ignores_nested_image_locations() {
        let urls = parse_sitemap(
            br#"<urlset><url><loc>https://example.com/page</loc><image:image><image:loc>https://example.com/image.png</image:loc></image:image></url></urlset>"#,
        );
        assert_eq!(
            urls,
            Some((false, vec!["https://example.com/page".to_string()]))
        );
    }

    #[test]
    fn robots_discovers_multiple_relative_and_absolute_sitemaps() {
        let base = Url::parse("https://example.com/docs/").unwrap();
        let urls = sitemap_urls_from_robots(
            b"Sitemap: /sitemap.xml\n sitemap: https://cdn.example.com/docs.xml\n",
            &base,
        );
        assert_eq!(
            urls.iter().map(Url::as_str).collect::<Vec<_>>(),
            vec![
                "https://example.com/sitemap.xml",
                "https://cdn.example.com/docs.xml"
            ]
        );
    }

    #[test]
    fn navigation_links_are_absolute_and_deduplicated() {
        let base = Url::parse("https://example.com/docs/page/").unwrap();
        let links = extract_navigation_links(
            r#"<nav><a href="../guide/">Guide</a><a href="../guide/">Guide</a></nav><main><p>Body</p></main>"#,
            &base,
        );
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].href, "https://example.com/docs/guide/");
    }

    #[test]
    fn same_scope_stays_under_starting_path() {
        let start = Url::parse("https://neo4j.com/docs/").unwrap();
        assert!(same_scope(
            &Url::parse("https://neo4j.com/docs/getting-started/").unwrap(),
            &start
        ));
        assert!(same_scope(
            &Url::parse("https://neo4j.com/docs").unwrap(),
            &start
        ));
        assert!(!same_scope(
            &Url::parse("https://neo4j.com/blog/").unwrap(),
            &start
        ));
        assert!(!same_scope(
            &Url::parse("https://docs.neo4j.com/docs/").unwrap(),
            &start
        ));
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
