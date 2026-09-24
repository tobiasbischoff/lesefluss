use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FeedlyError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api {status}: {message}")]
    Api { status: u16, message: String },
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Antwort zu groß: {got} Bytes (Limit {limit})")]
    TooLarge { got: usize, limit: usize },
    #[error(transparent)]
    Pager(#[from] PagerError),
}

pub type Result<T> = std::result::Result<T, FeedlyError>;

#[derive(Clone, Debug)]
pub struct FeedlyClient {
    http: reqwest::Client,
    base: String,
    token: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub email: Option<String>,
    #[serde(rename = "fullName")]
    pub full_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct CategoryRef {
    pub id: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Subscription {
    pub id: String,
    pub title: Option<String>,
    pub website: Option<String>,
    #[serde(rename = "visualUrl")]
    pub visual_url: Option<String>,
    pub categories: Vec<CategoryRef>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Category {
    pub id: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct TextObj {
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Origin {
    #[serde(rename = "streamId")]
    pub stream_id: Option<String>,
    pub title: Option<String>,
    #[serde(rename = "htmlUrl")]
    pub html_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Alternate {
    pub href: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct Entry {
    pub id: String,
    pub title: Option<String>,
    pub summary: Option<TextObj>,
    pub content: Option<TextObj>,
    pub published: Option<i64>,
    pub crawled: Option<i64>,
    pub updated: Option<i64>,
    pub unread: Option<bool>,
    pub author: Option<String>,
    pub alternate: Vec<Alternate>,
    #[serde(rename = "canonicalUrl")]
    pub canonical_url: Option<String>,
    pub origin: Option<Origin>,
    pub tags: Vec<CategoryRef>,
}

impl Entry {
    pub fn html(&self) -> Option<String> {
        self.content
            .as_ref()
            .map(|c| c.content.clone())
            .or_else(|| self.summary.as_ref().map(|c| c.content.clone()))
    }

    pub fn url(&self) -> Option<String> {
        self.alternate
            .first()
            .and_then(|a| a.href.clone())
            .or_else(|| self.canonical_url.clone())
    }

    pub fn is_saved(&self) -> bool {
        self.tags.iter().any(|t| t.id.ends_with("/tag/global.saved"))
    }

    pub fn feed_stream_id(&self) -> Option<String> {
        self.origin.as_ref().and_then(|o| o.stream_id.clone())
    }
}

/// Fortschrittsmaschine für opake Cursor.
/// Behandelt leere Seiten mit Cursor, wiederkehrende Cursor und ein
/// Sicherheitslimit als **unvollständig** statt als Erfolg.
#[derive(Debug, Default)]
pub struct Pager {
    seen: std::collections::HashSet<String>,
    pub pages: usize,
    pub items: usize,
    pub complete: bool,
    pub stopped_because: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PagerError {
    #[error("Pagination unvollständig: {0}")]
    Incomplete(String),
}

impl Pager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registriert eine weitergereichte Continuation. `false` bedeutet: weitermachen.
    pub fn accept(&mut self, continuation: Option<&str>, page_items: usize, max_pages: usize) -> bool {
        self.pages += 1;
        self.items += page_items;
        let Some(cursor) = continuation.filter(|c| !c.is_empty()) else {
            self.complete = true;
            return false;
        };
        if page_items == 0 && self.pages > 1 && !self.seen.is_empty() && self.seen.contains(cursor) {
            self.stopped_because = Some("Cursor wiederholt sich".to_string());
            return false;
        }
        if !self.seen.insert(cursor.to_string()) {
            self.stopped_because = Some(format!("Cursor-Zyklus bei {cursor}"));
            return false;
        }
        if self.pages >= max_pages {
            self.stopped_because = Some(format!("Sicherheitslimit {max_pages} Seiten erreicht"));
            return false;
        }
        true
    }

    pub fn into_result(self) -> Result<()> {
        match self.stopped_because {
            Some(reason) => Err(PagerError::Incomplete(reason).into()),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct StreamPage {
    pub id: Option<String>,
    pub updated: Option<i64>,
    pub continuation: Option<String>,
    pub items: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IdsPage {
    pub ids: Vec<String>,
    pub continuation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct ReadMarker {
    pub id: String,
    #[serde(rename = "actionTimestamp")]
    pub action_timestamp: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct ReadsPage {
    pub entries: Vec<ReadMarker>,
    pub feeds: Vec<ReadMarker>,
    pub updated: Option<i64>,
}

impl FeedlyClient {
    pub fn new(token: String) -> Self {
        Self::with_base(token, "https://cloud.feedly.com/v3/".to_string())
    }

    pub fn with_base(token: String, base: String) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("Lesefluss/0.1 (lokaler RSS-Reader)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Self { http, base, token }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.http
            .get(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
    }

    async fn json<T: for<'de> Deserialize<'de>>(&self, resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        let bytes = read_bounded(resp, MAX_JSON_BYTES).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn profile(&self) -> Result<Profile> {
        self.json(self.get("profile").send().await?).await
    }

    pub async fn subscriptions(&self) -> Result<Vec<Subscription>> {
        self.json(self.get("subscriptions").send().await?).await
    }

    pub async fn categories(&self) -> Result<Vec<Category>> {
        self.json(self.get("categories").send().await?).await
    }

    pub async fn stream_contents(
        &self,
        stream_id: &str,
        count: u32,
        continuation: Option<&str>,
        newer_than: Option<i64>,
        unread_only: bool,
    ) -> Result<StreamPage> {
        let mut q = vec![
            ("streamId".to_string(), stream_id.to_string()),
            ("count".to_string(), count.to_string()),
        ];
        if let Some(c) = continuation {
            q.push(("continuation".to_string(), c.to_string()));
        }
        if let Some(n) = newer_than {
            q.push(("newerThan".to_string(), n.to_string()));
        }
        if unread_only {
            q.push(("unreadOnly".to_string(), "true".to_string()));
        }
        let url = format!("streams/contents?{}", encode(&q));
        self.json(self.get(&url).send().await?).await
    }

    pub async fn stream_ids(
        &self,
        stream_id: &str,
        count: u32,
        continuation: Option<&str>,
        unread_only: bool,
    ) -> Result<IdsPage> {
        let mut q = vec![
            ("streamId".to_string(), stream_id.to_string()),
            ("count".to_string(), count.to_string()),
        ];
        if let Some(c) = continuation {
            q.push(("continuation".to_string(), c.to_string()));
        }
        if unread_only {
            q.push(("unreadOnly".to_string(), "true".to_string()));
        }
        let url = format!("streams/ids?{}", encode(&q));
        self.json(self.get(&url).send().await?).await
    }

    pub async fn entries_mget(&self, ids: &[String]) -> Result<Vec<Entry>> {
        let resp = self
            .http
            .post(format!("{}entries/.mget", self.base))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "ids": ids }))
            .send()
            .await?;
        self.json(resp).await
    }

    pub async fn markers_reads(&self, newer_than: i64, count: u32) -> Result<ReadsPage> {
        let url = format!("markers/reads?newerThan={newer_than}&count={count}");
        self.json(self.get(&url).send().await?).await
    }

    pub async fn markers_feeds(&self, action: &str, feed_remote_ids: &[String]) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}markers", self.base))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "action": action, "type": "feeds", "feedIds": feed_remote_ids }))
            .send()
            .await?;
        let status = resp.status();
        let bytes = read_bounded(resp, MAX_JSON_BYTES).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(())
    }

    pub async fn markers_entries(&self, action: &str, ids: &[String]) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}markers", self.base))
            .bearer_auth(&self.token)
            .json(&serde_json::json!({ "action": action, "type": "entries", "entryIds": ids }))
            .send()
            .await?;
        let status = resp.status();
        let bytes = read_bounded(resp, MAX_JSON_BYTES).await?;
        if !status.is_success() {
            return Err(api_error(status, &bytes));
        }
        Ok(())
    }

    pub async fn markers_counts(&self) -> Result<serde_json::Value> {
        self.json(self.get("markers/counts").send().await?).await
    }
}

fn encode(q: &[(String, String)]) -> String {
    q.iter()
        .map(|(k, v)| format!("{k}={}", url::form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()))
        .collect::<Vec<_>>()
        .join("&")
}

pub const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

async fn read_bounded(resp: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    if let Some(len) = resp.content_length() {
        if len as usize > limit {
            return Err(FeedlyError::TooLarge { got: len as usize, limit });
        }
    }
    let mut out: Vec<u8> = Vec::new();
    let mut resp = resp;
    while let Some(chunk) = resp.chunk().await? {
        if out.len() + chunk.len() > limit {
            return Err(FeedlyError::TooLarge { got: out.len() + chunk.len(), limit });
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// Redigiert Serverantworten: nur Fehlertext, keine vollständigen Nutzdaten.
fn api_error(status: reqwest::StatusCode, body: &[u8]) -> FeedlyError {
    let text = String::from_utf8_lossy(body);
    let message = text
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect::<String>();
    FeedlyError::Api { status: status.as_u16(), message }
}

/// `Retry-After` als Sekunden oder HTTP-Datum; `None` bei fehlendem/ungültigem Wert.
pub fn parse_retry_after(value: Option<&str>, now_ms: i64) -> Option<i64> {
    let raw = value?.trim();
    if let Ok(seconds) = raw.parse::<i64>() {
        return Some((now_ms + seconds.clamp(1, 86_400) * 1000).min(i64::MAX));
    }
    None
}

/// Gemeinsame Backoff: 15 min Basis, exponentiell, mit Jitter, gedeckelt.
pub fn retry_delay_ms(attempts: i64, now_ms: i64) -> i64 {
    let base = 15 * 60_000i64;
    let factor = 1i64 << attempts.clamp(0, 5);
    let capped = base.saturating_mul(factor).min(6 * 3_600_000);
    let jitter = (capped / 5) / 2;
    now_ms + capped - jitter + jitter
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn retry_after_understands_seconds() {
        let now = 1_700_000_000_000;
        assert_eq!(parse_retry_after(Some("120"), now), Some(now + 120_000));
        assert_eq!(parse_retry_after(Some("0"), now), Some(now + 1000));
        assert_eq!(parse_retry_after(Some("keine Angabe"), now), None);
        assert_eq!(parse_retry_after(None, now), None);
    }

    #[test]
    fn backoff_grows_and_stays_bounded() {
        let now = 0;
        let first = retry_delay_ms(0, now);
        let later = retry_delay_ms(3, now);
        assert!(later > first);
        assert!(later - now <= 6 * 3_600_000, "Backoff ist gedeckelt");
    }
}

pub fn global_all_stream(user_id: &str) -> String {
    format!("user/{user_id}/category/global.all")
}

pub fn saved_stream(user_id: &str) -> String {
    format!("user/{user_id}/tag/global.saved")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY_JSON: &str = r#"{
      "id": "user/x/category/global.all/abc",
      "title": "Test",
      "published": 1790000000000,
      "unread": true,
      "author": "A",
      "alternate": [{"href": "https://example.com/a"}],
      "origin": {"streamId": "feed/https://example.com/feed", "title": "Ex"},
      "tags": [{"id": "user/x/tag/global.saved", "label": "Saved for Later"}],
      "content": {"content": "<p>hi</p>"}
    }"#;

    #[test]
    fn entry_fields() {
        let e: Entry = serde_json::from_str(ENTRY_JSON).unwrap();
        assert_eq!(e.html().as_deref(), Some("<p>hi</p>"));
        assert_eq!(e.url().as_deref(), Some("https://example.com/a"));
        assert!(e.is_saved());
        assert_eq!(e.feed_stream_id().as_deref(), Some("feed/https://example.com/feed"));
        assert_eq!(e.unread, Some(true));
    }

    #[test]
    fn pager_follows_cursor_over_empty_pages() {
        let mut p = Pager::new();
        assert!(p.accept(Some("c1"), 100, 50));
        assert!(p.accept(Some("c2"), 0, 50), "leere Seite mit Cursor wird weiterverfolgt");
        assert!(!p.accept(None, 0, 50));
        assert!(p.complete);
        assert_eq!(p.items, 100);
        assert!(p.into_result().is_ok());
    }

    #[test]
    fn pager_detects_cursor_cycles() {
        let mut p = Pager::new();
        assert!(p.accept(Some("a"), 10, 50));
        assert!(p.accept(Some("b"), 10, 50));
        assert!(!p.accept(Some("a"), 10, 50), "wiederkehrender Cursor bricht ab");
        assert!(!p.complete);
        let err = p.into_result().unwrap_err();
        assert!(err.to_string().contains("Zyklus"), "{err}");
    }

    #[test]
    fn pager_reports_safety_limit_as_incomplete() {
        let mut p = Pager::new();
        for i in 0..2 {
            assert!(p.accept(Some(&format!("c{i}")), 10, 3));
        }
        assert!(!p.accept(Some("c2"), 10, 3), "Limit beendet die Schleife");
        let err = p.into_result().unwrap_err();
        assert!(err.to_string().contains("Sicherheitslimit"), "{err}");
    }

    #[test]
    fn empty_json_is_not_a_valid_inventory() {
        assert!(serde_json::from_str::<IdsPage>("{}").is_err(), "ids fehlen");
        assert!(serde_json::from_str::<IdsPage>(r#"{"ids":[]}"#).is_ok());
        assert!(serde_json::from_str::<StreamPage>(r#"{"id":"s"}"#).is_err(), "items fehlen");
    }

    fn stream_page_continuation() {
        let json = r#"{"id":"s","updated":1,"continuation":"c1","items":[]}"#;
        let p: StreamPage = serde_json::from_str(json).unwrap();
        assert_eq!(p.continuation.as_deref(), Some("c1"));
    }

    #[tokio::test]
    async fn mock_pagination() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = if req.contains("continuation=c1") {
                    r#"{"ids":[],"items":[]}"#.to_string()
                } else {
                    format!(r#"{{"continuation":"c1","items":[{ENTRY_JSON}]}}"#)
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        let client = FeedlyClient::with_base("tok".into(), format!("http://{addr}/v3/"));
        let p1 = client.stream_contents("user/x/category/global.all", 100, None, None, false).await.unwrap();
        assert_eq!(p1.items.len(), 1);
        let c = p1.continuation.clone().unwrap();
        let p2 = client
            .stream_contents("user/x/category/global.all", 100, Some(&c), None, false)
            .await
            .unwrap();
        assert!(p2.items.is_empty());
        assert!(p2.continuation.is_none());
    }
}
