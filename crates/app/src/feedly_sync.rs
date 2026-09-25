use crate::dbworker::DbWorker;
use crate::net::Net;
use crate::window::dbg_log;
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

/// Alle Schlüsselbundzugriffe laufen über einen Aufruf mit Zeitgrenze. Ein
/// blockierender oder hängender Secret Service darf weder den Worker noch die
/// Oberfläche dauerhaft festhalten.
const KEYRING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn secret_tool(args: &[&str], stdin_data: Option<&[u8]>) -> Option<std::process::Output> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("secret-tool")
        .args(args)
        .stdin(if stdin_data.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let (Some(data), Some(mut pipe)) = (stdin_data, child.stdin.take()) {
        let _ = pipe.write_all(data);
        drop(pipe);
    }
    let deadline = std::time::Instant::now() + KEYRING_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("[lf] secret-tool hat die Zeitgrenze überschritten");
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
    child.wait_with_output().ok()
}

fn secret_tool_text(args: &[&str], stdin_data: Option<&[u8]>) -> Option<String> {
    let out = secret_tool(args, stdin_data)?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Der Schlüsselbund-Eintrag ist an das bestätigte Profil gebunden, damit ein
/// Token nicht versehentlich mit der Outbox eines anderen Kontos gekoppelt wird.
pub fn keyring_account() -> Option<String> {
    secret_tool_text(&["lookup", "lesefluss", "feedly-account"], None)
}

pub fn bind_account(account_id: &str) -> bool {
    secret_tool_text(
        &[
            "store",
            "--label=Lesefluss: Feedly-Konto",
            "lesefluss",
            "feedly-account",
        ],
        Some(account_id.as_bytes()),
    )
    .is_some()
}

/// Löscht Token **und** Kontobindung und meldet zurück, ob etwas entfernt wurde.
pub fn forget_token() -> bool {
    forget_token_at(&token_path())
}

pub fn forget_token_at(path: &std::path::Path) -> bool {
    let mut removed = false;
    for key in ["feedly-token", "feedly-account"] {
        if let Some(out) = secret_tool(&["clear", "lesefluss", key], None) {
            removed |= out.status.success();
        }
    }
    if path.exists() {
        match std::fs::remove_file(&path) {
            Ok(()) => removed = true,
            Err(e) => eprintln!("[lf] Token-Datei konnte nicht entfernt werden: {e}"),
        }
    }
    removed
}

fn keyring_lookup() -> Option<String> {
    secret_tool_text(&["lookup", "lesefluss", "feedly-token"], None)
}

fn keyring_store(token: &str) -> bool {
    secret_tool_text(
        &[
            "store",
            "--label=Lesefluss: Feedly-Token",
            "lesefluss",
            "feedly-token",
        ],
        Some(token.trim().as_bytes()),
    )
    .is_some()
}

/// Liest das Token ohne den GTK-Thread zu blockieren:
/// Datei- und Schlüsselbundzugriff laufen in einem Worker.
pub async fn token_from_disk_async() -> Option<String> {
    tokio::task::spawn_blocking(token_from_disk)
        .await
        .ok()
        .flatten()
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

pub fn save_token(token: &str, account_id: Option<&str>) -> std::io::Result<()> {
    if keyring_store(token) {
        if let Some(acc) = account_id {
            let _ = bind_account(acc);
        }
        let _ = std::fs::remove_file(token_path());
        return Ok(());
    }
    let p = token_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    write_private(&p, token.trim().as_bytes())
}

/// Ersetzt die Token-Datei atomar über eine eigene temporäre Datei. `rename` folgt
/// keinem Symlink; ein vorhandener Symlink auf die Token-Datei wird damit ersetzt
/// statt beschrieben.
#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "kein Verzeichnis"))?;
    let unique = format!(
        ".{}.{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "token".into()),
        std::process::id()
    );
    let tmp = dir.join(unique);
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    let result = std::fs::rename(&tmp, path).and_then(|_| {
        if let Ok(handle) = std::fs::File::open(dir) {
            let _ = handle.sync_all();
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
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
            .unwrap_or_else(|| {
                eprintln!("[lf] Datenbank-Worker antwortet nicht — Anfrage verworfen");
                panic!("db worker unavailable");
            })
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
        html: e
            .html()
            .map(|h| reader::sanitize::sanitize(&h, e.url().as_deref()).html),
        content_hash: e.html().map(|h| crate::net::content_hash_hex(&h)),
    }
}

async fn ingest_entries(
    worker: &DbWorker,
    account_id: &str,
    generation: i64,
    entries: Vec<pf::Entry>,
) -> Result<usize, String> {
    let mut per_feed: std::collections::HashMap<i64, Vec<storage::NewArticle>> =
        std::collections::HashMap::new();
    let mut per_feed_media: Vec<(i64, Vec<(String, Vec<String>)>)> = Vec::new();
    let mut statuses: Vec<(String, bool, bool)> = Vec::new();
    for e in &entries {
        let Some(stream) = e.feed_stream_id() else {
            continue;
        };
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
        let urls =
            reader::sanitize::image_sources(&e.html().unwrap_or_else(|| "<p></p>".to_string()));
        per_feed_media.push((fid, vec![(e.id.clone(), urls)]));
        per_feed
            .entry(fid)
            .or_default()
            .push(entry_to_new_article(e));
    }
    let mut added = 0usize;
    for (fid, items) in per_feed {
        let (a, _u) = db(worker, move |db2| {
            db2.upsert_articles(fid, &items, storage::now_ms())
        })
        .await
        .map_err(|e| e.to_string())?;
        added += a;
    }
    for (fid, images) in per_feed_media {
        db(worker, move |db2| {
            for (article_id, urls) in images {
                db2.set_article_media(fid, &article_id, &urls)?;
            }
            Ok::<_, storage::StorageError>(())
        })
        .await
        .map_err(|e| e.to_string())?;
    }
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

/// Sicherheitsgrenzen gegen Endlosschleifen bei defekten Cursorn.
const MAX_CONTENT_PAGES: usize = 50;
const MAX_ID_PAGES: usize = 50;
const MAX_READS_PAGES: usize = 50;

/// Lädt **alle** Seiten eines ID-Inventars.
/// Nur `Inventory::Complete` darf eine Reconciliation auslösen; jeder Fehler,
/// Cursor-Zyklus oder das Sicherheitslimit liefert `Incomplete`.
pub async fn fetch_id_inventory(
    client: &pf::FeedlyClient,
    stream: &str,
    max_pages: usize,
) -> Result<pf::Inventory, storage::StorageError> {
    let mut pager = pf::Pager::new();
    let mut continuation: Option<String> = None;
    let mut ids: Vec<String> = Vec::new();
    loop {
        let page = client
            .stream_ids(stream, 1000, continuation.as_deref(), false)
            .await
            .map_err(io_err)?;
        ids.extend(page.ids.iter().cloned());
        match pager.after_page(page.continuation.as_deref(), page.ids.len(), max_pages) {
            pf::PageAction::Continue => {
                continuation = page.continuation.filter(|c| !c.is_empty());
            }
            pf::PageAction::Done => return Ok(pf::Inventory::Complete(ids)),
            pf::PageAction::Aborted => {
                return Ok(pf::Inventory::Incomplete(
                    pager
                        .stopped_because
                        .unwrap_or_else(|| "Abbruch ohne Angabe".to_string()),
                ))
            }
        }
    }
}

/// Lädt alle Inhaltsseiten und übergibt jede Seite an `on_page`.
/// Fehler in `on_page` brechen die Phase ab; es entsteht kein Watermark.
pub async fn fetch_stream_inventory<F, Fut>(
    client: &pf::FeedlyClient,
    stream: &str,
    newer_than: Option<i64>,
    max_pages: usize,
    mut on_page: F,
) -> Result<StreamSummary, storage::StorageError>
where
    F: FnMut(Vec<pf::Entry>) -> Fut,
    Fut: std::future::Future<Output = Result<usize, String>>,
{
    let mut pager = pf::Pager::new();
    let mut continuation: Option<String> = None;
    let mut checkpoint: Option<i64> = None;
    loop {
        let page = client
            .stream_contents(stream, 100, continuation.as_deref(), newer_than, false)
            .await
            .map_err(io_err)?;
        checkpoint = page.updated;
        on_page(page.items.clone()).await.map_err(string_err)?;
        match pager.after_page(page.continuation.as_deref(), page.items.len(), max_pages) {
            pf::PageAction::Continue => {
                continuation = page.continuation.filter(|c| !c.is_empty());
            }
            pf::PageAction::Done => {
                return Ok(StreamSummary {
                    pages: pager.pages,
                    items: pager.items,
                    server_checkpoint: checkpoint,
                })
            }
            pf::PageAction::Aborted => {
                return Err(storage::StorageError::Schema(format!(
                    "Inhaltsabruf unvollständig: {}",
                    pager
                        .stopped_because
                        .unwrap_or_else(|| "Abbruch ohne Angabe".to_string())
                )))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StreamSummary {
    pub pages: usize,
    pub items: usize,
    /// Kleinster vom Server gemeldeter `updated`-Stand der Phase, falls vorhanden.
    pub server_checkpoint: Option<i64>,
}

/// Gleicht lokale Merkungen mit einem **vollständigen** Remote-Inventar ab.
/// Bei unvollständigem Inventar wird nichts geändert — insbesondere kein
/// stilles Entspeichern und kein Fortschreiben des Watermarks.
pub async fn reconcile_saved(
    worker: &DbWorker,
    account_id: &str,
    saved: &pf::Inventory,
    pull_generation: i64,
) -> Result<(), storage::StorageError> {
    let remote = match saved {
        pf::Inventory::Complete(ids) => ids.clone(),
        pf::Inventory::Incomplete(reason) => {
            dbg_log(&format!(
                "Saved-Reconciliation übersprungen (Inventar unvollständig: {reason})"
            ));
            return Ok(());
        }
    };
    let local_saved: Vec<String> = db(worker, {
        let account_id = account_id.to_string();
        move |db2| db2.saved_ids_for_account(&account_id)
    })
    .await?;
    let remote_set: std::collections::HashSet<&str> = remote.iter().map(String::as_str).collect();
    let to_unsave: Vec<String> = local_saved
        .iter()
        .filter(|id| !remote_set.contains(id.as_str()))
        .cloned()
        .collect();
    let to_save: Vec<String> = remote
        .iter()
        .filter(|id| !local_saved.contains(id))
        .cloned()
        .collect();
    // Der Generationwert stammt vom Syncbeginn: zwischenzeitliche lokale
    // Änderungen bleiben dadurch erhalten.
    let generation = pull_generation;
    for id in &to_unsave {
        db(worker, {
            let account_id = account_id.to_string();
            let id = id.clone();
            move |db2| {
                db2.apply_remote_status(&account_id, &id, None, Some(false), generation)?;
                Ok::<_, storage::StorageError>(())
            }
        })
        .await?;
    }
    for id in &to_save {
        db(worker, {
            let account_id = account_id.to_string();
            let id = id.clone();
            move |db2| {
                db2.apply_remote_status(&account_id, &id, None, Some(true), generation)?;
                Ok::<_, storage::StorageError>(())
            }
        })
        .await?;
    }
    dbg_log(&format!(
        "Saved-Reconciliation: lokal {}, remote {}, entfernt {}, gesetzt {}",
        local_saved.len(),
        remote.len(),
        to_unsave.len(),
        to_save.len()
    ));
    Ok(())
}

/// Gleicht die lokalen Ungelesen-Markierungen mit einem **vollständigen**
/// Remote-Inventar ab. Nur nachgewiesene Vollständigkeit darf remote fehlende
/// Artikel als gelesen setzen; lokale Neuere-Mutationen bleiben unangetastet.
pub async fn reconcile_unread(
    worker: &DbWorker,
    account_id: &str,
    unread: &pf::Inventory,
    pull_generation: i64,
) -> Result<usize, storage::StorageError> {
    let remote = match unread {
        pf::Inventory::Complete(ids) => ids.clone(),
        pf::Inventory::Incomplete(reason) => {
            dbg_log(&format!(
                "Unread-Reconciliation übersprungen (Inventar unvollständig: {reason})"
            ));
            return Ok(0);
        }
    };
    let remote_set: std::collections::HashSet<&str> = remote.iter().map(String::as_str).collect();
    let local: Vec<String> = db(worker, {
        let account_id = account_id.to_string();
        move |db2| db2.unread_ids_for_account(&account_id)
    })
    .await?;
    let missing: Vec<String> = local
        .iter()
        .filter(|id| !remote_set.contains(id.as_str()))
        .cloned()
        .collect();
    let mut applied = 0usize;
    for id in missing {
        let changed = db(worker, {
            let account_id = account_id.to_string();
            move |db2| {
                // Fehlt die ID im Unread-Inventar, ist der Artikel remote gelesen.
                let res =
                    db2.apply_remote_status(&account_id, &id, Some(true), None, pull_generation)?;
                Ok::<_, storage::StorageError>(res.applied)
            }
        })
        .await?;
        if changed {
            applied += 1;
        }
    }
    dbg_log(&format!(
        "Unread-Reconciliation: remote {}, lokal {}, neu als gelesen {}",
        remote.len(),
        local.len(),
        applied
    ));
    Ok(applied)
}

/// Lädt fehlende Inhalte gespeicherter oder ungelesener Artikel per `.mget` nach.
pub async fn load_missing_contents(
    worker: &DbWorker,
    client: &pf::FeedlyClient,
    account_id: &str,
    pull_generation: i64,
    batch: usize,
) -> Result<usize, storage::StorageError> {
    let ids: Vec<String> = db(worker, {
        let account_id = account_id.to_string();
        move |db2| db2.articles_needing_content(&account_id, batch as u32)
    })
    .await?;
    if ids.is_empty() {
        return Ok(0);
    }
    let entries = client
        .entries_mget(&ids)
        .await
        .map_err(|e| storage::StorageError::Schema(format!("Nachladen fehlgeschlagen: {e}")))?;
    let received = entries.len();
    ingest_entries(worker, account_id, pull_generation, entries)
        .await
        .map_err(storage::StorageError::Schema)?;
    dbg_log(&format!(
        "Nachgeladene Inhalte: {received} Artikel (von {} IDs)",
        ids.len()
    ));
    Ok(received)
}

/// Prüft, ob der Server die gerade gesendete Absicht tatsächlich übernommen hat.
pub fn confirmed(entry: &pf::Entry, field: &str, desired: bool) -> bool {
    match field {
        "read" => entry.unread.map(|u| u != desired).unwrap_or(false),
        _ => entry.is_saved() == desired,
    }
}

pub fn initial_sync(worker: DbWorker, net: &Net, token: String) {
    let tx = net.event_sender();
    set_status(&worker, "initial_sync", None);
    let status_account = String::new();
    let sync_started_ms = storage::now_ms();
    net.spawn(async move {
        let client = pf::FeedlyClient::new(token);
        let res: storage::Result<usize> = async {
            let profile = client.profile().await.map_err(io_err)?;
            db(&worker, {
                let p = profile.clone();
                move |db2| {
                    db2.upsert_account(
                        &p.id,
                        "feedly",
                        p.full_name
                            .as_deref()
                            .or(p.email.as_deref())
                            .unwrap_or("Feedly"),
                    )
                }
            })
            .await?;
            let subs = client.subscriptions().await.map_err(io_err)?;
            let cats = client.categories().await.map_err(io_err)?;
            let account_id = profile.id.clone();
            // Profilbindung im Schlüsselbund: Token und Konto gehören zusammen.
            let _ = bind_account(&account_id);
            for c in &cats {
                let name = c.label.clone().unwrap_or_else(|| "Gruppe".into());
                db(&worker, {
                    let account_id = account_id.clone();
                    let c = c.clone();
                    move |db2| db2.upsert_group_remote(&account_id, &c.id, &name)
                })
                .await?;
            }
            let group_ids: std::collections::HashMap<String, i64> =
                db(&worker, |db2| db2.list_groups())
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
                    move |db2| {
                        db2.upsert_feed_remote(
                            &account_id,
                            &s.id,
                            &url,
                            &title,
                            s.website.as_deref(),
                        )
                    }
                })
                .await?;
                let gids: Vec<i64> = s
                    .categories
                    .iter()
                    .filter_map(|c| group_ids.get(&c.id).copied())
                    .collect();
                db(&worker, move |db2| db2.set_feed_groups(fid, &gids)).await?;
            }
            let stream = pf::global_all_stream(&profile.id);
            let newer_than = storage::now_ms() - 30 * 86_400_000;
            // Generation vor dem ersten Inhaltsabruf erfassen.
            let pull_gen = db(&worker, {
                let account_id = account_id.clone();
                move |db2| db2.pull_generation(&account_id)
            })
            .await?;
            let summary = fetch_stream_inventory(
                &client,
                &stream,
                Some(newer_than),
                MAX_CONTENT_PAGES,
                |items| {
                    let worker = worker.clone();
                    let account_id = account_id.clone();
                    async move { ingest_entries(&worker, &account_id, pull_gen, items).await }
                },
            )
            .await?;
            let added = summary.items;
            let saved =
                fetch_id_inventory(&client, &pf::saved_stream(&profile.id), MAX_ID_PAGES).await?;
            reconcile_saved(&worker, &account_id, &saved, pull_gen).await?;
            dbg_log(&format!(
                "Erst-Sync: {} Seiten, {added} Inhalte, Saved-Inventar {}",
                summary.pages,
                match &saved {
                    pf::Inventory::Complete(ids) => format!("vollständig ({} IDs)", ids.len()),
                    pf::Inventory::Incomplete(r) => format!("unvollständig: {r}"),
                }
            ));
            // Sicherer Checkpoint: der früheste Stand, den diese Phase
            // abgedeckt hat — der Sync-Start, nicht die lokale Endzeit.
            let checkpoint = summary
                .server_checkpoint
                .unwrap_or(sync_started_ms)
                .min(sync_started_ms);
            dbg_log(&format!(
                "Watermark auf {checkpoint} (Start {sync_started_ms})"
            ));
            db(&worker, move |db2| {
                db2.set_last_sync(&account_id, checkpoint)
            })
            .await?;
            Ok(added)
        }
        .await;
        match res {
            Ok(added) => {
                set_status(&worker, "ready", Some(&status_account));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncDone { added });
            }
            Err(e) => {
                let (status, detail) = classify_storage(&e);
                set_status(&worker, status, Some(&status_account));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                    message: detail.unwrap_or_else(|| e.to_string()),
                    status: Some(status.to_string()),
                    retry_after_ms: None,
                });
            }
        }
    });
}

pub fn process_outbox(worker: DbWorker, net: &Net, token: String, account_id: String) {
    let tx = net.event_sender();
    net.spawn(async move {
        let client = pf::FeedlyClient::new(token);
        let now = storage::now_ms();
        // Claim und Requestrevision werden gemeinsam erfasst: ein zweiter
        // Prozessor kann dieselbe Zeile nicht parallel senden.
        let rows = db(&worker, {
            let account_id = account_id.clone();
            move |db2| db2.outbox_claim(&account_id, now, 100)
        })
        .await;
        let Ok(rows) = rows else { return };
        if rows.is_empty() {
            return;
        }
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
            let entries_for_notice: Vec<(String, String)> = entries
                .iter()
                .map(|(_, _, entity)| (account_id.clone(), entity.clone()))
                .collect();
            match client.markers_entries(action, &entry_ids).await {
                Ok(()) => {
                    let total = sent.len();
                    let _ = db(&worker, move |db2| db2.outbox_ack(&sent)).await;
                    // Bestätigung durch einen **danach** begonnenen Statusabruf.
                    // Serverkonsistenz kann verzögert sein; eine Abweichung erzeugt
                    // eine neue, revisionsfeste Absicht statt eines stillen Erfolgs.
                    if let Ok(remote) = client.entries_mget(&entry_ids).await {
                        let mismatched: Vec<String> = remote
                            .iter()
                            .filter(|e| !confirmed(e, &field, desired))
                            .map(|e| e.id.clone())
                            .collect();
                        if !mismatched.is_empty() {
                            let count = mismatched.len();
                            let account_for_db = account_id.clone();
                            let field_for_db = field.clone();
                            let _ = db(&worker, move |db2| {
                                for entity in &mismatched {
                                    db2.enqueue_outbox(
                                        &account_for_db,
                                        entity,
                                        &field_for_db,
                                        desired,
                                    )?;
                                }
                                let notices: Vec<(String, String)> = mismatched
                                    .iter()
                                    .map(|e| (account_for_db.clone(), e.clone()))
                                    .collect();
                                db2.mark_unsynced(&notices)?;
                                Ok::<_, storage::StorageError>(())
                            })
                            .await;
                            let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                                message: format!(
                                    "Feedly hat {count} von {total} Änderungen noch nicht bestätigt — erneuter Versuch vorgemerkt"
                                ),
                                status: Some("degraded".into()),
                                retry_after_ms: None,
                            });
                        }
                    }
                }
                Err(pf::FeedlyError::Api { status: 404, message, .. }) => {
                    let _ = db(&worker, {
                        let sent = sent.clone();
                        move |db2| {
                            let affected = db2.outbox_fail_permanent(&sent)?;
                            if affected > 0 {
                                db2.mark_unsynced(&entries_for_notice)?;
                            }
                            Ok::<_, storage::StorageError>(affected)
                        }
                    })
                    .await;
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                        message: format!(
                            "404 für {n} Änderungen: {message} — bleiben lokal erhalten und werden nicht erneut gesendet",
                            n = sent.len()
                        ),
                        status: Some("degraded".into()),
                        retry_after_ms: None,
                    });
                }
                Err(pf::FeedlyError::Api { status: 429, retry_after_ms, .. }) => {
                    // quota: der Server nennt den Zeitpunkt, sonst greift eine
                    // vorsichtige Standardwartezeit.
                    let next = retry_after_ms.unwrap_or_else(|| storage::now_ms() + 5 * 60_000);
                    let _ = db(&worker, move |db2| db2.outbox_fail(&sent, next, false)).await;
                    set_status(&worker, "rate_limited", Some(&account_id));
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                        status: Some("rate_limited".into()),
                        retry_after_ms: Some(next),
                        message: format!("Drosselung durch Feedly — neuer Versuch ab {}", fmt_ms(next)),
                    });
                }
                Err(pf::FeedlyError::Api { status, message, .. }) if status == 401 || status == 403 => {
                    // Zentraler Auth-Stopp: Konto wird sichtbar gesperrt und lange
                    // zurückgestellt, statt endlos mit fester Wartezeit zu versuchen.
                    let next = storage::now_ms() + 30 * 60_000;
                    let _ = db(&worker, move |db2| db2.outbox_fail(&sent, next, false)).await;
                    set_status(
                        &worker,
                        "auth_required",
                        Some(&account_id),
                    );
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                        status: Some("auth_required".into()),
                        retry_after_ms: Some(next),
                        message: format!("Auth/Rechte ({status}): {message} — bitte neu verbinden"),
                    });
                }
                Err(e) => {
                    let next = storage::now_ms() + 60_000;
                    let _ = db(&worker, move |db2| db2.outbox_fail(&sent, next, false)).await;
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                        message: e.to_string(),
                        status: None,
                        retry_after_ms: Some(next),
                    });
                }
            }
        }
    });
}

