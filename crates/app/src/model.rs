use crate::state::day_key_label;
use std::collections::{HashMap, HashSet};
use storage::{ArticleRow, Counts, FeedRow, GroupRow, Filter, Scope};

#[derive(Clone, Debug)]
pub enum ListRow {
    Header { key: String, label: String },
    Item(ArticleRow),
}

pub struct UiState {
    pub scope: Scope,
    pub filter: Filter,
    pub search: Option<String>,
    pub rows: Vec<ListRow>,
    pub feeds: Vec<FeedRow>,
    pub groups: Vec<GroupRow>,
    pub counts: Counts,
    pub selected: Option<(i64, String)>,
    pub collapsed: HashSet<i64>,
    pub last_sync: Option<i64>,
    pub cursor: Option<(i64, String)>,
    pub loading_more: bool,
    pub unread_guard: HashSet<String>,
    pub last_opened: HashMap<(Scope, Filter), (i64, String)>,
    pub fetching: HashSet<i64>,
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
            counts: Counts::default(),
            selected: None,
            collapsed: HashSet::new(),
            last_sync: None,
            cursor: None,
            loading_more: false,
            unread_guard: HashSet::new(),
            last_opened: HashMap::new(),
            fetching: HashSet::new(),
        }
    }
}

impl UiState {
    pub fn effective_scope(&self) -> Option<Scope> {
        if self.search.is_some() {
            None
        } else {
            Some(self.scope.clone())
        }
    }

    pub fn build_rows(&mut self, articles: Vec<ArticleRow>) {
        self.rows.clear();
        let mut last_day: Option<String> = None;
        for a in articles {
            let (key, label) = day_key_label(a.published_ms);
            if last_day.as_deref() != Some(key.as_str()) {
                self.rows.push(ListRow::Header { key: key.clone(), label });
                last_day = Some(key);
            }
            self.rows.push(ListRow::Item(a));
        }
        self.cursor = self.last_item().map(|a| (a.published_ms, a.id.clone()));
    }

    pub fn append_rows(&mut self, articles: Vec<ArticleRow>) {
        let mut last_day = self.rows.iter().rev().find_map(|r| match r {
            ListRow::Header { key, .. } => Some(key.clone()),
            _ => None,
        });
        for a in articles {
            let (key, label) = day_key_label(a.published_ms);
            if last_day.as_deref() != Some(key.as_str()) {
                self.rows.push(ListRow::Header { key: key.clone(), label });
                last_day = Some(key);
            }
            self.rows.push(ListRow::Item(a));
        }
        self.cursor = self.last_item().map(|a| (a.published_ms, a.id.clone()));
    }

    pub fn last_item(&self) -> Option<&ArticleRow> {
        self.rows.iter().rev().find_map(|r| match r {
            ListRow::Item(a) => Some(a),
            _ => None,
        })
    }

    pub fn row_pos(&self, id: &str) -> Option<usize> {
        self.rows.iter().position(|r| match r {
            ListRow::Item(a) => a.id == id,
            _ => false,
        })
    }

    pub fn article(&self, id: &str) -> Option<&ArticleRow> {
        self.rows.iter().find_map(|r| match r {
            ListRow::Item(a) if a.id == id => Some(a),
            _ => None,
        })
    }

    pub fn article_mut(&mut self, id: &str) -> Option<&mut ArticleRow> {
        self.rows.iter_mut().find_map(|r| match r {
            ListRow::Item(a) if a.id == id => Some(a),
            _ => None,
        })
    }

    pub fn feed_unread(&self, feed_id: i64) -> i64 {
        self.counts.per_feed.iter().find(|(f, _)| *f == feed_id).map(|(_, c)| *c).unwrap_or(0)
    }

    pub fn group_unread(&self, group_id: i64) -> i64 {
        self.counts.per_group.iter().find(|(g, _)| *g == group_id).map(|(_, c)| *c).unwrap_or(0)
    }

    pub fn feed_title(&self, feed_id: i64) -> String {
        self.feeds.iter().find(|f| f.id == feed_id).map(|f| f.title.clone()).unwrap_or_else(|| "Feed".into())
    }
}
