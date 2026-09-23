use rusqlite::{params, Connection, OptionalExtension};
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
}

#[derive(Clone, Debug)]
pub struct ArticleRow {
    pub id: String,
    pub feed_id: i64,
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

#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub unread: i64,
    pub saved: i64,
    pub total: i64,
    pub per_feed: Vec<(i64, i64)>,
    pub per_group: Vec<(i64, i64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Unread,
    All,
    Saved,
    Feed(i64),
    Group(i64),
    Search(String),
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

const MIGRATIONS: &[(i64, &str)] = &[(
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
)];

pub struct Database {
    conn: Connection,
}

fn clamp_future(ms: i64, now: i64) -> i64 {
    ms.min(now)
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
        db.migrate()?;
        Ok(db)
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
        let current: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(version),0) FROM schema_version", [], |r| r.get(0))?;
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

    pub fn ensure_local_account(&self) -> Result<String> {
        let id = "local".to_string();
        self.conn.execute(
            "INSERT INTO accounts(id, kind, name, created_ms) VALUES (?1,'local','Lokale Bibliothek',?2)
             ON CONFLICT(id) DO NOTHING",
            params![id, now_ms()],
        )?;
        Ok(id)
    }

    pub fn add_feed(&self, account_id: &str, feed_url: &str, title: &str, website: Option<&str>, accent: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO feeds(account_id, feed_url, title, website, accent, added_ms)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![account_id, feed_url, title, website, accent, now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_feed_title(&self, feed_id: i64, title: &str, website: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET title=?2, website=COALESCE(?3, website) WHERE id=?1",
            params![feed_id, title, website],
        )?;
        Ok(())
    }

    pub fn list_feeds(&self) -> Result<Vec<FeedRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_url, title, website, accent FROM feeds ORDER BY lower(title)",
        )?;
        let mut feeds: Vec<FeedRow> = stmt
            .query_map([], |r| {
                Ok(FeedRow {
                    id: r.get(0)?,
                    feed_url: r.get(1)?,
                    title: r.get(2)?,
                    website: r.get(3)?,
                    accent: r.get(4)?,
                    groups: Vec::new(),
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        let mut gs = self.conn.prepare("SELECT feed_id, group_id FROM feed_groups")?;
        let pairs: Vec<(i64, i64)> = gs
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        for f in feeds.iter_mut() {
            f.groups = pairs.iter().filter(|(fid, _)| *fid == f.id).map(|(_, g)| *g).collect();
        }
        Ok(feeds)
    }

    pub fn add_group(&self, account_id: &str, name: &str, parent_id: Option<i64>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO groups(account_id, name, parent_id) VALUES (?1,?2,?3)",
            params![account_id, name, parent_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_groups(&self) -> Result<Vec<GroupRow>> {
        let mut stmt = self.conn.prepare("SELECT id, name, parent_id FROM groups ORDER BY name")?;
        let rows = stmt
            .query_map([], |r| Ok(GroupRow { id: r.get(0)?, name: r.get(1)?, parent_id: r.get(2)? }))?
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
            .query_row("SELECT id FROM feeds WHERE feed_url=?1", params![url], |r| r.get(0))
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
                params![feed_id, id, title, author, url, published_ms, sort, excerpt, now],
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
            let feed_title: String = self
                .conn
                .query_row("SELECT title FROM feeds WHERE id=?1", params![feed_id], |r| r.get(0))?;
            let rowid: i64 = self.conn.query_row(
                "SELECT rowid FROM articles WHERE feed_id=?1 AND id=?2",
                params![feed_id, id],
                |r| r.get(0),
            )?;
            self.conn.execute("DELETE FROM article_fts WHERE rowid=?1", params![rowid])?;
            self.conn.execute(
                "INSERT INTO article_fts(rowid, title, author, feed_title, body) VALUES (?1,?2,?3,?4,?5)",
                params![rowid, title, author.unwrap_or(""), feed_title, plain],
            )?;
        }
        Ok(!exists)
    }

    pub fn upsert_articles(&self, feed_id: i64, items: &[NewArticle], now: i64) -> Result<(usize, usize)> {
        let tx = self.conn.unchecked_transaction()?;
        let (mut added, mut updated) = (0usize, 0usize);
        let feed_title: String = tx.query_row("SELECT title FROM feeds WHERE id=?1", params![feed_id], |r| r.get(0))?;
        for item in items {
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
            if let Some(html) = &item.html {
                let plain = strip_html(html);
                tx.execute(
                    "INSERT INTO article_contents(feed_id, article_id, html, plain, fetched_ms)
                     VALUES (?1,?2,?3,?4,?5)
                     ON CONFLICT(feed_id, article_id) DO UPDATE SET html=excluded.html, plain=excluded.plain, fetched_ms=excluded.fetched_ms",
                    params![feed_id, item.id, html, plain, now],
                )?;
                let rowid: i64 = tx.query_row(
                    "SELECT rowid FROM articles WHERE feed_id=?1 AND id=?2",
                    params![feed_id, item.id],
                    |r| r.get(0),
                )?;
                tx.execute("DELETE FROM article_fts WHERE rowid=?1", params![rowid])?;
                tx.execute(
                    "INSERT INTO article_fts(rowid, title, author, feed_title, body) VALUES (?1,?2,?3,?4,?5)",
                    params![rowid, item.title, item.author.clone().unwrap_or_default(), feed_title, plain],
                )?;
            }
        }
        tx.commit()?;
        Ok((added, updated))
    }

    pub fn set_status(&self, feed_id: i64, id: &str, read: Option<bool>, saved: Option<bool>) -> Result<()> {
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
        let sql = format!("UPDATE articles SET {} WHERE feed_id=? AND id=?", sets.join(","));
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
        source: &Source,
        before: Option<(i64, &str)>,
        limit: u32,
    ) -> Result<Vec<ArticleRow>> {
        match source {
            Source::Search(q) => return self.search(q, limit),
            _ => {}
        }
        let mut sql = String::from(
            "SELECT a.id, a.feed_id, f.title, f.accent, a.title, a.author, a.url, a.published_ms,
                    a.excerpt, a.unread, a.saved,
                    EXISTS(SELECT 1 FROM article_contents c WHERE c.feed_id=a.feed_id AND c.article_id=a.id)
             FROM articles a JOIN feeds f ON f.id=a.feed_id
             JOIN feed_groups fg ON fg.feed_id=a.feed_id",
        );
        let mut where_clauses: Vec<String> = Vec::new();
        let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        match source {
            Source::Unread => where_clauses.push("a.unread=1".into()),
            Source::Saved => where_clauses.push("a.saved=1".into()),
            Source::All => {}
            Source::Feed(id) => {
                where_clauses.push("a.feed_id=?".into());
                args.push(Box::new(*id));
            }
            Source::Group(_) | Source::Search(_) => {}
        }
        let join_groups = matches!(source, Source::Group(_));
        if !join_groups {
            sql = sql.replace(" JOIN feed_groups fg ON fg.feed_id=a.feed_id", "");
        } else if let Source::Group(g) = source {
            where_clauses.push("fg.group_id=?".into());
            args.push(Box::new(*g));
        }
        if let Some((sort_ms, id)) = before {
            where_clauses.push("(a.sort_ms < ? OR (a.sort_ms = ? AND a.id < ?))".into());
            args.push(Box::new(sort_ms));
            args.push(Box::new(sort_ms));
            args.push(Box::new(id.to_string()));
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        if join_groups {
            sql.push_str(" GROUP BY a.id, a.feed_id");
        }
        sql.push_str(" ORDER BY a.sort_ms DESC, a.id DESC LIMIT ?");
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
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<ArticleRow>> {
        let fts = build_fts_query(query);
        if fts.is_empty() {
            return Ok(Vec::new());
        }
        let sql = "SELECT a.id, a.feed_id, f.title, f.accent, a.title, a.author, a.url, a.published_ms,
                          a.excerpt, a.unread, a.saved, 1
                   FROM article_fts fts
                   JOIN articles a ON a.rowid = fts.rowid
                   JOIN feeds f ON f.id=a.feed_id
                   WHERE article_fts MATCH ?1
                   ORDER BY rank
                   LIMIT ?2";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![fts, limit as i64], |r| {
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
                    has_content: true,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn counts(&self) -> Result<Counts> {
        let mut c = Counts::default();
        c.unread = self.conn.query_row("SELECT COUNT(*) FROM articles WHERE unread=1", [], |r| r.get(0))?;
        c.saved = self.conn.query_row("SELECT COUNT(*) FROM articles WHERE saved=1", [], |r| r.get(0))?;
        c.total = self.conn.query_row("SELECT COUNT(*) FROM articles", [], |r| r.get(0))?;
        let mut stmt = self.conn.prepare("SELECT feed_id, COUNT(*) FROM articles WHERE unread=1 GROUP BY feed_id")?;
        c.per_feed = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        let mut stmt = self.conn.prepare(
            "SELECT fg.group_id, COUNT(DISTINCT a.id) FROM articles a
             JOIN feed_groups fg ON fg.feed_id=a.feed_id WHERE a.unread=1 GROUP BY fg.group_id",
        )?;
        c.per_group = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        Ok(c)
    }

    pub fn mark_source_read(&self, source: &Source, ids: &[(i64, String)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (feed_id, id) in ids {
            tx.execute(
                "UPDATE articles SET unread=0 WHERE feed_id=?1 AND id=?2",
                params![feed_id, id],
            )?;
        }
        let _ = source;
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
             WHERE s.next_fetch_ms IS NULL OR s.next_fetch_ms <= ?1
             ORDER BY COALESCE(s.next_fetch_ms, 0)",
        )?;
        let rows = stmt
            .query_map(params![now], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        Ok(rows)
    }

    pub fn update_fetch_error(&self, feed_id: i64, error_count: i64, last_error: &str, next_fetch_ms: i64, last_fetch_ms: i64) -> Result<()> {
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

    fn seed(db: &Database) -> i64 {
        db.ensure_local_account().unwrap();
        db.add_feed("local", "https://example.com/feed.xml", "Example", Some("https://example.com"), "#123456")
            .unwrap()
    }

    #[test]
    fn migration_and_upsert_preserve_status() {
        let db = Database::open_in_memory().unwrap();
        let feed = seed(&db);
        let now = now_ms();
        db.upsert_article(feed, "a1", "Titel eins", None, None, now - 1000, "Auszug", Some("<p>Hallo Welt</p>"), now)
            .unwrap();
        db.set_status(feed, "a1", Some(true), Some(true)).unwrap();
        db.upsert_article(feed, "a1", "Titel eins (update)", None, None, now - 1000, "Auszug neu", Some("<p>Hallo Welt 2</p>"), now + 5)
            .unwrap();
        let (unread, saved) = db.article_status(feed, "a1").unwrap().unwrap();
        assert!(!unread, "Re-Import darf nicht ungelesen machen");
        assert!(saved, "Re-Import darf Speicherstatus nicht verlieren");
        let rows = db.query_articles(&Source::All, None, 10).unwrap();
        assert_eq!(rows[0].title, "Titel eins (update)");
    }

    #[test]
    fn keyset_pagination_and_filters() {
        let db = Database::open_in_memory().unwrap();
        let feed = seed(&db);
        let now = now_ms();
        for i in 0..25 {
            db.upsert_article(feed, &format!("a{i}"), &format!("T{i}"), None, None, now - i * 1000, "x", None, now)
                .unwrap();
        }
        let page1 = db.query_articles(&Source::All, None, 10).unwrap();
        assert_eq!(page1.len(), 10);
        let last = page1.last().unwrap();
        let page2 = db
            .query_articles(&Source::All, Some((last.published_ms, &last.id)), 10)
            .unwrap();
        assert_eq!(page2.len(), 10);
        assert!(!page1.iter().any(|r| page2.iter().any(|s| s.id == r.id)));
        db.set_status(feed, "a3", Some(true), None).unwrap();
        let unread = db.query_articles(&Source::Unread, None, 100).unwrap();
        assert_eq!(unread.len(), 24);
        assert!(!unread.iter().any(|r| r.id == "a3"));
    }

    #[test]
    fn fts_search_unicode() {
        let db = Database::open_in_memory().unwrap();
        let feed = seed(&db);
        let now = now_ms();
        db.upsert_article(feed, "s1", "Grüße aus München", None, None, now, "Straße und Café", Some("<p>Straße und <b>Café</b></p>"), now).unwrap();
        db.upsert_article(feed, "s2", "Other", None, None, now, "nothing", Some("<p>nothing</p>"), now).unwrap();
        let hits = db.search("cafe", 10).unwrap();
        assert_eq!(hits.len(), 1, "remove_diacritics sollte Café finden");
        let hits = db.search("münchen", 10).unwrap();
        assert_eq!(hits.len(), 1);
        let hits = db.search("\"\"", 10).unwrap();
        assert!(hits.is_empty());
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
        db.upsert_article(f1, "x", "X", None, None, now, "e", None, now).unwrap();
        db.upsert_article(f2, "y", "Y", None, None, now, "e", None, now).unwrap();
        let c = db.counts().unwrap();
        assert_eq!(c.per_group, vec![(g, 2)]);
        let rows = db.query_articles(&Source::Group(g), None, 10).unwrap();
        assert_eq!(rows.len(), 2);
    }
}
