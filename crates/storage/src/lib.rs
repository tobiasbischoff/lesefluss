use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("schema-fehler: {0}")]
    Schema(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Clone, Debug)]
pub struct FeedRow {
    pub id: i64,
    pub account_id: String,
    pub remote_id: Option<String>,
    pub feed_url: String,
    pub title: String,
    pub website: Option<String>,
    pub accent: String,
    pub groups: Vec<i64>,
}

#[derive(Clone, Debug)]
pub struct GroupRow {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub remote_id: Option<String>,
    pub account_id: String,
}

#[derive(Clone, Debug)]
pub struct ArticleRow {
    pub id: String,
    /// Optionale kleine Vorschau als Daten-URI (PNG, max. 128 px).
    pub thumb: Option<String>,
    pub feed_id: i64,
    pub sort_ms: i64,
    pub feed_title: String,
    pub accent: String,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_ms: i64,
    pub excerpt: String,
    pub unread: bool,
    pub saved: bool,
    pub has_content: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RemoteApply {
    pub applied: bool,
    pub skipped: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub unread: i64,
    pub saved: i64,
    pub total: i64,
    pub per_feed: Vec<(i64, i64)>,
    pub per_group: Vec<(i64, i64)>,
    pub per_account: Vec<(String, i64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Global,
    Feed(i64),
    Group(i64),
    Account(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    Unread,
    All,
    Saved,
}

#[derive(Clone, Debug)]
pub struct NewArticle {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub url: Option<String>,
    pub published_ms: i64,
    pub excerpt: String,
    pub html: Option<String>,
    pub content_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub struct OutboxRow {
    pub id: i64,
    pub account_id: String,
    pub entity_id: String,
    pub field: String,
    pub desired: bool,
    pub revision: i64,
}

#[derive(Clone, Debug, Default)]
pub struct FetchState {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub last_fetch_ms: Option<i64>,
    pub next_fetch_ms: Option<i64>,
    pub error_count: i64,
    pub last_error: Option<String>,
}

const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        r#"
CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    remote_user TEXT,
    created_ms INTEGER NOT NULL
);
CREATE TABLE account_state (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    last_sync_ms INTEGER
);
CREATE TABLE groups (
    id INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    parent_id INTEGER REFERENCES groups(id) ON DELETE SET NULL
);
CREATE TABLE feeds (
    id INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    feed_url TEXT NOT NULL,
    title TEXT NOT NULL,
    website TEXT,
    accent TEXT NOT NULL DEFAULT '#7A8B99',
    added_ms INTEGER NOT NULL,
    UNIQUE(account_id, feed_url)
);
CREATE TABLE feed_groups (
    feed_id INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    UNIQUE(feed_id, group_id)
);
CREATE TABLE articles (
    id TEXT NOT NULL,
    feed_id INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    author TEXT,
    url TEXT,
    published_ms INTEGER NOT NULL,
    sort_ms INTEGER NOT NULL,
    first_seen_ms INTEGER NOT NULL,
    excerpt TEXT NOT NULL DEFAULT '',
    unread INTEGER NOT NULL DEFAULT 1,
    saved INTEGER NOT NULL DEFAULT 0,
    updated_ms INTEGER NOT NULL,
    UNIQUE(feed_id, id)
);
CREATE INDEX articles_sort ON articles(sort_ms DESC, id DESC);
CREATE INDEX articles_unread ON articles(unread) WHERE unread = 1;
CREATE INDEX articles_saved ON articles(saved) WHERE saved = 1;
CREATE TABLE article_contents (
    article_id TEXT NOT NULL,
    feed_id INTEGER NOT NULL,
    html TEXT NOT NULL,
    plain TEXT NOT NULL DEFAULT '',
    hash TEXT,
    fetched_ms INTEGER NOT NULL,
    PRIMARY KEY (feed_id, article_id)
) WITHOUT ROWID;
CREATE TABLE feed_fetch_state (
    feed_id INTEGER PRIMARY KEY REFERENCES feeds(id) ON DELETE CASCADE,
    etag TEXT,
    last_modified TEXT,
    last_fetch_ms INTEGER,
    next_fetch_ms INTEGER,
    error_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE VIRTUAL TABLE article_fts USING fts5(
    title, author, feed_title, body,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#,
    ),
    (
        2,
        r#"
CREATE TABLE article_media (
    feed_id INTEGER NOT NULL,
    article_id TEXT NOT NULL,
    url TEXT NOT NULL,
    PRIMARY KEY (feed_id, article_id, url)
);
CREATE TABLE tombstones (
    feed_id INTEGER NOT NULL,
    article_id TEXT NOT NULL,
    deleted_ms INTEGER NOT NULL,
    PRIMARY KEY (feed_id, article_id)
);
CREATE TABLE read_positions (
    feed_id INTEGER NOT NULL,
    article_id TEXT NOT NULL,
    content_hash TEXT,
    anchor_idx INTEGER NOT NULL DEFAULT 0,
    offset_px INTEGER NOT NULL DEFAULT 0,
    updated_ms INTEGER NOT NULL,
    PRIMARY KEY (feed_id, article_id)
);
"#,
    ),
    (
        3,
        r#"
ALTER TABLE feeds ADD COLUMN remote_id TEXT;
ALTER TABLE groups ADD COLUMN remote_id TEXT;
"#,
    ),
    (
        4,
        r#"
CREATE TABLE outbox (
    id INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    field TEXT NOT NULL,
    desired INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_try_ms INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    created_ms INTEGER NOT NULL
);
CREATE UNIQUE INDEX outbox_unique ON outbox(account_id, entity_id, field);
"#,
    ),
    (
        5,
        r#"
CREATE TABLE prefs (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#,
    ),
    (
        6,
        r#"
ALTER TABLE feeds ADD COLUMN active INTEGER NOT NULL DEFAULT 1;
ALTER TABLE articles ADD COLUMN pruned_ms INTEGER;
CREATE TRIGGER articles_fts_cleanup AFTER DELETE ON articles BEGIN
    DELETE FROM article_fts WHERE rowid=OLD.rowid;
END;
DELETE FROM article_fts WHERE rowid NOT IN (SELECT rowid FROM articles);
CREATE TABLE field_revisions (
    account_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    field TEXT NOT NULL,
    revision INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    PRIMARY KEY (account_id, entity_id, field)
);
INSERT INTO field_revisions(account_id, entity_id, field, revision, updated_ms)
    SELECT account_id, entity_id, field, MAX(revision), MAX(created_ms)
    FROM outbox GROUP BY account_id, entity_id, field;
CREATE INDEX articles_account ON articles(feed_id, id);
"#,
    ),
    (
        7,
        r#"
CREATE TABLE remote_confirmations (
    account_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    field TEXT NOT NULL,
    value INTEGER NOT NULL,
    confirmed_revision INTEGER NOT NULL,
    updated_ms INTEGER NOT NULL,
    PRIMARY KEY (account_id, entity_id, field)
);
INSERT INTO remote_confirmations(account_id, entity_id, field, value, confirmed_revision, updated_ms)
    SELECT account_id, entity_id, field, desired, revision, ?1 FROM outbox;
"#,
    ),
    (
        8,
        r#"
ALTER TABLE articles ADD COLUMN unsynced INTEGER NOT NULL DEFAULT 0;
UPDATE articles SET unsynced=1 WHERE EXISTS (
    SELECT 1 FROM outbox o WHERE o.entity_id=articles.id AND o.status='failed');
"#,
    ),
    (
        9,
        r#"
CREATE TABLE IF NOT EXISTS feed_aliases (
    feed_id INTEGER NOT NULL,
    url TEXT NOT NULL,
    final_url TEXT NOT NULL,
    seen_ms INTEGER NOT NULL,
    PRIMARY KEY (feed_id, url)
);
"#,
    ),
    (
        10,
        r#"
CREATE TABLE account_status (
    account_id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    detail TEXT,
    last_success_ms INTEGER,
    last_error_ms INTEGER,
    pending_changes INTEGER NOT NULL DEFAULT 0
);
"#,
    ),
    (
        11,
        r#"
ALTER TABLE feeds ADD COLUMN user_title TEXT;
"#,
    ),
    (
        12,
        r#"
ALTER TABLE articles ADD COLUMN thumb TEXT;
"#,
    ),
    (
        13,
        r#"
ALTER TABLE field_revisions ADD COLUMN sequence INTEGER NOT NULL DEFAULT 0;
CREATE TABLE account_sequence (
    account_id TEXT PRIMARY KEY,
    counter INTEGER NOT NULL
);
UPDATE field_revisions SET sequence = revision;
INSERT INTO account_sequence(account_id, counter)
    SELECT account_id, MAX(revision) FROM field_revisions GROUP BY account_id;
"#,
    ),
];

/// Höchste von dieser App verstandene Schemastufe.
pub fn max_schema_version() -> i64 {
    MIGRATIONS.iter().map(|(v, _)| *v).max().unwrap_or(0)
}

pub struct Database {
    conn: Connection,
}

const OUTBOX_UPSERT: &str =
    "INSERT INTO outbox(account_id, entity_id, field, desired, revision, created_ms)
     VALUES (?1,?2,?3,?4,?5,?6)
     ON CONFLICT(account_id, entity_id, field) DO UPDATE SET
       desired=excluded.desired,
       revision=excluded.revision,
       attempts=0,
       next_try_ms=0,
       status='pending'";

fn bump_outbox(
    tx: &rusqlite::Transaction,
    account_id: &str,
    entity_id: &str,
    field: &str,
    desired: bool,
) -> rusqlite::Result<i64> {
    let now = now_ms();
    // Konto-globale, monoton steigende Sequenz: nur damit sind Revisionen
    // verschiedener Artikel miteinander vergleichbar.
    tx.execute(
        "INSERT INTO account_sequence(account_id, counter) VALUES (?1, 1)
         ON CONFLICT(account_id) DO UPDATE SET counter=account_sequence.counter+1",
        params![account_id],
    )?;
    let sequence: i64 = tx.query_row(
        "SELECT counter FROM account_sequence WHERE account_id=?1",
        params![account_id],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO field_revisions(account_id, entity_id, field, revision, sequence, updated_ms)
         VALUES (?1,?2,?3,1,?4,?5)
         ON CONFLICT(account_id, entity_id, field) DO UPDATE SET
           revision=field_revisions.revision+1,
           sequence=excluded.sequence,
           updated_ms=?5",
        params![account_id, entity_id, field, sequence, now],
    )?;
    let revision: i64 = tx.query_row(
        "SELECT revision FROM field_revisions WHERE account_id=?1 AND entity_id=?2 AND field=?3",
        params![account_id, entity_id, field],
        |r| r.get(0),
    )?;
    tx.execute(
        OUTBOX_UPSERT,
        params![account_id, entity_id, field, desired as i64, revision, now],
    )?;
    Ok(revision)
}

fn path_key(key: &(Option<i64>, String)) -> String {
    format!(
        "{}::{}",
        key.0.map(|v| v.to_string()).unwrap_or_default(),
        key.1
    )
}

fn accent_for(url: &str) -> String {
    const ACCENTS: [&str; 8] = [
        "#4F8A8B", "#B36A5E", "#6A7BBE", "#8A7A4F", "#7B5EA7", "#5E8A6A", "#A76B8A", "#6B6E7B",
    ];
    let mut hash: u64 = 0;
    for b in url.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(b as u64);
    }
    ACCENTS[(hash as usize) % ACCENTS.len()].to_string()
}

fn clamp_future(ms: i64, now: i64) -> i64 {
    ms.min(now)
}

/// Spielt die Migrationen bis `version` in einer frischen Datenbank nach und liefert
/// den daraus folgenden Tabellensatz. Damit kann die Prüfung nicht veralten.
fn expected_schema(version: i64) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    for (v, sql) in MIGRATIONS {
        if *v <= version {
            conn.execute_batch(sql)?;
            conn.execute(
                "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_ms INTEGER NOT NULL)",
                [],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO schema_version(version, applied_ms) VALUES (?1, ?2)",
                params![v, now_ms()],
            )?;
        }
    }
    actual_schema(&conn)
}

fn actual_schema(conn: &Connection) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let mut tables: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let names: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    for name in names {
        let mut columns = Vec::new();
        let mut info = conn.prepare(&format!("PRAGMA table_info({name})"))?;
        let rows = info.query_map([], |r| r.get::<_, String>(1))?;
        for column in rows {
            columns.push(column?);
        }
        columns.sort();
        tables.insert(name, columns);
    }
    Ok(tables)
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.conn.pragma_update(None, "journal_mode", "WAL")?;
        db.conn.pragma_update(None, "foreign_keys", "ON")?;
        db.conn.pragma_update(None, "busy_timeout", "5000")?;
        db.backup_before_migration(path)?;
        db.migrate()?;
        Ok(db)
    }

