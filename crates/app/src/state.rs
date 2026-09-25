pub fn day_key_label(ms: i64) -> (String, String) {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000))
        .unwrap_or_else(|_| glib::DateTime::now_local().expect("lokale Zeitzone"));
    let key = dt
        .format("%Y-%m-%d")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let now = glib::DateTime::now_local().expect("lokale Zeitzone");
    let today = now
        .format("%Y-%m-%d")
        .map(|s| s.to_string())
        .unwrap_or_default();
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
        dt.format("%A, %-d. %B")
            .map(|s| s.to_string())
            .unwrap_or(key.clone())
    };
    (key, label)
}

pub fn fmt_time(ms: i64) -> String {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000)).ok();
    dt.and_then(|d| d.format("%H:%M").map(|s| s.to_string()).ok())
        .unwrap_or_default()
}

pub fn fmt_full(ms: i64) -> String {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000)).ok();
    dt.and_then(|d| {
        d.format("%A, %-d. %B %Y, %H:%M")
            .map(|s| s.to_string())
            .ok()
    })
    .unwrap_or_default()
}
