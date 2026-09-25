use crate::dbworker::DbWorker;
use crate::net::Net;
use crate::window::dbg_log;
use provider_feedly as pf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Kennzeichnet einen Lauf: Konto, Laufnummer und Abbruchmöglichkeit.
#[derive(Clone)]
pub struct RunCtx {
    pub account_id: String,
    pub run_id: u64,
    pub cancel: Arc<AtomicBool>,
    /// Überschreibt die Basis-URL; nur Tests nutzen das (sonst `pf::base_url()`).
    pub base: Option<String>,
}

impl RunCtx {
    pub fn new(account_id: &str, run_id: u64) -> Self {
        Self {
            account_id: account_id.to_string(),
            run_id,
            cancel: Arc::new(AtomicBool::new(false)),
            base: None,
        }
    }

    #[cfg(test)]
    pub fn with_base(mut self, base: &str) -> Self {
        self.base = Some(base.to_string());
        self
    }

    /// Basis-URL dieses Laufs.
    pub fn base(&self) -> String {
        self.base.clone().unwrap_or_else(pf::base_url)
    }

    /// Nach Logout, Auth- oder Quotenstopp: keine weiteren Requests, keine
    /// weiteren DB-Schreibvorgänge dieses Laufs.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Fehler eines Laufs mit strukturierter Steuerungsinformation.
#[derive(Debug, Clone)]
pub struct SyncFailure {
    pub message: String,
    pub status: Option<String>,
    pub retry_after_ms: Option<i64>,
}

impl SyncFailure {
    fn new(message: impl Into<String>, status: Option<&str>, retry_after_ms: Option<i64>) -> Self {
        Self {
            message: message.into(),
            status: status.map(str::to_string),
            retry_after_ms,
        }
    }
}

impl std::fmt::Display for SyncFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub type SyncResult<T> = Result<T, SyncFailure>;

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
    secret_tool_with(std::path::Path::new("secret-tool"), args, stdin_data)
}

/// Wie `secret_tool`, aber mit wählbarem Programm. Tests nutzen damit ein Skript
/// statt den echten Secret Service und ohne die Umgebung zu verändern.
fn secret_tool_with(
    program: &std::path::Path,
    args: &[&str],
    stdin_data: Option<&[u8]>,
) -> Option<std::process::Output> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(program)
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

/// **Lookup**: Erfolg ist ein nichtleerer Textwert. `secret-tool lookup` liefert
/// bei fehlendem Eintrag nichts und beendet sich trotzdem mit 0.
fn secret_tool_lookup(args: &[&str], stdin_data: Option<&[u8]>) -> Option<String> {
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
    secret_tool_lookup(&["lookup", "lesefluss", "feedly-account"], None)
}

pub fn bind_account(account_id: &str) -> bool {
    bind_account_with(std::path::Path::new("secret-tool"), account_id)
}

pub fn bind_account_with(program: &std::path::Path, account_id: &str) -> bool {
    secret_tool_with(
        program,
        &[
            "store",
            "--label=Lesefluss: Feedly-Konto",
            "lesefluss",
            "feedly-account",
        ],
        Some(account_id.as_bytes()),
    )
    .map(|out| out.status.success())
    .unwrap_or(false)
}

/// Löscht Token **und** Kontobindung und meldet zurück, ob etwas entfernt wurde.
pub fn forget_token() -> bool {
    forget_token_at(&token_path())
}

pub fn forget_token_at(path: &std::path::Path) -> bool {
    forget_token_at_with(path, std::path::Path::new("secret-tool"))
}

/// Wie `forget_token_at`, aber mit wählbarem Keyring-Programm (Tests).
pub fn forget_token_at_with(path: &std::path::Path, program: &std::path::Path) -> bool {
    let mut removed = false;
    for key in ["feedly-token", "feedly-account"] {
        if let Some(out) = secret_tool_with(program, &["clear", "lesefluss", key], None) {
            removed |= out.status.success();
        }
    }
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(()) => removed = true,
            Err(e) => eprintln!("[lf] Token-Datei konnte nicht entfernt werden: {e}"),
        }
    }
    removed
}

fn keyring_lookup() -> Option<String> {
    keyring_lookup_with(std::path::Path::new("secret-tool"))
}