fn fmt_ms(ms: i64) -> String {
    let minutes = (ms - storage::now_ms()).max(0) / 60_000 + 1;
    format!("in ca. {minutes} min")
}

fn set_status(worker: &DbWorker, status: &str, account_id: Option<&str>) {
    let worker = worker.clone();
    let value = status.to_string();
    let acc = account_id.map(str::to_string);
    std::thread::spawn(move || {
        let _ = worker.send(move |db| {
            if let Some(acc) = &acc {
                db.set_account_status(acc, &value, None)?;
            }
            Ok::<_, storage::StorageError>(())
        });
    });
}

pub fn delta_sync(
    worker: DbWorker,
    net: &Net,
    token: String,
    account_id: String,
    last_sync_ms: i64,
) {
    let tx = net.event_sender();
    set_status(&worker, "syncing", Some(&account_id));
    let status_account = account_id.clone();
    let _ = &status_account;
    let sync_started_ms = storage::now_ms();
    net.spawn(async move {
        let client = pf::FeedlyClient::new(token);
        let res: storage::Result<usize> = async {
            let overlap = last_sync_ms - 5 * 60_000;
            let pull_gen = db(&worker, {
                let account_id = account_id.clone();
                move |db2| db2.pull_generation(&account_id)
            })
            .await?;
            let mut reads_pager = pf::Pager::new();
            let mut reads_continuation: Option<String> = None;
            loop {
                let page = client
                    .markers_reads_page(overlap, 1000, reads_continuation.as_deref())
                    .await
                    .map_err(io_err)?;
                for m in &page.entries {
                    db(&worker, {
                        let account_id = account_id.clone();
                        let id = m.id.clone();
                        move |db2| {
                            db2.apply_remote_status(&account_id, &id, Some(true), None, pull_gen)?;
                            Ok::<_, storage::StorageError>(())
                        }
                    })
                    .await?;
                }
                let next = page.continuation.filter(|c| !c.is_empty());
                match reads_pager.after_page(next.as_deref(), page.entries.len(), MAX_READS_PAGES) {
                    pf::PageAction::Continue => reads_continuation = next,
                    pf::PageAction::Done => break,
                    pf::PageAction::Aborted => {
                        return Err(storage::StorageError::Schema(format!(
                            "Read-Abgleich unvollständig: {}",
                            reads_pager
                                .stopped_because
                                .unwrap_or_else(|| "Abbruch ohne Angabe".to_string())
                        )))
                    }
                }
            }
            let stream = pf::global_all_stream(&account_id);
            let summary = fetch_stream_inventory(
                &client,
                &stream,
                Some(overlap),
                MAX_CONTENT_PAGES,
                |items| {
                    let worker = worker.clone();
                    let account_id = account_id.clone();
                    async move { ingest_entries(&worker, &account_id, pull_gen, items).await }
                },
            )
            .await?;
            let added = summary.items;
            let saved =
                fetch_id_inventory(&client, &pf::saved_stream(&account_id), MAX_ID_PAGES).await?;
            dbg_log(&format!(
                "Delta: {} Seiten, {added} Inhalte; Saved: {}",
                summary.pages,
                match &saved {
                    pf::Inventory::Complete(ids) => format!("vollständig ({} IDs)", ids.len()),
                    pf::Inventory::Incomplete(r) => format!("unvollständig: {r}"),
                }
            ));
            reconcile_saved(&worker, &account_id, &saved, pull_gen).await?;
            // Sicherer Checkpoint: der früheste Stand, den diese Phase
            // abgedeckt hat — der Sync-Start, nicht die lokale Endzeit.
            let checkpoint = summary
                .server_checkpoint
                .unwrap_or(sync_started_ms)
                .min(sync_started_ms);
            dbg_log(&format!(
                "Watermark auf {checkpoint} (Start {sync_started_ms})"
            ));
            db(&worker, move |db2| {
                db2.set_last_sync(&account_id, checkpoint)
            })
            .await?;
            Ok(added)
        }
        .await;
        match res {
            Ok(added) => {
                set_status(&worker, "ready", Some(&status_account));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncDone { added });
            }
            Err(e) => {
                let (status, detail) = classify_storage(&e);
                set_status(&worker, status, Some(&status_account));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                    message: detail.unwrap_or_else(|| e.to_string()),
                    status: Some(status.to_string()),
                    retry_after_ms: None,
                });
            }
        }
    });
}

