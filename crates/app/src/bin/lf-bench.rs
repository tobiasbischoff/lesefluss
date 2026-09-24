use std::time::Instant;
use storage::{Database, Filter, NewArticle, Scope};

fn pct(vals: &[u128], p: f64) -> u128 {
    if vals.is_empty() {
        return 0;
    }
    let idx = ((vals.len() as f64 - 1.0) * p).round() as usize;
    vals[idx]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let count: usize = args
        .iter()
        .position(|a| a == "--seed")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    let path = args
        .iter()
        .position(|a| a == "--db")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "/tmp/lf-bench.db".to_string());

    let _ = std::fs::remove_file(&path);
    let db = Database::open(std::path::Path::new(&path)).expect("db open");
    db.ensure_local_account().expect("account");
    let mut feed_ids = Vec::new();
    for f in 0..10 {
        feed_ids.push(
            db.add_feed(
                "local",
                &format!("https://bench.example/{f}.xml"),
                &format!("Benchfeed {f}"),
                None,
                "#888888",
            )
            .expect("feed"),
        );
    }
    let now = storage::now_ms();
    let t0 = Instant::now();
    let mut batch = Vec::new();
    for i in 0..count {
        batch.push(NewArticle {
            id: format!("bench-{i}"),
            title: format!("Artikel {i}: Testwort Szenario {}", i % 97),
            author: Some("Bench Author".into()),
            url: Some(format!("https://bench.example/a/{i}")),
            published_ms: now - (i as i64 % 365) * 86_400_000 - (i as i64 % 86_400) * 1000,
            excerpt: format!("Auszug {i} mit Testwort und etwas Fülltext für die Suche."),
            html: Some(format!("<p>Absatz eins {i} mit <b>Testwort</b>.</p><p>Absatz zwei mit etwas mehr Text, damit die Volltextsuche Materie hat.</p>")),
            content_hash: None,
        });
        if batch.len() == 500 {
            let fid = feed_ids[i % 10];
            db.upsert_articles(fid, &batch, now).expect("upsert");
            batch.clear();
        }
    }
    if !batch.is_empty() {
        let fid = feed_ids[0];
        db.upsert_articles(fid, &batch, now).expect("upsert");
    }
    println!(
        "seed {} Artikel in {:?} ms",
        count,
        t0.elapsed().as_millis()
    );

    let mut list_times = Vec::new();
    for _ in 0..50 {
        let t = Instant::now();
        let rows = db
            .query_articles(&Scope::Global, Filter::Unread, None, 200)
            .expect("query");
        assert!(!rows.is_empty());
        list_times.push(t.elapsed().as_micros());
    }
    list_times.sort_unstable();
    println!(
        "list_unread_200: p50={}µs p95={}µs",
        pct(&list_times, 0.5),
        pct(&list_times, 0.95)
    );

    let mut keyset_times = Vec::new();
    let first = db
        .query_articles(&Scope::Global, Filter::Unread, None, 200)
        .expect("q");
    let last = first.last().expect("last");
    for _ in 0..50 {
        let t = Instant::now();
        let rows = db
            .query_articles(
                &Scope::Global,
                Filter::Unread,
                Some((last.published_ms, &last.id)),
                200,
            )
            .expect("query");
        assert!(!rows.is_empty());
        keyset_times.push(t.elapsed().as_micros());
    }
    keyset_times.sort_unstable();
    println!(
        "list_keyset_200: p50={}µs p95={}µs",
        pct(&keyset_times, 0.5),
        pct(&keyset_times, 0.95)
    );

    let mut search_times = Vec::new();
    for _ in 0..50 {
        let t = Instant::now();
        let rows = db.search("testwort", 100).expect("search");
        assert!(!rows.is_empty());
        search_times.push(t.elapsed().as_micros());
    }
    search_times.sort_unstable();
    println!(
        "search_100: p50={}µs p95={}µs",
        pct(&search_times, 0.5),
        pct(&search_times, 0.95)
    );

    let t = Instant::now();
    let c = db.counts().expect("counts");
    println!(
        "counts: unread={} total={} in {:?} ms",
        c.unread,
        c.total,
        t.elapsed().as_millis()
    );

    let t = Instant::now();
    db.set_status(first[0].feed_id, &first[0].id, Some(true), None)
        .expect("status");
    println!("set_status: {:?} µs", t.elapsed().as_micros());
}