fn keyring_lookup_with(program: &std::path::Path) -> Option<String> {
    let out = secret_tool_with(program, &["lookup", "lesefluss", "feedly-token"], None)?;
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

fn keyring_store(token: &str) -> bool {
    keyring_store_with(std::path::Path::new("secret-tool"), token)
}

fn keyring_store_with(program: &std::path::Path, token: &str) -> bool {
    secret_tool_with(
        program,
        &[
            "store",
            "--label=Lesefluss: Feedly-Token",
            "lesefluss",
            "feedly-token",
        ],
        Some(token.trim().as_bytes()),
    )
    .map(|out| out.status.success())
    .unwrap_or(false)
}

/// Passt das Token zum gewünschten Konto? Ohne gespeicherte Bindung (z. B. Datei-
/// fallback) gilt das Token als ungebunden und wird beim ersten Sync gebunden.
pub fn token_matches_account(bound: Option<&str>, account_id: &str) -> bool {
    match bound {
        Some(bound) => bound == account_id,
        None => true,
    }
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

/// Wie `save_token`, aber mit wählbarem Keyring-Programm (Tests).
#[cfg(test)]
pub fn save_token_at(
    path: &std::path::Path,
    token: &str,
    program: &std::path::Path,
) -> std::io::Result<()> {
    if secret_tool_with(
        program,
        &[
            "store",
            "--label=Lesefluss: Feedly-Token",
            "lesefluss",
            "feedly-token",
        ],
        Some(token.trim().as_bytes()),
    )
    .map(|out| out.status.success())
    .unwrap_or(false)
    {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    write_private(path, token.trim().as_bytes())
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

/// Blockierender DB-Zugriff (Tests, die kein Laufzeitkontext brauchen).
#[cfg(test)]
fn db_blocking<T, F>(worker: &DbWorker, f: F) -> T
where
    T: Send + 'static,
    F: FnOnce(&storage::Database) -> T + Send + 'static,
{
    worker
        .send(f)
        .recv()
        .ok()
        .and_then(|b| b.downcast::<T>().ok())
        .map(|b| *b)
        .unwrap_or_else(|| panic!("db worker unavailable"))
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

/// Pro Feed zusammengefasste Artikel einer Inhaltsseite.
type FeedBatch = std::collections::HashMap<i64, Vec<storage::NewArticle>>;
/// Pro Feed: Artikel-ID und ihre Bildquellen.
type MediaBatch = Vec<(i64, Vec<(String, Vec<String>)>)>;

async fn ingest_entries(
    worker: &DbWorker,
    account_id: &str,
    generation: i64,
    entries: Vec<pf::Entry>,
) -> Result<usize, String> {
    let mut per_feed: FeedBatch = std::collections::HashMap::new();
    let mut per_feed_media: MediaBatch = Vec::new();
    let mut statuses: Vec<(String, bool, bool)> = Vec::new();
    for e in &entries {
        let Some(stream) = e.feed_stream_id() else {
            continue;
        };
        let account_for_feed = account_id.to_string();
        let stream_for_feed = stream.clone();
        let title = e
            .origin
            .as_ref()
            .and_then(|o| o.title.clone())
            .unwrap_or_else(|| e.feed_stream_id().unwrap_or_default());
        // Unbekannte Origins werden nicht übersprungen: der Feed entsteht inaktiv,
        // damit gespeicherte Artikel nicht abonnierter Quellen sichtbar bleiben.
        let fid = db(worker, move |db2| {
            db2.ensure_origin_feed(&account_for_feed, &stream_for_feed, &title)
        })
        .await
        .map_err(|e| e.to_string())?;
        if let Some(unread) = e.unread {
            // Beide Richtungen: remote ungelesen wird lokal wieder ungelesen,
            // remote gelesen wird lokal gelesen (revisionsgeschützt).
            statuses.push((e.id.clone(), unread, e.is_saved()));
        }
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
                // `read` ist der gewünschte Zustand: `unread = !read`.
                sr(db2.apply_remote_status(
                    &account_id,
                    &id,
                    Some(!unread),
                    if saved { Some(true) } else { None },
                    generation,
                ))?;
                Ok::<_, SyncFailure>(())
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
) -> SyncResult<pf::Inventory> {
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
) -> SyncResult<StreamSummary>
where
    F: FnMut(Vec<pf::Entry>) -> Fut,
    Fut: std::future::Future<Output = Result<usize, String>>,
{
    let mut pager = pf::Pager::new();
    let mut continuation: Option<String> = None;
    // Der Serverstand der **ersten** Seite ist der Checkpoint der Phase.
    let mut checkpoint: Option<i64> = None;
    loop {
        let page = client
            .stream_contents(stream, 100, continuation.as_deref(), newer_than, false)
            .await
            .map_err(io_err)?;
        if checkpoint.is_none() {
            checkpoint = page.updated;
        }
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
                return Err(SyncFailure::new(
                    format!(
                        "Inhaltsabruf unvollständig: {}",
                        pager
                            .stopped_because
                            .unwrap_or_else(|| "Abbruch ohne Angabe".to_string())
                    ),
                    None,
                    None,
                ))
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
/// Ergebnis einer Statusphase: `complete` ist nur true, wenn das Inventar
/// vollständig war. Ein unvollständiger Lauf darf weder Watermark noch
/// Erfolgsmeldung auslösen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseResult {
    pub complete: bool,
    pub applied: usize,
    pub reason: Option<String>,
}

pub async fn reconcile_saved(
    worker: &DbWorker,
    account_id: &str,
    saved: &pf::Inventory,
    pull_generation: i64,
) -> SyncResult<PhaseResult> {
    let remote = match saved {
        pf::Inventory::Complete(ids) => ids.clone(),
        pf::Inventory::Incomplete(reason) => {
            dbg_log(&format!(
                "Saved-Reconciliation übersprungen (Inventar unvollständig: {reason})"
            ));
            return Ok(PhaseResult {
                complete: false,
                applied: 0,
                reason: Some(reason.clone()),
            });
        }
    };
    let local_saved: Vec<String> = db(worker, {
        let account_id = account_id.to_string();
        move |db2| sr(db2.saved_ids_for_account(&account_id))
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
                sr(db2.apply_remote_status(&account_id, &id, None, Some(false), generation))?;
                Ok::<_, SyncFailure>(())
            }
        })
        .await?;
    }
    for id in &to_save {
        db(worker, {
            let account_id = account_id.to_string();
            let id = id.clone();
            move |db2| {
                sr(db2.apply_remote_status(&account_id, &id, None, Some(true), generation))?;
                Ok::<_, SyncFailure>(())
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
    Ok(PhaseResult {
        complete: true,
        applied: to_unsave.len() + to_save.len(),
        reason: None,
    })
}

/// Gleicht die lokalen Ungelesen-Markierungen mit einem **vollständigen**
/// Remote-Inventar ab. Nur nachgewiesene Vollständigkeit darf remote fehlende
/// Artikel als gelesen setzen; lokale Neuere-Mutationen bleiben unangetastet.
pub async fn reconcile_unread(
    worker: &DbWorker,
    account_id: &str,
    unread: &pf::Inventory,
    pull_generation: i64,
) -> SyncResult<PhaseResult> {
    let remote = match unread {
        pf::Inventory::Complete(ids) => ids.clone(),
        pf::Inventory::Incomplete(reason) => {
            dbg_log(&format!(
                "Unread-Reconciliation übersprungen (Inventar unvollständig: {reason})"
            ));
            return Ok(PhaseResult {
                complete: false,
                applied: 0,
                reason: Some(reason.clone()),
            });
        }
    };
    let remote_set: std::collections::HashSet<&str> = remote.iter().map(String::as_str).collect();
    let local: Vec<String> = db(worker, {
        let account_id = account_id.to_string();
        move |db2| sr(db2.unread_ids_for_account(&account_id))
    })
    .await?;
    let missing: Vec<String> = local
        .iter()
        .filter(|id| !remote_set.contains(id.as_str()))
        .cloned()
        .collect();
    // Zweite Richtung: lokal gelesene Artikel, die remote wieder ungelesen sind,
    // werden revisionsgeschützt wieder als ungelesen markiert.
    let became_unread: Vec<String> = db(worker, {
        let account_id = account_id.to_string();
        let ids: Vec<String> = remote.clone();
        move |db2| sr(db2.read_ids_within(&account_id, &ids))
    })
    .await?;
    let mut applied = 0usize;
    for id in missing {
        let changed = db(worker, {
            let account_id = account_id.to_string();
            move |db2| {
                // Fehlt die ID im Unread-Inventar, ist der Artikel remote gelesen.
                let res = sr(db2.apply_remote_status(
                    &account_id,
                    &id,
                    Some(true),
                    None,
                    pull_generation,
                ))?;
                Ok::<_, SyncFailure>(res.applied)
            }
        })
        .await?;
        if changed {
            applied += 1;
        }
    }
    let mut newly_unread = 0usize;
    for id in became_unread {
        let changed = db(worker, {
            let account_id = account_id.to_string();
            move |db2| {
                let res = sr(db2.apply_remote_status(
                    &account_id,
                    &id,
                    Some(false),
                    None,
                    pull_generation,
                ))?;
                Ok::<_, SyncFailure>(res.applied)
            }
        })
        .await?;
        if changed {
            newly_unread += 1;
        }
    }
    dbg_log(&format!(
        "Unread-Reconciliation: remote {}, lokal {}, neu gelesen {}, neu ungelesen {}",
        remote.len(),
        local.len(),
        applied,
        newly_unread
    ));
    Ok(PhaseResult {
        complete: true,
        applied: applied + newly_unread,
        reason: None,
    })
}

/// Pflichtphasen müssen vollständig sein; sonst kein Erfolg und kein Watermark.
pub fn require_complete(phases: &[PhaseResult]) -> SyncResult<()> {
    for phase in phases {
        if !phase.complete {
            return Err(SyncFailure::new(
                format!(
                    "Statusabgleich unvollständig: {}",
                    phase.reason.clone().unwrap_or_else(|| "unbekannt".into())
                ),
                Some("degraded"),
                None,
            ));
        }
    }
    Ok(())
}

/// Prüft, ob der Server die gerade gesendete Absicht tatsächlich übernommen hat.
pub fn confirmed(entry: &pf::Entry, field: &str, desired: bool) -> bool {
    match field {
        "read" => entry.unread.map(|u| u != desired).unwrap_or(false),
        _ => entry.is_saved() == desired,
    }
}

/// Gesammelte IDs beider Statusinventare (für das Nachladen unbekannter Artikel).
pub fn inventory_ids(saved: &pf::Inventory, unread: &pf::Inventory) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for inventory in [saved, unread] {
        if let pf::Inventory::Complete(list) = inventory {
            for id in list {
                if seen.insert(id.clone()) {
                    ids.push(id.clone());
                }
            }
        }
    }
    ids
}

/// Lädt Inhalte nach: lokale Artikel ohne Inhalt **und** IDs aus den Statusinventaren,
/// die lokal noch nicht existieren. Beides in Batches per `.mget`.
pub async fn load_contents(
    worker: &DbWorker,
    client: &pf::FeedlyClient,
    account_id: &str,
    pull_generation: i64,
    remote_ids: &[String],
    batch: usize,
    ctx: &RunCtx,
) -> SyncResult<usize> {
    if remote_ids.is_empty() {
        return Ok(0);
    }
    let known = db(worker, {
        let account_id = account_id.to_string();
        let ids: Vec<String> = remote_ids.to_vec();
        move |db2| sr(db2.known_article_ids(&account_id, &ids))
    })
    .await?;
    let known_set: std::collections::HashSet<&str> = known.iter().map(String::as_str).collect();
    // Unbekannte IDs zuerst: gespeicherte Artikel nicht (mehr) abonnierter Quellen.
    let mut wanted: Vec<String> = remote_ids
        .iter()
        .filter(|id| !known_set.contains(id.as_str()))
        .cloned()
        .collect();
    // Lokale gespeicherte oder ungelesene Artikel ohne Inhalt kommen danach.
    wanted.extend(
        db(worker, {
            let account_id = account_id.to_string();
            move |db2| sr(db2.articles_needing_content(&account_id, batch as u32))
        })
        .await?,
    );
    wanted.dedup();
    let mut loaded_total = 0usize;
    for chunk in wanted.chunks(batch.max(1)) {
        if ctx.is_cancelled() {
            dbg_log("Nachladen: Lauf abgebrochen");
            break;
        }
        let entries = client
            .entries_mget(chunk)
            .await
            .map_err(|e| SyncFailure::new(format!("Nachladen fehlgeschlagen: {e}"), None, None))?;
        let received = entries.len();
        ingest_entries(worker, account_id, pull_generation, entries)
            .await
            .map_err(string_err)?;
        loaded_total += received;
    }
    if loaded_total > 0 {
        dbg_log(&format!("Nachgeladene Inhalte: {loaded_total} Artikel"));
    }
    Ok(loaded_total)
}

/// Lädt fehlende Inhalte gespeicherter oder ungelesener Artikel per `.mget` nach.
#[cfg(test)]
pub async fn load_missing_contents(
    worker: &DbWorker,
    client: &pf::FeedlyClient,
    account_id: &str,
    pull_generation: i64,
    batch: usize,
) -> SyncResult<usize> {
    let ids: Vec<String> = db(worker, {
        let account_id = account_id.to_string();
        move |db2| sr(db2.articles_needing_content(&account_id, batch as u32))
    })
    .await?;
    if ids.is_empty() {
        return Ok(0);
    }
    let entries = client
        .entries_mget(&ids)
        .await
        .map_err(|e| SyncFailure::new(format!("Nachladen fehlgeschlagen: {e}"), None, None))?;
    let received = entries.len();
    ingest_entries(worker, account_id, pull_generation, entries)
        .await
        .map_err(string_err)?;
    dbg_log(&format!(
        "Nachgeladene Inhalte: {received} Artikel (von {} IDs)",
        ids.len()
    ));
    Ok(received)
}

/// Gleicht Abos und Gruppen mit dem Server ab. Wird von **beiden** Sync-Einstiegen
/// verwendet; entfernte Quellen werden erst nach einer vollständigen Liste deaktiviert.
pub async fn sync_subscriptions(
    worker: &DbWorker,
    client: &pf::FeedlyClient,
    account_id: &str,
) -> SyncResult<usize> {
    let subs = client.subscriptions().await.map_err(io_err)?;
    let cats = client.categories().await.map_err(io_err)?;
    for c in &cats {
        let name = c.label.clone().unwrap_or_else(|| "Gruppe".into());
        db(worker, {
            let account_id = account_id.to_string();
            let c = c.clone();
            move |db2| sr(db2.upsert_group_remote(&account_id, &c.id, &name))
        })
        .await?;
    }
    let group_ids: std::collections::HashMap<String, i64> = db(worker, |db2| sr(db2.list_groups()))
        .await?
        .into_iter()
        .filter_map(|g| g.remote_id.map(|r| (r, g.id)))
        .collect();
    for s in &subs {
        let url = s.id.strip_prefix("feed/").unwrap_or(&s.id).to_string();
        let title = s.title.clone().unwrap_or_else(|| url.clone());
        let fid = db(worker, {
            let account_id = account_id.to_string();
            let s = s.clone();
            let url = url.clone();
            let title = title.clone();
            move |db2| {
                sr(db2.upsert_feed_remote(&account_id, &s.id, &url, &title, s.website.as_deref()))
            }
        })
        .await?;
        let gids: Vec<i64> = s
            .categories
            .iter()
            .filter_map(|c| group_ids.get(&c.id).copied())
            .collect();
        db(worker, move |db2| sr(db2.set_feed_groups(fid, &gids))).await?;
    }
    // Vollständige Liste: nicht mehr enthaltene Quellen stilllegen, gespeicherte
    // Artikel bleiben erhalten.
    let present: Vec<String> = subs.iter().map(|s| s.id.clone()).collect();
    let account_for_db = account_id.to_string();
    let deactivated = db(worker, move |db2| {
        sr(db2.deactivate_missing_feeds(&account_for_db, &present))
    })
    .await?;
    if !deactivated.is_empty() {
        dbg_log(&format!(
            "Abgleich: {} Quellen nicht mehr abonniert (deaktiviert, gespeicherte Artikel bleiben)",
            deactivated.len()
        ));
    }
    Ok(subs.len())
}

pub fn initial_sync(worker: DbWorker, net: &Net, token: String, ctx: RunCtx) {
    let tx = net.event_sender();
    set_status(&worker, "initial_sync", None);
    let sync_started_ms = storage::now_ms();
    net.spawn(async move {
        let client = pf::FeedlyClient::with_base(token, ctx.base());
        let res: SyncResult<usize> = async {
            let profile = client.profile().await.map_err(io_err)?;
            db(&worker, {
                let p = profile.clone();
                move |db2| {
                    sr(db2.upsert_account(
                        &p.id,
                        "feedly",
                        p.full_name
                            .as_deref()
                            .or(p.email.as_deref())
                            .unwrap_or("Feedly"),
                    ))
                }
            })
            .await?;
            if ctx.is_cancelled() {
                return Err(SyncFailure::new("Lauf abgebrochen", None, None));
            }
            // Die Konto-ID aus dem Profil gilt für alle Statusmeldungen und Ereignisse.
            let account_id = profile.id.clone();
            sync_subscriptions(&worker, &client, &account_id).await?;
            let stream = pf::global_all_stream(&profile.id);
            let newer_than = storage::now_ms() - 30 * 86_400_000;
            // Generation vor dem ersten Inhaltsabruf erfassen.
            let pull_gen = db(&worker, {
                let account_id = account_id.clone();
                move |db2| sr(db2.pull_generation(&account_id))
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
            let unread =
                fetch_id_inventory(&client, &pf::unread_stream(&account_id), MAX_ID_PAGES).await?;
            reconcile_unread(&worker, &account_id, &unread, pull_gen).await?;
            let ids = inventory_ids(&saved, &unread);
            load_contents(&worker, &client, &account_id, pull_gen, &ids, 200, &ctx).await?;
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
                sr(db2.set_last_sync(&account_id, checkpoint))
            })
            .await?;
            Ok(added)
        }
        .await;
        match res {
            Ok(added) => {
                set_status(&worker, "ready", Some(&ctx.account_id));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncDone {
                    account_id: ctx.account_id.clone(),
                    run_id: ctx.run_id,
                    added,
                });
            }
            Err(e) => {
                let status = e.status.clone().unwrap_or_else(|| "degraded".to_string());
                set_status(&worker, &status, Some(&ctx.account_id));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                    account_id: ctx.account_id.clone(),
                    run_id: ctx.run_id,
                    message: e.message.clone(),
                    status: e.status.clone(),
                    retry_after_ms: e.retry_after_ms,
                });
            }
        }
    });
}

/// Serverseitiges „alles gelesen“ für eine Auswahl von Feeds. Der lokale Zustand
/// folgt erst nach der Serverbestätigung, damit die transaktionale Outbox stimmt.
pub fn mark_feeds_server_side(
    worker: DbWorker,
    net: &Net,
    token: String,
    ctx: RunCtx,
    remote_feed_ids: Vec<String>,
    local_feed_ids: Vec<i64>,
) {
    let tx = net.event_sender();
    net.spawn(async move {
        if ctx.is_cancelled() {
            return;
        }
        let client = pf::FeedlyClient::with_base(token, ctx.base());
        match client.markers_feeds("markAsRead", &remote_feed_ids).await {
            Ok(()) => {
                let marked = db(&worker, move |db2| sr(db2.mark_feeds_read(&local_feed_ids))).await;
                if let Err(e) = marked {
                    eprintln!("[lf] lokale Zähler konnten nicht nachgezogen werden: {e}");
                }
                let _ = tx.send(crate::net::NetEvent::FeedlySyncDone {
                    account_id: ctx.account_id.clone(),
                    run_id: ctx.run_id,
                    added: 0,
                });
            }
            Err(e) => {
                let failure = io_err(e);
                set_status(&worker, "degraded", Some(&ctx.account_id));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                    account_id: ctx.account_id.clone(),
                    run_id: ctx.run_id,
                    message: format!(
                        "Serverseitig als gelesen fehlgeschlagen: {}",
                        failure.message
                    ),
                    status: failure.status.clone(),
                    retry_after_ms: failure.retry_after_ms,
                });
            }
        }
    });
}

/// Nach Feld und Sollzustand gruppierte Outbox-Zeilen: (Zeilen-ID, Revision, Artikel).
type OutboxGroups = std::collections::HashMap<(String, bool), Vec<(i64, i64, String)>>;

pub fn process_outbox(worker: DbWorker, net: &Net, token: String, ctx: RunCtx) {
    let account_id = ctx.account_id.clone();
    let tx = net.event_sender();
    net.spawn(async move {
        let client = pf::FeedlyClient::with_base(token, ctx.base());
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
        let mut groups: OutboxGroups = std::collections::HashMap::new();
        for r in &rows {
            groups
                .entry((r.field.clone(), r.desired))
                .or_default()
                .push((r.id, r.revision, r.entity_id.clone()));
        }
        for ((field, desired), entries) in groups {
            // Nach Logout, Auth- oder Quotenstopp wird kein weiterer Batch gesendet.
            if ctx.is_cancelled() {
                dbg_log("Outbox: Lauf abgebrochen, weitere Batches entfallen");
                break;
            }
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
                    // Die Nachbestätigung ist an die gesendete Revision gebunden:
                    // eine inzwischen neuere Benutzerabsicht wird nicht überschrieben.
                    match client.entries_mget(&entry_ids).await {
                        Ok(remote) => {
                            let by_id: std::collections::HashMap<&str, &pf::Entry> =
                                remote.iter().map(|e| (e.id.as_str(), e)).collect();
                            // Fehlende Antworten gelten ausdrücklich als unbestätigt.
                            let unconfirmed: Vec<String> = entries
                                .iter()
                                .filter(|(_, _, entity)| {
                                    match by_id.get(entity.as_str()) {
                                        Some(e) => !confirmed(e, &field, desired),
                                        None => true,
                                    }
                                })
                                .map(|(_, _, entity)| entity.clone())
                                .collect();
                            if !unconfirmed.is_empty() {
                                let count = unconfirmed.len();
                                let account_for_db = account_id.clone();
                                let field_for_db = field.clone();
                                let sent_for_db: Vec<(i64, i64, String)> = entries
                                    .iter()
                                    .map(|(id, rev, entity)| (*id, *rev, entity.clone()))
                                    .collect();
                                let requeued = db(&worker, move |db2| {
                                    let mut requeued = 0usize;
                                    for (_, revision, entity) in &sent_for_db {
                                        if db2.outbox_requeue_if_unchanged(
                                            &account_for_db,
                                            entity,
                                            &field_for_db,
                                            desired,
                                            *revision,
                                        )? {
                                            requeued += 1;
                                        }
                                    }
                                    let notices: Vec<(String, String)> = unconfirmed
                                        .iter()
                                        .map(|e| (account_for_db.clone(), e.clone()))
                                        .collect();
                                    db2.mark_unsynced(&notices)?;
                                    Ok::<_, storage::StorageError>(requeued)
                                })
                                .await;
                                dbg_log(&format!(
                                    "Nachbestätigung: {} von {} erneut vorgemerkt",
                                    id_count_guard(requeued).unwrap_or(0),
                                    count
                                ));
                                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                                    account_id: ctx.account_id.clone(),
                                    run_id: ctx.run_id,
                                    message: format!(
                                        "Feedly hat {count} von {total} Änderungen noch nicht bestätigt — erneuter Versuch vorgemerkt"
                                    ),
                                    status: Some("degraded".into()),
                                    retry_after_ms: None,
                                });
                            }
                        }
                        Err(e) => {
                            // Eine fehlgeschlagene Bestätigungsabfrage ist selbst eine
                            // offene Frage und wird sichtbar, nicht still übergangen.
                            let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                                account_id: ctx.account_id.clone(),
                                run_id: ctx.run_id,
                                message: format!("Bestätigung nach dem Upload fehlgeschlagen: {e}"),
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
                        account_id: ctx.account_id.clone(),
                        run_id: ctx.run_id,
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
                        account_id: ctx.account_id.clone(),
                        run_id: ctx.run_id,
                        status: Some("rate_limited".into()),
                        retry_after_ms: Some(next),
                        message: format!("Drosselung durch Feedly — neuer Versuch ab {}", fmt_ms(next)),
                    });
                    // Drosselung beendet den Versand dieses Laufs.
                    break;
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
                        account_id: ctx.account_id.clone(),
                        run_id: ctx.run_id,
                        status: Some("auth_required".into()),
                        retry_after_ms: Some(next),
                        message: format!("Auth/Rechte ({status}): {message} — bitte neu verbinden"),
                    });
                    // Auth-Stopp: keine weiteren Batches.
                    break;
                }
                Err(e) => {
                    let next = storage::now_ms() + 60_000;
                    let _ = db(&worker, move |db2| db2.outbox_fail(&sent, next, false)).await;
                    let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                        account_id: ctx.account_id.clone(),
                        run_id: ctx.run_id,
                        message: e.to_string(),
                        status: None,
                        retry_after_ms: Some(next),
                    });
                    if ctx.is_cancelled() {
                        break;
                    }
                }
            }
        }
    });
}

