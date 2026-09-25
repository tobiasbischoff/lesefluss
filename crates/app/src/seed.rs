use std::collections::HashMap;

pub fn seed_fixtures(db: &storage::Database) -> storage::Result<()> {
    let lib = crate::fixtures::build();
    let now = storage::now_ms();
    let mut group_ids: HashMap<String, i64> = HashMap::new();
    for g in &lib.groups {
        let id = db.add_group("local", &g.name, None)?;
        group_ids.insert(g.id.clone(), id);
    }
    let mut feed_ids: HashMap<String, i64> = HashMap::new();
    for f in &lib.feeds {
        let url = f
            .website
            .clone()
            .unwrap_or_else(|| format!("https://example.org/{}.xml", f.id));
        let fid = db.add_feed("local", &url, &f.title, f.website.as_deref(), &f.accent)?;
        let gids: Vec<i64> = f
            .groups
            .iter()
            .filter_map(|g| group_ids.get(g).copied())
            .collect();
        db.set_feed_groups(fid, &gids)?;
        feed_ids.insert(f.id.clone(), fid);
    }
    let mut by_feed: HashMap<String, Vec<storage::NewArticle>> = HashMap::new();
    for a in &lib.articles {
        by_feed
            .entry(a.feed_id.clone())
            .or_default()
            .push(storage::NewArticle {
                id: a.id.clone(),
                title: a.title.clone(),
                author: a.author.clone(),
                url: a.url.clone(),
                published_ms: a.published_at,
                excerpt: a.excerpt.clone(),
                html: lib.contents.get(&a.id).cloned(),
                content_hash: None,
            });
    }
    for (feed_key, items) in by_feed {
        let Some(fid) = feed_ids.get(&feed_key).copied() else {
            continue;
        };
        db.upsert_articles(fid, &items, now)?;
    }
    for a in &lib.articles {
        let Some(fid) = feed_ids.get(&a.feed_id).copied() else {
            continue;
        };
        if !a.unread || a.saved {
            db.set_status(fid, &a.id, Some(!a.unread), Some(a.saved))?;
        }
    }
    db.set_last_sync("local", now)?;
    Ok(())
}
