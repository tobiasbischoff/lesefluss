use crate::dbworker::DbWorker;
use provider_local::{DiscoverCandidate, FetchOutcome, HttpClient};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use storage::{FetchState, NewArticle};
use tokio::sync::Semaphore;

pub const BASE_INTERVAL_MS: i64 = 30 * 60 * 1000;

#[derive(Clone, Debug)]
pub enum NetEvent {
    FetchStarted(i64),
    FetchDone {
        feed_id: i64,
        added: usize,
        updated: usize,
        title: Option<String>,
        website: Option<String>,
    },
    FetchNotModified(i64),
    FetchFailed {
        feed_id: i64,
        message: String,
    },
    DiscoveryDone {
        input: String,
        candidates: Vec<DiscoverCandidate>,
    },
    DiscoveryFailed {
        input: String,
        message: String,
    },
    FeedlySyncDone {
        added: usize,
    },
    FeedlySyncFailed {
        message: String,
    },
}

pub struct Net {
    pub events: std::sync::mpsc::Receiver<NetEvent>,
    tx: std::sync::mpsc::Sender<NetEvent>,
    rt: tokio::runtime::Runtime,
    http: HttpClient,
}

fn global_sem() -> &'static Semaphore {
    static S: OnceLock<Semaphore> = OnceLock::new();
    S.get_or_init(|| Semaphore::new(6))
}

fn host_sem(host: &str) -> Arc<Semaphore> {
    static M: OnceLock<Mutex<HashMap<String, Arc<Semaphore>>>> = OnceLock::new();
    let map = M.get_or_init(|| Mutex::new(HashMap::new()));
    let mut g = map.lock().expect("host sem map");
    g.entry(host.to_string()).or_insert_with(|| Arc::new(Semaphore::new(2))).clone()
}

fn jitter(ms: i64) -> i64 {
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as i64)
        .unwrap_or(0);
    let factor = 0.9 + (nano % 200) as f64 / 1000.0;
    (ms as f64 * factor) as i64
}

pub fn content_hash_hex(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn backoff_ms(error_count: i64) -> i64 {
    let shift = error_count.clamp(0, 4);
    (BASE_INTERVAL_MS << shift).min(24 * 60 * 60 * 1000)
}

impl Net {
    pub fn start() -> Self {
        let (tx, events) = std::sync::mpsc::channel();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("lf-net")
            .enable_all()
            .build()
            .expect("tokio runtime");
        Self { events, tx, rt, http: HttpClient::new() }
    }

    pub fn spawn_scheduler(&self, worker: DbWorker) {
        let net_self = NetHandle {
            tx: self.tx.clone(),
            rt_handle: self.rt.handle().clone(),
            http: self.http.clone(),
        };
        std::thread::Builder::new()
            .name("lf-sched".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(30));
                let now = storage::now_ms();
                let rx = worker.send(move |db| db.due_feeds(now));
                let due = match rx.recv() {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let due = match due.downcast_ref::<storage::Result<Vec<(i64, String)>>>() {
                    Some(Ok(d)) => d.clone(),
                    _ => continue,
                };
                for (feed_id, url) in due {
                    net_self.fetch_feed(worker.clone(), feed_id, url, false);
                }
            })
            .expect("scheduler thread");
    }

    pub fn fetch_feed(&self, worker: DbWorker, feed_id: i64, url: String, force: bool) {
        let http = self.http.clone();
        let tx = self.tx.clone();
        let handle = self.rt.handle().clone();
        handle.spawn(async move {
            fetch_and_store(worker, http, tx, feed_id, url, force).await;
        });
    }

    pub fn event(&self, ev: NetEvent) {
        let _ = self.tx.send(ev);
    }

    pub fn event_sender(&self) -> std::sync::mpsc::Sender<NetEvent> {
        self.tx.clone()
    }

    pub fn http(&self) -> HttpClient {
        self.http.clone()
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.rt.spawn(fut);
    }

    pub fn discover(&self, input: String) {
        let http = self.http.clone();
        let tx = self.tx.clone();
        let handle = self.rt.handle().clone();
        handle.spawn(async move {
            match provider_local::discover(&http, &input).await {
                Ok(candidates) => {
                    let _ = tx.send(NetEvent::DiscoveryDone { input, candidates });
                }
                Err(e) => {
                    let _ = tx.send(NetEvent::DiscoveryFailed { input, message: e.to_string() });
                }
            }
        });
    }
}

#[derive(Clone)]
struct NetHandle {
    tx: std::sync::mpsc::Sender<NetEvent>,
    rt_handle: tokio::runtime::Handle,
    http: HttpClient,
}

impl NetHandle {
    fn fetch_feed(&self, worker: DbWorker, feed_id: i64, url: String, force: bool) {
        let http = self.http.clone();
        let tx = self.tx.clone();
        let handle = self.rt_handle.clone();
        handle.spawn(async move {
            fetch_and_store(worker, http, tx, feed_id, url, force).await;
        });
    }
}