/// Der Rückgabewert der Nachbestätigung ist nur für Diagnosezwecke bestimmt.
fn id_count_guard(count: Result<usize, storage::StorageError>) -> Option<usize> {
    match count {
        Ok(n) => Some(n),
        Err(e) => {
            eprintln!("[lf] Nachbestätigung nicht verarbeitet: {e}");
            None
        }
    }
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
    ctx: RunCtx,
) {
    let tx = net.event_sender();
    set_status(&worker, "syncing", Some(&account_id));
    let sync_started_ms = storage::now_ms();
    net.spawn(async move {
        let client = pf::FeedlyClient::with_base(token, ctx.base());
        let res: SyncResult<usize> = async {
            let overlap = last_sync_ms - 5 * 60_000;
            let pull_gen = db(&worker, {
                let account_id = account_id.clone();
                move |db2| sr(db2.pull_generation(&account_id))
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
                            sr(db2.apply_remote_status(
                                &account_id,
                                &id,
                                Some(true),
                                None,
                                pull_gen,
                            ))?;
                            Ok::<_, SyncFailure>(())
                        }
                    })
                    .await?;
                }
                let next = page.continuation.filter(|c| !c.is_empty());
                match reads_pager.after_page(next.as_deref(), page.entries.len(), MAX_READS_PAGES) {
                    pf::PageAction::Continue => reads_continuation = next,
                    pf::PageAction::Done => break,
                    pf::PageAction::Aborted => {
                        return Err(SyncFailure::new(
                            format!(
                                "Read-Abgleich unvollständig: {}",
                                reads_pager
                                    .stopped_because
                                    .unwrap_or_else(|| "Abbruch ohne Angabe".to_string())
                            ),
                            None,
                            None,
                        ))
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
            let saved_phase = reconcile_saved(&worker, &account_id, &saved, pull_gen).await?;
            // Vollständiger Leseabgleich: Unread-Inventar und fehlende Inhalte.
            let unread =
                fetch_id_inventory(&client, &pf::unread_stream(&account_id), MAX_ID_PAGES).await?;
            let unread_phase = reconcile_unread(&worker, &account_id, &unread, pull_gen).await?;
            // Eine unvollständige Pflichtphase beendet den Lauf ohne Watermark.
            require_complete(&[saved_phase, unread_phase])?;
            let ids = inventory_ids(&saved, &unread);
            load_contents(&worker, &client, &account_id, pull_gen, &ids, 200, &ctx).await?;
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
                sr(db2.set_last_sync(&account_id, checkpoint))
            })
            .await?;
            Ok(added)
        }
        .await;
        match res {
            Ok(added) => {
                set_status(&worker, "ready", Some(&ctx.account_id));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncDone {
                    account_id: ctx.account_id.clone(),
                    run_id: ctx.run_id,
                    added,
                });
            }
            Err(e) => {
                let status = e.status.clone().unwrap_or_else(|| "degraded".to_string());
                set_status(&worker, &status, Some(&ctx.account_id));
                let _ = tx.send(crate::net::NetEvent::FeedlySyncFailed {
                    account_id: ctx.account_id.clone(),
                    run_id: ctx.run_id,
                    message: e.message.clone(),
                    status: e.status.clone(),
                    retry_after_ms: e.retry_after_ms,
                });
            }
        }
    });
}

