pub mod media;
pub mod netpolicy;

use std::collections::HashSet;

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

pub async fn read_bounded(resp: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
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

const MAX_REDIRECTS: usize = 5;

impl HttpClient {
    pub fn new() -> Self {
        Self::with_trusted(Vec::new())
    }

    fn with_trusted(trusted_origins: Vec<String>) -> Self {
        // Redirects werden nicht automatisch verfolgt: jeder Hop wird vorher geprüft.
        // Der Resolver gibt ausschließlich policy-konforme Adressen an den Connector.
        let resolver = std::sync::Arc::new(netpolicy::PolicyResolver::new(trusted_origins.clone()));
        let inner = reqwest::Client::builder()
            .user_agent("Lesefluss/0.1 (+https://example.org; lokaler RSS-Reader)")
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .dns_resolver(resolver)
            .build()
            .expect("reqwest client");
        Self {
            inner,
            trusted_origins,
            allow_private_hosts: false,
        }
    }

    /// Ein GET, bei dem jede Weiterleitung vor dem Kontaktieren des Ziels geprüft wird.
    async fn get_checked(&self, url: &str, headers: &[(&str, &str)]) -> Result<reqwest::Response> {
        let mut current = self.guard(url)?;
        for _ in 0..=MAX_REDIRECTS {
            let mut req = self.inner.get(current.clone());
            for (name, value) in headers {
                req = req.header(*name, *value);
            }
            let resp = req.send().await?;
            let status = resp.status();
            let is_redirect = matches!(
                status,
                reqwest::StatusCode::MOVED_PERMANENTLY
                    | reqwest::StatusCode::FOUND
                    | reqwest::StatusCode::SEE_OTHER
                    | reqwest::StatusCode::TEMPORARY_REDIRECT
                    | reqwest::StatusCode::PERMANENT_REDIRECT
            );
            if !is_redirect {
                return Ok(resp);
            }
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    ProviderError::Blocked(format!("Weiterleitung ohne Ziel ({status})"))
                })?
                .to_string();
            let next = current.join(&location).map_err(|_| {
                ProviderError::Blocked(format!("Ungültiges Weiterleitungsziel: {location}"))
            })?;
            current = self.guard(next.as_str())?;
        }
        Err(ProviderError::Blocked(format!(
            "Zu viele Weiterleitungen (>{MAX_REDIRECTS})"
        )))
    }

    pub async fn fetch_feed(
        &self,
        url: &str,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<FetchOutcome> {
        let mut conditional: Vec<(&str, &str)> = Vec::new();
        if let Some(e) = etag {
            conditional.push(("If-None-Match", e));
        }
        if let Some(lm) = last_modified {
            conditional.push(("If-Modified-Since", lm));
        }
        let resp = self.get_checked(url, &conditional).await?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(FetchOutcome::NotModified);
        }
        if !resp.status().is_success() {
            return Err(ProviderError::Parse(format!("HTTP {}", resp.status())));
        }
        let new_etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok().map(str::to_string));
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
        let resp = self.get_checked(url, &[]).await?;
        if !resp.status().is_success() {
            return Err(ProviderError::Parse(format!("HTTP {}", resp.status())));
        }
        let final_url = resp.url().to_string();
        self.guard(&final_url)?;
        let bytes = read_bounded(resp, media::MAX_IMAGE_BYTES).await?;
        Ok((final_url, bytes))
    }

    pub async fn fetch_raw(&self, url: &str) -> Result<(String, Vec<u8>)> {
        let resp = self.get_checked(url, &[]).await?;
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
        let identity = identity_of(
            &e.id,
            link.as_deref(),
            &e.title
                .as_ref()
                .map(|t| t.content.clone())
                .unwrap_or_default(),
            published,
        );
        out.items.push(ParsedItem {
            identity,
            title: e
                .title
                .map(|t| t.content)
                .unwrap_or_else(|| "(ohne Titel)".into()),
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
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(400)
        .collect()
}

pub fn identity_of(
    guid: &str,
    url: Option<&str>,
    title: &str,
    published_ms: Option<i64>,
) -> String {
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
    let Ok(mut u) = url::Url::parse(raw) else {
        return raw.to_string();
    };
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
        cands.push(DiscoverCandidate {
            url: normalize_url(&abs),
            title,
        });
    }
    if cands.is_empty() {
        for guess in [
            "/feed",
            "/rss",
            "/feed.xml",
            "/rss.xml",
            "/atom.xml",
            "/index.xml",
        ] {
            let candidate = url::Url::parse(&final_url)
                .and_then(|b| b.join(guess))
                .map(|u| u.to_string())
                .unwrap_or_default();
            if let Ok((_, b2)) = client.fetch_raw(&candidate).await {
                if looks_like_feed(&b2) {
                    let parsed = parse_feed(&b2)?;
                    cands.push(DiscoverCandidate {
                        url: normalize_url(&candidate),
                        title: parsed
                            .title
                            .unwrap_or_else(|| host_of(&candidate).to_string()),
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
    // ASCII-Kleinschreibung ist längenstabil; Unicode-Kleinschreibung ist es nicht.
    let head: Vec<u8> = bytes
        .iter()
        .take(2048)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    let head = String::from_utf8_lossy(&head);
    head.contains("<rss") || head.contains("<feed") || head.contains("<rdf:rdf")
}

fn extract_alternate_links(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let document = scraper::Html::parse_document(html);
    let Ok(selector) = scraper::Selector::parse("link[rel][type][href]") else {
        return out;
    };
    let mut seen = HashSet::new();
    for element in document.select(&selector) {
        let value = element.value();
        let rel = value.attr("rel").unwrap_or_default().to_ascii_lowercase();
        let mime = value.attr("type").unwrap_or_default().to_ascii_lowercase();
        if !rel.split_whitespace().any(|r| r == "alternate") {
            continue;
        }
        if !matches!(
            mime.trim(),
            "application/rss+xml"
                | "application/atom+xml"
                | "application/feed+json"
                | "application/xml"
                | "text/xml"
        ) {
            continue;
        }
        let Some(href) = value.attr("href") else {
            continue;
        };
        let href = href.trim();
        if href.is_empty() {
            continue;
        }
        match url::Url::parse(href) {
            Ok(parsed) if netpolicy::scheme_allowed(&parsed) => {}
            Ok(_) => continue,
            Err(_) if href.starts_with("//") => continue,
            Err(_) => {}
        }
        if !seen.insert(href.to_string()) {
            continue;
        }
        let title = value.attr("title").unwrap_or("Feed").to_string();
        out.push((href.to_string(), title));
    }
    out
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
        assert!(f.items[0]
            .content_html
            .as_deref()
            .unwrap()
            .contains("Volltext"));
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
        assert_eq!(
            f.items[0].url.as_deref(),
            Some("https://example.com/atom/1")
        );
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
        assert_eq!(
            normalize_url("https://Example.com:443/a/"),
            "https://example.com/a/"
        );
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

    #[test]
    fn discovery_ignores_dangerous_schemes_and_unicode_traps() {
        let html = "<html><head>\
<link rel=\"alternate\" type=\"application/rss+xml\" href=\"javascript:alert(1)\">\
<link rel=\"alternate\" type=\"application/rss+xml\" href=\"file:///etc/passwd\">\
<link rel=\"alternate\" type=\"application/rss+xml\" href=\"https://ok.example/f.xml\">\
</head><body>İİİ</body></html>";
        let cands = extract_alternate_links(html);
        assert_eq!(cands.len(), 1, "nur https bleibt: {cands:?}");
        assert_eq!(cands[0].0, "https://ok.example/f.xml");
        assert!(!looks_like_feed(
            b"<?xml version=\"1.0\"?><html><body>kein Feed</body></html>"
        ));
        assert!(looks_like_feed(
            b"<?xml version=\"1.0\"?><rss version=\"2.0\"></rss>"
        ));
        assert!(looks_like_feed(
            "<feed xmlns=\"http://www.w3.org/2005/Atom\">".as_bytes()
        ));
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
                    if s.write_all(format!("{:x}\r\n", block.len()).as_bytes())
                        .is_err()
                    {
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
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                counter.fetch_add(1, Ordering::SeqCst);
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        let client = HttpClient::new();
        let url = format!("http://{addr}/feed.xml");
        assert!(matches!(
            client.fetch_feed(&url, None, None).await,
            Err(ProviderError::Blocked(_))
        ));
        assert!(matches!(
            client.fetch_raw(&url).await,
            Err(ProviderError::Blocked(_))
        ));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "der lokale Mock darf überhaupt keinen Request erhalten"
        );
    }

    #[tokio::test]
    async fn weiterleitung_auf_ungueltiges_ziel_wird_vor_dem_request_geprueft() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let ziel = TcpListener::bind("127.0.0.1:0").unwrap();
        let ziel_addr = ziel.local_addr().unwrap();
        let ziel_hits = Arc::new(AtomicUsize::new(0));
        let ziel_counter = ziel_hits.clone();
        std::thread::spawn(move || {
            for stream in ziel.incoming() {
                ziel_counter.fetch_add(1, Ordering::SeqCst);
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        let quelle = TcpListener::bind("127.0.0.1:0").unwrap();
        let quelle_addr = quelle.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in quelle.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let location = format!("http://{ziel_addr}/ziel");
                let head = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
                );
                let _ = s.write_all(head.as_bytes());
            }
        });
        // Nur die Quell-Origin ist bewusst freigegeben, das Weiterleitungsziel nicht.
        let client = HttpClient::new().with_trusted_origin(&format!("http://{quelle_addr}"));
        let url = format!("http://{quelle_addr}/start");
        let res = client.fetch_raw(&url).await;
        assert!(
            matches!(res, Err(ProviderError::Blocked(_))),
            "Weiterleitungsziel muss abgewiesen werden: {res:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(
            ziel_hits.load(Ordering::SeqCst),
            0,
            "das Weiterleitungsziel erhält keinen Request"
        );
    }

    #[tokio::test]
    async fn erlaubte_weiterleitung_wird_verfolgt() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        let ziel = TcpListener::bind("127.0.0.1:0").unwrap();
        let ziel_addr = ziel.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in ziel.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        let quelle = TcpListener::bind("127.0.0.1:0").unwrap();
        let quelle_addr = quelle.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in quelle.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let location = format!("http://{ziel_addr}/ziel");
                let head = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
                );
                let _ = s.write_all(head.as_bytes());
            }
        });
        let client = HttpClient::new()
            .with_trusted_origin(&format!("http://{quelle_addr}"))
            .with_trusted_origin(&format!("http://{ziel_addr}"));
        let url = format!("http://{quelle_addr}/start");
        let (final_url, body) = client.fetch_raw(&url).await.unwrap();
        assert_eq!(body, b"ok");
        assert!(final_url.contains(&format!("{ziel_addr}")));
    }

    #[tokio::test]
    async fn resolver_liefert_keine_gesperrten_adressen() {
        use reqwest::dns::Resolve as _;
        use std::str::FromStr;
        let resolver = netpolicy::PolicyResolver::new(vec!["http://127.0.0.1:8080".to_string()]);
        let name = reqwest::dns::Name::from_str("localhost").unwrap();
        let outcome = resolver.resolve(name).await;
        assert!(
            outcome.is_err(),
            "localhost liefert keine freigegebenen Adressen, sondern einen Fehler"
        );
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
        match client
            .fetch_feed(&url, Some(r#""v1""#), None)
            .await
            .unwrap()
        {
            FetchOutcome::NotModified => {}
            _ => panic!(
                "erwartete NotModified, Request war: {}",
                std::fs::read_to_string("/tmp/mock-req.log").unwrap_or_default()
            ),
        }
    }
}