async fn db_call<T, F>(worker: &DbWorker, f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce(&storage::Database) -> T + Send + 'static,
{
    let worker = worker.clone();
    tokio::task::spawn_blocking(move || worker.send(f).recv().ok()?.downcast::<T>().ok().map(|b| *b))
        .await
        .ok()?
}

async fn fetch_and_store(
    worker: DbWorker,
    http: HttpClient,
    tx: std::sync::mpsc::Sender<NetEvent>,
    feed_id: i64,
    url: String,
    force: bool,
) {
    let _ = tx.send(NetEvent::FetchStarted(feed_id));
    let host = url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let _global = match global_sem().acquire().await {
        Ok(g) => g,
        Err(_) => return,
    };
    let host_sem = host_sem(&host);
    let _per_host = match host_sem.acquire().await {
        Ok(g) => g,
        Err(_) => return,
    };

    let st: FetchState = db_call(&worker, move |db| db.fetch_state(feed_id))
        .await
        .and_then(|r| r.ok())
        .unwrap_or_default();
    let now = storage::now_ms();
    if !force {
        if let Some(next) = st.next_fetch_ms {
            if next > now {
                return;
            }
        }
    }

    let outcome = http.fetch_feed(&url, st.etag.as_deref(), st.last_modified.as_deref()).await;
    let now = storage::now_ms();
    match outcome {
        Ok(FetchOutcome::NotModified) => {
            let next = now + jitter(BASE_INTERVAL_MS);
            let keep = st.clone();
            let _ = db_call(&worker, move |db| {
                db.set_fetch_state(
                    feed_id,
                    &FetchState {
                        etag: keep.etag.clone(),
                        last_modified: keep.last_modified.clone(),
                        last_fetch_ms: Some(now),
                        next_fetch_ms: Some(next),
                        error_count: 0,
                        last_error: None,
                    },
                )
            })
            .await;
            let _ = tx.send(NetEvent::FetchNotModified(feed_id));
        }
        Ok(FetchOutcome::Fetched { bytes, etag, last_modified, final_url }) => {
            let parsed = tokio::task::spawn_blocking(move || provider_local::parse_feed(&bytes)).await;
            match parsed {
                Ok(Ok(pf)) => {
                    let site = pf.website.clone();
                    let mut media_map: Vec<(String, Vec<String>)> = Vec::new();
                    let items: Vec<NewArticle> = pf
                        .items
                        .into_iter()
                        .map(|i| {
                            let (html, imgs) = match i.content_html {
                                Some(h) => {
                                    let base = i.url.clone().or_else(|| site.clone());
                                    let cr = reader::sanitize::sanitize(&h, base.as_deref());
                                    (Some(cr.html), cr.images)
                                }
                                None => (None, Vec::new()),
                            };
                            let content_hash = html.as_deref().map(content_hash_hex);
                            media_map.push((i.identity.clone(), imgs));
                            NewArticle {
                                id: i.identity,
                                title: i.title,
                                author: i.author,
                                url: i.url,
                                published_ms: i.published_ms.unwrap_or(now),
                                excerpt: i.excerpt,
                                html,
                                content_hash,
                            }
                        })
                        .collect();
                    let title = pf.title.clone();
                    let website = pf.website.clone();
                    let final_url2 = final_url.clone();
                    let res = db_call(&worker, move |db| {
                        let up = db.upsert_articles(feed_id, &items, now)?;
                        for (id, imgs) in &media_map {
                            db.set_article_media(feed_id, id, imgs)?;
                        }
                        if let Some(t) = pf.title.as_deref() {
                            db.update_feed_title(feed_id, t, pf.website.as_deref())?;
                        }
                        let _ = final_url2;
                        db.set_fetch_state(
                            feed_id,
                            &FetchState {
                                etag,
                                last_modified,
                                last_fetch_ms: Some(now),
                                next_fetch_ms: Some(now + jitter(BASE_INTERVAL_MS)),
                                error_count: 0,
                                last_error: None,
                            },
                        )?;
                        Ok::<(usize, usize), storage::StorageError>(up)
                    })
                    .await;
                    match res {
                        Some(Ok((added, updated))) => {
                            let _ = tx.send(NetEvent::FetchDone { feed_id, added, updated, title, website });
                        }
                        Some(Err(e)) => {
                            let _ = tx.send(NetEvent::FetchFailed { feed_id, message: e.to_string() });
                        }
                        None => {
                            let _ = tx.send(NetEvent::FetchFailed { feed_id, message: "db".into() });
                        }
                    }
                }
                Ok(Err(e)) => fail(&worker, &tx, feed_id, st.error_count, e.to_string(), now).await,
                Err(e) => fail(&worker, &tx, feed_id, st.error_count, e.to_string(), now).await,
            }
        }
        Err(e) => fail(&worker, &tx, feed_id, st.error_count, e.to_string(), now).await,
    }
}

async fn fail(
    worker: &DbWorker,
    tx: &std::sync::mpsc::Sender<NetEvent>,
    feed_id: i64,
    prev_errors: i64,
    message: String,
    now: i64,
) {
    let errors = prev_errors + 1;
    let next = now + jitter(backoff_ms(errors));
    let msg2 = message.clone();
    let _ = db_call(worker, move |db| db.update_fetch_error(feed_id, errors, &msg2, next, now)).await;
    let _ = tx.send(NetEvent::FetchFailed { feed_id, message });
}