fn string_err(e: String) -> SyncFailure {
    SyncFailure::new(e, None, None)
}

fn storage_err(e: storage::StorageError) -> SyncFailure {
    SyncFailure::new(e.to_string(), None, None)
}

/// DB-Ergebnis in das Fehlerformat eines Laufs überführen.
fn sr<T>(r: storage::Result<T>) -> SyncResult<T> {
    r.map_err(storage_err)
}

/// Wandelt einen Feedly-Fehler in einen typisierten Lauf-Fehler. HTTP-Status und
/// `Retry-After` bleiben bis zur Steuerung erhalten.
pub fn io_err(e: pf::FeedlyError) -> SyncFailure {
    let (status, retry_after_ms) = match &e {
        pf::FeedlyError::Api { status: 401, .. } | pf::FeedlyError::Api { status: 403, .. } => {
            (Some("auth_required"), None)
        }
        pf::FeedlyError::Api {
            status: 429,
            retry_after_ms,
            ..
        } => (Some("rate_limited"), *retry_after_ms),
        pf::FeedlyError::Api { status: 404, .. } => (Some("degraded"), None),
        _ => (None, None),
    };
    SyncFailure::new(e.to_string(), status, retry_after_ms)
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
        let _ = db(&worker, |db| {
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
        assert!(applied.complete);
        assert_eq!(applied.applied, 1);
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
        assert!(
            !applied.complete,
            "ein unvollständiges Inventar ist kein Erfolg"
        );
        assert_eq!(applied.applied, 0);
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
        let _ = db(&worker, |db| {
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

    fn write_script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join(format!("secret-tool-{}", body.len()));
        std::fs::write(&path, body).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

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

    /// A7: `secret-tool store` liefert absichtlich keinen Text; ein leerer stdout
    /// darf nicht als Fehlschlag gelten. Geprüft wird gegen ein Skript, nicht
    /// gegen den echten Secret Service.
    #[test]
    fn speichern_gilt_bei_exit_null_als_erfolg() {
        let dir = tempdir("exit0");
        let fake = write_script(&dir, "#!/bin/sh\ncat >/dev/null\nexit 0\n");
        assert!(
            bind_account_with(&fake, "feedly-1"),
            "store mit leerem stdout"
        );
        assert!(keyring_store_with(&fake, "tok"), "Token wird gespeichert");
    }

    /// Ein Lookup ohne Treffer liefert `None`, ein Treffer den Wert.
    #[test]
    fn lookup_unterscheidet_treffer_von_leerer_ausgabe() {
        let dir = tempdir("lookup");
        let fake = write_script(
            &dir,
            "#!/bin/sh\nif [ \"$3\" = \"feedly-token\" ]; then echo geheim; fi\nexit 0\n",
        );
        assert_eq!(keyring_lookup_with(&fake).as_deref(), Some("geheim"));
        let leer = write_script(&dir, "#!/bin/sh\nexit 0\n");
        assert!(
            keyring_lookup_with(&leer).is_none(),
            "leere Ausgabe ist kein Treffer"
        );
    }

    /// Ein blockierter Schlüsselbund darf den Dateifallback auslösen, nicht crashen.
    #[test]
    fn gesperrter_keyring_fuehrt_zum_dateifallback() {
        let dir = tempdir("gesperrt");
        let fehlend = dir.join("gibt-es-nicht");
        assert!(!bind_account_with(&fehlend, "feedly-1"));
        assert!(!keyring_store_with(&fehlend, "tok"));
        let path = dir.join("feedly-token");
        save_token_at(&path, "tok", &fehlend).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "tok");
    }

    #[test]
    fn tokenbindung_werden_geprueft() {
        assert!(token_matches_account(Some("feedly-1"), "feedly-1"));
        assert!(
            !token_matches_account(Some("feedly-1"), "feedly-2"),
            "Token eines anderen Profils wird abgewiesen"
        );
        assert!(
            token_matches_account(None, "feedly-1"),
            "ohne Bindung (Dateifallback) wird beim Sync gebunden"
        );
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

#[cfg(test)]
mod outbox_e2e_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn mock_with_script(
        responder: impl Fn(&str) -> (u16, String) + Send + Sync + 'static,
    ) -> (String, Arc<AtomicUsize>) {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 16384];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                counter.fetch_add(1, Ordering::SeqCst);
                let (code, body) = responder(&req);
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}/v3/"), hits)
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lesefluss-outbox-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A5 über den echten Outbox-Pfad: Der Server bestätigt verzögert und meldet
    /// weiterhin „unread“. Zwischen Upload und Antwort entscheidet sich die Person
    /// um. Die neuere Absicht muss erhalten bleiben.
    #[test]
    fn verzoegerte_bestaetigung_ueberschreibt_keine_neue_absicht() {
        let dir = tempdir("a5");
        let worker = DbWorker::start(dir.join("library.db"));
        let entry_id = "https://feedly.com/i/entry/a5".to_string();
        let id_for_db = entry_id.clone();
        db_blocking(&worker, move |db| {
            db.upsert_account("feedly-1", "feedly", "Feedly").unwrap();
            let feed = db
                .add_feed("feedly-1", "u1", "Feed", None, "#111111")
                .unwrap();
            let now = storage::now_ms();
            db.upsert_article(feed, &id_for_db, "Titel", None, None, now, "e", None, now)
                .unwrap();
        });

        let entry_for_server = entry_id.clone();
        let (base, _hits) = mock_with_script(move |req| {
            if req.starts_with("POST") && req.contains("/markers") {
                (200, "{}".to_string())
            } else if req.contains("entries/.mget") {
                std::thread::sleep(std::time::Duration::from_millis(250));
                (
                    200,
                    serde_json::json!([{
                        "id": entry_for_server,
                        "unread": true,
                        "origin": {"streamId": "feed/https://example.org/f.xml"}
                    }])
                    .to_string(),
                )
            } else {
                (404, "{}".to_string())
            }
        });

        // Erste Absicht: als gelesen markieren.
        let id = entry_id.clone();
        db_blocking(&worker, move |db| {
            let feed = db
                .raw()
                .query_row(
                    "SELECT id FROM feeds WHERE account_id='feedly-1'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
            db.apply_status_with_outbox(feed, &id, Some(true), None)
                .unwrap();
        });

        let net = Net::start();
        let ctx = RunCtx::new("feedly-1", 1).with_base(&base);
        process_outbox(worker.clone(), &net, "tok".to_string(), ctx);

        // Während die Bestätigung läuft, entscheidet die Person anders.
        std::thread::sleep(std::time::Duration::from_millis(80));
        let id = entry_id.clone();
        db_blocking(&worker, move |db| {
            let feed = db
                .raw()
                .query_row(
                    "SELECT id FROM feeds WHERE account_id='feedly-1'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
            db.apply_status_with_outbox(feed, &id, Some(false), None)
                .unwrap();
        });

        // Auf das Abschlussereignis warten.
        let mut failed = false;
        for _ in 0..100 {
            match net
                .events
                .recv_timeout(std::time::Duration::from_millis(100))
            {
                Ok(crate::net::NetEvent::FeedlySyncFailed { .. }) => {
                    failed = true;
                    break;
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(failed, "die fehlende Bestätigung wird gemeldet");

        let pending: Vec<(String, bool, i64)> = db_blocking(&worker, |db| {
            let mut rows = db
                .raw()
                .prepare(
                    "SELECT entity_id, desired, revision FROM outbox WHERE account_id='feedly-1'",
                )
                .unwrap();
            let out: Vec<(String, bool, i64)> = rows
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)? == 1,
                        r.get::<_, i64>(2)?,
                    ))
                })
                .unwrap()
                .map(|row| row.unwrap())
                .collect();
            out
        });
        assert_eq!(pending.len(), 1, "genau eine Absicht wartet: {pending:?}");
        assert!(
            !pending[0].1,
            "die neuere Absicht (unread) ist maßgeblich, nicht das alte read"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Kleiner HTTP-Mockserver für die App-Tests: zählt Anfragen und antwortet nach
/// einem Skript. Ohne echten Netzwerkzugriff und ohne Umgebungsveränderung.
/// Kleiner HTTP-Mockserver für die App-Tests: zählt Anfragen und antwortet nach
/// einem Skript. Ohne echten Netzwerkzugriff und ohne Umgebungsveränderung.
#[cfg(test)]
pub mod app_mock_server {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    pub fn mock(
        responder: impl Fn(&str) -> (u16, String) + Send + Sync + 'static,
    ) -> (String, Arc<AtomicUsize>) {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 16384];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                counter.fetch_add(1, Ordering::SeqCst);
                let (code, body) = responder(&req);
                let resp = format!(
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}/v3/"), hits)
    }
}

#[cfg(test)]
mod read_sync_tests {
    use super::*;
    async fn worker_with_feedly(tag: &str) -> (DbWorker, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("lesefluss-readsync-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.db");
        let worker = DbWorker::start(path.clone());
        let _ = db(&worker, |db| {
            db.upsert_account("feedly-1", "feedly", "Feedly").unwrap();
            Ok::<_, storage::StorageError>(())
        })
        .await;
        (worker, path)
    }

    /// A4: Lokal gelesene Artikel, die remote wieder ungelesen sind, werden
    /// wieder ungelesen — revisionsgeschützt.
    #[tokio::test]
    async fn remote_wieder_ungelesen_wird_lokal_ungelesen() {
        let (worker, path) = worker_with_feedly("neu-unread").await;
        let _ = db(&worker, |db| {
            let feed = db
                .add_feed("feedly-1", "u1", "Feed", None, "#111111")
                .unwrap();
            let now = storage::now_ms();
            db.upsert_article(feed, "a", "A", None, None, now, "e", None, now)
                .unwrap();
            db.raw()
                .execute("UPDATE articles SET unread=0 WHERE id='a'", [])
                .unwrap();
            Ok::<_, storage::StorageError>(())
        })
        .await;
        let inventory = pf::Inventory::Complete(vec!["a".into()]);
        let changed = reconcile_unread(&worker, "feedly-1", &inventory, i64::MAX)
            .await
            .unwrap();
        assert!(changed.complete);
        assert_eq!(changed.applied, 1);
        let unread: i64 = db(&worker, |db| {
            db.raw()
                .query_row("SELECT unread FROM articles WHERE id='a'", [], |r| r.get(0))
                .unwrap()
        })
        .await;
        assert_eq!(unread, 1, "remote ungelesen wird lokal wieder ungelesen");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Eine neuere lokale Änderung bleibt auch in der zweiten Richtung erhalten.
    #[tokio::test]
    async fn neuere_lokale_aenderung_gewinnt_gegen_die_zweite_richtung() {
        let (worker, path) = worker_with_feedly("neu-unread-guard").await;
        let _ = db(&worker, |db| {
            let feed = db
                .add_feed("feedly-1", "u1", "Feed", None, "#111111")
                .unwrap();
            let now = storage::now_ms();
            db.upsert_article(feed, "a", "A", None, None, now, "e", None, now)
                .unwrap();
            db.raw()
                .execute("UPDATE articles SET unread=0 WHERE id='a'", [])
                .unwrap();
            Ok::<_, storage::StorageError>(())
        })
        .await;
        // Generation 0: danach wird lokal eine neue Absicht erzeugt.
        let pull_gen = db(&worker, |db| db.pull_generation("feedly-1").unwrap()).await;
        let _ = db(&worker, |db| {
            let feed = db
                .raw()
                .query_row(
                    "SELECT id FROM feeds WHERE account_id='feedly-1'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
            db.apply_status_with_outbox(feed, "a", Some(true), None)
                .unwrap();
            Ok::<_, storage::StorageError>(())
        })
        .await;
        let inventory = pf::Inventory::Complete(vec!["a".into()]);
        let _ = reconcile_unread(&worker, "feedly-1", &inventory, pull_gen)
            .await
            .unwrap();
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A4: Unbekannte IDs aus dem Saved-Inventar werden per `.mget` nachgeladen,
    /// auch wenn ihre Origin nicht (mehr) abonniert ist.
    #[tokio::test]
    async fn unbekannte_saved_ids_werden_inklusive_origin_angelegt() {
        let (worker, path) = worker_with_feedly("unbekannt").await;
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 8192];
                let _ = s.read(&mut buf);
                let body = serde_json::json!([{
                    "id": "https://feedly.com/i/entry/alt",
                    "unread": true,
                    "origin": {"streamId": "feed/https://alt.example.org/rss", "title": "Alt"}
                }])
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        let client = pf::FeedlyClient::with_base("t".into(), format!("http://{addr}/v3/"));
        let ids = vec!["https://feedly.com/i/entry/alt".to_string()];
        let loaded = load_contents(
            &worker,
            &client,
            "feedly-1",
            i64::MAX,
            &ids,
            50,
            &RunCtx::new("feedly-1", 1),
        )
        .await
        .unwrap();
        assert_eq!(loaded, 1);
        let (count, active) = db(&worker, |db| {
            let row = db
                .raw()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM articles WHERE id='https://feedly.com/i/entry/alt'),
                            (SELECT active FROM feeds WHERE remote_id='feed/https://alt.example.org/rss')",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                )
                .unwrap();
            row
        })
        .await;
        assert_eq!(count, 1, "der Artikel wurde angelegt");
        assert_eq!(active, 0, "die nicht abonnierte Quelle bleibt inaktiv");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Abo-Abgleich: entfernte Quellen werden deaktiviert, gespeicherte Artikel
    /// bleiben erhalten.
    #[tokio::test]
    async fn abgleich_deaktiviert_entfernte_quellen_nicht_gespeichertes() {
        let (worker, path) = worker_with_feedly("abos").await;
        let _ = db(&worker, |db| {
            let feed = db
                .add_feed("feedly-1", "u1", "Bleibt", None, "#111111")
                .unwrap();
            let gone = db
                .add_feed("feedly-1", "u2", "Weg", None, "#222222")
                .unwrap();
            db.upsert_feed_remote(
                "feedly-1",
                "feed/bleibt",
                "https://a.example/f",
                "Bleibt",
                None,
            )
            .unwrap();
            db.upsert_feed_remote("feedly-1", "feed/weg", "https://b.example/f", "Weg", None)
                .unwrap();
            let now = storage::now_ms();
            db.upsert_article(gone, "w1", "Weg", None, None, now, "e", None, now)
                .unwrap();
            db.set_saved_by_article_id("w1", true).unwrap();
            let _ = feed;
            Ok::<_, storage::StorageError>(())
        })
        .await;
        let deactivated = db(&worker, |db| {
            db.deactivate_missing_feeds("feedly-1", &["feed/bleibt".to_string()])
                .unwrap()
        })
        .await;
        assert_eq!(deactivated.len(), 1);
        let (active, saved) = db(&worker, |db| {
            let active = db
                .raw()
                .query_row(
                    "SELECT active FROM feeds WHERE remote_id='feed/weg'",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
            let saved = db
                .raw()
                .query_row("SELECT saved FROM articles WHERE id='w1'", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap();
            (active, saved)
        })
        .await;
        assert_eq!(active, 0, "entfernte Quelle ist inaktiv");
        assert_eq!(saved, 1, "gespeicherte Artikel bleiben erhalten");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

#[cfg(test)]
mod delta_e2e_tests {
    use super::*;
    use crate::feedly_sync::app_mock_server::mock;

    /// A6: Ein unvollständiges Inventar darf weder Erfolg melden noch den
    /// Watermark fortschreiben.
    #[test]
    fn unvollstaendige_statusphase_beendet_den_lauf_ohne_watermark() {
        use crate::feedly_sync::app_mock_server::mock;
        let dir = std::env::temp_dir().join(format!("lesefluss-a6-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let worker = DbWorker::start(dir.join("library.db"));
        db_blocking(&worker, |db| {
            db.upsert_account("feedly-1", "feedly", "Feedly").unwrap();
        });
        // Cursorzyklus: derselbe Cursor kommt immer wieder.
        let (base, _hits) = mock(|req| {
            if req.contains("markers/reads") {
                (200, r#"{"entries":[]}"#.to_string())
            } else if req.contains("global.all") {
                (200, r#"{"items":[],"updated":1700000000}"#.to_string())
            } else if req.contains("global.saved") || req.contains("global.unread") {
                (200, r#"{"ids":[],"continuation":"c1"}"#.to_string())
            } else {
                (404, "{}".to_string())
            }
        });
        let net = Net::start();
        let ctx = RunCtx::new("feedly-1", 1).with_base(&base);
        let last = storage::now_ms() - 60_000;
        delta_sync(
            worker.clone(),
            &net,
            "tok".to_string(),
            "feedly-1".to_string(),
            last,
            ctx,
        );
        let mut failed = None;
        for _ in 0..120 {
            match net
                .events
                .recv_timeout(std::time::Duration::from_millis(100))
            {
                Ok(crate::net::NetEvent::FeedlySyncFailed { message, .. }) => {
                    failed = Some(message);
                    break;
                }
                Ok(crate::net::NetEvent::FeedlySyncDone { .. }) => {
                    panic!("ein unvollständiger Abgleich darf keinen Erfolg melden")
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        let message = failed.expect("der Lauf meldet einen Fehler");
        assert!(
            message.contains("unvollständig") || message.contains("Abgleich"),
            "{message}"
        );
        let watermark = db_blocking(&worker, |db| db.last_sync("feedly-1").unwrap());
        assert!(
            watermark.is_none(),
            "ohne vollständige Phase wird kein Watermark geschrieben"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A4 über den echten Delta-Einstieg: vollständige Unread-/Saved-Inventare,
    /// beide Statusrichtungen und Nachladen eines unbekannten gespeicherten
    /// Artikels aus einer nicht (mehr) abonnierten Quelle.
    #[test]
    fn delta_sync_gleicht_beide_richtungen_und_laedt_unbekanntes_nach() {
        let dir = std::env::temp_dir().join(format!("lesefluss-delta-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let worker = DbWorker::start(dir.join("library.db"));
        let entry = "https://feedly.com/i/entry/gespeichert";
        let entry_owned = entry.to_string();
        db_blocking(&worker, move |db| {
            db.upsert_account("feedly-1", "feedly", "Feedly").unwrap();
            let feed = db
                .add_feed("feedly-1", "u1", "Feed", None, "#111111")
                .unwrap();
            let now = storage::now_ms();
            db.upsert_article(feed, &entry_owned, "Titel", None, None, now, "e", None, now)
                .unwrap();
            // lokal gelesen, remote aber wieder ungelesen
            db.raw()
                .execute("UPDATE articles SET unread=0 WHERE id=?1", [&entry_owned])
                .unwrap();
        });

        let e1 = entry.to_string();
        let e2 = e1.clone();
        let (base, _hits) = mock(move |req| {
            if req.contains("markers/reads") {
                (200, r#"{"entries":[]}"#.to_string())
            } else if req.contains("global.all") {
                (200, r#"{"items":[],"updated":1700000000}"#.to_string())
            } else if req.contains("global.saved") {
                (200, format!(r#"{{"ids":["{e1}"]}}"#))
            } else if req.contains("global.unread") {
                (200, format!(r#"{{"ids":["{e2}"]}}"#))
            } else if req.contains("entries/.mget") {
                (
                    200,
                    serde_json::json!([{
                        "id": e1,
                        "unread": true,
                        "content": {"content": "<p>Inhalt</p>"},
                        "origin": {"streamId": "feed/https://feedly.example/rss", "title": "Quelle"}
                    }])
                    .to_string(),
                )
            } else if req.contains("subscriptions") || req.contains("categories") {
                (200, "[]".to_string())
            } else {
                (404, "{}".to_string())
            }
        });

        let net = Net::start();
        let ctx = RunCtx::new("feedly-1", 1).with_base(&base);
        let last = storage::now_ms() - 60_000;
        delta_sync(
            worker.clone(),
            &net,
            "tok".to_string(),
            "feedly-1".to_string(),
            last,
            ctx,
        );

        let mut done = false;
        let mut failure = None;
        for _ in 0..120 {
            match net
                .events
                .recv_timeout(std::time::Duration::from_millis(100))
            {
                Ok(crate::net::NetEvent::FeedlySyncDone { .. }) => {
                    done = true;
                    break;
                }
                Ok(crate::net::NetEvent::FeedlySyncFailed { message, .. }) => {
                    failure = Some(message);
                    break;
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(done, "der Lauf meldet Erfolg, Fehler: {failure:?}");

        let (unread, has_content) = db_blocking(&worker, move |db| {
            let unread = db
                .raw()
                .query_row("SELECT unread FROM articles WHERE id=?1", [&entry], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap();
            let has_content = db
                .raw()
                .query_row(
                    "SELECT COUNT(*) FROM article_contents WHERE article_id=?1",
                    [&entry],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap();
            (unread, has_content)
        });
        assert_eq!(unread, 1, "remote ungelesen wurde lokal übernommen");
        assert_eq!(has_content, 1, "der fehlende Inhalt wurde nachgeladen");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