pub fn classify_storage(error: &storage::StorageError) -> (&'static str, Option<String>) {
    match error {
        storage::StorageError::Schema(msg) if msg.contains("429") => ("rate_limited", None),
        storage::StorageError::Schema(msg)
            if msg.contains("api 401") || msg.contains("api 403") =>
        {
            (
                "auth_required",
                Some("Anmeldung erforderlich — bitte neu verbinden".into()),
            )
        }
        storage::StorageError::Io(_) => ("offline", None),
        _ => ("degraded", None),
    }
}

pub fn classify(error: &pf::FeedlyError) -> (&'static str, Option<String>) {
    match error {
        pf::FeedlyError::Api { status: 401, .. } | pf::FeedlyError::Api { status: 403, .. } => (
            "auth_required",
            Some("Anmeldung erforderlich — bitte neu verbinden".into()),
        ),
        pf::FeedlyError::Api {
            status: 429,
            message,
            ..
        } => (
            "rate_limited",
            Some(format!("Drosselung durch Feedly: {message}")),
        ),
        pf::FeedlyError::Api { status: 404, .. } => (
            "degraded",
            Some("Einige Objekte sind nicht mehr verfügbar".into()),
        ),
        pf::FeedlyError::Http(_) => ("offline", None),
        _ => ("degraded", None),
    }
}

