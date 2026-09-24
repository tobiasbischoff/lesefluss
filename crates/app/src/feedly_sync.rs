use crate::dbworker::DbWorker;
use crate::net::Net;
use provider_feedly as pf;


pub fn token_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".config")
        });
    base.join("lesefluss").join("feedly-token")
}

fn keyring_lookup() -> Option<String> {
    let out = std::process::Command::new("secret-tool")
        .args(["lookup", "lesefluss", "feedly-token"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn keyring_store(token: &str) -> bool {
    use std::io::Write;
    let mut child = match std::process::Command::new("secret-tool")
        .args(["store", "--label=Lesefluss: Feedly-Token", "lesefluss", "feedly-token"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(token.trim().as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

pub fn token_from_disk() -> Option<String> {
    if let Some(t) = keyring_lookup() {
        return Some(t);
    }
    let t = std::fs::read_to_string(token_path()).ok()?;
    let t = t.trim().to_string();
    if t.is_empty() {
        return None;
    }
    if keyring_store(&t) {
        let _ = std::fs::remove_file(token_path());
    }
    Some(t)
}

pub fn save_token(token: &str) -> std::io::Result<()> {
    if keyring_store(token) {
        let _ = std::fs::remove_file(token_path());
        return Ok(());
    }
    let p = token_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, token.trim())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

async fn db<T, F>(worker: &DbWorker, f: F) -> T
where
    T: Send + 'static,
    F: FnOnce(&storage::Database) -> T + Send + 'static,
{
    let w = worker.clone();
    tokio::task::spawn_blocking(move || {
        w.send(f)
            .recv()
            .ok()
            .and_then(|b| b.downcast::<T>().ok())
            .map(|b| *b)
            .expect("db antwort")
    })
    .await
    .expect("db join")
}

fn entry_to_new_article(e: &pf::Entry) -> storage::NewArticle {
    storage::NewArticle {
        id: e.id.clone(),
        title: e.title.clone().unwrap_or_else(|| "(ohne Titel)".into()),
        author: e.author.clone(),
        url: e.url(),
        published_ms: e.published.or(e.crawled).unwrap_or_else(storage::now_ms),
        excerpt: e
            .summary
            .as_ref()
            .map(|s| reader::sanitize::sanitize(&s.content, None).html)
            .map(|h| storage::strip_html(&h))
            .unwrap_or_default(),
        html: e.html().map(|h| reader::sanitize::sanitize(&h, e.url().as_deref()).html),
        content_hash: e.html().map(|h| crate::net::content_hash_hex(&h)),
    }
}

async fn ingest_entries(
    worker: &DbWorker,
    account_id: &str,
    entries: Vec<pf::Entry>,
) -> Result<usize, String> {
    let mut per_feed: std::collections::HashMap<i64, Vec<storage::NewArticle>> = std::collections::HashMap::new();
    let mut statuses: Vec<(String, bool, bool)> = Vec::new();
    for e in &entries {
        let Some(stream) = e.feed_stream_id() else { continue };
        let fid = match db(worker, {
            let account_id = account_id.to_string();
            let stream = stream.clone();
            move |db2| db2.feed_id_by_remote(&account_id, &stream)
        })
        .await
        {
            Ok(Some(f)) => f,
            _ => continue,
        };
        statuses.push((e.id.clone(), e.unread.unwrap_or(true), e.is_saved()));
        per_feed.entry(fid).or_default().push(entry_to_new_article(e));
    }
    let mut added = 0usize;
    for (fid, items) in per_feed {
        let (a, _u) = db(worker, move |db2| db2.upsert_articles(fid, &items, storage::now_ms()))
            .await
            .map_err(|e| e.to_string())?;
        added += a;
    }
    let generation = db(worker, {
        let account_id = account_id.to_string();
        move |db2| db2.pull_generation(&account_id)
    })
    .await
    .map_err(|e| e.to_string())?;
    for (id, unread, saved) in statuses {
        db(worker, {
            let account_id = account_id.to_string();
            move |db2| {
                db2.apply_remote_status(
                    &account_id,
                    &id,
                    if unread { None } else { Some(true) },
                    if saved { Some(true) } else { None },
                    generation,
                )?;
                Ok::<_, storage::StorageError>(())
            }
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(added)
}

pub fn initial_sync(worker: DbWorker, net: &Net, token: String) {
    let tx = net.event_sender();
    net.spawn(async move {
        let client = pf::FeedlyClient::new(token);
        let res: storage::Result<usize> = async {
            let profile = client.profile().await.map_err(io_err)?;
            db(&worker, {
                let p = profile.clone();
                move |db2| {
                    db2.upsert_account(&p.id, "feedly", p.full_name.as_deref().or(p.email.as_deref()).unwrap_or("Feedly"))
                }
            })
            .await?;
            let subs = client.subscriptions().await.map_err(io_err)?;
            let cats = client.categories().await.map_err(io_err)?;
            let account_id = profile.id.clone();
            for c in &cats {
                let name = c.label.clone().unwrap_or_else(|| "Gruppe".into());
                db(&worker, {
                    let account_id = account_id.clone();
                    let c = c.clone();
                    move |db2| db2.upsert_group_remote(&account_id, &c.id, &name)
                })
                .await?;
            }
            let group_ids: std::collections::HashMap<String, i64> = db(&worker, |db2| db2.list_groups())
                .await?
                .into_iter()
                .filter_map(|g| g.remote_id.map(|r| (r, g.id)))
                .collect();
            for s in &subs {
                let url = s.id.strip_prefix("feed/").unwrap_or(&s.id).to_string();
                let title = s.title.clone().unwrap_or_else(|| url.clone());
                let fid = db(&worker, {
                    let account_id = account_id.clone();
                    let s = s.clone();
                    let url = url.clone();
                    let title = title.clone();
                    move |db2| db2.upsert_feed_remote(&account_id, &s.id, &url, &title, s.website.as_deref())
                })
                .await?;
                let gids: Vec<i64> = s.categories.iter().filter_map(|c| group_ids.get(&c.id).copied()).collect();
                db(&worker, move |db2| db2.set_feed_groups(fid, &gids)).await?;
            }
            let stream = pf::global_all_stream(&profile.id);
            let newer_than = storage::now_ms() - 30 * 86_400_000;
            let mut continuation: Option<String> = None;
            let mut added = 0usize;
            for _page in 0..100 {
                let page = client
                    .stream_contents(&stream, 100, continuation.as_deref(), Some(newer_than), false)
                    .await
                    .map_err(io_err)?;
                let items = page.items.clone();
                added += ingest_entries(&worker, &account_id, items)
                    .await
                    .map_err(string_err)?;
                match page.continuation {
                    Some(c) if !c.is_empty() => continuation = Some(c),
                    _ => break,
                }
            }
            db(&worker, move |db2| db2.set_last_sync(&account_id, storage::now_ms())).await?;
            Ok(added)
        }
        .await;
        match res {
            Ok(added) => {
                let _ = tx.send(crate::net::NetEvent::FeedlySyncDone { added });
            }
            Err(e) => {
                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed { message: e.to_string() });
            }
        }
    });
}

pub fn process_outbox(worker: DbWorker, net: &Net, token: String, account_id: String) {
    let tx = net.event_sender();
    net.spawn(async move {
        let client = pf::FeedlyClient::new(token);
        let now = storage::now_ms();
        let rows = db(&worker, {
            let account_id = account_id.clone();
            move |db2| db2.outbox_pending(&account_id, now, 100)
        })
        .await;
        let Ok(rows) = rows else { return };
        if rows.is_empty() {
            return;
        }
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        let _ = db(&worker, move |db2| db2.outbox_mark_inflight(&ids)).await;
        let mut groups: std::collections::HashMap<(String, bool), Vec<(i64, i64, String)>> = std::collections::HashMap::new();
        for r in &rows {
            groups
                .entry((r.field.clone(), r.desired))
                .or_default()
                .push((r.id, r.revision, r.entity_id.clone()));
        }
        for ((field, desired), entries) in groups {
            let action = match (field.as_str(), desired) {
                ("read", true) => "markAsRead",
                ("read", false) => "keepUnread",
                ("saved", true) => "markAsSaved",
                _ => "markAsUnsaved",
            };
            let entry_ids: Vec<String> = entries.iter().map(|(_, _, e)| e.clone()).collect();
            let sent: Vec<(i64, i64)> = entries.iter().map(|(id, rev, _)| (*id, *rev)).collect();
            let all_ids: Vec<i64> = entries.iter().map(|(id, _, _)| *id).collect();
            let entries_for_notice: Vec<(String, String)> = entries
                .iter()
                .map(|(_, _, entity)| (account_id.clone(), entity.clone()))
                .collect();
            match client.markers_entries(action, &entry_ids).await {
                Ok(()) => {
                    let _ = db(&worker, move |db2| db2.outbox_ack(&sent)).await;
                }
                Err(pf::FeedlyError::Api { status: 404, message }) => {
                    let permanent_ids: Vec<i64> = sent.iter().map(|(id, _)| *id).collect();
                    let _ = db(&worker, move |db2| {
                        db2.outbox_fail_permanent(&permanent_ids)?;
                        db2.mark_unsynced(&entries_for_notice)?;
                        Ok::<_, storage::StorageError>(())
                    })
                    .await;
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                        message: format!(
                            "404 für {n} Änderungen: {message} — bleiben lokal erhalten und werden nicht erneut gesendet",
                            n = sent.len()
                        ),
                    });
                }
                Err(pf::FeedlyError::Api { status: 429, .. }) => {
                    let next = storage::now_ms() + 5 * 60_000;
                    let _ = db(&worker, move |db2| db2.outbox_fail(&all_ids, next, false)).await;
                }
                Err(pf::FeedlyError::Api { status, message }) if status == 401 || status == 403 => {
                    let next = storage::now_ms() + 15 * 60_000;
                    let _ = db(&worker, move |db2| db2.outbox_fail(&all_ids, next, false)).await;
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                        message: format!("Auth/Rechte ({status}): {message}"),
                    });
                }
                Err(e) => {
                    let next = storage::now_ms() + 60_000;
                    let permanent = false;
                    let _ = db(&worker, move |db2| db2.outbox_fail(&all_ids, next, permanent)).await;
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed { message: e.to_string() });
                }
            }
        }
    });
}

pub fn delta_sync(worker: DbWorker, net: &Net, token: String, account_id: String, last_sync_ms: i64) {
    let tx = net.event_sender();
    net.spawn(async move {
        let client = pf::FeedlyClient::new(token);
        let res: storage::Result<usize> = async {
            let overlap = last_sync_ms - 5 * 60_000;
            let pull_gen = db(&worker, {
                let account_id = account_id.clone();
                move |db2| db2.pull_generation(&account_id)
            })
            .await?;
            if let Ok(reads) = client.markers_reads(overlap, 1000).await {
                for m in reads.entries {
                    let _ = db(&worker, {
                        let account_id = account_id.clone();
                        let id = m.id.clone();
                        move |db2| db2.apply_remote_status(&account_id, &id, Some(true), None, pull_gen)
                    })
                    .await;
                }
            }
            let stream = pf::global_all_stream(&account_id);
            let mut continuation: Option<String> = None;
            let mut added = 0usize;
            for _page in 0..50 {
                let page = client
                    .stream_contents(&stream, 100, continuation.as_deref(), Some(overlap), false)
                    .await
                    .map_err(io_err)?;
                added += ingest_entries(&worker, &account_id, page.items.clone())
                    .await
                    .map_err(string_err)?;
                match page.continuation {
                    Some(c) if !c.is_empty() => continuation = Some(c),
                    _ => break,
                }
            }
            let saved_stream = pf::saved_stream(&account_id);
            let mut continuation: Option<String> = None;
            let mut remote_saved: Vec<String> = Vec::new();
            for _page in 0..50 {
                let page = client
                    .stream_ids(&saved_stream, 1000, continuation.as_deref(), false)
                    .await
                    .map_err(io_err)?;
                let n = page.ids.len();
                remote_saved.extend(page.ids);
                match page.continuation {
                    Some(c) if !c.is_empty() && n > 0 => continuation = Some(c),
                    _ => break,
                }
            }
            let local_saved: Vec<String> = db(&worker, {
                let account_id = account_id.clone();
                move |db2| db2.saved_ids_for_account(&account_id)
            })
            .await?;
            let remote_set: std::collections::HashSet<&str> =
                remote_saved.iter().map(|s| s.as_str()).collect();
            for id in &local_saved {
                if remote_set.contains(id.as_str()) {
                    continue;
                }
                let _ = db(&worker, {
                    let account_id = account_id.clone();
                    let id = id.clone();
                    move |db2| db2.apply_remote_status(&account_id, &id, None, Some(false), pull_gen)
                })
                .await;
            }
            for id in &remote_saved {
                let _ = db(&worker, {
                    let account_id = account_id.clone();
                    let id = id.clone();
                    move |db2| db2.apply_remote_status(&account_id, &id, None, Some(true), pull_gen)
                })
                .await;
            }
            db(&worker, move |db2| db2.set_last_sync(&account_id, storage::now_ms())).await?;
            Ok(added)
        }
        .await;
        match res {
            Ok(added) => {
                let _ = tx.send(crate::net::NetEvent::FeedlySyncDone { added });
            }
            Err(e) => {
                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed { message: e.to_string() });
            }
        }
    });
}

fn string_err(e: String) -> storage::StorageError {
    storage::StorageError::Schema(e)
}

fn io_err(e: pf::FeedlyError) -> storage::StorageError {
    storage::StorageError::Schema(e.to_string())
}
