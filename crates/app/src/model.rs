pub use crate::list::ListRow;
use crate::list::RowCell;
use crate::state::day_key_label;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use storage::{ArticleRow, Counts, FeedRow, Filter, GroupRow, Scope};

pub struct UiState {
    pub scope: Scope,
    pub filter: Filter,
    pub search: Option<String>,
    pub rows: Vec<ListRow>,
    pub feeds: Vec<FeedRow>,
    pub groups: Vec<GroupRow>,
    pub accounts: Vec<(String, String, String)>,
    /// Koordiniert alle Feedly-Zyklen pro Konto (Erst-Sync, Delta, Outbox, Aktionen).
    pub coordinator: sync_engine::SyncCoordinator,
    /// Aktuell laufender Netzauftrag; ältere Ereignisse werden verworfen.
    pub active_feedly_run: Option<crate::window::FeedlyRun>,
    /// Ziel-Feeds einer wartenden Serveraktion.
    pub pending_feedly_params: Option<(String, crate::window::FeedlyParams)>,
    pub next_feedly_sync: i64,
    pub feedly_last_sync: i64,
    pub counts: Counts,
    pub selected: Option<(i64, String)>,
    pub collapsed: HashSet<i64>,
    pub last_sync: Option<i64>,
    /// Keyset-Cursor: (sort_ms, feed_id, id) ist total eindeutig.
    pub cursor: Option<(i64, i64, String)>,
    pub loading_more: bool,
    pub unread_guard: HashSet<String>,
    pub last_opened: HashMap<(Scope, Filter), (i64, String)>,
    pub fetching: HashSet<i64>,
    pub menu_source: Option<storage::Scope>,
    pub pending_new_articles: usize,
    pub feedly_status: Option<(String, Option<String>)>,
    pub feedly_sync_running: bool,
    pub feedly_sync_queued: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            scope: Scope::Global,
            filter: Filter::Unread,
            search: None,
            rows: Vec::new(),
            feeds: Vec::new(),
            groups: Vec::new(),
            accounts: Vec::new(),
            coordinator: sync_engine::SyncCoordinator::new(),
            active_feedly_run: None,
            pending_feedly_params: None,
            next_feedly_sync: 0,
            feedly_last_sync: 0,
            counts: Counts::default(),
            selected: None,
            collapsed: HashSet::new(),
            last_sync: None,
            cursor: None,
            loading_more: false,
            unread_guard: HashSet::new(),
            last_opened: HashMap::new(),
            fetching: HashSet::new(),
            menu_source: None,
            pending_new_articles: 0,
            feedly_status: None,
            feedly_sync_running: false,
            feedly_sync_queued: false,
        }
    }
}

/// Logischer Schlüssel eines Artikels: Feed plus ID, nicht die ID allein.
pub type ArticleKey = (i64, String);

pub fn cursor_of(a: ArticleRow) -> (i64, i64, String) {
    (a.sort_ms, a.feed_id, a.id)
}

impl UiState {
    pub fn effective_scope(&self) -> Option<Scope> {
        if self.search.is_some() {
            None
        } else {
            Some(self.scope.clone())
        }
    }

    fn push_rows(&mut self, articles: Vec<ArticleRow>) {
        let mut last_day = self.rows.iter().rev().find_map(|r| match r {
            ListRow::Header { key, .. } => Some(key.clone()),
            _ => None,
        });
        for a in articles {
            let (key, label) = day_key_label(a.published_ms);
            if last_day.as_deref() != Some(key.as_str()) {
                self.rows.push(ListRow::Header {
                    key: key.clone(),
                    label,
                });
                last_day = Some(key);
            }
            self.rows.push(ListRow::Item(Rc::new(RowCell::new(a))));
        }
    }

    pub fn build_rows(&mut self, articles: Vec<ArticleRow>) {
        self.rows.clear();
        self.push_rows(articles);
        self.cursor = self.last_item().map(cursor_of);
    }

    pub fn append_rows(&mut self, articles: Vec<ArticleRow>) {
        self.push_rows(articles);
        self.cursor = self.last_item().map(cursor_of);
    }

    /// Markiert das Ende der Liste: kein Cursor, kein Nachladen.
    pub fn mark_end(&mut self) {
        self.cursor = None;
    }

    pub fn last_item(&self) -> Option<ArticleRow> {
        self.rows.iter().rev().find_map(|r| r.article())
    }

    /// Position über den logischen Schlüssel (Feed + Artikel-ID). Gleiche GUIDs in
    /// verschiedenen Feeds sind damit unterscheidbar.
    pub fn row_pos(&self, feed_id: i64, id: &str) -> Option<usize> {
        self.rows.iter().position(|r| {
            r.article()
                .map(|a| a.feed_id == feed_id && a.id == id)
                .unwrap_or(false)
        })
    }

    pub fn article(&self, feed_id: i64, id: &str) -> Option<ArticleRow> {
        self.rows.iter().find_map(|r| {
            let a = r.article()?;
            if a.feed_id == feed_id && a.id == id {
                Some(a)
            } else {
                None
            }
        })
    }

    /// Fallback für Stellen, die nur die ID kennen (Reader-Callbacks). Eine
    /// `feed_id` von 0 bedeutet: die ID wird in der geladenen Liste eindeutig
    /// gesucht; Mehrdeutigkeit wird zugunsten der ersten sichtbaren Zeile
    /// aufgelöst und protokolliert.
    pub fn article_by_id(&self, id: &str, feed_id: i64) -> Option<ArticleRow> {
        if feed_id != 0 {
            if let Some(row) = self.article(feed_id, id) {
                return Some(row);
            }
        }
        let mut found: Option<ArticleRow> = None;
        for r in &self.rows {
            let Some(a) = r.article() else { continue };
            if a.id == id {
                if found.is_some() {
                    return found;
                }
                found = Some(a);
            }
        }
        found
    }

    pub fn set_article(&self, feed_id: i64, id: &str, new: ArticleRow) {
        for r in &self.rows {
            if let ListRow::Item(cell) = r {
                let matches = {
                    let current = cell.data.borrow();
                    current.feed_id == feed_id && current.id == id
                };
                if matches {
                    cell.update(new);
                    return;
                }
            }
        }
    }

    pub fn last_sync_for_feedly(&self) -> i64 {
        self.last_sync
            .unwrap_or(storage::now_ms() - 30 * 86_400_000)
    }

    pub fn feed_unread(&self, feed_id: i64) -> i64 {
        self.counts
            .per_feed
            .iter()
            .find(|(f, _)| *f == feed_id)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    pub fn group_unread(&self, group_id: i64) -> i64 {
        self.counts
            .per_group
            .iter()
            .find(|(g, _)| *g == group_id)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    pub fn feed_title(&self, feed_id: i64) -> String {
        self.feeds
            .iter()
            .find(|f| f.id == feed_id)
            .map(|f| f.title.clone())
            .unwrap_or_else(|| "Feed".into())
    }
}