fn string_err(e: String) -> storage::StorageError {
    storage::StorageError::Schema(e)
}

fn io_err(e: pf::FeedlyError) -> storage::StorageError {
    storage::StorageError::Schema(e.to_string())
}

#[cfg(test)]
mod r1_regression {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn mock_server(
        responder: impl Fn(&str) -> (u16, String) + Send + Sync + 'static,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                counter.fetch_add(1, Ordering::SeqCst);
                let (code, body) = responder(&req);
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}/v3/"), hits)
    }

    fn worker_with_articles(saved: &[&str], unsaved: &[&str]) -> (DbWorker, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "lesefluss-r1-{}-{}.db",
            std::process::id(),
            saved
                .iter()
                .chain(unsaved.iter())
                .copied()
                .collect::<Vec<_>>()
                .join("-")
        ));
        let _ = std::fs::remove_file(&path);
        let worker = DbWorker::start(path.clone());
        let now = storage::now_ms();
        let owned: Vec<String> = saved.iter().map(|s| s.to_string()).collect();
        let plain: Vec<String> = unsaved.iter().map(|s| s.to_string()).collect();
        worker
            .send(move |db| {
                db.upsert_account("feedly-1", "feedly", "Feedly").unwrap();
                let feed = db
                    .add_feed("feedly-1", "u1", "Feed", None, "#111111")
                    .unwrap();
                for id in &owned {
                    db.upsert_article(feed, id, "Titel", None, None, now, "e", None, now)
                        .unwrap();
                    db.set_saved_by_article_id(id, true).unwrap();
                }
                for id in &plain {
                    db.upsert_article(feed, id, "Titel", None, None, now, "e", None, now)
                        .unwrap();
                }
                Ok::<_, storage::StorageError>(())
            })
            .recv()
            .unwrap();
        (worker, path)
    }

    #[tokio::test]
    async fn id_inventory_holt_wirklich_alle_seiten() {
        let (base, hits) = mock_server(|req| {
            if req.contains("continuation=c2") {
                (200, r#"{"ids":["d"]}"#.into())
            } else if req.contains("continuation=c1") {
                (200, r#"{"ids":["c"],"continuation":"c2"}"#.into())
            } else {
                (200, r#"{"ids":["a","b"],"continuation":"c1"}"#.into())
            }
        });
        let client = pf::FeedlyClient::with_base("t".into(), base);
        let result = fetch_id_inventory(&client, "user/x/saved", 50)
            .await
            .unwrap();
        match result {
            pf::Inventory::Complete(ids) => assert_eq!(ids, vec!["a", "b", "c", "d"]),
            pf::Inventory::Incomplete(r) => panic!("Inventar sollte vollständig sein: {r}"),
        }
        assert_eq!(
            hits.load(Ordering::SeqCst),
            3,
            "alle drei Seiten müssen geholt werden"
        );
    }

    #[tokio::test]
    async fn id_inventory_meldet_fehler_auf_seite_zwei_als_unvollstaendig() {
        let (base, hits) = mock_server(|req| {
            if req.contains("continuation=c1") {
                (500, r#"{"error":"boom"}"#.into())
            } else {
                (200, r#"{"ids":["a"],"continuation":"c1"}"#.into())
            }
        });
        let client = pf::FeedlyClient::with_base("t".into(), base);
        let err = fetch_id_inventory(&client, "user/x/saved", 50)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("api 500"), "{err}");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "die zweite Seite muss wirklich angefragt werden"
        );
    }

    #[tokio::test]
    async fn id_inventory_erkennt_cursor_zyklus() {
        let (base, _) = mock_server(|_| (200, r#"{"ids":["a"],"continuation":"c1"}"#.into()));
        let client = pf::FeedlyClient::with_base("t".into(), base);
        match fetch_id_inventory(&client, "user/x/saved", 50)
            .await
            .unwrap()
        {
            pf::Inventory::Incomplete(r) => assert!(r.contains("Zyklus"), "{r}"),
            pf::Inventory::Complete(_) => panic!("Zyklus darf nicht als vollständig gelten"),
        }
    }

    #[tokio::test]
    async fn unvollstaendiges_saved_inventar_entspeichert_nichts() {
        let (worker, path) = worker_with_articles(&["lokal-a"], &[]);
        let saved = pf::Inventory::Incomplete("Cursor-Zyklus".into());
        reconcile_saved(&worker, "feedly-1", &saved, 0)
            .await
            .unwrap();
        let local: Vec<String> =
            db(&worker, |db| db.saved_ids_for_account("feedly-1").unwrap()).await;
        assert_eq!(
            local,
            vec!["lokal-a".to_string()],
            "Datenverlust ist verboten"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn zwischenzeitliche_lokale_aenderung_gewinnt_gegenueber_alterem_pull() {
        let (worker, path) = worker_with_articles(&["weg"], &["neu"]);
        // Pull beginnt: Generation 0, weil noch keine Outbox-Mutation existiert.
        let pull_gen = 0i64;
        // Danach ändert die Person den Artikel lokal.
        db(&worker, |db| {
            db.apply_status_with_outbox(1, "neu", Some(true), Some(true))
                .unwrap();
        })
        .await;
        let saved = pf::Inventory::Complete(vec!["neu".into()]);
        reconcile_saved(&worker, "feedly-1", &saved, pull_gen)
            .await
            .unwrap();
        let state: Vec<(bool, bool)> = db(&worker, |db| {
            let row = db
                .raw()
                .query_row(
                    "SELECT unread, saved FROM articles WHERE id='neu'",
                    [],
                    |r| Ok((r.get::<_, i64>(0)? == 1, r.get::<_, i64>(1)? == 1)),
                )
                .unwrap();
            vec![row]
        })
        .await;
        assert_eq!(
            state,
            vec![(false, true)],
            "die neuere lokale Absicht bleibt"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn vollstaendiges_unread_inventar_setzt_fehlende_als_gelesen() {
        let (worker, path) = worker_with_articles(&[], &["bleibt", "weg"]);
        let inventory = pf::Inventory::Complete(vec!["bleibt".into()]);
        let applied = reconcile_unread(&worker, "feedly-1", &inventory, i64::MAX)
            .await
            .unwrap();
        assert_eq!(applied, 1);
        let unread: Vec<String> =
            db(&worker, |db| db.unread_ids_for_account("feedly-1").unwrap()).await;
        assert_eq!(unread, vec!["bleibt".to_string()]);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn unvollstaendiges_unread_inventar_aendert_nichts() {
        let (worker, path) = worker_with_articles(&[], &["a", "b"]);
        let inventory = pf::Inventory::Incomplete("Cursor-Zyklus".into());
        let applied = reconcile_unread(&worker, "feedly-1", &inventory, i64::MAX)
            .await
            .unwrap();
        assert_eq!(applied, 0);
        let unread: Vec<String> =
            db(&worker, |db| db.unread_ids_for_account("feedly-1").unwrap()).await;
        assert_eq!(
            unread.len(),
            2,
            "ohne Vollständigkeit bleibt alles ungelesen"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn fehlende_inhalte_werden_nachgeladen() {
        let entry_id = "https://feedly.com/i/entry/ohne-inhalt".to_string();
        let (worker, path) = worker_with_articles(&["mit-inhalt"], &[&entry_id]);
        let (base, hits) = mock_server(|req| {
            if req.contains("entries/.mget") {
                (
                    200,
                    serde_json::json!([{
                        "id": "https://feedly.com/i/entry/ohne-inhalt",
                        "origin": {
                            "streamId": "feed/https://example.org/feed.xml"
                        },
                        "title": "Nachgeladen",
                        "published": 1_700_000_000i64,
                        "updated": 1_700_000_000i64,
                        "content": {"content": "<p>Inhalt</p>"}
                    }])
                    .to_string(),
                )
            } else {
                (404, "{}".to_string())
            }
        });
        db(&worker, |db| {
            db.raw()
                .execute(
                    "UPDATE feeds SET remote_id='feed/https://example.org/feed.xml' WHERE account_id='feedly-1'",
                    [],
                )
                .unwrap();
        })
        .await;
        let client = pf::FeedlyClient::with_base("t".into(), base);
        let loaded = load_missing_contents(&worker, &client, "feedly-1", i64::MAX, 50)
            .await
            .unwrap();
        assert_eq!(loaded, 1, "der fehlende Inhalt wurde nachgeladen");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let has_content: i64 = db(&worker, |db| {
            db.raw()
                .query_row(
                    "SELECT COUNT(*) FROM article_contents c JOIN articles a ON a.id=c.article_id
                     WHERE a.id='https://feedly.com/i/entry/ohne-inhalt'",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        })
        .await;
        assert_eq!(has_content, 1);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn vollstaendiges_saved_inventar_gleicht_beide_richtungen_ab() {
        let (worker, path) = worker_with_articles(&["weg", "bleibt"], &["neu"]);
        let saved = pf::Inventory::Complete(vec!["bleibt".into(), "neu".into()]);
        reconcile_saved(&worker, "feedly-1", &saved, i64::MAX)
            .await
            .unwrap();
        let mut local: Vec<String> =
            db(&worker, |db| db.saved_ids_for_account("feedly-1").unwrap()).await;
        local.sort();
        assert_eq!(local, vec!["bleibt".to_string(), "neu".to_string()]);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn leere_seite_mit_cursor_wird_verfolgt() {
        let (base, hits) = mock_server(|req| {
            if req.contains("continuation=c2") {
                (200, r#"{"ids":["c"]}"#.into())
            } else if req.contains("continuation=c1") {
                (200, r#"{"ids":[],"continuation":"c2"}"#.into())
            } else {
                (200, r#"{"ids":["a"],"continuation":"c1"}"#.into())
            }
        });
        let client = pf::FeedlyClient::with_base("t".into(), base);
        match fetch_id_inventory(&client, "user/x/saved", 50)
            .await
            .unwrap()
        {
            pf::Inventory::Complete(ids) => assert_eq!(ids, vec!["a", "c"]),
            pf::Inventory::Incomplete(r) => panic!("leere Seite mit Cursor ist kein Abbruch: {r}"),
        }
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn sicherheitslimit_liefert_unvollstaendiges_inventar() {
        let server_cursor = Arc::new(AtomicUsize::new(0));
        let cursor = server_cursor.clone();
        let (base, hits) = mock_server(move |_| {
            let n = cursor.fetch_add(1, Ordering::SeqCst) + 1;
            (200, format!(r#"{{"ids":["x{n}"],"continuation":"c{n}"}}"#))
        });
        let client = pf::FeedlyClient::with_base("t".into(), base);
        match fetch_id_inventory(&client, "user/x/saved", 3)
            .await
            .unwrap()
        {
            pf::Inventory::Incomplete(r) => assert!(r.contains("Sicherheitslimit"), "{r}"),
            pf::Inventory::Complete(_) => panic!("Limit darf nicht als vollständig gelten"),
        }
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn inhalt_seite_zwei_fehler_bricht_und_verarbeitet_nicht_weiter() {
        let (base, hits) = mock_server(|req| {
            if req.contains("continuation=c1") {
                (503, r#"{"error":"busy"}"#.into())
            } else {
                (200, r#"{"items":[],"continuation":"c1"}"#.into())
            }
        });
        let client = pf::FeedlyClient::with_base("t".into(), base);
        let seen = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        let err = fetch_stream_inventory(
            &client,
            "user/x/category/global.all",
            None,
            MAX_CONTENT_PAGES,
            move |_items| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(1usize)
                }
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("api 503"), "{err}");
        assert_eq!(
            seen.load(Ordering::SeqCst),
            1,
            "nur die erfolgreiche Seite wird verarbeitet"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }
}

#[cfg(test)]
mod r6_token_tests {
    use super::*;

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lesefluss-token-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn token_datei_wird_atomar_und_nur_fuer_den_benutzer_lesbar_ersetzt() {
        let dir = tempdir("atomar");
        let path = dir.join("feedly-token");
        write_private(&path, b"erst").unwrap();
        write_private(&path, b"zweit").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"zweit");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "nur der Benutzer darf lesen");
        }
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "keine temporären Reste: {leftovers:?}"
        );
    }

    #[test]
    fn symlink_auf_die_token_datei_wird_ersetzt_statt_beschrieben() {
        let dir = tempdir("symlink");
        let path = dir.join("feedly-token");
        let victim = dir.join("opfer");
        std::fs::write(&victim, b"unveraendert").unwrap();
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        write_private(&path, b"neuer-token").unwrap();
        assert_eq!(
            std::fs::read(&victim).unwrap(),
            b"unveraendert",
            "das Ziel des Symlinks bleibt unberührt"
        );
        let meta = std::fs::symlink_metadata(&path).unwrap();
        assert!(
            meta.file_type().is_file(),
            "die Token-Datei ist jetzt eine reguläre Datei"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"neuer-token");
    }

    #[test]
    fn abmelden_entfernt_die_token_datei_und_meldet_erfolg() {
        let dir = tempdir("logout");
        let path = dir.join("feedly-token");
        write_private(&path, b"token").unwrap();
        let removed = forget_token_at(&path);
        assert!(removed, "es gab etwas zu entfernen");
        assert!(!path.exists());
        assert!(!forget_token_at(&path), "zweites Entfernen ist ein No-op");
    }
}

#[cfg(test)]
mod confirm_tests {
    use super::*;

    fn entry(id: &str, unread: Option<bool>, saved: bool) -> pf::Entry {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "title": "T",
            "unread": unread,
            "tags": if saved {
                serde_json::json!([{"id": "user/1/tag/global.saved"}])
            } else {
                serde_json::json!([{"id": "user/1/tag/global.unsaved"}])
            },
            "origin": {"streamId": "feed/https://example.org/f.xml"}
        }))
        .expect("Eintrag")
    }

    #[test]
    fn serverbestaetigung_wird_geprueft() {
        let read = entry("a", Some(false), false);
        assert!(confirmed(&read, "read", true), "gelesen ist bestätigt");
        assert!(!confirmed(&read, "read", false));
        let unread = entry("b", Some(true), false);
        assert!(confirmed(&unread, "read", false));
        let saved = entry("c", Some(true), true);
        assert!(confirmed(&saved, "saved", true));
        assert!(!confirmed(&saved, "saved", false));
        let unknown = entry("d", None, false);
        assert!(
            !confirmed(&unknown, "read", true),
            "fehlende Serverangabe gilt nicht als Bestätigung"
        );
    }
}
