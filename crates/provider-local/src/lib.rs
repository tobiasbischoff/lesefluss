pub mod media;
pub mod netpolicy;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("parse: {0}")]
    Parse(String),
    #[error("zu groß: {0} Bytes")]
    TooLarge(usize),
    #[error("Ziel abgelehnt: {0}")]
    Blocked(String),
    #[error("kein Feed gefunden")]
    NoFeed,
}

pub type Result<T> = std::result::Result<T, ProviderError>;

pub const MAX_FEED_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_DISCOVERY_BYTES: usize = 4 * 1024 * 1024;

pub async fn read_bounded(
    resp: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>> {
    if let Some(len) = resp.content_length() {
        if len as usize > limit {
            return Err(ProviderError::TooLarge(len as usize));
        }
    }
    let mut out: Vec<u8> = Vec::new();
    let mut stream = resp;
    while let Some(chunk) = stream.chunk().await? {
        if out.len() + chunk.len() > limit {
            return Err(ProviderError::TooLarge(out.len() + chunk.len()));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct HttpClient {
    inner: reqwest::Client,
    trusted_origins: Vec<String>,
    allow_private_hosts: bool,
}

impl HttpClient {
    /// Nur für Tests und bewusst freigegebene Intranet-Feeds.
    pub fn with_private_hosts(mut self, allow: bool) -> Self {
        self.allow_private_hosts = allow;
        self
    }

    pub fn with_trusted_origin(mut self, origin: &str) -> Self {
        self.trusted_origins.push(origin.to_string());
        self
    }

    fn guard(&self, url: &str) -> Result<url::Url> {
        if self.allow_private_hosts {
            let parsed =
                url::Url::parse(url).map_err(|_| ProviderError::Blocked(url.to_string()))?;
            if !netpolicy::scheme_allowed(&parsed) {
                return Err(ProviderError::Blocked(parsed.scheme().to_string()));
            }
            return Ok(parsed);
        }
        netpolicy::check_url(url, &self.trusted_origins)
            .map_err(|e| ProviderError::Blocked(e.to_string()))
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        let inner = reqwest::Client::builder()
            .user_agent("Lesefluss/0.1 (+https://example.org; lokaler RSS-Reader)")
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("reqwest client");
        Self { inner, trusted_origins: Vec::new(), allow_private_hosts: false }
    }

    pub async fn fetch_feed(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchOutcome> {
        self.guard(url)?;
        let mut req = self.inner.get(url);
        if let Some(e) = etag {
            req = req.header("If-None-Match", e);
        }
        if let Some(lm) = last_modified {
            req = req.header("If-Modified-Since", lm);
        }
        let resp = req.send().await?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(FetchOutcome::NotModified);
        }
        if !resp.status().is_success() {
            return Err(ProviderError::Parse(format!("HTTP {}", resp.status())));
        }
        let new_etag = resp.headers().get("etag").and_then(|v| v.to_str().ok().map(str::to_string));
        let new_lm = resp
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok().map(str::to_string));
        let final_url = resp.url().to_string();
        self.guard(&final_url)?;
        let bytes = read_bounded(resp, MAX_FEED_BYTES).await?;
        Ok(FetchOutcome::Fetched {
            bytes,
            etag: new_etag,
            last_modified: new_lm,
            final_url,
        })
    }

    pub async fn fetch_image(&self, url: &str) -> Result<(String, Vec<u8>)> {
        self.guard(url)?;
        let resp = self.inner.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(ProviderError::Parse(format!("HTTP {}", resp.status())));
        }
        let final_url = resp.url().to_string();
        self.guard(&final_url)?;
        let bytes = read_bounded(resp, media::MAX_IMAGE_BYTES).await?;
        Ok((final_url, bytes))
    }

    pub async fn fetch_raw(&self, url: &str) -> Result<(String, Vec<u8>)> {
        self.guard(url)?;
        let resp = self.inner.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(ProviderError::Parse(format!("HTTP {}", resp.status())));
        }
        let final_url = resp.url().to_string();
        self.guard(&final_url)?;
        let bytes = read_bounded(resp, MAX_DISCOVERY_BYTES).await?;
        Ok((final_url, bytes))
    }
}

#[derive(Clone, Debug)]
pub enum FetchOutcome {
    NotModified,
    Fetched {
        bytes: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
        final_url: String,
    },
}

#[derive(Clone, Debug)]
pub struct ParsedItem {
    pub identity: String,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_ms: Option<i64>,
    pub excerpt: String,
    pub content_html: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedFeed {
    pub title: Option<String>,
    pub website: Option<String>,
    pub items: Vec<ParsedItem>,
}

pub fn parse_feed(bytes: &[u8]) -> Result<ParsedFeed> {
    let feed = feed_rs::parser::parse(bytes).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let mut out = ParsedFeed {
        title: feed.title.map(|t| t.content),
        website: feed.links.first().map(|l| l.href.clone()),
        items: Vec::new(),
    };
    for e in feed.entries {
        let link = e.links.first().map(|l| l.href.clone());
        let published = e.published.or(e.updated).map(|dt| dt.timestamp_millis());
        let content_html = e
            .content
            .and_then(|c| c.body)
            .or_else(|| e.summary.as_ref().map(|s| s.content.clone()));
        let excerpt = e
            .summary
            .map(|s| storage_plain(&s.content))
            .unwrap_or_default();
        let identity = identity_of(&e.id, link.as_deref(), &e.title.as_ref().map(|t| t.content.clone()).unwrap_or_default(), published);
        out.items.push(ParsedItem {
            identity,
            title: e.title.map(|t| t.content).unwrap_or_else(|| "(ohne Titel)".into()),
            author: e.authors.first().map(|a| a.name.clone()),
            url: link,
            published_ms: published,
            excerpt,
            content_html,
        });
    }
    Ok(out)
}

fn storage_plain(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(400).collect()
}

pub fn identity_of(guid: &str, url: Option<&str>, title: &str, published_ms: Option<i64>) -> String {
    let g = guid.trim();
    if !g.is_empty() && g.len() <= 512 {
        return g.to_string();
    }
    if let Some(u) = url {
        let n = normalize_url(u);
        if !n.is_empty() {
            return n;
        }
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    title.hash(&mut h);
    published_ms.hash(&mut h);
    url.hash(&mut h);
    format!("hash:{:016x}", h.finish())
}

pub fn normalize_url(raw: &str) -> String {
    let Ok(mut u) = url::Url::parse(raw) else { return raw.to_string() };
    u.set_fragment(None);
    let host = u.host_str().map(|h| h.to_lowercase());
    let _ = u.set_host(host.as_deref());
    let scheme = u.scheme().to_string();
    if let Some(port) = u.port() {
        if (scheme == "http" && port == 80) || (scheme == "https" && port == 443) {
            let _ = u.set_port(None);
        }
    }
    u.to_string()
}

#[derive(Clone, Debug)]
pub struct DiscoverCandidate {
    pub url: String,
    pub title: String,
}

pub async fn discover(client: &HttpClient, input: &str) -> Result<Vec<DiscoverCandidate>> {
    let url = if input.starts_with("http://") || input.starts_with("https://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let (final_url, bytes) = client.fetch_raw(&url).await?;
    if looks_like_feed(&bytes) {
        let parsed = parse_feed(&bytes)?;
        return Ok(vec![DiscoverCandidate {
            url: final_url,
            title: parsed.title.unwrap_or_else(|| host_of(&url)),
        }]);
    }
    let html = String::from_utf8_lossy(&bytes);
    let mut cands = Vec::new();
    for (href, title) in extract_alternate_links(&html) {
        let abs = url::Url::parse(&final_url)
            .and_then(|b| b.join(&href))
            .map(|u| u.to_string())
            .unwrap_or(href);
        cands.push(DiscoverCandidate { url: normalize_url(&abs), title });
    }
    if cands.is_empty() {
        for guess in ["/feed", "/rss", "/feed.xml", "/rss.xml", "/atom.xml", "/index.xml"] {
            let candidate = url::Url::parse(&final_url)
                .and_then(|b| b.join(guess))
                .map(|u| u.to_string())
                .unwrap_or_default();
            if let Ok((_, b2)) = client.fetch_raw(&candidate).await {
                if looks_like_feed(&b2) {
                    let parsed = parse_feed(&b2)?;
                    cands.push(DiscoverCandidate {
                        url: normalize_url(&candidate),
                        title: parsed.title.unwrap_or_else(|| host_of(&candidate).to_string()),
                    });
                }
            }
            if cands.len() >= 4 {
                break;
            }
        }
    }
    if cands.is_empty() {
        return Err(ProviderError::NoFeed);
    }
    cands.dedup_by(|a, b| a.url == b.url);
    Ok(cands)
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

fn looks_like_feed(bytes: &[u8]) -> bool {
    let head: String = bytes.iter().take(512).map(|b| *b as char).collect();
    let head = head.to_lowercase();
    head.contains("<rss") || head.contains("<feed") || head.contains("rdf:rdf") || head.contains("<?xml")
}

fn extract_alternate_links(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lower = html.to_lowercase();
    let mut pos = 0;
    while let Some(idx) = lower[pos..].find("<link").map(|i| i + pos) {
        let end = match lower[idx..].find('>') {
            Some(e) => idx + e,
            None => break,
        };
        let tag = &html[idx..end];
        pos = end + 1;
        let tag_lower = tag.to_lowercase();
        if !tag_lower.contains("rel=\"alternate\"") && !tag_lower.contains("rel='alternate'") {
            continue;
        }
        let is_feed_type = ["application/rss+xml", "application/atom+xml", "application/xml", "text/xml"]
            .iter()
            .any(|t| tag_lower.contains(&format!("type=\"{t}\"")) || tag_lower.contains(&format!("type='{t}'")));
        if !is_feed_type {
            continue;
        }
        if let Some(href) = attr(tag, "href") {
            let title = attr(tag, "title").unwrap_or_else(|| "Feed".into());
            out.push((href, title));
        }
    }
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pat1 = format!("{name}=\"");
    let pat2 = format!("{name}='");
    for pat in [pat1, pat2] {
        let lower_tag = tag.to_lowercase();
        if let Some(i) = lower_tag.find(&pat) {
            let start = i + pat.len();
            let quote = pat.chars().last().unwrap();
            if let Some(e) = tag[start..].find(quote) {
                return Some(tag[start..start + e].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel>
<title>Testfeed</title><link>https://example.com/</link>
<item><guid>item-1</guid><title>Erster &amp; Co</title><link>https://example.com/1</link>
<pubDate>Wed, 23 Sep 2026 10:00:00 GMT</pubDate>
<description><![CDATA[<p>Auszug <b>fett</b></p>]]></description>
<content:encoded xmlns:content="http://purl.org/rss/1.0/modules/content/"><![CDATA[<p>Volltext</p>]]></content:encoded>
</item>
<item><title>Ohne GUID</title><link>https://example.com/2?b=2&amp;a=1#frag</link></item>
</channel></rss>"#;

    const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<title>Atomfeed</title>
<entry><id>urn:uuid:1234</id><title>Atom eins</title>
<updated>2026-09-22T08:00:00Z</updated>
<link href="https://example.com/atom/1"/>
<summary>Zusammenfassung</summary>
</entry></feed>"#;

    #[test]
    fn parse_rss2_fields() {
        let f = parse_feed(RSS.as_bytes()).unwrap();
        assert_eq!(f.title.as_deref(), Some("Testfeed"));
        assert_eq!(f.items.len(), 2);
        assert_eq!(f.items[0].identity, "item-1");
        assert_eq!(f.items[0].title, "Erster & Co");
        assert!(f.items[0].published_ms.is_some());
        assert!(f.items[0].content_html.as_deref().unwrap().contains("Volltext"));
        assert!(
            !f.items[1].identity.is_empty() && !f.items[1].identity.contains('#'),
            "Identität ohne Fragment: {}",
            f.items[1].identity
        );
    }

    #[test]
    fn parse_atom_fields() {
        let f = parse_feed(ATOM.as_bytes()).unwrap();
        assert_eq!(f.items[0].identity, "urn:uuid:1234");
        assert_eq!(f.items[0].url.as_deref(), Some("https://example.com/atom/1"));
        assert!(f.items[0].published_ms.is_some());
    }

    #[test]
    fn identity_fallback_hash() {
        let a = identity_of("", None, "Titel", Some(1));
        let b = identity_of("", None, "Titel", Some(1));
        let c = identity_of("", None, "Titel", Some(2));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with("hash:"));
    }

    #[test]
    fn normalize_url_rules() {
        assert_eq!(normalize_url("https://Example.com:443/a/"), "https://example.com/a/");
        assert_eq!(normalize_url("http://x.de:80/p"), "http://x.de/p");
        assert_eq!(normalize_url("https://x.de/p#sec"), "https://x.de/p");
        assert_eq!(normalize_url("https://x.de/P/"), "https://x.de/P/");
    }

    #[test]
    fn discovery_links_aus_html() {
        let html = r#"<html><head>
<link rel="alternate" type="application/rss+xml" title="Blog Feed" href="/feed.xml">
<link rel="alternate" type="text/html" href="/other">
</head><body></body></html>"#;
        let cands = extract_alternate_links(html);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].0, "/feed.xml");
        assert_eq!(cands[0].1, "Blog Feed");
    }

    #[tokio::test]
    async fn oversized_chunked_body_is_aborted_before_full_allocation() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let head = "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nTransfer-Encoding: chunked\r\n\r\n";
                let _ = s.write_all(head.as_bytes());
                let block = vec![b'a'; 64 * 1024];
                for _ in 0..400 {
                    if s.write_all(format!("{:x}\r\n", block.len()).as_bytes()).is_err() {
                        return;
                    }
                    if s.write_all(&block).is_err() {
                        return;
                    }
                    if s.write_all(b"\r\n").is_err() {
                        return;
                    }
                }
            }
        });
        let client = HttpClient::new().with_private_hosts(true);
        let url = format!("http://{addr}/feed.xml");
        match client.fetch_feed(&url, None, None).await {
            Err(ProviderError::TooLarge(_)) => {}
            Err(ProviderError::Http(_)) => {}
            other => panic!("erwartete Größen- oder Abbruchfehler, bekam {other:?}"),
        }
    }

    #[tokio::test]
    async fn private_targets_are_rejected_before_any_request() {
        let client = HttpClient::new();
        assert!(matches!(
            client.fetch_feed("http://127.0.0.1:9/feed.xml", None, None).await,
            Err(ProviderError::Blocked(_))
        ));
        assert!(matches!(
            client.fetch_raw("http://169.254.169.254/latest").await,
            Err(ProviderError::Blocked(_))
        ));
    }

    #[tokio::test]
    async fn fetch_etag_304() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 4096];
                let n = match s.read(&mut buf) {
                    Err(_) => continue,
                    Ok(n) => n,
                };
                let raw = String::from_utf8_lossy(&buf[..n]).to_string();
                let _ = std::fs::write("/tmp/mock-req.log", raw.clone());
                let req = raw.to_lowercase();
                let body = RSS.as_bytes();
                if req.contains("if-none-match:") {
                    let resp = "HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = s.write_all(resp.as_bytes());
                } else {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nETag: \"v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes());
                    let _ = s.write_all(body);
                }
            }
        });
        let client = HttpClient::new().with_private_hosts(true);
        let url = format!("http://{addr}/feed.xml");
        match client.fetch_feed(&url, None, None).await.unwrap() {
            FetchOutcome::Fetched { etag, .. } => assert_eq!(etag.as_deref(), Some(r#""v1""#)),
            _ => panic!("erwartete Fetched"),
        }
        match client.fetch_feed(&url, Some(r#""v1""#), None).await.unwrap() {
            FetchOutcome::NotModified => {}
            _ => panic!("erwartete NotModified, Request war: {}", std::fs::read_to_string("/tmp/mock-req.log").unwrap_or_default()),
        }
    }
}
