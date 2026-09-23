use crate::fixtures::Library;
use domain::{ArticleId, ArticleMeta, Feed, FeedId, GroupId};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SourceFilter {
    Unread,
    All,
    Saved,
    Group(GroupId),
    Feed(FeedId),
}

#[derive(Clone, Debug)]
pub enum ListRow {
    Header {
        #[allow(dead_code)]
        key: String,
        label: String,
    },
    Item { id: ArticleId },
}

pub fn feed_of<'a>(lib: &'a Library, id: &FeedId) -> Option<&'a Feed> {
    lib.feeds.iter().find(|f| &f.id == id)
}

pub fn article_of<'a>(lib: &'a Library, id: &str) -> Option<&'a ArticleMeta> {
    lib.articles.iter().find(|a| a.id == id)
}

pub fn article_mut<'a>(lib: &'a mut Library, id: &str) -> Option<&'a mut ArticleMeta> {
    lib.articles.iter_mut().find(|a| a.id == id)
}

pub fn feeds_in_group<'a>(lib: &'a Library, group_id: &GroupId) -> Vec<&'a Feed> {
    lib.feeds.iter().filter(|f| f.groups.iter().any(|g| g == group_id)).collect()
}

pub fn matches(lib: &Library, a: &ArticleMeta, filter: &SourceFilter) -> bool {
    match filter {
        SourceFilter::All => true,
        SourceFilter::Unread => a.unread,
        SourceFilter::Saved => a.saved,
        SourceFilter::Feed(id) => &a.feed_id == id,
        SourceFilter::Group(gid) => feeds_in_group(lib, gid).iter().any(|f| &f.id == &a.feed_id),
    }
}

pub fn visible_articles(lib: &Library, filter: &SourceFilter, now_ms: i64) -> Vec<ArticleId> {
    let mut arts: Vec<&ArticleMeta> =
        lib.articles.iter().filter(|a| matches(lib, a, filter)).collect();
    arts.sort_by(|a, b| {
        let ka = a.published_at.min(now_ms);
        let kb = b.published_at.min(now_ms);
        kb.cmp(&ka).then(a.id.cmp(&b.id))
    });
    arts.into_iter().map(|a| a.id.clone()).collect()
}

pub fn day_key_label(ms: i64) -> (String, String) {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000))
        .unwrap_or_else(|_| glib::DateTime::now_local().expect("lokale Zeitzone"));
    let key = dt.format("%Y-%m-%d").map(|s| s.to_string()).unwrap_or_default();
    let now = glib::DateTime::now_local().expect("lokale Zeitzone");
    let today = now.format("%Y-%m-%d").map(|s| s.to_string()).unwrap_or_default();
    let yest = now
        .add_days(-1)
        .ok()
        .and_then(|d| d.format("%Y-%m-%d").ok())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let label = if key == today {
        "Heute".to_string()
    } else if key == yest {
        "Gestern".to_string()
    } else {
        dt.format("%A, %-d. %B").map(|s| s.to_string()).unwrap_or(key.clone())
    };
    (key, label)
}

pub fn rows_for(lib: &Library, filter: &SourceFilter, now_ms: i64) -> Vec<ListRow> {
    let mut rows = Vec::new();
    let mut last_day: Option<String> = None;
    for id in visible_articles(lib, filter, now_ms) {
        let Some(a) = article_of(lib, &id) else { continue };
        let (key, label) = day_key_label(a.published_at.min(now_ms));
        if last_day.as_deref() != Some(key.as_str()) {
            rows.push(ListRow::Header { key: key.clone(), label });
            last_day = Some(key);
        }
        rows.push(ListRow::Item { id });
    }
    rows
}

pub fn fmt_time(ms: i64) -> String {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000)).ok();
    dt.and_then(|d| d.format("%H:%M").map(|s| s.to_string()).ok()).unwrap_or_default()
}

pub fn fmt_full(ms: i64) -> String {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000)).ok();
    dt.and_then(|d| d.format("%A, %-d. %B %Y, %H:%M").map(|s| s.to_string()).ok()).unwrap_or_default()
}

pub fn unread_total(lib: &Library) -> usize {
    lib.articles.iter().filter(|a| a.unread).count()
}

pub fn saved_total(lib: &Library) -> usize {
    lib.articles.iter().filter(|a| a.saved).count()
}

pub fn unread_in_feed(lib: &Library, feed_id: &FeedId) -> usize {
    lib.articles.iter().filter(|a| a.unread && &a.feed_id == feed_id).count()
}

pub fn unread_in_group(lib: &Library, group_id: &GroupId) -> usize {
    let feeds: HashSet<&str> =
        feeds_in_group(lib, group_id).iter().map(|f| f.id.as_str()).collect();
    lib.articles
        .iter()
        .filter(|a| a.unread && feeds.contains(a.feed_id.as_str()))
        .count()
}

pub fn source_label(lib: &Library, filter: &SourceFilter) -> String {
    match filter {
        SourceFilter::Unread => "Ungelesen".into(),
        SourceFilter::All => "Alle Artikel".into(),
        SourceFilter::Saved => "Gespeichert".into(),
        SourceFilter::Group(g) => lib
            .groups
            .iter()
            .find(|x| &x.id == g)
            .map(|x| x.name.clone())
            .unwrap_or_else(|| "Gruppe".into()),
        SourceFilter::Feed(f) => {
            feed_of(lib, f).map(|x| x.title.clone()).unwrap_or_else(|| "Feed".into())
        }
    }
}