    /// Vor jeder Schemaänderung wird eine konsistente Kopie (inklusive noch nicht
    /// eingecheckter WAL-Daten) angelegt. Ohne diese Sicherung wird nicht migriert.
    fn backup_before_migration(&self, path: &Path) -> Result<()> {
        let has_version_table: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if has_version_table == 0 {
            return Ok(());
        }
        let current: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_version",
            [],
            |r| r.get(0),
        )?;
        let max_known = MIGRATIONS.iter().map(|(v, _)| *v).max().unwrap_or(0);
        if current == 0 || current >= max_known {
            return Ok(());
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "library.db".to_string());
        let backup = path.with_file_name(format!("{name}.pre-migrate-{}.db", now_ms()));
        self.backup_to(&backup).map_err(|e| {
            StorageError::Schema(format!(
                "Vor der Schemaaktualisierung (Version {current} → {max_known}) konnte keine \
                 Sicherung unter {} angelegt werden: {e}",
                backup.display()
            ))
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.conn.pragma_update(None, "foreign_keys", "ON")?;
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY, applied_ms INTEGER NOT NULL);",
        )?;
        let current: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_version",
            [],
            |r| r.get(0),
        )?;
        let max_known = MIGRATIONS.iter().map(|(v, _)| *v).max().unwrap_or(0);
        if current > max_known {
            return Err(StorageError::Schema(format!(
                "Datenbank hat neuere Schema-Version {current} als diese App ({max_known}) — nur lesen, nicht schreiben"
            )));
        }
        for (version, sql) in MIGRATIONS {
            if *version > current {
                let tx = self.conn.unchecked_transaction()?;
                tx.execute_batch(sql)?;
                tx.execute(
                    "INSERT INTO schema_version(version, applied_ms) VALUES (?1, ?2)",
                    params![version, now_ms()],
                )?;
                tx.commit()?;
            }
        }
        Ok(())
    }

    pub fn upsert_account(&self, id: &str, kind: &str, name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO accounts(id, kind, name, created_ms) VALUES (?1,?2,?3,?4)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name",
            params![id, kind, name, now_ms()],
        )?;
        Ok(())
    }

    pub fn list_accounts(&self) -> Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, name FROM accounts ORDER BY kind, name")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn upsert_feed_remote(
        &self,
        account_id: &str,
        remote_id: &str,
        feed_url: &str,
        title: &str,
        website: Option<&str>,
    ) -> Result<i64> {
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM feeds WHERE account_id=?1 AND remote_id=?2",
                params![account_id, remote_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            self.conn.execute(
                "UPDATE feeds SET title=?2, website=COALESCE(?3, website), feed_url=?4 WHERE id=?1",
                params![id, title, website, feed_url],
            )?;
            return Ok(id);
        }
        let existing_url: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM feeds WHERE account_id=?1 AND feed_url=?2",
                params![account_id, feed_url],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing_url {
            self.conn.execute(
                "UPDATE feeds SET remote_id=?2, title=?3 WHERE id=?1",
                params![id, remote_id, title],
            )?;
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO feeds(account_id, remote_id, feed_url, title, website, added_ms)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![account_id, remote_id, feed_url, title, website, now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn upsert_group_remote(
        &self,
        account_id: &str,
        remote_id: &str,
        name: &str,
    ) -> Result<i64> {
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM groups WHERE account_id=?1 AND remote_id=?2",
                params![account_id, remote_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            self.conn
                .execute("UPDATE groups SET name=?2 WHERE id=?1", params![id, name])?;
            return Ok(id);
        }
        self.conn.execute(
            "INSERT INTO groups(account_id, remote_id, name) VALUES (?1,?2,?3)",
            params![account_id, remote_id, name],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn feed_id_by_remote(&self, account_id: &str, remote_id: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM feeds WHERE account_id=?1 AND remote_id=?2",
                params![account_id, remote_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn set_read_by_article_id(&self, article_id: &str, read: bool) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE articles SET unread=?2 WHERE id=?1",
            params![article_id, if read { 0 } else { 1 }],
        )?)
    }

    pub fn set_saved_by_article_id(&self, article_id: &str, saved: bool) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE articles SET saved=?2 WHERE id=?1",
            params![article_id, if saved { 1 } else { 0 }],
        )?)
    }

    /// IDs der lokal als ungelesen markierten Artikel eines Kontos.
    pub fn unread_ids_for_account(&self, account_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id FROM articles a JOIN feeds f ON f.id=a.feed_id
             WHERE f.account_id=?1 AND a.unread=1",
        )?;
        let rows = stmt
            .query_map(params![account_id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// IDs gespeicherter oder ungelesener Artikel des Kontos, deren Inhalt fehlt.
    /// Sie werden per `.mget` nachgeladen, statt still zu fehlen.
    pub fn articles_needing_content(&self, account_id: &str, limit: u32) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id FROM articles a JOIN feeds f ON f.id=a.feed_id
             WHERE f.account_id=?1 AND (a.saved=1 OR a.unread=1)
               AND NOT EXISTS (SELECT 1 FROM article_contents c
                               WHERE c.feed_id=a.feed_id AND c.article_id=a.id)
             ORDER BY a.sort_ms DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![account_id, limit], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn saved_ids_for_account(&self, account_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id FROM articles a JOIN feeds f ON f.id=a.feed_id
             WHERE f.account_id=?1 AND a.saved=1",
        )?;
        let rows = stmt
            .query_map(params![account_id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn ensure_local_account(&self) -> Result<String> {
        let id = "local".to_string();
        self.conn.execute(
            "INSERT INTO accounts(id, kind, name, created_ms) VALUES (?1,'local','Lokale Bibliothek',?2)
             ON CONFLICT(id) DO NOTHING",
            params![id, now_ms()],
        )?;
        Ok(id)
    }

    pub fn add_feed(
        &self,
        account_id: &str,
        feed_url: &str,
        title: &str,
        website: Option<&str>,
        accent: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO feeds(account_id, feed_url, title, website, accent, added_ms)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![account_id, feed_url, title, website, accent, now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Vom Nutzer gesetzter Titel bleibt erhalten, wenn der Feed sie später
    /// erneut liefert (Publisher-Titel und Nutzertitel sind getrennt).
    pub fn set_user_title(&self, feed_id: i64, title: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET user_title=?2 WHERE id=?1",
            params![feed_id, title],
        )?;
        Ok(())
    }

    pub fn display_title(&self, feed_id: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT COALESCE(user_title, title) FROM feeds WHERE id=?1",
                params![feed_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Abbestellen: Feed stilllegen, Inhalte und gemerkte Artikel behalten.
    pub fn deactivate_feed(&self, feed_id: i64) -> Result<usize> {
        self.conn
            .execute("UPDATE feeds SET active=0 WHERE id=?1", params![feed_id])?;
        let saved: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM articles WHERE feed_id=?1 AND saved=1",
            params![feed_id],
            |r| r.get(0),
        )?;
        Ok(saved as usize)
    }

    pub fn group_by_name(&self, account_id: &str, name: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM groups WHERE account_id=?1 AND name=?2",
                params![account_id, name],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn mark_feed_read(&self, feed_id: i64) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE articles SET unread=0 WHERE feed_id=?1",
            params![feed_id],
        )?)
    }

    pub fn set_feed_active(&self, feed_id: i64, active: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET active=?2 WHERE id=?1",
            params![feed_id, if active { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub fn feed_is_active(&self, feed_id: i64) -> Result<bool> {
        let v: i64 = self.conn.query_row(
            "SELECT COALESCE(active,1) FROM feeds WHERE id=?1",
            params![feed_id],
            |r| r.get(0),
        )?;
        Ok(v != 0)
    }

    pub fn update_feed_title(
        &self,
        feed_id: i64,
        title: &str,
        website: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET title=?2, website=COALESCE(?3, website) WHERE id=?1",
            params![feed_id, title, website],
        )?;
        Ok(())
    }

    pub fn list_feeds(&self) -> Result<Vec<FeedRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, remote_id, feed_url, COALESCE(user_title, title), website, accent
             FROM feeds WHERE COALESCE(active,1)=1 ORDER BY lower(COALESCE(user_title, title))",
        )?;
        let mut feeds: Vec<FeedRow> = stmt
            .query_map([], |r| {
                Ok(FeedRow {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    remote_id: r.get(2)?,
                    feed_url: r.get(3)?,
                    title: r.get(4)?,
                    website: r.get(5)?,
                    accent: r.get(6)?,
                    groups: Vec::new(),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        let mut gs = self
            .conn
            .prepare("SELECT feed_id, group_id FROM feed_groups")?;
        let pairs: Vec<(i64, i64)> = gs
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for f in feeds.iter_mut() {
            f.groups = pairs
                .iter()
                .filter(|(fid, _)| *fid == f.id)
                .map(|(_, g)| *g)
                .collect();
        }
        Ok(feeds)
    }

    /// Importiert ein OPML-Draft transaktional in das Zielkonto.
    /// Bestehende Feeds werden nie ersetzt, nur Gruppenzuordnungen ergänzt.
    pub fn import_opml_entries(
        &self,
        account_id: &str,
        entries: &[(String, String, Option<String>, Vec<String>)],
    ) -> Result<(usize, usize)> {
        let tx = self.conn.unchecked_transaction()?;
        let mut group_ids: std::collections::HashMap<(Option<i64>, String), i64> =
            std::collections::HashMap::new();
        let mut group_path: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        let mut new_feeds = 0usize;
        let mut merged = 0usize;
        for (title, url, website, groups) in entries {
            let mut gids: Vec<i64> = Vec::new();
            let mut parent: Option<i64> = None;
            for name in groups {
                let key = (parent, name.clone());
                let gid = match group_path.get(&path_key(&key)) {
                    Some(g) => *g,
                    None => {
                        let existing: Option<i64> = tx
                            .query_row(
                                "SELECT id FROM groups WHERE account_id=?1 AND name=?2
                                 AND COALESCE(parent_id,-1)=COALESCE(?3,-1)",
                                params![account_id, name, parent],
                                |r| r.get(0),
                            )
                            .optional()?;
                        let gid = match existing {
                            Some(g) => g,
                            None => {
                                tx.execute(
                                    "INSERT INTO groups(account_id, name, parent_id) VALUES (?1,?2,?3)",
                                    params![account_id, name, parent],
                                )?;
                                tx.last_insert_rowid()
                            }
                        };
                        group_path.insert(path_key(&key), gid);
                        group_ids.insert(key, gid);
                        gid
                    }
                };
                gids.push(gid);
                parent = Some(gid);
            }
            let existing_feed: Option<i64> = tx
                .query_row(
                    "SELECT id FROM feeds WHERE account_id=?1 AND feed_url=?2",
                    params![account_id, url],
                    |r| r.get(0),
                )
                .optional()?;
            match existing_feed {
                Some(fid) => {
                    let mut all: Vec<i64> = {
                        let mut stmt =
                            tx.prepare("SELECT group_id FROM feed_groups WHERE feed_id=?1")?;
                        let rows = stmt
                            .query_map(params![fid], |r| r.get(0))?
                            .collect::<std::result::Result<Vec<i64>, _>>()?;
                        rows
                    };
                    for g in gids {
                        if !all.contains(&g) {
                            tx.execute(
                                "INSERT OR IGNORE INTO feed_groups(feed_id, group_id) VALUES (?1,?2)",
                                params![fid, g],
                            )?;
                            all.push(g);
                        }
                    }
                    merged += 1;
                }
                None => {
                    tx.execute(
                        "INSERT INTO feeds(account_id, feed_url, title, website, accent, added_ms)
                         VALUES (?1,?2,?3,?4,?5,?6)",
                        params![account_id, url, title, website, &accent_for(url), now_ms()],
                    )?;
                    let fid = tx.last_insert_rowid();
                    for g in gids {
                        tx.execute(
                            "INSERT OR IGNORE INTO feed_groups(feed_id, group_id) VALUES (?1,?2)",
                            params![fid, g],
                        )?;
                    }
                    new_feeds += 1;
                }
            }
        }
        tx.commit()?;
        Ok((new_feeds, merged))
    }

    pub fn add_group(&self, account_id: &str, name: &str, parent_id: Option<i64>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO groups(account_id, name, parent_id) VALUES (?1,?2,?3)",
            params![account_id, name, parent_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_groups(&self) -> Result<Vec<GroupRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, parent_id, remote_id, account_id FROM groups ORDER BY name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(GroupRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    parent_id: r.get(2)?,
                    remote_id: r.get(3)?,
                    account_id: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn set_feed_groups(&self, feed_id: i64, group_ids: &[i64]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM feed_groups WHERE feed_id=?1", params![feed_id])?;
        for g in group_ids {
            tx.execute(
                "INSERT OR IGNORE INTO feed_groups(feed_id, group_id) VALUES (?1,?2)",
                params![feed_id, g],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn feed_id_by_url(&self, url: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM feeds WHERE feed_url=?1",
                params![url],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_article(
        &self,
        feed_id: i64,
        id: &str,
        title: &str,
        author: Option<&str>,
        url: Option<&str>,
        published_ms: i64,
        excerpt: &str,
        html: Option<&str>,
        now: i64,
    ) -> Result<bool> {
        if self.is_tombstoned(feed_id, id)? {
            return Ok(false);
        }
        let exists: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM articles WHERE feed_id=?1 AND id=?2",
            params![feed_id, id],
            |r| r.get::<_, i64>(0),
        )? > 0;
        let sort = clamp_future(published_ms, now);
        if exists {
            self.conn.execute(
                "UPDATE articles SET title=?3, author=?4, url=?5, published_ms=?6, sort_ms=?7,
                 excerpt=?8, updated_ms=?9 WHERE feed_id=?1 AND id=?2",
                params![
                    feed_id,
                    id,
                    title,
                    author,
                    url,
                    published_ms,
                    sort,
                    excerpt,
                    now
                ],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO articles(feed_id, id, title, author, url, published_ms, sort_ms, first_seen_ms, excerpt, unread, saved, updated_ms)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,0,?10)",
                params![feed_id, id, title, author, url, published_ms, sort, now, excerpt, now],
            )?;
        }
        if let Some(html) = html {
            let plain = strip_html(html);
            self.conn.execute(
                "INSERT INTO article_contents(feed_id, article_id, html, plain, fetched_ms)
                 VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(feed_id, article_id) DO UPDATE SET html=excluded.html, plain=excluded.plain, fetched_ms=excluded.fetched_ms",
                params![feed_id, id, html, plain, now],
            )?;
            self.conn.execute(
                "UPDATE articles SET pruned_ms=NULL WHERE feed_id=?1 AND id=?2",
                params![feed_id, id],
            )?;
        }
        let feed_title: String = self.conn.query_row(
            "SELECT title FROM feeds WHERE id=?1",
            params![feed_id],
            |r| r.get(0),
        )?;
        let rowid: i64 = self.conn.query_row(
            "SELECT rowid FROM articles WHERE feed_id=?1 AND id=?2",
            params![feed_id, id],
            |r| r.get(0),
        )?;
        let body: String = self
            .conn
            .query_row(
                "SELECT plain FROM article_contents WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default();
        self.conn
            .execute("DELETE FROM article_fts WHERE rowid=?1", params![rowid])?;
        self.conn.execute(
            "INSERT INTO article_fts(rowid, title, author, feed_title, body) VALUES (?1,?2,?3,?4,?5)",
            params![rowid, title, author.unwrap_or(""), feed_title, body],
        )?;
        Ok(!exists)
    }

    pub fn upsert_articles(
        &self,
        feed_id: i64,
        items: &[NewArticle],
        now: i64,
    ) -> Result<(usize, usize)> {
        let tx = self.conn.unchecked_transaction()?;
        let (mut added, mut updated) = (0usize, 0usize);
        let feed_title: String = tx.query_row(
            "SELECT title FROM feeds WHERE id=?1",
            params![feed_id],
            |r| r.get(0),
        )?;
        for item in items {
            let tomb: bool = tx.query_row(
                "SELECT COUNT(*) FROM tombstones WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, item.id],
                |r| r.get::<_, i64>(0),
            )? > 0;
            if tomb {
                continue;
            }
            let exists: bool = tx.query_row(
                "SELECT COUNT(*) FROM articles WHERE feed_id=?1 AND id=?2",
                params![feed_id, item.id],
                |r| r.get::<_, i64>(0),
            )? > 0;
            let sort = clamp_future(item.published_ms, now);
            if exists {
                tx.execute(
                    "UPDATE articles SET title=?3, author=?4, url=?5, published_ms=?6, sort_ms=?7, excerpt=?8, updated_ms=?9
                     WHERE feed_id=?1 AND id=?2",
                    params![feed_id, item.id, item.title, item.author, item.url, item.published_ms, sort, item.excerpt, now],
                )?;
                updated += 1;
            } else {
                tx.execute(
                    "INSERT INTO articles(feed_id, id, title, author, url, published_ms, sort_ms, first_seen_ms, excerpt, unread, saved, updated_ms)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,1,0,?10)",
                    params![feed_id, item.id, item.title, item.author, item.url, item.published_ms, sort, now, item.excerpt, now],
                )?;
                added += 1;
            }
            let plain = item.html.as_deref().map(strip_html);
            if let Some(html) = &item.html {
                tx.execute(
                    "INSERT INTO article_contents(feed_id, article_id, html, plain, hash, fetched_ms)
                     VALUES (?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(feed_id, article_id) DO UPDATE SET html=excluded.html, plain=excluded.plain, hash=excluded.hash, fetched_ms=excluded.fetched_ms",
                    params![feed_id, item.id, html, plain.clone().unwrap_or_default(), item.content_hash, now],
                )?;
                tx.execute(
                    "UPDATE articles SET pruned_ms=NULL WHERE feed_id=?1 AND id=?2",
                    params![feed_id, item.id],
                )?;
            }
            let rowid: i64 = tx.query_row(
                "SELECT rowid FROM articles WHERE feed_id=?1 AND id=?2",
                params![feed_id, item.id],
                |r| r.get(0),
            )?;
            let existing_body: String = tx
                .query_row(
                    "SELECT plain FROM article_contents WHERE feed_id=?1 AND article_id=?2",
                    params![feed_id, item.id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or_default();
            let body = plain.clone().unwrap_or(existing_body);
            tx.execute("DELETE FROM article_fts WHERE rowid=?1", params![rowid])?;
            tx.execute(
                "INSERT INTO article_fts(rowid, title, author, feed_title, body) VALUES (?1,?2,?3,?4,?5)",
                params![rowid, item.title, item.author.clone().unwrap_or_default(), feed_title, body],
            )?;
        }
        tx.commit()?;
        Ok((added, updated))
    }

    pub fn set_status(
        &self,
        feed_id: i64,
        id: &str,
        read: Option<bool>,
        saved: Option<bool>,
    ) -> Result<()> {
        let mut sets = Vec::new();
        let mut vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        if let Some(r) = read {
            sets.push("unread=?");
            vals.push(Box::new(!r as i64));
        }
        if let Some(s) = saved {
            sets.push("saved=?");
            vals.push(Box::new(s as i64));
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "UPDATE articles SET {} WHERE feed_id=? AND id=?",
            sets.join(",")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut all: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v.as_ref()).collect();
        all.push(&feed_id);
        all.push(&id);
        stmt.execute(rusqlite::params_from_iter(all))?;
        Ok(())
    }

    pub fn article_status(&self, feed_id: i64, id: &str) -> Result<Option<(bool, bool)>> {
        self.conn
            .query_row(
                "SELECT unread, saved FROM articles WHERE feed_id=?1 AND id=?2",
                params![feed_id, id],
                |r| Ok((r.get::<_, i64>(0)? == 1, r.get::<_, i64>(1)? == 1)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn content_html(&self, feed_id: i64, id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT html FROM article_contents WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn query_articles(
        &self,
        scope: &Scope,
        filter: Filter,
        before: Option<(i64, i64, &str)>,
        limit: u32,
    ) -> Result<Vec<ArticleRow>> {
        self.query_articles_ordered(scope, filter, before, limit, true)
    }

    /// `ascending=false` liefert älteste zuerst; der Cursor bleibt eindeutig,
    /// weil (sort_ms, feed_id, id) in beiden Richtungen total geordnet ist.
    pub fn query_articles_ordered(
        &self,
        scope: &Scope,
        filter: Filter,
        before: Option<(i64, i64, &str)>,
        limit: u32,
        newest_first: bool,
    ) -> Result<Vec<ArticleRow>> {
        let mut sql = String::from(
            "SELECT a.id, a.feed_id, f.title, f.accent, a.title, a.author, a.url, a.published_ms,
                    a.excerpt, a.unread, a.saved,
                    EXISTS(SELECT 1 FROM article_contents c WHERE c.feed_id=a.feed_id AND c.article_id=a.id),
                    a.sort_ms, a.thumb
             FROM articles a JOIN feeds f ON f.id=a.feed_id
             JOIN feed_groups fg ON fg.feed_id=a.feed_id",
        );
        let mut where_clauses: Vec<String> = Vec::new();
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        match filter {
            Filter::Unread => where_clauses.push("a.unread=1".into()),
            Filter::Saved => where_clauses.push("a.saved=1".into()),
            Filter::All => {}
        }
        match scope {
            Scope::Global => {}
            Scope::Feed(id) => {
                where_clauses.push("a.feed_id=?".into());
                args.push(Box::new(*id));
            }
            Scope::Account(acc) => {
                where_clauses.push("f.account_id=?".into());
                args.push(Box::new(acc.clone()));
            }
            Scope::Group(_) => {}
        }
        let join_groups = matches!(scope, Scope::Group(_));
        if !join_groups {
            sql = sql.replace(" JOIN feed_groups fg ON fg.feed_id=a.feed_id", "");
        } else if let Scope::Group(g) = scope {
            where_clauses.push("fg.group_id=?".into());
            args.push(Box::new(*g));
        }
        if let Some((sort_ms, feed_id, id)) = before {
            // (sort_ms, feed_id, id) ist total eindeutig: gleiche Zeit und gleiche
            // GUID in zwei Feeds erzeugen keine Lücke und keinen Wiederholer.
            let order = if newest_first { "<" } else { ">" };
            where_clauses.push(format!(
                "(a.sort_ms {order} ? OR (a.sort_ms = ? AND a.feed_id {order} ?) \
                 OR (a.sort_ms = ? AND a.feed_id = ? AND a.id {order} ?))"
            ));
            args.push(Box::new(sort_ms));
            args.push(Box::new(sort_ms));
            args.push(Box::new(feed_id));
            args.push(Box::new(sort_ms));
            args.push(Box::new(feed_id));
            args.push(Box::new(id.to_string()));
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        if join_groups {
            sql.push_str(" GROUP BY a.id, a.feed_id");
        }
        if newest_first {
            sql.push_str(" ORDER BY a.sort_ms DESC, a.feed_id DESC, a.id DESC LIMIT ?");
        } else {
            sql.push_str(" ORDER BY a.sort_ms ASC, a.feed_id ASC, a.id ASC LIMIT ?");
        }
        args.push(Box::new(limit as i64));
        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|v| v.as_ref()).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(refs), |r| {
                Ok(ArticleRow {
                    id: r.get(0)?,
                    feed_id: r.get(1)?,
                    feed_title: r.get(2)?,
                    accent: r.get(3)?,
                    title: r.get(4)?,
                    author: r.get(5)?,
                    url: r.get(6)?,
                    published_ms: r.get(7)?,
                    excerpt: r.get(8)?,
                    unread: r.get::<_, i64>(9)? == 1,
                    saved: r.get::<_, i64>(10)? == 1,
                    has_content: r.get::<_, i64>(11)? == 1,
                    sort_ms: r.get(12)?,
                    thumb: r.get(13)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn search(
        &self,
        query: &str,
        scope: &Scope,
        filter: Filter,
        before: Option<(i64, i64, &str)>,
        limit: u32,
    ) -> Result<Vec<ArticleRow>> {
        self.search_ordered(query, scope, filter, before, limit, true)
    }

    pub fn search_ordered(
        &self,
        query: &str,
        scope: &Scope,
        filter: Filter,
        before: Option<(i64, i64, &str)>,
        limit: u32,
        newest_first: bool,
    ) -> Result<Vec<ArticleRow>> {
        let fts = build_fts_query(query);
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let mut where_clauses = vec!["article_fts MATCH ?1".to_string()];
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(fts)];
        match filter {
            Filter::Unread => where_clauses.push("a.unread=1".into()),
            Filter::Saved => where_clauses.push("a.saved=1".into()),
            Filter::All => {}
        }
        match scope {
            Scope::Global => {}
            Scope::Feed(id) => {
                where_clauses.push("a.feed_id=?".into());
                args.push(Box::new(*id));
            }
            Scope::Account(acc) => {
                where_clauses.push("f.account_id=?".into());
                args.push(Box::new(acc.clone()));
            }
            Scope::Group(g) => {
                where_clauses
                    .push("a.feed_id IN (SELECT feed_id FROM feed_groups WHERE group_id=?)".into());
                args.push(Box::new(*g));
            }
        }
        if let Some((sort_ms, feed_id, id)) = before {
            let cmp = if newest_first { "<" } else { ">" };
            where_clauses.push(format!(
                "(a.sort_ms {cmp} ? OR (a.sort_ms = ? AND a.feed_id {cmp} ?) \
                 OR (a.sort_ms = ? AND a.feed_id = ? AND a.id {cmp} ?))"
            ));
            args.push(Box::new(sort_ms));
            args.push(Box::new(sort_ms));
            args.push(Box::new(feed_id));
            args.push(Box::new(sort_ms));
            args.push(Box::new(feed_id));
            args.push(Box::new(id.to_string()));
        }
        let order = if newest_first {
            "a.sort_ms DESC, a.feed_id DESC, a.id DESC"
        } else {
            "a.sort_ms ASC, a.feed_id ASC, a.id ASC"
        };
        let sql = format!(
            "SELECT a.id, a.feed_id, f.title, f.accent, a.title, a.author, a.url, a.published_ms,
                          a.excerpt, a.unread, a.saved, (c.article_id IS NOT NULL), a.sort_ms, a.thumb
                   FROM article_fts fts
                   JOIN articles a ON a.rowid = fts.rowid
                   JOIN feeds f ON f.id=a.feed_id
                   LEFT JOIN article_contents c ON c.feed_id=a.feed_id AND c.article_id=a.id
                   WHERE {}
                   ORDER BY {order}
                   LIMIT {}",
            where_clauses.join(" AND "),
            (limit as i64) + 1
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|v| v.as_ref()).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(refs), |r| {
                Ok(ArticleRow {
                    id: r.get(0)?,
                    feed_id: r.get(1)?,
                    feed_title: r.get(2)?,
                    accent: r.get(3)?,
                    title: r.get(4)?,
                    author: r.get(5)?,
                    url: r.get(6)?,
                    published_ms: r.get(7)?,
                    excerpt: r.get(8)?,
                    unread: r.get::<_, i64>(9)? == 1,
                    saved: r.get::<_, i64>(10)? == 1,
                    has_content: r.get::<_, i64>(11)? == 1,
                    sort_ms: r.get(12)?,
                    thumb: r.get(13)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn counts(&self) -> Result<Counts> {
        let mut c = Counts::default();
        c.unread = self.distinct_count("WHERE a.unread=1")?;
        c.saved = self.distinct_count("WHERE a.saved=1")?;
        c.total = self.distinct_count("")?;
        let mut stmt = self.conn.prepare(
            "SELECT feed_id, COUNT(DISTINCT id) FROM articles WHERE unread=1 GROUP BY feed_id",
        )?;
        c.per_feed = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT group_id, COUNT(*) FROM (
                 SELECT DISTINCT fg.group_id, a.feed_id, a.id FROM articles a
                 JOIN feed_groups fg ON fg.feed_id=a.feed_id WHERE a.unread=1
             ) GROUP BY group_id",
        )?;
        c.per_group = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT f.account_id, COUNT(DISTINCT a.id) FROM articles a JOIN feeds f ON f.id=a.feed_id
             WHERE a.unread=1 AND f.account_id != 'local' GROUP BY f.account_id",
        )?;
        c.per_account = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(c)
    }

    fn distinct_count(&self, clause: &str) -> Result<i64> {
        let sql = format!(
            "SELECT COUNT(*) FROM (
               SELECT DISTINCT
                 CASE WHEN f.account_id='local' THEN 'feed:' || a.feed_id ELSE 'account:' || f.account_id END AS scope_key,
                 a.id
               FROM articles a JOIN feeds f ON f.id=a.feed_id {clause})"
        );
        Ok(self.conn.query_row(&sql, [], |r| r.get(0))?)
    }

    pub fn mark_source_read(&self, ids: &[(i64, String)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (feed_id, id) in ids {
            tx.execute(
                "UPDATE articles SET unread=0 WHERE feed_id=?1 AND id=?2",
                params![feed_id, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn fetch_state(&self, feed_id: i64) -> Result<FetchState> {
        let row = self
            .conn
            .query_row(
                "SELECT etag, last_modified, last_fetch_ms, next_fetch_ms, error_count, last_error
                 FROM feed_fetch_state WHERE feed_id=?1",
                params![feed_id],
                |r| {
                    Ok(FetchState {
                        etag: r.get(0)?,
                        last_modified: r.get(1)?,
                        last_fetch_ms: r.get(2)?,
                        next_fetch_ms: r.get(3)?,
                        error_count: r.get(4)?,
                        last_error: r.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row.unwrap_or_default())
    }

    pub fn record_feed_alias(&self, feed_id: i64, url: &str, final_url: &str) -> Result<()> {
        if url == final_url {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO feed_aliases(feed_id, url, final_url, seen_ms) VALUES (?1,?2,?3,?4)
             ON CONFLICT(feed_id, url) DO UPDATE SET final_url=excluded.final_url, seen_ms=excluded.seen_ms",
            params![feed_id, url, final_url, now_ms()],
        )?;
        Ok(())
    }

    pub fn feed_alias(&self, feed_id: i64, url: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT final_url FROM feed_aliases WHERE feed_id=?1 AND url=?2",
                params![feed_id, url],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn set_fetch_state(&self, feed_id: i64, st: &FetchState) -> Result<()> {
        self.conn.execute(
            "INSERT INTO feed_fetch_state(feed_id, etag, last_modified, last_fetch_ms, next_fetch_ms, error_count, last_error)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(feed_id) DO UPDATE SET etag=excluded.etag, last_modified=excluded.last_modified,
               last_fetch_ms=excluded.last_fetch_ms, next_fetch_ms=excluded.next_fetch_ms,
               error_count=excluded.error_count, last_error=excluded.last_error",
            params![
                feed_id,
                st.etag,
                st.last_modified,
                st.last_fetch_ms,
                st.next_fetch_ms,
                st.error_count,
                st.last_error
            ],
        )?;
        Ok(())
    }

    pub fn due_feeds(&self, now: i64) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.feed_url FROM feeds f
             LEFT JOIN feed_fetch_state s ON s.feed_id=f.id
             WHERE f.account_id='local' AND COALESCE(f.active,1)=1
               AND (s.next_fetch_ms IS NULL OR s.next_fetch_ms <= ?1)
             ORDER BY COALESCE(s.next_fetch_ms, 0)",
        )?;
        let rows = stmt
            .query_map(params![now], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Kleine Vorschau als Daten-URI (PNG, max. 128 px); `None` entfernt sie.
    pub fn set_article_thumb(
        &self,
        feed_id: i64,
        article_id: &str,
        thumb: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE articles SET thumb=?3 WHERE feed_id=?1 AND id=?2",
            params![feed_id, article_id, thumb],
        )?;
        Ok(())
    }

    pub fn article_media_urls(&self, feed_id: i64, article_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT url FROM article_media WHERE feed_id=?1 AND article_id=?2")?;
        let rows = stmt
            .query_map(params![feed_id, article_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    pub fn set_article_media(&self, feed_id: i64, article_id: &str, urls: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM article_media WHERE feed_id=?1 AND article_id=?2",
            params![feed_id, article_id],
        )?;
        for u in urls {
            tx.execute(
                "INSERT OR IGNORE INTO article_media(feed_id, article_id, url) VALUES (?1,?2,?3)",
                params![feed_id, article_id, u],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn pinned_media_urls(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT m.url FROM article_media m JOIN articles a
             ON a.feed_id=m.feed_id AND a.id=m.article_id WHERE a.saved=1",
        )?;
        let rows = stmt
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn is_tombstoned(&self, feed_id: i64, article_id: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM tombstones WHERE feed_id=?1 AND article_id=?2",
            params![feed_id, article_id],
            |r| r.get::<_, i64>(0),
        )? > 0)
    }

    pub fn prune_old_read(&self, now_ms: i64, days: i64) -> Result<usize> {
        let cutoff = now_ms - days * 86_400_000;
        let tx = self.conn.unchecked_transaction()?;
        let ids: Vec<(i64, String)> = tx
            .prepare(
                "SELECT a.feed_id, a.id FROM articles a JOIN feeds f ON f.id=a.feed_id
                 WHERE a.unread=0 AND a.saved=0 AND a.first_seen_ms < ?1
                   AND NOT EXISTS (
                     SELECT 1 FROM outbox o
                     WHERE o.entity_id=a.id AND o.status IN ('pending','inflight')
                       AND o.account_id=f.account_id)
                 ORDER BY a.feed_id, a.id",
            )?
            .query_map(params![cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for (feed_id, id) in &ids {
            tx.execute(
                "DELETE FROM article_contents WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, id],
            )?;
            tx.execute(
                "DELETE FROM article_media WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, id],
            )?;
            tx.execute(
                "DELETE FROM read_positions WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, id],
            )?;
            tx.execute(
                "UPDATE articles SET pruned_ms=?3 WHERE feed_id=?1 AND id=?2",
                params![feed_id, id, now_ms],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO tombstones(feed_id, article_id, deleted_ms) VALUES (?1,?2,?3)",
                params![feed_id, id, now_ms],
            )?;
        }
        tx.commit()?;
        Ok(ids.len())
    }

    pub fn save_read_position(
        &self,
        feed_id: i64,
        article_id: &str,
        content_hash: Option<&str>,
        anchor_idx: i64,
        offset_px: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO read_positions(feed_id, article_id, content_hash, anchor_idx, offset_px, updated_ms)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(feed_id, article_id) DO UPDATE SET
               content_hash=excluded.content_hash, anchor_idx=excluded.anchor_idx,
               offset_px=excluded.offset_px, updated_ms=excluded.updated_ms",
            params![feed_id, article_id, content_hash, anchor_idx, offset_px, now_ms()],
        )?;
        Ok(())
    }

    pub fn read_position(
        &self,
        feed_id: i64,
        article_id: &str,
    ) -> Result<Option<(Option<String>, i64, i64)>> {
        self.conn
            .query_row(
                "SELECT content_hash, anchor_idx, offset_px FROM read_positions WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, article_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn content_hash(&self, feed_id: i64, article_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT hash FROM article_contents WHERE feed_id=?1 AND article_id=?2",
                params![feed_id, article_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Vollständige strukturelle Prüfung eines Restore-Kandidaten: SQLite-Integrität,
    /// versionstreue Tabellensätze samt Spalten, Fremdschlüssel und Schemastufe.
    /// Formale Gültigkeit (vier Tabellennamen) genügt ausdrücklich nicht.
    pub fn validate_candidate(path: &std::path::Path) -> Result<i64> {
        let file = std::fs::File::open(path)?;
        let meta = file.metadata()?;
        if meta.len() < 512 {
            return Err(StorageError::Schema(
                "Datei ist keine SQLite-Datenbank".into(),
            ));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| StorageError::Schema(format!("Keine SQLite-Datenbank: {e}")))?;
        let integrity: String = conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|e| StorageError::Schema(format!("Integritätsprüfung fehlgeschlagen: {e}")))?;
        if integrity != "ok" {
            return Err(StorageError::Schema(format!(
                "Integritätsprüfung: {integrity}"
            )));
        }
        for table in ["accounts", "feeds", "articles", "schema_version"] {
            let found: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![table],
                    |r| r.get(0),
                )
                .map_err(|e| StorageError::Schema(e.to_string()))?;
            if found == 0 {
                return Err(StorageError::Schema(format!(
                    "Erwartete Tabelle {table} fehlt — keine Lesefluss-Bibliothek"
                )));
            }
        }
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_version",
                [],
                |r| r.get(0),
            )
            .map_err(|e| StorageError::Schema(e.to_string()))?;
        let max_known = MIGRATIONS.iter().map(|(v, _)| *v).max().unwrap_or(0);
        if version > max_known {
            return Err(StorageError::Schema(format!(
                "Schema-Version {version} ist neuer als diese App ({max_known})"
            )));
        }
        if version < 1 {
            return Err(StorageError::Schema(
                "Die Datei enthält keine Lesefluss-Migration — keine Lesefluss-Bibliothek".into(),
            ));
        }
        let expected = expected_schema(version)?;
        let actual = actual_schema(&conn)?;
        for (table, columns) in &expected {
            match actual.get(table) {
                None => {
                    return Err(StorageError::Schema(format!(
                        "Tabelle {table} fehlt — die Datei ist keine Lesefluss-Bibliothek"
                    )))
                }
                Some(found) => {
                    for column in columns {
                        if !found.contains(column) {
                            return Err(StorageError::Schema(format!(
                                "Spalte {table}.{column} fehlt — unvollständiges Schema"
                            )));
                        }
                    }
                }
            }
        }
        let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
        let violations: Vec<String> = stmt
            .query_map([], |r| {
                Ok(format!(
                    "{}#{}",
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?.unwrap_or_default()
                ))
            })?
            .filter_map(|row| row.ok())
            .collect();
        if !violations.is_empty() {
            return Err(StorageError::Schema(format!(
                "Fremdschlüssel verletzt: {}",
                violations.join(", ")
            )));
        }
        Ok(version)
    }

    /// Prüft einen Kandidaten vollständig, migriert ihn auf den aktuellen Stand und
    /// hinterlässt eine einzelne, checkpointete Datei ohne WAL-Reste. Erst danach darf
    /// der App-Code die Datei aktivieren.
    pub fn prepare_restore_candidate(path: &std::path::Path) -> Result<i64> {
        let version = Self::validate_candidate(path)?;
        let max_known = max_schema_version();
        let db = Database::open(path)?;
        let migrated: i64 = db.conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_version",
            [],
            |r| r.get(0),
        )?;
        if version < max_known && migrated <= version {
            return Err(StorageError::Schema(format!(
                "Migration blieb bei Version {migrated} statt {version} zu erhöhen"
            )));
        }
        let expected = expected_schema(migrated)?;
        let actual = actual_schema(&db.conn)?;
        for (table, columns) in &expected {
            match actual.get(table) {
                Some(found) if columns.iter().all(|c| found.contains(c)) => {}
                Some(_) => {
                    return Err(StorageError::Schema(format!(
                        "Migration unvollständig: Spalten in {table} fehlen"
                    )))
                }
                None => {
                    return Err(StorageError::Schema(format!(
                        "Migration unvollständig: Tabelle {table} fehlt"
                    )))
                }
            }
        }
        let integrity: String = db
            .conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|e| StorageError::Schema(format!("Integritätsprüfung fehlgeschlagen: {e}")))?;
        if integrity != "ok" {
            return Err(StorageError::Schema(format!(
                "Integritätsprüfung nach Migration: {integrity}"
            )));
        }
        db.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
        Ok(migrated)
    }

    pub fn backup_to(&self, path: &std::path::Path) -> Result<()> {
        self.conn.execute(
            "VACUUM INTO ?1",
            params![path.to_string_lossy().to_string()],
        )?;
        Ok(())
    }

    /// Zugriff auf die rohe Verbindung für Werkzeuge und Integrationstests.
    pub fn raw(&self) -> &Connection {
        &self.conn
    }

    /// Schreibt alle bestätigten Transaktionen in die Hauptdatei zurück. Danach ist die
    /// Hauptdatei für Austausch oder Kopie vollständig; WAL und SHM sind leer.
    pub fn wal_checkpoint(&self) -> Result<()> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    pub fn enqueue_outbox(
        &self,
        account_id: &str,
        entity_id: &str,
        field: &str,
        desired: bool,
    ) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        bump_outbox(&tx, account_id, entity_id, field, desired)?;
        tx.commit()?;
        Ok(())
    }

    pub fn account_id_for_feed(&self, feed_id: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT account_id FROM feeds WHERE id=?1",
                params![feed_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn apply_status_with_outbox(
        &self,
        feed_id: i64,
        article_id: &str,
        read: Option<bool>,
        saved: Option<bool>,
    ) -> Result<Option<String>> {
        let tx = self.conn.unchecked_transaction()?;
        let account: Option<String> = tx
            .query_row(
                "SELECT account_id FROM feeds WHERE id=?1",
                params![feed_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(r) = read {
            tx.execute(
                "UPDATE articles SET unread=?2, updated_ms=?4 WHERE feed_id=?1 AND id=?3",
                params![feed_id, if r { 0 } else { 1 }, article_id, now_ms()],
            )?;
        }
        if let Some(s) = saved {
            tx.execute(
                "UPDATE articles SET saved=?2, updated_ms=?4 WHERE feed_id=?1 AND id=?3",
                params![feed_id, if s { 1 } else { 0 }, article_id, now_ms()],
            )?;
        }
        if let Some(acc) = account.as_deref().filter(|a| *a != "local") {
            if let Some(r) = read {
                bump_outbox(&tx, acc, article_id, "read", r)?;
            }
            if let Some(s) = saved {
                bump_outbox(&tx, acc, article_id, "saved", s)?;
            }
        }
        tx.commit()?;
        Ok(account)
    }

    pub fn set_read_for_account(
        &self,
        account_id: &str,
        article_id: &str,
        read: bool,
    ) -> Result<usize> {
        self.conn
            .execute(
                "UPDATE articles SET unread=?2, updated_ms=?3
             WHERE id=?1 AND feed_id IN (SELECT id FROM feeds WHERE account_id=?4)",
                params![article_id, if read { 0 } else { 1 }, now_ms(), account_id],
            )
            .map_err(Into::into)
    }

    pub fn set_saved_for_account(
        &self,
        account_id: &str,
        article_id: &str,
        saved: bool,
    ) -> Result<usize> {
        self.conn
            .execute(
                "UPDATE articles SET saved=?2, updated_ms=?3
             WHERE id=?1 AND feed_id IN (SELECT id FROM feeds WHERE account_id=?4)",
                params![article_id, if saved { 1 } else { 0 }, now_ms(), account_id],
            )
            .map_err(Into::into)
    }

    /// Kontozustand nach §13.1: disconnected, initial_sync, ready, syncing,
    /// offline, rate_limited, auth_required, degraded.
    pub fn set_account_status(
        &self,
        account_id: &str,
        status: &str,
        detail: Option<&str>,
    ) -> Result<()> {
        let now = now_ms();
        let success = if status == "ready" { Some(now) } else { None };
        let error = if matches!(status, "rate_limited" | "auth_required" | "degraded") {
            Some(now)
        } else {
            None
        };
        self.conn.execute(
            "INSERT INTO account_status(account_id, status, detail, last_success_ms, last_error_ms)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(account_id) DO UPDATE SET
               status=excluded.status,
               detail=excluded.detail,
               last_success_ms=COALESCE(?4, account_status.last_success_ms),
               last_error_ms=COALESCE(?5, account_status.last_error_ms)",
            params![account_id, status, detail, success, error],
        )?;
        Ok(())
    }

    pub fn account_status(&self, account_id: &str) -> Result<Option<(String, Option<String>)>> {
        self.conn
            .query_row(
                "SELECT status, detail FROM account_status WHERE account_id=?1",
                params![account_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn stuck_changes(&self, account_id: &str) -> Result<i64> {
        let v: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE account_id=?1 AND status='failed'",
            params![account_id],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    /// Zählerstand aller lokalen Mutationen des Kontos. Wird **vor** einem
    /// HTTP-Abruf gelesen und bis zum Apply unverändert mitgeführt.
    pub fn pull_generation(&self, account_id: &str) -> Result<i64> {
        let v: Option<i64> = self
            .conn
            .query_row(
                "SELECT counter FROM account_sequence WHERE account_id=?1",
                params![account_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(v.unwrap_or(0))
    }

    pub fn record_remote_confirmation(
        &self,
        account_id: &str,
        entity_id: &str,
        field: &str,
        value: bool,
        revision: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO remote_confirmations(account_id, entity_id, field, value, confirmed_revision, updated_ms)
             VALUES (?1,?2,?3,?4,?5,?6)
             ON CONFLICT(account_id, entity_id, field) DO UPDATE SET
               value=excluded.value, confirmed_revision=excluded.confirmed_revision, updated_ms=excluded.updated_ms
             WHERE excluded.confirmed_revision >= remote_confirmations.confirmed_revision",
            params![account_id, entity_id, field, value as i64, revision, now_ms()],
        )?;
        Ok(())
    }

    pub fn apply_remote_status(
        &self,
        account_id: &str,
        article_id: &str,
        read: Option<bool>,
        saved: Option<bool>,
        pull_generation: i64,
    ) -> Result<RemoteApply> {
        let tx = self.conn.unchecked_transaction()?;
        let mut applied = false;
        let mut skipped = false;
        for (field, value) in [("read", read), ("saved", saved)] {
            let Some(value) = value else { continue };
            let (revision, sequence): (i64, i64) = tx
                .query_row(
                    "SELECT revision, sequence FROM field_revisions
                     WHERE account_id=?1 AND entity_id=?2 AND field=?3",
                    params![account_id, article_id, field],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .unwrap_or((0, 0));
            let pending: bool = tx.query_row(
                "SELECT COUNT(*) FROM outbox WHERE account_id=?1 AND entity_id=?2 AND field=?3
                 AND status IN ('pending','inflight')",
                params![account_id, article_id, field],
                |r| Ok(r.get::<_, i64>(0)? > 0),
            )?;
            // Nur eine Mutation, die **nach** dem Pull begann, darf den Pull
            // nicht überschreiben. Der Vergleich läuft über die account-globale
            // Sequenz, nicht über die feldweise Revision.
            if pending || sequence > pull_generation {
                skipped = true;
                continue;
            }
            if field == "read" {
                tx.execute(
                    "UPDATE articles SET unread=?2, updated_ms=?4
                     WHERE id=?3 AND feed_id IN (SELECT id FROM feeds WHERE account_id=?1)",
                    params![account_id, if value { 0 } else { 1 }, article_id, now_ms()],
                )?;
            } else {
                tx.execute(
                    "UPDATE articles SET saved=?2, updated_ms=?4
                     WHERE id=?3 AND feed_id IN (SELECT id FROM feeds WHERE account_id=?1)",
                    params![account_id, if value { 1 } else { 0 }, article_id, now_ms()],
                )?;
            }
            tx.execute(
                "INSERT INTO remote_confirmations(account_id, entity_id, field, value, confirmed_revision, updated_ms)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(account_id, entity_id, field) DO UPDATE SET
                   value=excluded.value, confirmed_revision=excluded.confirmed_revision, updated_ms=excluded.updated_ms",
                params![account_id, article_id, field, value as i64, revision, now_ms()],
            )?;
            applied = true;
        }
        tx.commit()?;
        Ok(RemoteApply { applied, skipped })
    }

    pub fn field_revision(&self, account_id: &str, entity_id: &str, field: &str) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT revision FROM field_revisions WHERE account_id=?1 AND entity_id=?2 AND field=?3",
                params![account_id, entity_id, field],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0))
            .map_err(Into::into)
    }

    pub fn outbox_pending(
        &self,
        account_id: &str,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<OutboxRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, entity_id, field, desired, revision FROM outbox
             WHERE account_id=?1 AND status='pending' AND next_try_ms<=?2
             ORDER BY id LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(params![account_id, now_ms, limit], |r| {
                Ok(OutboxRow {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    entity_id: r.get(2)?,
                    field: r.get(3)?,
                    desired: r.get::<_, i64>(4)? == 1,
                    revision: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    /// Holt und beansprucht die fälligen Zeilen in **einer** Transaktion. Damit
    /// können zwei Prozessoren dieselbe Zeile nicht gleichzeitig senden.
    pub fn outbox_claim(
        &self,
        account_id: &str,
        now_ms: i64,
        limit: u32,
    ) -> Result<Vec<OutboxRow>> {
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt = tx.prepare(
            "SELECT id, account_id, entity_id, field, desired, revision FROM outbox
             WHERE account_id=?1 AND status='pending' AND next_try_ms<=?2
             ORDER BY id LIMIT ?3",
        )?;
        let rows: Vec<OutboxRow> = stmt
            .query_map(params![account_id, now_ms, limit], |r| {
                Ok(OutboxRow {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    entity_id: r.get(2)?,
                    field: r.get(3)?,
                    desired: r.get::<_, i64>(4)? == 1,
                    revision: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        drop(stmt);
        for row in &rows {
            tx.execute(
                "UPDATE outbox SET status='inflight' WHERE id=?1 AND status='pending'",
                params![row.id],
            )?;
        }
        tx.commit()?;
        Ok(rows)
    }

    pub fn outbox_mark_inflight(&self, ids: &[i64]) -> Result<()> {
        for id in ids {
            self.conn.execute(
                "UPDATE outbox SET status='inflight' WHERE id=?1",
                params![id],
            )?;
        }
        Ok(())
    }

    pub fn outbox_ack(&self, sent: &[(i64, i64)]) -> Result<()> {
        for (id, revision) in sent {
            let cur: Option<i64> = self
                .conn
                .query_row(
                    "SELECT revision FROM outbox WHERE id=?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            match cur {
                Some(c) if c == *revision => {
                    let row: Option<(String, String, String, i64)> = self
                        .conn
                        .query_row(
                            "SELECT account_id, entity_id, field, desired FROM outbox WHERE id=?1",
                            params![id],
                            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                        )
                        .optional()?;
                    if let Some((acc, entity, field, desired)) = row {
                        self.record_remote_confirmation(
                            &acc,
                            &entity,
                            &field,
                            desired != 0,
                            *revision,
                        )?;
                    }
                    self.conn
                        .execute("DELETE FROM outbox WHERE id=?1", params![id])?;
                }
                Some(_) => {
                    self.conn.execute(
                        "UPDATE outbox SET status='pending', attempts=0, next_try_ms=0 WHERE id=?1",
                        params![id],
                    )?;
                }
                None => {}
            }
        }
        Ok(())
    }

    pub fn outbox_fail_permanent(&self, sent: &[(i64, i64)]) -> Result<usize> {
        self.outbox_fail(sent, 0, true)
    }

    pub fn outbox_stuck(&self, account_id: &str) -> Result<i64> {
        let v: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE account_id=?1 AND status='failed'",
            params![account_id],
            |r| r.get(0),
        )?;
        Ok(v)
    }

    /// Markiert Artikel, deren Statusänderung dauerhaft nicht bestätigt werden konnte.
    pub fn article_unsynced(&self, feed_id: i64, article_id: &str) -> Result<bool> {
        let v: i64 = self.conn.query_row(
            "SELECT unsynced FROM articles WHERE feed_id=?1 AND id=?2",
            params![feed_id, article_id],
            |r| r.get(0),
        )?;
        Ok(v != 0)
    }

    pub fn mark_unsynced(&self, entries: &[(String, String)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (account_id, entity_id) in entries {
            tx.execute(
                "UPDATE articles SET unsynced=1
                 WHERE id=?2 AND feed_id IN (SELECT id FROM feeds WHERE account_id=?1)",
                params![account_id, entity_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Fehlerbehandlung ausschließlich für die **gesendete** Revision. Eine inzwischen
    /// neuere lokale Absicht wird weder zurückgestellt noch als dauerhaft fehlgeschlagen
    /// markiert.
    pub fn outbox_fail(
        &self,
        sent: &[(i64, i64)],
        next_try_ms: i64,
        permanent: bool,
    ) -> Result<usize> {
        let mut touched = 0usize;
        for (id, revision) in sent {
            if permanent {
                touched += self.conn.execute(
                    "UPDATE outbox SET status='failed' WHERE id=?1 AND revision=?2",
                    params![id, revision],
                )?;
                self.conn.execute(
                    "UPDATE articles SET unsynced=1
                     WHERE id IN (SELECT entity_id FROM outbox WHERE id=?1 AND revision=?2)
                       AND feed_id IN (SELECT id FROM feeds WHERE account_id=(SELECT account_id FROM outbox WHERE id=?1))",
                    params![id, revision],
                )?;
            } else {
                touched += self.conn.execute(
                    "UPDATE outbox SET status='pending', attempts=attempts+1, next_try_ms=?3
                     WHERE id=?1 AND revision=?2",
                    params![id, revision, next_try_ms],
                )?;
            }
        }
        Ok(touched)
    }

    pub fn outbox_reset_inflight(&self) -> Result<()> {
        self.conn.execute(
            "UPDATE outbox SET status='pending' WHERE status='inflight'",
            [],
        )?;
        Ok(())
    }

    pub fn outbox_has_pending(
        &self,
        account_id: &str,
        entity_id: &str,
        field: &str,
    ) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE account_id=?1 AND entity_id=?2 AND field=?3 AND status IN ('pending','inflight')",
            params![account_id, entity_id, field],
            |r| r.get::<_, i64>(0),
        )? > 0)
    }

    pub fn outbox_counts(&self) -> Result<(i64, i64)> {
        let pending: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE status IN ('pending','inflight')",
            [],
            |r| r.get(0),
        )?;
        let failed: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM outbox WHERE status='failed'",
            [],
            |r| r.get(0),
        )?;
        Ok((pending, failed))
    }

    pub fn account_id_for_article(&self, article_id: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT f.account_id FROM articles a JOIN feeds f ON f.id=a.feed_id WHERE a.id=?1 LIMIT 1",
                params![article_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn get_pref(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM prefs WHERE key=?1", params![key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(Into::into)
    }

    /// Sortierung eines Kontos; ohne eigenen Eintrag gilt die globale Vorgabe.
    pub fn account_newest_first(&self, account_id: &str, fallback: bool) -> Result<bool> {
        match self.get_pref(&format!("sort_{account_id}"))? {
            Some(v) => Ok(v != "0"),
            None => Ok(fallback),
        }
    }

    pub fn set_account_newest_first(&self, account_id: &str, newest: bool) -> Result<()> {
        self.set_pref(
            &format!("sort_{account_id}"),
            if newest { "1" } else { "0" },
        )
    }

    pub fn set_pref(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO prefs(key, value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn mark_feeds_read(&self, feed_ids: &[i64]) -> Result<usize> {
        let mut n = 0usize;
        for fid in feed_ids {
            n += self.conn.execute(
                "UPDATE articles SET unread=0 WHERE feed_id=?1",
                params![fid],
            )?;
        }
        Ok(n)
    }

    pub fn update_fetch_error(
        &self,
        feed_id: i64,
        error_count: i64,
        last_error: &str,
        next_fetch_ms: i64,
        last_fetch_ms: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO feed_fetch_state(feed_id, error_count, last_error, next_fetch_ms, last_fetch_ms)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(feed_id) DO UPDATE SET error_count=excluded.error_count, last_error=excluded.last_error,
               next_fetch_ms=excluded.next_fetch_ms, last_fetch_ms=excluded.last_fetch_ms",
            params![feed_id, error_count, last_error, next_fetch_ms, last_fetch_ms],
        )?;
        Ok(())
    }

    pub fn set_last_sync(&self, account_id: &str, ms: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO account_state(account_id, last_sync_ms) VALUES (?1,?2)
             ON CONFLICT(account_id) DO UPDATE SET last_sync_ms=excluded.last_sync_ms",
            params![account_id, ms],
        )?;
        Ok(())
    }

    pub fn last_sync(&self, account_id: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT last_sync_ms FROM account_state WHERE account_id=?1",
                params![account_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut prev_ws = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            c => {
                if c.is_whitespace() {
                    if !prev_ws {
                        out.push(' ');
                    }
                    prev_ws = true;
                } else {
                    out.push(c);
                    prev_ws = false;
                }
            }
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

fn build_fts_query(query: &str) -> String {
    let mut parts = Vec::new();
    for token in query.split_whitespace() {
        let cleaned: String = token.chars().filter(|c| !"*\"()^:-".contains(*c)).collect();
        if cleaned.is_empty() {
            continue;
        }
        parts.push(format!("\"{}\"*", cleaned.replace('"', "")));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lesefluss-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn seed(db: &Database) -> i64 {
        db.ensure_local_account().unwrap();
        db.add_feed(
            "local",
            "https://example.com/feed.xml",
            "Example",
            Some("https://example.com"),
            "#123456",
        )
        .unwrap()
    }

    #[test]
    fn migration_and_upsert_preserve_status() {
        let db = Database::open_in_memory().unwrap();
        let feed = seed(&db);
        let now = now_ms();
        db.upsert_article(
            feed,
            "a1",
            "Titel eins",
            None,
            None,
            now - 1000,
            "Auszug",
            Some("<p>Hallo Welt</p>"),
            now,
        )
        .unwrap();
        db.set_status(feed, "a1", Some(true), Some(true)).unwrap();
        db.upsert_article(
            feed,
            "a1",
            "Titel eins (update)",
            None,
            None,
            now - 1000,
            "Auszug neu",
            Some("<p>Hallo Welt 2</p>"),
            now + 5,
        )
        .unwrap();
        let (unread, saved) = db.article_status(feed, "a1").unwrap().unwrap();
        assert!(!unread, "Re-Import darf nicht ungelesen machen");
        assert!(saved, "Re-Import darf Speicherstatus nicht verlieren");
        let rows = db
            .query_articles(&Scope::Global, Filter::All, None, 10)
            .unwrap();
        assert_eq!(rows[0].title, "Titel eins (update)");
    }

    #[test]
    fn keyset_pagination_and_filters() {
        let db = Database::open_in_memory().unwrap();
        let feed = seed(&db);
        let now = now_ms();
        for i in 0..25 {
            db.upsert_article(
                feed,
                &format!("a{i}"),
                &format!("T{i}"),
                None,
                None,
                now - i * 1000,
                "x",
                None,
                now,
            )
            .unwrap();
        }
        let page1 = db
            .query_articles(&Scope::Global, Filter::All, None, 10)
            .unwrap();
        assert_eq!(page1.len(), 10);
        let last = page1.last().unwrap();
        let page2 = db
            .query_articles(
                &Scope::Global,
                Filter::All,
                Some((last.sort_ms, last.feed_id, &last.id)),
                10,
            )
            .unwrap();
        assert_eq!(page2.len(), 10);
        assert!(!page1.iter().any(|r| page2.iter().any(|s| s.id == r.id)));
        db.set_status(feed, "a3", Some(true), None).unwrap();
        let unread = db
            .query_articles(&Scope::Global, Filter::Unread, None, 100)
            .unwrap();
        assert_eq!(unread.len(), 24);
        assert!(!unread.iter().any(|r| r.id == "a3"));
    }

    #[test]
    fn fts_search_unicode() {
        let db = Database::open_in_memory().unwrap();
        let feed = seed(&db);
        let now = now_ms();
        db.upsert_article(
            feed,
            "s1",
            "Grüße aus München",
            None,
            None,
            now,
            "Straße und Café",
            Some("<p>Straße und <b>Café</b></p>"),
            now,
        )
        .unwrap();
        db.upsert_article(
            feed,
            "s2",
            "Other",
            None,
            None,
            now,
            "nothing",
            Some("<p>nothing</p>"),
            now,
        )
        .unwrap();
        let hits = db
            .search("cafe", &Scope::Global, Filter::All, None, 10)
            .unwrap();
        assert_eq!(hits.len(), 1, "remove_diacritics sollte Café finden");
        let hits = db
            .search("münchen", &Scope::Global, Filter::All, None, 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        let hits = db
            .search("\"\"", &Scope::Global, Filter::All, None, 10)
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn outbox_coalesce_and_ack() {
        let db = Database::open_in_memory().unwrap();
        db.enqueue_outbox("acc", "e1", "read", true).unwrap();
        db.enqueue_outbox("acc", "e1", "read", false).unwrap();
        let pending = db.outbox_pending("acc", 10, 10).unwrap();
        assert_eq!(pending.len(), 1, "muss zu einer Zeile kondensieren");
        assert!(!pending[0].desired);
        assert_eq!(pending[0].revision, 2);
        db.outbox_mark_inflight(&[pending[0].id]).unwrap();
        assert!(db.outbox_pending("acc", 10, 10).unwrap().is_empty());
        db.enqueue_outbox("acc", "e1", "read", true).unwrap();
        db.outbox_ack(&[(pending[0].id, 2)]).unwrap();
        let after = db.outbox_pending("acc", 10, 10).unwrap();
        assert_eq!(
            after.len(),
            1,
            "verspaetetes ACK darf neuere Mutation nicht loeschen"
        );
        assert_eq!(after[0].revision, 3);
        db.outbox_ack(&[(after[0].id, 3)]).unwrap();
        assert!(db.outbox_pending("acc", 10, 10).unwrap().is_empty());
        assert!(db.outbox_has_pending("acc", "e1", "read").unwrap() == false);
    }

    #[test]
    fn batch_upsert_single_transaction() {
        let db = Database::open_in_memory().unwrap();
        let feed = seed(&db);
        let now = now_ms();
        let items: Vec<NewArticle> = (0..50)
            .map(|i| NewArticle {
                id: format!("b{i}"),
                title: format!("B{i}"),
                author: None,
                url: None,
                published_ms: now - i * 60_000,
                excerpt: "e".into(),
                html: Some(format!("<p>Text {i}</p>")),
                content_hash: None,
            })
            .collect();
        let (added, updated) = db.upsert_articles(feed, &items, now).unwrap();
        assert_eq!((added, updated), (50, 0));
        db.set_status(feed, "b7", Some(true), Some(true)).unwrap();
        let (added, updated) = db.upsert_articles(feed, &items, now + 10).unwrap();
        assert_eq!((added, updated), (0, 50));
        let (unread, saved) = db.article_status(feed, "b7").unwrap().unwrap();
        assert!(!unread && saved);
        let c = db.counts().unwrap();
        assert_eq!(c.total, 50);
        assert_eq!(c.unread, 49);
    }

    #[test]
    fn group_counts_dedup() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let f1 = db.add_feed(&acc, "u1", "F1", None, "#111111").unwrap();
        let f2 = db.add_feed(&acc, "u2", "F2", None, "#222222").unwrap();
        let g = db.add_group(&acc, "Tech", None).unwrap();
        db.set_feed_groups(f1, &[g]).unwrap();
        db.set_feed_groups(f2, &[g]).unwrap();
        let now = now_ms();
        db.upsert_article(f1, "x", "X", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(f2, "y", "Y", None, None, now, "e", None, now)
            .unwrap();
        let c = db.counts().unwrap();
        assert_eq!(c.per_group, vec![(g, 2)]);
        let rows = db
            .query_articles(&Scope::Group(g), Filter::Unread, None, 10)
            .unwrap();
        assert_eq!(rows.len(), 2);
    }

    fn remote_account(db: &Database) -> String {
        db.conn
            .execute(
                "INSERT INTO accounts(id,kind,name,created_ms) VALUES ('feedly-1','feedly','Feedly',0)",
                [],
            )
            .unwrap();
        "feedly-1".to_string()
    }

    #[test]
    fn user_title_survives_publisher_updates() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db
            .add_feed(&acc, "https://a.example/f.xml", "Original", None, "#111111")
            .unwrap();
        db.set_user_title(feed, "Mein Titel").unwrap();
        db.update_feed_title(feed, "Neuer Publisher-Titel", None)
            .unwrap();
        assert_eq!(
            db.display_title(feed).unwrap().as_deref(),
            Some("Mein Titel")
        );
        let feeds = db.list_feeds().unwrap();
        assert_eq!(feeds[0].title, "Mein Titel");
    }

    #[test]
    fn unsubscribing_keeps_saved_articles_and_stops_fetching() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db
            .add_feed(&acc, "https://a.example/f.xml", "Feed", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(
            feed,
            "kept",
            "Wichtig",
            None,
            None,
            now,
            "e",
            Some("<p>x</p>"),
            now,
        )
        .unwrap();
        db.upsert_article(
            feed,
            "plain",
            "Normal",
            None,
            None,
            now,
            "e",
            Some("<p>y</p>"),
            now,
        )
        .unwrap();
        db.set_saved_by_article_id("kept", true).unwrap();
        assert!(db.feed_is_active(feed).unwrap());
        let kept = db.deactivate_feed(feed).unwrap();
        assert_eq!(kept, 1, "ein gespeicherter Artikel bleibt erhalten");
        assert!(!db.feed_is_active(feed).unwrap());
        assert!(
            db.due_feeds(now).unwrap().is_empty(),
            "keine Abrufe mehr für stillgelegte Feeds"
        );
        let articles = db
            .query_articles(&Scope::Global, Filter::All, None, 10)
            .unwrap();
        assert_eq!(articles.len(), 2, "Inhalte bleiben lesbar");
        assert!(articles.iter().any(|a| a.saved));
        db.set_feed_active(feed, true).unwrap();
        assert_eq!(
            db.due_feeds(now).unwrap().len(),
            1,
            "Reabo holt den Bestand wieder"
        );
    }

    #[test]
    fn opml_import_is_transactional_and_scope_aware() {
        let db = Database::open_in_memory().unwrap();
        let local = db.ensure_local_account().unwrap();
        let remote = remote_account(&db);
        let entries = vec![
            (
                "F1".to_string(),
                "https://a.example/f.xml".to_string(),
                Some("https://a.example".to_string()),
                vec!["Technik".to_string(), "Rust".to_string()],
            ),
            (
                "F1".to_string(),
                "https://a.example/f.xml".to_string(),
                None,
                vec!["Technik".to_string()],
            ),
        ];
        let (new, merged) = db.import_opml_entries(&local, &entries).unwrap();
        assert_eq!(
            (new, merged),
            (1, 1),
            "zweiter Durchlauf führt nicht zu einem Duplikat"
        );
        let feeds = db.list_feeds().unwrap();
        assert_eq!(feeds.len(), 1);
        assert_eq!(
            feeds[0].groups.len(),
            2,
            "verschachtelte Gruppen bleiben erhalten"
        );
        let groups = db.list_groups().unwrap();
        assert_eq!(groups.len(), 2);
        let child = groups.iter().find(|g| g.name == "Rust").unwrap();
        let parent = groups.iter().find(|g| g.name == "Technik").unwrap();
        assert_eq!(child.parent_id, Some(parent.id), "echte Parent-Gruppe");

        let (new_remote, _) = db.import_opml_entries(&remote, &entries).unwrap();
        assert_eq!(new_remote, 1, "anderes Konto erhält eigene Feeds");
        assert_eq!(db.list_feeds().unwrap().len(), 2);
        let remote_groups: Vec<_> = db
            .list_groups()
            .unwrap()
            .into_iter()
            .filter(|g| g.account_id == "feedly-1")
            .collect();
        assert_eq!(
            remote_groups.len(),
            2,
            "Gruppen werden nicht kontoübergreifend wiederverwendet"
        );
    }

    #[test]
    fn oldest_first_paginates_without_gaps_or_repeats() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db.add_feed(&acc, "u1", "Feed", None, "#111111").unwrap();
        let now = now_ms();
        for i in 0..6 {
            db.upsert_article(
                feed,
                &format!("a{i}"),
                "Titel",
                None,
                None,
                now - (5 - i) * 60_000,
                "e",
                None,
                now,
            )
            .unwrap();
        }
        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<(i64, i64, String)> = None;
        for _ in 0..4 {
            let rows = db
                .query_articles_ordered(
                    &Scope::Global,
                    Filter::All,
                    cursor.as_ref().map(|(m, f, i)| (*m, *f, i.as_str())),
                    2,
                    false,
                )
                .unwrap();
            if rows.is_empty() {
                break;
            }
            for r in &rows {
                seen.push(r.id.clone());
            }
            let last = rows.last().unwrap();
            cursor = Some((last.sort_ms, last.feed_id, last.id.clone()));
        }
        assert_eq!(seen.len(), 6, "jeder Artikel genau einmal: {seen:?}");
        assert_eq!(
            seen.first().map(String::as_str),
            Some("a0"),
            "älteste zuerst"
        );
        assert_eq!(seen.last().map(String::as_str), Some("a5"));
    }

    #[test]
    fn search_also_supports_oldest_first() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db.add_feed(&acc, "u1", "Feed", None, "#111111").unwrap();
        let now = now_ms();
        for i in 0..3 {
            db.upsert_article(
                feed,
                &format!("a{i}"),
                "Titel",
                None,
                None,
                now - (3 - i) * 1000,
                "e",
                Some("<p>zielwort</p>"),
                now,
            )
            .unwrap();
        }
        let rows = db
            .search_ordered("zielwort", &Scope::Global, Filter::All, None, 10, false)
            .unwrap();
        assert_eq!(rows[0].id, "a0");
        let rows = db
            .search_ordered("zielwort", &Scope::Global, Filter::All, None, 10, true)
            .unwrap();
        assert_eq!(rows[0].id, "a2");
    }

    #[test]
    fn alter_pull_ueberschreibt_keine_juengere_aenderung_eines_anderen_artikels() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        db.upsert_account("feedly-1", "feedly", "Feedly").unwrap();
        let feed = db
            .add_feed("feedly-1", "u1", "Feed", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(feed, "a", "A", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(feed, "b", "B", None, None, now, "e", None, now)
            .unwrap();

        // Artikel A zehnmal ändern und quittieren: hohe feldweise Revision.
        for i in 0..10 {
            db.enqueue_outbox("feedly-1", "a", "read", i % 2 == 0)
                .unwrap();
            let rows = db.outbox_pending("feedly-1", now_ms(), 10).unwrap();
            let sent: Vec<(i64, i64)> = rows.iter().map(|r| (r.id, r.revision)).collect();
            let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
            db.outbox_mark_inflight(&ids).unwrap();
            db.outbox_ack(&sent).unwrap();
        }
        // Pull beginnt und merkt sich diesen Stand.
        let pull_gen = db.pull_generation("feedly-1").unwrap();
        assert!(pull_gen >= 10, "Sequenz zählt alle Mutationen");

        // Artikel B wird danach lokal geändert und quittiert.
        db.enqueue_outbox("feedly-1", "b", "read", true).unwrap();
        let rows = db.outbox_pending("feedly-1", now_ms(), 10).unwrap();
        let sent: Vec<(i64, i64)> = rows.iter().map(|r| (r.id, r.revision)).collect();
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        db.outbox_mark_inflight(&ids).unwrap();
        db.outbox_ack(&sent).unwrap();
        db.raw()
            .execute("UPDATE articles SET unread=0 WHERE id='b'", [])
            .unwrap();

        // Das alte Pull-Ernis darf B nicht zurücksetzen.
        let applied = db
            .apply_remote_status("feedly-1", "b", Some(false), None, pull_gen)
            .unwrap();
        assert!(
            applied.skipped,
            "B wurde als zwischenzeitlich geändert erkannt"
        );
        let unread_b: i64 = db
            .raw()
            .query_row("SELECT unread FROM articles WHERE id='b'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(unread_b, 0, "die neuere lokale Absicht bleibt erhalten");
    }

    #[test]
    fn gleiche_id_in_zwei_feeds_ueberspringt_keine_zeile() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let f1 = db.add_feed(&acc, "u1", "Feed A", None, "#111111").unwrap();
        let f2 = db.add_feed(&acc, "u2", "Feed B", None, "#222222").unwrap();
        let now = now_ms();
        // Gleiche GUID und gleiche Zeit in beiden Feeds.
        db.upsert_article(f1, "gleich", "A", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(f2, "gleich", "B", None, None, now, "e", None, now)
            .unwrap();
        let mut seen: Vec<(i64, String)> = Vec::new();
        let mut cursor: Option<(i64, i64, String)> = None;
        for _ in 0..4 {
            let rows = db
                .query_articles_ordered(
                    &Scope::Global,
                    Filter::All,
                    cursor.as_ref().map(|(m, f, i)| (*m, *f, i.as_str())),
                    1,
                    true,
                )
                .unwrap();
            if rows.is_empty() {
                break;
            }
            for r in &rows {
                assert!(
                    !seen.contains(&(r.feed_id, r.id.clone())),
                    "Zeile darf nicht erneut geliefert werden"
                );
                seen.push((r.feed_id, r.id.clone()));
            }
            let last = rows.last().unwrap();
            cursor = Some((last.sort_ms, last.feed_id, last.id.clone()));
        }
        assert_eq!(seen.len(), 2, "beide Zeilen werden geliefert: {seen:?}");
    }

    #[test]
    fn gruppenzaehlung_zaehlt_gleiche_id_je_feed_getrennt() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let f1 = db.add_feed(&acc, "u1", "Feed A", None, "#111111").unwrap();
        let f2 = db.add_feed(&acc, "u2", "Feed B", None, "#222222").unwrap();
        let group = db.add_group(&acc, "Gruppe", None).unwrap();
        db.set_feed_groups(f1, &[group]).unwrap();
        db.set_feed_groups(f2, &[group]).unwrap();
        let now = now_ms();
        db.upsert_article(f1, "gleich", "A", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(f2, "gleich", "B", None, None, now, "e", None, now)
            .unwrap();
        let counts = db.counts().unwrap();
        assert_eq!(counts.unread, 2);
        assert_eq!(
            counts.per_group,
            vec![(group, 2)],
            "zwei Feeds mit derselben GUID sind zwei Artikel"
        );
    }

    #[test]
    fn keyset_cursor_uses_sort_key_and_never_repeats() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db.add_feed(&acc, "u1", "Feed", None, "#111111").unwrap();
        let now = now_ms();
        db.upsert_article(feed, "a", "A", None, None, now - 1000, "e", None, now)
            .unwrap();
        db.upsert_article(feed, "b", "B", None, None, now - 1000, "e", None, now)
            .unwrap();
        db.upsert_article(feed, "c", "C", None, None, now - 1000, "e", None, now)
            .unwrap();
        let first = db
            .query_articles(&Scope::Global, Filter::All, None, 2)
            .unwrap();
        assert_eq!(first.len(), 2);
        let last = first.last().unwrap();
        let cursor = (last.sort_ms, last.feed_id, last.id.as_str());
        let second = db
            .query_articles(&Scope::Global, Filter::All, Some(cursor), 2)
            .unwrap();
        assert_eq!(second.len(), 1);
        let ids: std::collections::HashSet<&str> = first
            .iter()
            .map(|r| r.id.as_str())
            .chain(second.iter().map(|r| r.id.as_str()))
            .collect();
        assert_eq!(ids.len(), 3, "keine doppelten Zeilen trotz gleicher Zeit");
    }

    #[test]
    fn future_timestamps_are_clamped_without_breaking_the_cursor() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db.add_feed(&acc, "u1", "Feed", None, "#111111").unwrap();
        let now = now_ms();
        db.upsert_article(
            feed,
            "future",
            "Zukunft",
            None,
            None,
            now + 10_000_000_000,
            "e",
            None,
            now,
        )
        .unwrap();
        db.upsert_article(
            feed,
            "normal",
            "Normal",
            None,
            None,
            now - 500,
            "e",
            None,
            now,
        )
        .unwrap();
        let first = db
            .query_articles(&Scope::Global, Filter::All, None, 1)
            .unwrap();
        let cursor = (first[0].sort_ms, first[0].feed_id, first[0].id.as_str());
        assert!(
            cursor.0 <= now,
            "Sortierschlüssel ist begrenzt: {}",
            cursor.0
        );
        let second = db
            .query_articles(&Scope::Global, Filter::All, Some(cursor), 1)
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_ne!(
            second[0].id, first[0].id,
            "derselbe Artikel darf nicht erneut erscheinen"
        );
    }

    #[test]
    fn search_respects_scope_filter_and_cursor() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let f1 = db.add_feed(&acc, "u1", "F1", None, "#111111").unwrap();
        let f2 = db.add_feed(&acc, "u2", "F2", None, "#222222").unwrap();
        let now = now_ms();
        for i in 0..5 {
            db.upsert_article(
                f1,
                &format!("a{i}"),
                "Suchtreffer A",
                None,
                None,
                now - i * 1000,
                "e",
                Some("<p>zielwort</p>"),
                now,
            )
            .unwrap();
            db.upsert_article(
                f2,
                &format!("b{i}"),
                "Suchtreffer B",
                None,
                None,
                now - i * 1000,
                "e",
                Some("<p>zielwort</p>"),
                now,
            )
            .unwrap();
        }
        db.set_status(f1, "a1", Some(true), None).unwrap();
        let scoped = db
            .search("zielwort", &Scope::Feed(f2), Filter::All, None, 100)
            .unwrap();
        assert!(scoped.iter().all(|r| r.feed_id == f2), "Feed-Filter");
        let unread = db
            .search("zielwort", &Scope::Global, Filter::Unread, None, 100)
            .unwrap();
        assert!(unread.iter().all(|r| r.unread), "Ungelesen-Filter");
        let page1 = db
            .search("zielwort", &Scope::Global, Filter::All, None, 4)
            .unwrap();
        assert_eq!(
            page1.len(),
            5,
            "4 Treffer plus eine Extrazeile signalisiert weitere"
        );
        let cursor = (page1[3].sort_ms, page1[3].feed_id, page1[3].id.as_str());
        let page2 = db
            .search("zielwort", &Scope::Global, Filter::All, Some(cursor), 4)
            .unwrap();
        assert!(
            !page2.iter().any(|r| r.id == page1[3].id),
            "kein doppelter Treffer"
        );
        let seen: std::collections::HashSet<&str> = page1
            .iter()
            .take(4)
            .map(|r| r.id.as_str())
            .chain(page2.iter().map(|r| r.id.as_str()))
            .collect();
        assert_eq!(seen.len(), 9, "4 + 5 der 10 Treffer, überschneidungsfrei");
    }

    #[test]
    fn identity_is_account_and_article_id() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let remote = remote_account(&db);
        let f1 = db.add_feed(&acc, "u1", "F1", None, "#111111").unwrap();
        let f2 = db.add_feed(&acc, "u2", "F2", None, "#222222").unwrap();
        let f3 = db.add_feed(&remote, "u3", "F3", None, "#333333").unwrap();
        let now = now_ms();
        db.upsert_article(f1, "same", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(f2, "same", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(f3, "same", "Titel", None, None, now, "e", None, now)
            .unwrap();
        let c = db.counts().unwrap();
        assert_eq!(
            c.total, 3,
            "zwei lokale Feeds plus Feedly sind drei Artikel"
        );
        assert_eq!(c.unread, 3);
        assert_eq!(c.per_feed, vec![(f1, 1), (f2, 1), (f3, 1)]);
        assert_eq!(c.per_account, vec![(remote, 1)]);
    }

    #[test]
    fn remote_status_updates_stay_inside_the_account() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let remote = remote_account(&db);
        let local_feed = db.add_feed(&acc, "u1", "Lokal", None, "#111111").unwrap();
        let remote_feed = db
            .add_feed(&remote, "u2", "Feedly", None, "#222222")
            .unwrap();
        let now = now_ms();
        db.upsert_article(local_feed, "same", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(
            remote_feed,
            "same",
            "Titel",
            None,
            None,
            now,
            "e",
            None,
            now,
        )
        .unwrap();
        assert_eq!(db.set_read_for_account(&remote, "same", true).unwrap(), 1);
        let (local_unread, _) = db.article_status(local_feed, "same").unwrap().unwrap();
        let (remote_unread, _) = db.article_status(remote_feed, "same").unwrap().unwrap();
        assert!(local_unread, "lokaler Artikel bleibt ungelesen");
        assert!(!remote_unread, "Feedly-Artikel wird gelesen");
    }

    #[test]
    fn status_and_outbox_commit_together() {
        let db = Database::open_in_memory().unwrap();
        let remote = remote_account(&db);
        let feed = db
            .add_feed(&remote, "u1", "Feedly", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(feed, "e1", "Titel", None, None, now, "e", None, now)
            .unwrap();
        assert_eq!(
            db.apply_status_with_outbox(feed, "e1", Some(true), Some(true))
                .unwrap()
                .as_deref(),
            Some("feedly-1")
        );
        let (unread, saved) = db.article_status(feed, "e1").unwrap().unwrap();
        assert!(!unread && saved);
        let pending = db.outbox_pending(&remote, now, 10).unwrap();
        assert_eq!(pending.len(), 2, "read und saved liegen in der Outbox");
        assert_eq!(db.field_revision(&remote, "e1", "read").unwrap(), 1);
        let sent: Vec<(i64, i64)> = pending.iter().map(|p| (p.id, p.revision)).collect();
        db.outbox_ack(&sent).unwrap();
        assert!(db.outbox_pending(&remote, now, 10).unwrap().is_empty());
        db.apply_status_with_outbox(feed, "e1", Some(false), None)
            .unwrap();
        assert_eq!(
            db.field_revision(&remote, "e1", "read").unwrap(),
            2,
            "Revision bleibt dauerhaft monoton"
        );
    }

    #[test]
    fn alter_requestfehler_legt_die_neue_revision_nicht_dauerhaft_still() {
        let db = Database::open_in_memory().unwrap();
        let remote = remote_account(&db);
        let feed = db
            .add_feed(&remote, "u1", "Feedly", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(feed, "e1", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.apply_status_with_outbox(feed, "e1", Some(true), None)
            .unwrap();
        let claimed = db.outbox_claim(&remote, now, 10).unwrap();
        let sent: Vec<(i64, i64)> = claimed.iter().map(|r| (r.id, r.revision)).collect();
        // Während des Requests entsteht eine neue Absicht im selben Feld:
        // höhere Revision, dieselbe Outbox-Zeile.
        db.apply_status_with_outbox(feed, "e1", Some(false), None)
            .unwrap();
        let affected = db.outbox_fail_permanent(&sent).unwrap();
        assert_eq!(affected, 0, "die alte Revision wird nicht mehr markiert");
        assert_eq!(
            db.outbox_stuck(&remote).unwrap(),
            0,
            "die neue Absicht bleibt sendbar"
        );
        let pending = db.outbox_pending(&remote, now, 10).unwrap();
        assert_eq!(pending.len(), 1, "genau die neue Absicht wartet");
        assert_eq!(pending[0].field, "read");
        assert!(
            !pending[0].desired,
            "die neuere Absicht (unread) ist maßgeblich"
        );
    }

    #[test]
    fn claim_verhindert_doppeltes_senden_durch_zwei_prozessoren() {
        let db = Database::open_in_memory().unwrap();
        let remote = remote_account(&db);
        let feed = db
            .add_feed(&remote, "u1", "Feedly", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(feed, "e1", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.apply_status_with_outbox(feed, "e1", Some(true), None)
            .unwrap();
        let first = db.outbox_claim(&remote, now, 10).unwrap();
        let second = db.outbox_claim(&remote, now, 10).unwrap();
        assert_eq!(first.len(), 1);
        assert!(
            second.is_empty(),
            "eine beanspruchte Zeile wird nicht erneut ausgegeben"
        );
    }

    #[test]
    fn temporaerer_fehler_stellt_nur_die_gesendete_revision_zurueck() {
        let db = Database::open_in_memory().unwrap();
        let remote = remote_account(&db);
        let feed = db
            .add_feed(&remote, "u1", "Feedly", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(feed, "e1", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.apply_status_with_outbox(feed, "e1", Some(true), None)
            .unwrap();
        let claimed = db.outbox_claim(&remote, now, 10).unwrap();
        let sent: Vec<(i64, i64)> = claimed.iter().map(|r| (r.id, r.revision)).collect();
        let later = now + 60_000;
        let affected = db.outbox_fail(&sent, later, false).unwrap();
        assert_eq!(affected, 1);
        let retry = db.outbox_pending(&remote, later, 10).unwrap();
        assert_eq!(retry.len(), 1, "nach Zeitablauf erneut fällig");
    }

    #[test]
    fn permanent_outbox_failure_is_kept_and_marked_unsynced() {
        let db = Database::open_in_memory().unwrap();
        let remote = remote_account(&db);
        let feed = db
            .add_feed(&remote, "u1", "Feedly", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(feed, "e1", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.apply_status_with_outbox(feed, "e1", Some(true), None)
            .unwrap();
        let rows = db.outbox_claim(&remote, now, 10).unwrap();
        let sent: Vec<(i64, i64)> = rows.iter().map(|r| (r.id, r.revision)).collect();
        db.outbox_fail_permanent(&sent).unwrap();
        assert!(
            db.outbox_pending(&remote, now + 86_400_000, 10)
                .unwrap()
                .is_empty(),
            "kein erneuter Versuch"
        );
        assert_eq!(
            db.outbox_stuck(&remote).unwrap(),
            1,
            "bleibt sichtbar erhalten"
        );
        assert_eq!(db.article_unsynced(feed, "e1").unwrap(), true);
    }

    #[test]
    fn candidate_validation_rejects_garbage_and_foreign_databases() {
        let dir = tempdir("validate");
        let garbage = dir.join("garbage.db");
        std::fs::write(&garbage, vec![0u8; 4096]).unwrap();
        assert!(
            Database::validate_candidate(&garbage).is_err(),
            "kein SQLite"
        );

        let truncated = dir.join("truncated.db");
        let db = Database::open(&dir.join("live.db")).unwrap();
        db.ensure_local_account().unwrap();
        db.backup_to(&truncated).unwrap();
        {
            let bytes = std::fs::read(&truncated).unwrap();
            let mut cut = bytes.clone();
            cut.truncate(bytes.len() / 2);
            std::fs::write(&truncated, &cut).unwrap();
        }
        assert!(
            Database::validate_candidate(&truncated).is_err(),
            "abgeschnittene Datenbank wird abgelehnt"
        );

        let foreign = dir.join("foreign.db");
        let conn = Connection::open(&foreign).unwrap();
        conn.execute_batch("CREATE TABLE unrelated(x);").unwrap();
        drop(conn);
        assert!(
            Database::validate_candidate(&foreign).is_err(),
            "fremde Datenbank"
        );

        let good = dir.join("good.db");
        db.backup_to(&good).unwrap();
        assert!(Database::validate_candidate(&good).is_ok());
    }

    #[test]
    fn candidate_validation_rejects_newer_schema() {
        let dir = tempdir("newer");
        let candidate = dir.join("candidate.db");
        let db = Database::open(&dir.join("live.db")).unwrap();
        db.ensure_local_account().unwrap();
        db.backup_to(&candidate).unwrap();
        {
            let conn = Connection::open(&candidate).unwrap();
            conn.execute(
                "INSERT INTO schema_version(version, applied_ms) VALUES (9999, 0)",
                [],
            )
            .unwrap();
        }
        let err = Database::validate_candidate(&candidate).unwrap_err();
        assert!(err.to_string().contains("neuer"), "{err}");
    }

    #[test]
    fn backup_keeps_outbox_and_preferences() {
        let dir = tempdir("backup");
        let live = dir.join("live.db");
        let target = dir.join("backup.db");
        let db = Database::open(&live).unwrap();
        let remote = remote_account(&db);
        let feed = db
            .add_feed(&remote, "u1", "Feedly", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(
            feed,
            "e1",
            "Titel",
            None,
            None,
            now,
            "e",
            Some("<p>x</p>"),
            now,
        )
        .unwrap();
        db.apply_status_with_outbox(feed, "e1", Some(true), Some(true))
            .unwrap();
        db.set_pref("theme", "omarchy").unwrap();
        db.backup_to(&target).unwrap();
        let restored = Database::open(&target).unwrap();
        assert_eq!(
            restored.get_pref("theme").unwrap().as_deref(),
            Some("omarchy")
        );
        assert_eq!(restored.outbox_pending(&remote, now, 10).unwrap().len(), 2);
        assert_eq!(
            restored.article_status(feed, "e1").unwrap(),
            Some((false, true))
        );
    }

    #[test]
    fn stale_pull_cannot_overwrite_a_confirmed_local_intent() {
        let db = Database::open_in_memory().unwrap();
        let remote = remote_account(&db);
        let feed = db
            .add_feed(&remote, "u1", "Feedly", None, "#111111")
            .unwrap();
        let now = now_ms();
        db.upsert_article(
            feed,
            "e1",
            "Titel",
            None,
            None,
            now,
            "e",
            Some("<p>x</p>"),
            now,
        )
        .unwrap();

        let pull_gen = db.pull_generation(&remote).unwrap();
        let pending = db.outbox_pending(&remote, now, 10).unwrap();

        db.apply_status_with_outbox(feed, "e1", Some(false), None)
            .unwrap();
        assert_eq!(db.field_revision(&remote, "e1", "read").unwrap(), 1);
        let rows = db.outbox_pending(&remote, now, 10).unwrap();
        let ids: Vec<(i64, i64)> = rows.iter().map(|r| (r.id, r.revision)).collect();
        db.outbox_ack(&ids).unwrap();

        let late = db
            .apply_remote_status(&remote, "e1", Some(true), None, pull_gen)
            .unwrap();
        assert!(late.skipped && !late.applied, "alter Pull wird verworfen");
        let (unread, _) = db.article_status(feed, "e1").unwrap().unwrap();
        assert!(unread, "jüngere lokale Absicht bleibt erhalten");

        let fresh_gen = db.pull_generation(&remote).unwrap();
        let applied = db
            .apply_remote_status(&remote, "e1", Some(true), None, fresh_gen)
            .unwrap();
        assert!(applied.applied, "frischer Remote-Stand gewinnt");
        let (unread, _) = db.article_status(feed, "e1").unwrap().unwrap();
        assert!(!unread);
    }

    #[test]
    fn remote_pull_never_touches_local_articles() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let remote = remote_account(&db);
        let local_feed = db.add_feed(&acc, "u1", "Lokal", None, "#111111").unwrap();
        let remote_feed = db
            .add_feed(&remote, "u2", "Feedly", None, "#222222")
            .unwrap();
        let now = now_ms();
        db.upsert_article(local_feed, "same", "Titel", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(
            remote_feed,
            "same",
            "Titel",
            None,
            None,
            now,
            "e",
            None,
            now,
        )
        .unwrap();
        let gen = db.pull_generation(&remote).unwrap();
        db.apply_remote_status(&remote, "same", Some(true), None, gen)
            .unwrap();
        let (local_unread, _) = db.article_status(local_feed, "same").unwrap().unwrap();
        let (remote_unread, _) = db.article_status(remote_feed, "same").unwrap().unwrap();
        assert!(local_unread, "lokaler Artikel bleibt unberührt");
        assert!(!remote_unread);
    }

    #[test]
    fn local_status_change_creates_no_outbox() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db.add_feed(&acc, "u1", "Lokal", None, "#111111").unwrap();
        let now = now_ms();
        db.upsert_article(feed, "a1", "Titel", None, None, now, "e", None, now)
            .unwrap();
        assert_eq!(
            db.apply_status_with_outbox(feed, "a1", Some(true), None)
                .unwrap()
                .as_deref(),
            Some("local")
        );
        assert!(db.outbox_pending(&acc, now, 10).unwrap().is_empty());
    }

    #[test]
    fn due_feeds_only_returns_active_local_feeds() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let remote = remote_account(&db);
        let local = db.add_feed(&acc, "u1", "Lokal", None, "#111111").unwrap();
        let _remote_feed = db
            .add_feed(&remote, "u2", "Feedly", None, "#222222")
            .unwrap();
        let inactive = db
            .add_feed(&acc, "u3", "Archiviert", None, "#333333")
            .unwrap();
        db.conn
            .execute("UPDATE feeds SET active=0 WHERE id=?1", params![inactive])
            .unwrap();
        let due = db.due_feeds(now_ms()).unwrap();
        assert_eq!(due, vec![(local, "u1".to_string())]);
    }

    #[test]
    fn retention_keeps_metadata_and_protects_pending_outbox() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let remote = remote_account(&db);
        let local_feed = db.add_feed(&acc, "u1", "Lokal", None, "#111111").unwrap();
        let remote_feed = db
            .add_feed(&remote, "u2", "Feedly", None, "#222222")
            .unwrap();
        let now = now_ms();
        let old = now - 200 * 86_400_000;
        db.upsert_article(
            local_feed,
            "old-local",
            "Alt lokal",
            None,
            None,
            old,
            "e",
            Some("<p>alt</p>"),
            now,
        )
        .unwrap();
        db.upsert_article(
            remote_feed,
            "old-remote",
            "Alt remote",
            None,
            None,
            old,
            "e",
            Some("<p>alt</p>"),
            now,
        )
        .unwrap();
        db.set_status(local_feed, "old-local", Some(true), None)
            .unwrap();
        db.set_status(remote_feed, "old-remote", Some(true), None)
            .unwrap();
        db.conn
            .execute("UPDATE articles SET first_seen_ms=?2", params![0, old])
            .unwrap();
        db.enqueue_outbox(&remote, "old-remote", "read", true)
            .unwrap();
        let pruned = db.prune_old_read(now, 90).unwrap();
        assert_eq!(pruned, 1, "nur der Artikel ohne ausstehende Mutation");
        assert!(
            db.article_status(local_feed, "old-local")
                .unwrap()
                .is_some(),
            "Metadaten bleiben erhalten"
        );
        let rows = db
            .query_articles(&Scope::Global, Filter::All, None, 10)
            .unwrap();
        let local_row = rows
            .iter()
            .find(|r| r.id == "old-local")
            .expect("Zeile bleibt in der Liste");
        let remote_row = rows
            .iter()
            .find(|r| r.id == "old-remote")
            .expect("Zeile bleibt in der Liste");
        assert!(
            !local_row.has_content,
            "Inhalt des lokalen Altartikels wurde bereinigt"
        );
        assert!(
            remote_row.has_content,
            "Artikel mit ausstehender Mutation bleibt vollständig"
        );
        assert!(db
            .search("alt", &Scope::Global, Filter::All, None, 10)
            .unwrap()
            .iter()
            .any(|r| r.id == "old-remote"));
    }

    #[test]
    fn fts_index_survives_rowid_reuse() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db.add_feed(&acc, "u1", "Feed", None, "#111111").unwrap();
        let now = now_ms();
        db.upsert_article(
            feed,
            "gone",
            "Verschwindet",
            None,
            None,
            now,
            "e",
            Some("<p>oldsecretword</p>"),
            now,
        )
        .unwrap();
        assert_eq!(
            db.search("oldsecretword", &Scope::Global, Filter::All, None, 10)
                .unwrap()
                .len(),
            1
        );
        db.conn
            .execute(
                "DELETE FROM articles WHERE feed_id=?1 AND id='gone'",
                params![feed],
            )
            .unwrap();
        db.conn
            .execute(
                "DELETE FROM article_contents WHERE feed_id=?1 AND article_id='gone'",
                params![feed],
            )
            .unwrap();
        db.upsert_article(
            feed,
            "neu",
            "Neuer Artikel",
            None,
            None,
            now,
            "e",
            Some("<p>anderes wort</p>"),
            now,
        )
        .unwrap();
        assert!(
            db.search("oldsecretword", &Scope::Global, Filter::All, None, 10)
                .unwrap()
                .is_empty(),
            "verwaiste FTS-Zeile darf nicht treffen"
        );
        assert_eq!(
            db.search("anderes", &Scope::Global, Filter::All, None, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn articles_without_html_are_searchable_by_title() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db.add_feed(&acc, "u1", "Feed", None, "#111111").unwrap();
        let now = now_ms();
        db.upsert_article(
            feed,
            "a",
            "Sondertitel Quaternionen",
            Some("Autorin"),
            None,
            now,
            "e",
            None,
            now,
        )
        .unwrap();
        let hits = db
            .search("sondertitel", &Scope::Global, Filter::All, None, 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].has_content, "kein Inhalt, aber Treffer");
    }

    /// Legt eine Bibliothek mit genau dem Schemastand `version` an, wie ein altes Backup.
    fn old_version_db(path: &std::path::Path, version: i64) {
        let conn = Connection::open(path).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_ms INTEGER NOT NULL);",
        )
        .unwrap();
        for (v, sql) in MIGRATIONS {
            if *v <= version {
                conn.execute_batch(sql).unwrap();
                conn.execute(
                    "INSERT INTO schema_version(version, applied_ms) VALUES (?1, ?2)",
                    params![v, 0],
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn restore_kandidat_aelterer_version_wird_vor_der_aktivierung_migriert() {
        let dir = tempdir("restore-migriert");
        let candidate = dir.join("backup.db");
        old_version_db(&candidate, 2);
        let version = Database::prepare_restore_candidate(&candidate).unwrap();
        assert_eq!(version, max_schema_version());
        let db = Database::open(&candidate).unwrap();
        let columns: Vec<String> = db
            .conn
            .prepare("SELECT name FROM pragma_table_info('feeds')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(columns.contains(&"remote_id".to_string()));
        assert!(columns.contains(&"user_title".to_string()));
    }

    #[test]
    fn schema_aenderung_sichert_vorher_konsistent() {
        let dir = tempdir("pre-migrate");
        let path = dir.join("library.db");
        old_version_db(&path, 3);
        {
            let db = Database::open(&path).unwrap();
            db.ensure_local_account().unwrap();
        }
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("pre-migrate"))
            .collect();
        assert_eq!(
            backups.len(),
            1,
            "vor der Migration existiert eine Sicherung: {backups:?}"
        );
        let conn =
            Connection::open_with_flags(&dir.join(&backups[0]), OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_version",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            version, 3,
            "die Sicherung enthält den Stand vor der Änderung"
        );
    }

    #[test]
    fn fremde_datenbank_mit_gleichem_namen_wird_abgewiesen() {
        let dir = tempdir("fremd");
        let path = dir.join("fremd.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE accounts(id TEXT, name TEXT);
                 CREATE TABLE feeds(id TEXT);
                 CREATE TABLE articles(id TEXT);
                 CREATE TABLE schema_version(version INTEGER);",
            )
            .unwrap();
        }
        let err = Database::validate_candidate(&path).unwrap_err();
        assert!(err.to_string().contains("Lesefluss"), "{err}");
    }

    #[test]
    fn kandidat_mit_verletzten_fremdschluesseln_wird_abgewiesen() {
        let dir = tempdir("fk");
        let path = dir.join("kaputt.db");
        let db = Database::open(&path).unwrap();
        db.raw().execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        db.raw()
            .execute(
                "INSERT INTO articles(id, feed_id, title, published_ms, sort_ms, first_seen_ms, updated_ms)
                 VALUES ('verwaist', 9999, 'T', 0, 0, 0, 0)",
                [],
            )
            .unwrap();
        db.raw().execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        let err = Database::validate_candidate(&path).unwrap_err();
        assert!(err.to_string().contains("Fremdschlüssel"), "{err}");
    }

    #[test]
    fn sortierung_ist_je_konto_und_faellt_zurueck() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_account("feedly-1", "feedly", "Feedly").unwrap();
        assert!(db.account_newest_first("feedly-1", true).unwrap());
        assert!(!db.account_newest_first("feedly-1", false).unwrap());
        db.set_account_newest_first("feedly-1", false).unwrap();
        assert!(
            !db.account_newest_first("feedly-1", true).unwrap(),
            "eigene Einstellung des Kontos gewinnt"
        );
        assert!(
            !db.account_newest_first("local", false).unwrap(),
            "andere Konten behalten die Vorgabe"
        );
        assert!(
            db.account_newest_first("local", true).unwrap(),
            "die Vorgabe ist wirklich die Vorgabe"
        );
    }

    #[test]
    fn saved_ids_scoped_to_account() {
        let db = Database::open_in_memory().unwrap();
        let acc = db.ensure_local_account().unwrap();
        db.conn.execute("INSERT INTO accounts(id,kind,name,created_ms) VALUES ('feedly-1','feedly','Feedly',0)", []).unwrap();
        let f_local = db.add_feed(&acc, "u1", "Lokal", None, "#111111").unwrap();
        let f_remote = db
            .add_feed("feedly-1", "u2", "Feedly", None, "#222222")
            .unwrap();
        let now = now_ms();
        db.upsert_article(f_local, "a", "A", None, None, now, "e", None, now)
            .unwrap();
        db.upsert_article(f_remote, "b", "B", None, None, now, "e", None, now)
            .unwrap();
        db.set_saved_by_article_id("a", true).unwrap();
        db.set_saved_by_article_id("b", true).unwrap();
        assert_eq!(
            db.saved_ids_for_account(&acc).unwrap(),
            vec!["a".to_string()]
        );
        assert_eq!(
            db.saved_ids_for_account("feedly-1").unwrap(),
            vec!["b".to_string()]
        );
    }
}
