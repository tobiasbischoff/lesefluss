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

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
pub struct StreamPage {
    pub id: Option<String>,
    pub updated: Option<i64>,
    pub continuation: Option<String>,
    pub items: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(default)]
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
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let message = String::from_utf8_lossy(&bytes).chars().take(300).collect();
            return Err(FeedlyError::Api { status: status.as_u16(), message });
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
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let message = String::from_utf8_lossy(&bytes).chars().take(300).collect();
            return Err(FeedlyError::Api { status: status.as_u16(), message });
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
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let message = String::from_utf8_lossy(&bytes).chars().take(300).collect();
            return Err(FeedlyError::Api { status: status.as_u16(), message });
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
