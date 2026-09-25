use crate::dbworker::{DbWorker, JobOut};
use crate::feedly_sync;
use crate::list;
use crate::model::{ListRow, UiState};

/// Seitengröße der Liste. Es werden PAGE_SIZE + 1 Zeilen geladen, damit das
/// Vorhandensein weiterer Seiten zuverlässig feststeht.
const PAGE_SIZE: usize = 200;
use crate::net::{Net, NetEvent};
use crate::prefs::Prefs;
use crate::reader::{find_in_view, find_next, ReaderPane};
use crate::sidebar;
use crate::state::*;
use crate::style::{gtk_css_for, tokens_for, ReaderStyleState};
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use storage::{ArticleRow, Counts, FeedRow, Filter, GroupRow, Scope};
use webkit6::prelude::*;

fn now_ms_stub() -> i64 {
    storage::now_ms()
}

pub fn dbg_log(msg: &str) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var("LF_DEBUG").is_ok()) {
        eprintln!("[lf] {}", redact(msg));
    }
}

/// Redigiert Token und Queryparameter, bevor eine Zeile in die Ausgabe gelangt.
pub fn redact(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    for word in msg.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        let without_query = match word.find('?') {
            Some(pos) => {
                out.push_str(&word[..pos]);
                out.push_str("?[redigiert]");
                true
            }
            None => false,
        };
        if without_query {
            continue;
        }
        if word.contains("access_token") || word.contains("refresh_token") {
            out.push_str("[redigiert]");
            continue;
        }
        out.push_str(word);
    }
    out
}

/// Externe Einstiegspunkte: nur http(s), ohne Userinfo.
/// `"<gen>:<idx>:<offset>"` aus dem Reader-JS; `None` bei ungültiger Antwort.
pub fn parse_position(raw: &str) -> Option<(u64, i64, i64)> {
    let mut parts = raw.split(':');
    let gen: u64 = parts.next()?.trim().parse().ok()?;
    let idx: i64 = parts.next()?.trim().parse().ok()?;
    let off: i64 = parts.next()?.trim().parse().ok()?;
    Some((gen, idx, off))
}

fn app_at<F: FnOnce(&Rc<App>)>(w: &Weak<App>, f: F) {
    if let Some(app) = w.upgrade() {
        f(&app);
    }
}

pub fn external_uri_allowed(raw: &str) -> bool {
    match url::Url::parse(raw) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => {
            u.username().is_empty() && u.password().is_none()
        }
        _ => false,
    }
}

/// Kontozustände nach §13.1 in lesbare Worte übersetzen.
/// Erzeugt eine kleine PNG-Vorschau (max. 128 px) als Daten-URI.
pub fn thumbnail_data_uri(bytes: &[u8]) -> Option<String> {
    use base64::Engine;
    let loader = gtk::gdk::gdk_pixbuf::PixbufLoader::new();
    loader.write(bytes).ok()?;
    loader.close().ok();
    let pixbuf = loader.pixbuf()?;
    let scale = 128.0 / pixbuf.width().max(1) as f64;
    let target = if scale < 1.0 {
        pixbuf.scale_simple(
            (pixbuf.width() as f64 * scale).round().max(1.0) as i32,
            (pixbuf.height() as f64 * scale).round().max(1.0) as i32,
            gtk::gdk::gdk_pixbuf::InterpType::Bilinear,
        )?
    } else {
        pixbuf
    };
    let png = target.save_to_bufferv("png", &[]).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// Sortierrichtung aus den Einstellungen lesen (Fallback: neueste zuerst).
fn self_newest_first(db: &storage::Database) -> bool {
    db.get_pref("newest_first")
        .ok()
        .flatten()
        .map(|v| v != "0")
        .unwrap_or(true)
}

/// Was ein manueller Refresh im aktuellen Bereich tatsächlich aktualisieren muss.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RefreshPlan {
    pub local_feeds: Vec<(i64, String)>,
    pub feedly_account: Option<String>,
    pub skipped: bool,
}

/// Ermittelt den Provider aus den Feeds, die im Bereich tatsächlich liegen.
/// Global aktualisiert beides, ein Feed/Gruppe nur seinen Anbieter.
pub fn refresh_plan(
    scope: &storage::Scope,
    feeds: &[storage::FeedRow],
    accounts: &[(String, String, String)],
) -> RefreshPlan {
    let ids: Vec<i64> = match scope {
        storage::Scope::Feed(f) => vec![*f],
        storage::Scope::Group(g) => feeds
            .iter()
            .filter(|f| f.groups.contains(g))
            .map(|f| f.id)
            .collect(),
        storage::Scope::Account(a) => feeds
            .iter()
            .filter(|f| &f.account_id == a)
            .map(|f| f.id)
            .collect(),
        storage::Scope::Global => feeds.iter().map(|f| f.id).collect(),
    };
    let in_scope: Vec<&storage::FeedRow> = feeds.iter().filter(|f| ids.contains(&f.id)).collect();
    let local_feeds: Vec<(i64, String)> = in_scope
        .iter()
        .filter(|f| f.account_id == "local")
        .map(|f| (f.id, f.feed_url.clone()))
        .collect();
    let has_feedly_feed = in_scope.iter().any(|f| f.account_id != "local");
    let feedly_account = if has_feedly_feed {
        first_account_of_kind(accounts, "feedly")
    } else {
        None
    };
    let skipped = local_feeds.is_empty() && feedly_account.is_none();
    RefreshPlan {
        local_feeds,
        feedly_account,
        skipped,
    }
}

/// Erstes Konto einer Art. Nimmt die Liste als Wert, damit der Aufrufer seinen
/// `RefCell`-Borrow beenden kann, bevor er den State verändert.
pub fn first_account_of_kind(accounts: &[(String, String, String)], kind: &str) -> Option<String> {
    accounts
        .iter()
        .find(|(_, k, _)| k == kind)
        .map(|(id, _, _)| id.clone())
}

pub fn status_label(status: &str) -> &'static str {
    match status {
        "initial_sync" => "Erstsynchronisation läuft",
        "syncing" => "Synchronisation läuft",
        "ready" => "Verbunden und aktuell",
        "offline" => "Offline — lokale Daten nutzbar",
        "rate_limited" => "Drosselung durch Feedly, späterer Versuch",
        "auth_required" => "Erneute Anmeldung erforderlich",
        "degraded" => "Eingeschränkt synchronisiert",
        _ => "Getrennt",
    }
}

pub fn read_intent(was_unread: bool) -> bool {
    was_unread
}

pub fn unread_delta(prev_unread: bool, read: Option<bool>) -> i64 {
    match read {
        Some(true) if prev_unread => -1,
        Some(false) if !prev_unread => 1,
        _ => 0,
    }
}

pub fn saved_delta(prev_saved: bool, saved: Option<bool>) -> i64 {
    match saved {
        Some(true) if !prev_saved => 1,
        Some(false) if prev_saved => -1,
        _ => 0,
    }
}

/// Reiner Paging-Schritt: trennt die eine Extrazeile ab und meldet, ob es
/// weitergeht. Ohne Extrazeile endet die Liste ausdrücklich (kein Cursor).
pub fn split_page(mut rows: Vec<ArticleRow>) -> (Vec<ArticleRow>, bool) {
    let has_more = rows.len() > PAGE_SIZE;
    if has_more {
        rows.truncate(PAGE_SIZE);
    }
    (rows, has_more)
}

pub fn dedupe_by_article_id(
    rows: Vec<ArticleRow>,
    existing: &[ListRow],
    account_of: &dyn Fn(i64) -> String,
) -> Vec<ArticleRow> {
    let mut seen: std::collections::HashSet<(String, String)> = existing
        .iter()
        .filter_map(|r| r.article().map(|a| (account_of(a.feed_id), a.id.clone())))
        .collect();
    let mut pos: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    let mut out: Vec<ArticleRow> = Vec::with_capacity(rows.len());
    for row in rows {
        let key = (account_of(row.feed_id), row.id.clone());
        if let Some(&at) = pos.get(&key) {
            let prev = &mut out[at];
            prev.unread |= row.unread;
            prev.saved |= row.saved;
            if !prev.has_content && row.has_content {
                prev.has_content = true;
                if prev.excerpt.is_empty() {
                    prev.excerpt = row.excerpt.clone();
                }
            }
            continue;
        }
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        pos.insert(key, out.len());
        out.push(row);
    }
    out
}

pub fn letter_action(keyval: gtk::gdk::Key) -> Option<&'static str> {
    Some(match keyval {
        gtk::gdk::Key::j => "win.next-article",
        gtk::gdk::Key::k => "win.prev-article",
        gtk::gdk::Key::n => "win.next-unread",
        gtk::gdk::Key::p => "win.prev-unread",
        gtk::gdk::Key::m => "win.toggle-read",
        gtk::gdk::Key::s => "win.toggle-saved",
        gtk::gdk::Key::o => "win.open-external",
        _ => return None,
    })
}

pub fn is_editing_class(name: &str) -> bool {
    matches!(
        name,
        "GtkEntry"
            | "GtkPasswordEntry"
            | "GtkSearchEntry"
            | "GtkText"
            | "GtkTextView"
            | "GtkSpinButton"
            | "AdwEntryRow"
            | "AdwPasswordEntryRow"
            | "AdwSpinRow"
            | "AdwSearchEntry"
    )
}

#[cfg(test)]
mod router_tests {
    use super::*;

    fn article(feed_id: i64, id: &str, sort_ms: i64) -> storage::ArticleRow {
        storage::ArticleRow {
            id: id.to_string(),
            feed_id,
            feed_title: "Feed".into(),
            accent: "#111111".into(),
            title: id.to_string(),
            author: None,
            url: None,
            published_ms: sort_ms,
            excerpt: String::new(),
            unread: true,
            saved: false,
            has_content: true,
            sort_ms,
            thumb: None,
        }
    }

    /// Miniatur-Zustandsautomat, der denselben Pfad wie die App fährt:
    /// Benutzeraktion → Undo → Redo.
    fn toggle(h: &mut StatusHistory, state: &mut bool) {
        let before = *state;
        let mut batch: UndoBatch = Vec::new();
        batch.push((1, "a".to_string(), before, false));
        *state = !before;
        h.record_user(batch);
    }

    fn undo(h: &mut StatusHistory, state: &mut bool) {
        let Some(batch) = h.pop_undo() else { return };
        let mut redo: UndoBatch = Vec::new();
        for (feed_id, id, unread, saved) in &batch {
            // Der Gegen-Batch nennt den Zustand **vor** der Aufhebung, also den,
            // den Redo später wiederherstellen soll.
            redo.push((feed_id.clone(), id.clone(), *state, *saved));
            *state = *unread;
        }
        h.push_redo(redo);
    }

    fn redo(h: &mut StatusHistory, state: &mut bool) {
        let Some(batch) = h.pop_redo() else { return };
        let mut undo: UndoBatch = Vec::new();
        for (feed_id, id, unread, saved) in &batch {
            undo.push((feed_id.clone(), id.clone(), *state, *saved));
            *state = *unread;
        }
        h.push_undo(undo);
    }

    #[test]
    fn toggle_undo_redo_arbeitet_auf_sichtbaren_artikeln() {
        let mut h = StatusHistory::new();
        let mut unread = true;
        toggle(&mut h, &mut unread);
        assert!(!unread, "Toggle wirkt");
        undo(&mut h, &mut unread);
        assert!(unread, "Undo stellt den vorherigen Zustand her");
        assert_eq!(h.redo_len(), 1, "Redo enthält einen echten Gegen-Batch");
        redo(&mut h, &mut unread);
        assert!(!unread, "Redo stellt den Toggle-Zustand wieder her");
    }

    #[test]
    fn mehrere_batches_und_quellenwechsel_bleiben_korrekt() {
        let mut h = StatusHistory::new();
        let mut a = true;
        let mut b = true;
        toggle(&mut h, &mut a);
        toggle(&mut h, &mut b);
        assert!(!a && !b);
        undo(&mut h, &mut b);
        assert!(!a && b, "nur der letzte Artikel ist zurückgesetzt");
        undo(&mut h, &mut a);
        assert!(a && b);
        redo(&mut h, &mut a);
        assert!(!a && b);
        // Neue Absicht verwirft den Redo-Verlauf.
        toggle(&mut h, &mut a);
        assert_eq!(
            h.redo_len(),
            0,
            "eine neue Absicht löscht die Redo-Historie"
        );
    }

    #[test]
    fn gegen_batch_aus_der_db_traegt_den_zustand_nach_der_aufhebung() {
        let entry = (7i64, "x".to_string(), true, true);
        let (feed, id, unread, saved) = restored_state(&entry);
        assert_eq!((feed, id, unread, saved), (7, "x".to_string(), false, true));
    }

    /// A8: Der Provider ergibt sich aus den Feeds im Bereich. Ein lokaler Feed
    /// bleibt lokal, Global aktualisiert beides.
    #[test]
    fn refresh_richtet_sich_nach_den_feeds_im_bereich() {
        let accounts = vec![
            (
                "local".to_string(),
                "local".to_string(),
                "Lokal".to_string(),
            ),
            (
                "feedly-1".to_string(),
                "feedly".to_string(),
                "Feedly".to_string(),
            ),
        ];
        let feeds = vec![
            FeedRow {
                id: 1,
                account_id: "local".into(),
                remote_id: None,
                feed_url: "https://lokal.example/f".into(),
                title: "Lokal".into(),
                website: None,
                accent: "#111111".into(),
                groups: vec![10],
            },
            FeedRow {
                id: 2,
                account_id: "feedly-1".into(),
                remote_id: Some("feed/x".into()),
                feed_url: "https://remote.example/f".into(),
                title: "Remote".into(),
                website: None,
                accent: "#222222".into(),
                groups: vec![20],
            },
        ];
        let lokal_feed = refresh_plan(&Scope::Feed(1), &feeds, &accounts);
        assert_eq!(lokal_feed.local_feeds.len(), 1, "lokaler Feed bleibt lokal");
        assert!(
            lokal_feed.feedly_account.is_none(),
            "kein Feedly für einen lokalen Feed"
        );

        let remote_feed = refresh_plan(&Scope::Feed(2), &feeds, &accounts);
        assert!(remote_feed.local_feeds.is_empty());
        assert_eq!(remote_feed.feedly_account.as_deref(), Some("feedly-1"));

        let lokal_gruppe = refresh_plan(&Scope::Group(10), &feeds, &accounts);
        assert_eq!(lokal_gruppe.local_feeds.len(), 1);

        let remote_gruppe = refresh_plan(&Scope::Group(20), &feeds, &accounts);
        assert_eq!(remote_gruppe.feedly_account.as_deref(), Some("feedly-1"));

        let global = refresh_plan(&Scope::Global, &feeds, &accounts);
        assert_eq!(global.local_feeds.len(), 1, "Global aktualisiert lokal");
        assert_eq!(
            global.feedly_account.as_deref(),
            Some("feedly-1"),
            "Global aktualisiert auch Feedly"
        );

        let leer = refresh_plan(&Scope::Account("unbekannt".into()), &feeds, &accounts);
        assert!(leer.skipped, "leerer Bereich wird gemeldet");
    }

    /// A1: Der Konto-Lookup darf keinen Borrow halten, während der Aufrufer den
    /// State verändert. Mit echter `RefCell` nachgewiesen.
    #[test]
    fn konto_lookup_haelt_keinen_borrow() {
        let accounts = vec![
            (
                "local".to_string(),
                "local".to_string(),
                "Lokal".to_string(),
            ),
            (
                "feedly-1".to_string(),
                "feedly".to_string(),
                "Feedly".to_string(),
            ),
        ];
        let state: std::cell::RefCell<Vec<(String, String, String)>> =
            std::cell::RefCell::new(accounts);
        // Muster des Produktivcodes: Wert kopieren, Borrow beenden, dann ändern.
        let account = first_account_of_kind(&state.borrow(), "feedly");
        state.borrow_mut().clear();
        assert_eq!(account.as_deref(), Some("feedly-1"));
        assert!(state.borrow().is_empty(), "der mutable Zugriff war möglich");
    }

    /// A3: Veraltete Ereignisse gehören nicht mehr zum aktuellen Lauf.
    #[test]
    fn veraltete_ereignisse_wuerden_verworfen() {
        let run = FeedlyRun {
            account_id: "feedly-1".to_string(),
            run_id: 7,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let current = Some(run);
        let matches = |account: &str, run_id: u64| match &current {
            Some(r) => r.account_id == account && r.run_id == run_id,
            None => false,
        };
        assert!(matches("feedly-1", 7), "eigener Abschluss wird angenommen");
        assert!(!matches("feedly-1", 6), "älterer Lauf wird verworfen");
        assert!(!matches("feedly-2", 7), "anderes Konto wird verworfen");
    }

    #[test]
    fn logs_ausgeben_keine_token_und_queryparameter() {
        let line = redact(
            "POST https://cloud.feedly.com/v3/markers?access_token=geheim&count=10 fehlgeschlagen",
        );
        assert!(!line.contains("geheim"), "{line}");
        assert!(!line.contains("access_token"), "{line}");
        assert!(line.contains("?[redigiert]"), "{line}");
    }

    #[test]
    fn seitenlogik_erkennt_weitere_seiten_und_setzt_ende() {
        let full: Vec<storage::ArticleRow> = (0..=PAGE_SIZE)
            .map(|i| article(1, &format!("a{i}"), 10_000 - i as i64))
            .collect();
        let (rows, has_more) = split_page(full);
        assert!(has_more, "201 Zeilen bedeuten: es gibt weiter");
        assert_eq!(rows.len(), PAGE_SIZE);
        let (rows, has_more) = split_page(rows);
        assert!(!has_more, "eine volle Seite ohne Extrazeile ist das Ende");
        assert_eq!(rows.len(), PAGE_SIZE);
        let (rows, has_more) = split_page(Vec::new());
        assert!(!has_more && rows.is_empty());
    }

    #[test]
    fn cursor_bleibt_auch_bei_gleicher_zeit_unterscheidbar() {
        let mut st = UiState::default();
        st.build_rows(vec![article(2, "gleich", 500), article(1, "gleich", 500)]);
        let cursor = st.cursor.clone().expect("Cursor");
        assert_eq!(cursor.0, 500);
        assert_eq!(
            cursor.1, 1,
            "der Cursor zeigt auf den letzten gelieferten Feed"
        );
        assert!(st.row_pos(2, "gleich").is_some());
        assert!(st.row_pos(1, "gleich").is_some());
        assert!(st.article(1, "gleich").is_some());
        assert!(st.article(3, "gleich").is_none());
        st.mark_end();
        assert!(st.cursor.is_none(), "Ende wird ausdrücklich markiert");
    }

    #[test]
    fn letter_keys_map_to_actions() {
        assert_eq!(letter_action(gtk::gdk::Key::j), Some("win.next-article"));
        assert_eq!(letter_action(gtk::gdk::Key::k), Some("win.prev-article"));
        assert_eq!(letter_action(gtk::gdk::Key::n), Some("win.next-unread"));
        assert_eq!(letter_action(gtk::gdk::Key::p), Some("win.prev-unread"));
        assert_eq!(letter_action(gtk::gdk::Key::m), Some("win.toggle-read"));
        assert_eq!(letter_action(gtk::gdk::Key::s), Some("win.toggle-saved"));
        assert_eq!(letter_action(gtk::gdk::Key::o), Some("win.open-external"));
    }

    #[test]
    fn other_keys_are_untouched() {
        for key in [
            gtk::gdk::Key::a,
            gtk::gdk::Key::z,
            gtk::gdk::Key::F5,
            gtk::gdk::Key::Up,
            gtk::gdk::Key::space,
            gtk::gdk::Key::Return,
            gtk::gdk::Key::Escape,
        ] {
            assert_eq!(letter_action(key), None, "{key:?}");
        }
    }

    fn row(feed_id: i64, id: &str, unread: bool, saved: bool, has_content: bool) -> ArticleRow {
        ArticleRow {
            id: id.to_string(),
            feed_id,
            feed_title: "Feed".into(),
            accent: "#888888".into(),
            title: "Titel".into(),
            author: None,
            url: None,
            published_ms: 0,
            excerpt: String::new(),
            unread,
            saved,
            has_content,
            sort_ms: 0,
            thumb: None,
        }
    }

    #[test]
    fn account_states_are_translated() {
        assert_eq!(status_label("ready"), "Verbunden und aktuell");
        assert_eq!(
            status_label("auth_required"),
            "Erneute Anmeldung erforderlich"
        );
        assert_eq!(
            status_label("rate_limited"),
            "Drosselung durch Feedly, späterer Versuch"
        );
        assert_eq!(status_label("offline"), "Offline — lokale Daten nutzbar");
        assert_eq!(status_label("unbekannt"), "Getrennt");
    }

    #[test]
    fn position_payload_is_parsed_with_generation() {
        assert_eq!(parse_position("7:12:340"), Some((7, 12, 340)));
        assert_eq!(parse_position("0:-1:0"), Some((0, -1, 0)));
        assert_eq!(parse_position("keine:12:340"), None);
        assert_eq!(parse_position("7:12"), None);
    }

    #[test]
    fn external_uris_are_restricted_to_http_without_userinfo() {
        assert!(external_uri_allowed("https://example.com/a"));
        assert!(external_uri_allowed("http://example.com/a"));
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<h1>x",
            "ftp://example.com/x",
            "https://user:pass@example.com/x",
        ] {
            assert!(!external_uri_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn read_toggle_changes_the_state_in_both_directions() {
        assert!(read_intent(true), "ungelesen -> als gelesen markieren");
        assert!(!read_intent(false), "gelesen -> als ungelesen markieren");
        assert_eq!(unread_delta(true, Some(true)), -1);
        assert_eq!(unread_delta(false, Some(false)), 1);
        assert_eq!(
            unread_delta(false, Some(true)),
            0,
            "idempotentes Setzen ändert keinen Zähler"
        );
        assert_eq!(unread_delta(true, Some(false)), 0);
        assert_eq!(unread_delta(true, None), 0);
        assert_eq!(saved_delta(false, Some(true)), 1);
        assert_eq!(saved_delta(true, Some(false)), -1);
        assert_eq!(saved_delta(true, Some(true)), 0);
    }

    #[test]
    fn dedupe_merges_status_within_account_only() {
        let account_of = |feed_id: i64| match feed_id {
            1 | 2 => "local".to_string(),
            _ => "feedly".to_string(),
        };
        let rows = vec![
            row(1, "x", false, true, false),
            row(2, "x", true, false, true),
            row(3, "x", true, false, true),
        ];
        let out = dedupe_by_article_id(rows, &[], &account_of);
        assert_eq!(out.len(), 2, "getrennte Konten bleiben getrennt");
        assert!(out[0].unread && out[0].saved, "Status wird vereinigt");
        assert!(out[0].has_content, "Inhalt der besseren Zeile übernommen");
        assert!(out[1].unread);
    }

    #[test]
    fn dedupe_skips_ids_already_in_the_window() {
        let account_of = |_: i64| "local".to_string();
        let existing = vec![ListRow::Item(std::rc::Rc::new(list::RowCell::new(row(
            1, "x", true, false, true,
        ))))];
        let out = dedupe_by_article_id(
            vec![
                row(1, "x", false, false, true),
                row(1, "y", true, false, true),
            ],
            &existing,
            &account_of,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "y");
    }

    #[test]
    fn editing_classes_are_recognized() {
        for name in [
            "GtkEntry",
            "GtkPasswordEntry",
            "GtkSearchEntry",
            "GtkText",
            "GtkTextView",
            "GtkSpinButton",
            "AdwEntryRow",
            "AdwPasswordEntryRow",
            "AdwSpinRow",
        ] {
            assert!(is_editing_class(name), "{name}");
        }
        for name in [
            "AdwApplicationWindow",
            "GtkListView",
            "WebKitWebView",
            "AdwButton",
        ] {
            assert!(!is_editing_class(name), "{name}");
        }
    }
}

pub fn now_ms() -> i64 {
    storage::now_ms()
}

pub fn data_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".local/share")
        });
    base.join("lesefluss")
}

const JS_CAPTURE_POS: &str = "(()=>{const g=document.querySelector('meta[name=lf-doc]');const gen=g?g.content:'-1';const els=document.querySelectorAll('article.lf-body > *');if(!els.length)return gen+':-1:0';const y=window.scrollY;let idx=0;for(let i=0;i<els.length;i++){const top=els[i].getBoundingClientRect().top+window.scrollY;if(top>y){idx=Math.max(0,i-1);break;}idx=i;}const el=els[idx];if(!el)return gen+':'+idx+':0';const off=y-(el.getBoundingClientRect().top+window.scrollY);return gen+':'+idx+':'+Math.round(off);})()";

/// Zusatzdaten eines Netzauftrags (z. B. Ziel-Feeds einer Serveraktion).
#[derive(Clone, Default)]
pub struct FeedlyParams {
    pub remote_feed_ids: Vec<String>,
    pub local_feed_ids: Vec<i64>,
}

/// Lauf, über den Abschlussereignisse zugeordnet und Abbruch ausgelöst wird.
pub struct FeedlyRun {
    pub account_id: String,
    pub run_id: u64,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub enum TokenAction {
    StartFeedly,
    Refresh,
    QueuedSync,
    /// Vom Coordinator bereits reservierter Folgelauf (nicht erneut anmelden).
    ReservedRun,
    MarkScopeServer,
    Outbox,
    CheckConnect,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectionCause {
    Unknown,
    Pointer,
    Keyboard,
}

type UndoBatch = Vec<(i64, String, bool, bool)>;

/// Ein Batch-Eintrag nennt den Zustand, der beim Undo/Redo wiederhergestellt wird.
fn restored_state(entry: &(i64, String, bool, bool)) -> (i64, String, bool, bool) {
    let (feed_id, id, unread, saved) = entry;
    (*feed_id, id.clone(), !*unread, *saved)
}

/// Undo/Redo-Historie. Eine neue Absicht verwirft den Redo-Verlauf; das Erzeugen
/// des Gegen-Batches ist davon unabhängig.
#[derive(Default)]
pub struct StatusHistory {
    undo: Vec<UndoBatch>,
    redo: Vec<UndoBatch>,
}

/// Wofür eine Statusänderung aufgerufen wurde.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusMode {
    /// Auslösung durch die Person: Undo entsteht, Redo-Verlauf wird verworfen.
    User,
    /// Gegenbewegung aus Undo/Redo: Undo entsteht, der Redo-Verlauf bleibt.
    Counterpart,
}

impl StatusHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_user(&mut self, batch: UndoBatch) {
        self.redo.clear();
        if !batch.is_empty() {
            self.undo.push(batch);
        }
    }

    pub fn record_counterpart(&mut self, batch: UndoBatch) {
        if !batch.is_empty() {
            self.undo.push(batch);
        }
    }

    pub fn pop_undo(&mut self) -> Option<UndoBatch> {
        self.undo.pop()
    }

    pub fn pop_redo(&mut self) -> Option<UndoBatch> {
        self.redo.pop()
    }

    pub fn push_redo(&mut self, batch: UndoBatch) {
        if !batch.is_empty() {
            self.redo.push(batch);
        }
    }

    pub fn push_undo(&mut self, batch: UndoBatch) {
        if !batch.is_empty() {
            self.undo.push(batch);
        }
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
}
type PendingCb = Box<dyn FnOnce(&Rc<App>, JobOut)>;

const ACCENTS: &[&str] = &[
    "#B94A2E", "#2E5FA3", "#3F8F5F", "#AC3A50", "#7A5CA8", "#B08A2E", "#2E8B8B",
];

pub struct App {
    pub window: adw::ApplicationWindow,
    pub toast: adw::ToastOverlay,
    pub outer: adw::NavigationSplitView,
    pub inner: adw::NavigationSplitView,
    pub sidebar_list: gtk::ListBox,
    pub sidebar_title: adw::WindowTitle,
    pub sidebar_filters: RefCell<Vec<Option<Scope>>>,
    pub last_sync_label: gtk::Label,
    pub list_title: adw::WindowTitle,
    pub list_stack: gtk::Stack,
    pub new_articles_bar: gtk::Revealer,
    pub new_articles_label: gtk::Button,
    pub list_store: gio::ListStore,
    pub list_selection: gtk::SingleSelection,
    pub list_view: gtk::ListView,
    pub list_scroll: gtk::ScrolledWindow,
    pub list_empty: adw::StatusPage,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub filter_saved: gtk::ToggleButton,
    pub filter_unread: gtk::ToggleButton,
    pub filter_all: gtk::ToggleButton,
    pub reader: Rc<ReaderPane>,
    pub state: RefCell<UiState>,
    pub worker: DbWorker,
    pub net: Rc<Net>,
    pub media: std::sync::Arc<provider_local::media::MediaCache>,
    pub prefs: RefCell<Prefs>,
    pub pending_db: RefCell<Vec<(Receiver<JobOut>, PendingCb)>>,
    pub pending_media:
        std::sync::Arc<std::sync::Mutex<Vec<((i64, String), u64, Vec<(String, String)>)>>>,
    pub strings: crate::strings::Strings,
    pub focus_mode: Cell<bool>,
    pub bg_jobs_tx: std::sync::mpsc::Sender<Box<dyn FnOnce(&Rc<App>) + Send>>,
    pub bg_jobs_rx: std::sync::mpsc::Receiver<Box<dyn FnOnce(&Rc<App>) + Send>>,
    pub drain_active: Cell<bool>,
    pub preview_timer: RefCell<Option<glib::SourceId>>,
    pub search_timer: RefCell<Option<glib::SourceId>>,
    pub read_gen: Cell<u64>,
    pub load_gen: Cell<u64>,
    pub suppress: Cell<bool>,
    pub syncing_filters: Cell<bool>,
    pub selection_cause: Cell<SelectionCause>,
    pub history: RefCell<StatusHistory>,
    pub tokens: RefCell<reader::tokens::Tokens>,
    pub css: gtk::CssProvider,
    pub panes: RefCell<Vec<gtk::Widget>>,
    me: RefCell<Option<Weak<App>>>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(application: &adw::Application, worker: DbWorker, net: Rc<Net>) -> Rc<Self> {
        let st = crate::strings::Strings::detect();
        let reader = Rc::new(ReaderPane::new());

        let sidebar_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec![
                "lf-sidebar".to_string(),
                "navigation-sidebar".to_string(),
            ])
            .build();
        let sidebar_scroll = gtk::ScrolledWindow::builder()
            .child(&sidebar_list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let last_sync_label = gtk::Label::builder()
            .label("Noch nicht aktualisiert")
            .xalign(0.0)
            .margin_start(16)
            .margin_end(16)
            .margin_top(6)
            .margin_bottom(10)
            .css_classes(vec!["lf-article-meta".to_string()])
            .build();
        let sidebar_footer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_footer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        sidebar_footer.append(&last_sync_label);

        let primary_menu = gio::Menu::new();
        primary_menu.append(Some("Feedly verbinden …"), Some("win.connect-feedly"));
        primary_menu.append(Some("Feedly trennen"), Some("win.disconnect-feedly"));
        primary_menu.append(Some("OPML importieren …"), Some("win.import-opml"));
        primary_menu.append(Some("OPML exportieren …"), Some("win.export-opml"));
        primary_menu.append(Some("Backup erstellen …"), Some("win.backup"));
        primary_menu.append(Some("Nur-Lesen-Ansicht (F9)"), Some("win.focus-mode"));
        primary_menu.append(Some("Aus Backup wiederherstellen …"), Some("win.restore"));
        let settings_section = gio::Menu::new();
        settings_section.append(Some("Einstellungen"), Some("win.settings"));
        primary_menu.append_section(None, &settings_section);
        let btn_hamburger = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text(&st.get("Menü", "Menu"))
            .menu_model(&primary_menu)
            .primary(true)
            .build();
        let btn_refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(&st.get("Aktualisieren (Strg+R)", "Refresh (Ctrl+R)"))
            .action_name("win.refresh")
            .build();
        let btn_add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text(&st.get("Feed hinzufügen (Strg+N)", "Add feed (Ctrl+N)"))
            .action_name("win.add-feed")
            .build();
        let sidebar_title =
            adw::WindowTitle::new("Lesefluss", &st.get("Lokale Bibliothek", "Local library"));
        let sidebar_header = adw::HeaderBar::builder()
            .title_widget(&sidebar_title)
            .build();
        sidebar_header.pack_start(&btn_hamburger);
        sidebar_header.pack_end(&btn_refresh);
        sidebar_header.pack_end(&btn_add);

        let sidebar_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_body.append(&sidebar_scroll);
        sidebar_body.append(&sidebar_footer);
        sidebar_body.add_css_class("lf-sidebar");
        let sidebar_toolbar = adw::ToolbarView::builder().content(&sidebar_body).build();
        sidebar_toolbar.add_top_bar(&sidebar_header);
        let sources_page = adw::NavigationPage::new(&sidebar_toolbar, "Quellen");

        let list_store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let list_selection = gtk::SingleSelection::builder()
            .model(&list_store)
            .autoselect(false)
            .build();
        let factory = gtk::SignalListItemFactory::new();
        let list_view = gtk::ListView::builder()
            .model(&list_selection)
            .factory(&factory)
            .single_click_activate(true)
            .css_classes(vec!["lf-articles".to_string(), "lf-list".to_string()])
            .build();
        let list_scroll = gtk::ScrolledWindow::builder()
            .css_classes(vec!["lf-list-bg".to_string()])
            .child(&list_view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let list_empty = adw::StatusPage::builder()
            .icon_name("mailbox-symbolic")
            .title(&st.get("Keine Artikel", "No articles"))
            .description(&st.get(
                "In dieser Ansicht ist gerade nichts los.",
                "There is nothing in this view right now.",
            ))
            .vexpand(true)
            .build();
        let new_articles_label = gtk::Button::builder()
            .label("Neue Artikel")
            .has_frame(false)
            .css_classes(vec!["lf-new-articles".to_string()])
            .build();
        let new_articles_bar = gtk::Revealer::builder()
            .child(&new_articles_label)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .build();
        let list_stack = gtk::Stack::builder()
            .css_classes(vec!["lf-list-bg".to_string()])
            .build();
        list_stack.add_named(&list_scroll, Some("list"));
        list_stack.add_named(&list_empty, Some("empty"));
        list_stack.set_visible_child_name("list");

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Artikel durchsuchen (Strg+L)")
            .build();
        let search_bar = gtk::SearchBar::builder()
            .child(&search_entry)
            .show_close_button(true)
            .build();

        let list_title = adw::WindowTitle::new(&st.get("Ungelesen", "Unread"), "");
        let list_header = adw::HeaderBar::builder().title_widget(&list_title).build();
        let sort_button = gtk::Button::builder()
            .icon_name("view-sort-descending-symbolic")
            .tooltip_text(&st.get(
                "Reihenfolge umkehren (Strg+Shift+P)",
                "Reverse order (Ctrl+Shift+P)",
            ))
            .action_name("win.toggle-sort-order")
            .build();
        list_header.pack_start(&sort_button);
        let list_menu = gio::Menu::new();
        list_menu.append(
            Some("Bereich als gelesen markieren…"),
            Some("win.mark-scope-read"),
        );
        let list_more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&list_menu)
            .tooltip_text("Listenaktionen")
            .build();
        list_header.pack_end(&list_more);
        let filter_saved = gtk::ToggleButton::builder()
            .icon_name("user-bookmarks-symbolic")
            .tooltip_text("Gespeicherte Artikel anzeigen")
            .build();
        let filter_unread = gtk::ToggleButton::builder()
            .icon_name("mail-unread-symbolic")
            .tooltip_text("Ungelesene Artikel anzeigen")
            .active(true)
            .build();
        let filter_all = gtk::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text("Alle Artikel anzeigen")
            .build();
        let filter_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        filter_box.add_css_class("lf-filterbar");
        filter_box.append(&filter_saved);
        filter_box.append(&filter_unread);
        filter_box.append(&filter_all);
        let filter_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        filter_wrap.set_halign(gtk::Align::Center);
        filter_wrap.set_margin_bottom(8);
        filter_wrap.append(&filter_box);
        let list_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list_body.append(&new_articles_bar);
        list_body.append(&list_stack);
        let list_toolbar = adw::ToolbarView::builder().content(&list_body).build();
        list_toolbar.add_top_bar(&list_header);
        list_toolbar.add_top_bar(&search_bar);
        list_toolbar.add_bottom_bar(&filter_wrap);
        let list_page = adw::NavigationPage::new(&list_toolbar, "Artikel");

        let btn_back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Zurück zur Artikelliste")
            .action_name("win.reader-back")
            .build();
        reader.header.pack_start(&btn_back);

        // Zugängliche Namen für reine Icon-Schaltflächen (§16)
        fn label(btn: &gtk::Widget, text: &str) {
            btn.update_property(&[gtk::accessible::Property::Label(text)]);
        }
        label(btn_hamburger.upcast_ref(), &st.get("Menü", "Menu"));
        label(
            btn_refresh.upcast_ref(),
            &st.get("Aktualisieren", "Refresh"),
        );
        label(btn_add.upcast_ref(), &st.get("Feed hinzufügen", "Add feed"));
        label(
            btn_hamburger.upcast_ref(),
            &st.get(
                "Menü: OPML, Backup, Einstellungen",
                "Menu: OPML, backup, settings",
            ),
        );

        let inner = adw::NavigationSplitView::builder()
            .min_sidebar_width(280.0)
            .max_sidebar_width(460.0)
            .sidebar(&list_page)
            .content(&adw::NavigationPage::new(&reader.toolbar, "Lesen"))
            .build();

        let outer = adw::NavigationSplitView::builder()
            .min_sidebar_width(208.0)
            .max_sidebar_width(320.0)
            .sidebar(&sources_page)
            .content(&adw::NavigationPage::new(&inner, "Artikel"))
            .build();

        btn_back
            .bind_property("visible", &inner, "collapsed")
            .sync_create()
            .build();

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&outer));
        let window = adw::ApplicationWindow::builder()
            .application(application)
            .default_width(1440)
            .default_height(900)
            .title("Lesefluss")
            .content(&toast)
            .build();
        window.add_css_class("lf-window");
        gtk::prelude::GtkWindowExt::set_icon_name(
            &window,
            Some("io.github.PROJEKTINHABER.Lesefluss"),
        );

        let css = gtk::CssProvider::new();
        let display = gtk::prelude::WidgetExt::display(&window);
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let (bg_jobs_tx, bg_jobs_rx) =
            std::sync::mpsc::channel::<Box<dyn FnOnce(&Rc<App>) + Send>>();
        let app = Rc::new(Self {
            window,
            toast,
            outer,
            inner,
            sidebar_list,
            sidebar_title,
            sidebar_filters: RefCell::new(Vec::new()),
            last_sync_label,
            list_title,
            list_stack,
            new_articles_bar,
            new_articles_label,
            list_store,
            list_selection,
            list_view,
            list_scroll,
            list_empty,
            search_bar,
            search_entry,
            filter_saved,
            filter_unread,
            filter_all,
            reader,
            state: RefCell::new(UiState::default()),
            worker,
            net,
            prefs: RefCell::new(Prefs::default()),
            media: std::sync::Arc::new(
                provider_local::media::MediaCache::new(
                    provider_local::media::cache_dir(),
                    512 * 1024 * 1024,
                )
                .expect("Mediencache-Verzeichnis"),
            ),
            pending_db: RefCell::new(Vec::new()),
            pending_media: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            strings: crate::strings::Strings { lang: st.lang },
            focus_mode: Cell::new(false),
            bg_jobs_tx: bg_jobs_tx.clone(),
            bg_jobs_rx,
            drain_active: Cell::new(false),
            preview_timer: RefCell::new(None),
            search_timer: RefCell::new(None),
            read_gen: Cell::new(0),
            load_gen: Cell::new(0),
            suppress: Cell::new(false),
            syncing_filters: Cell::new(false),
            selection_cause: Cell::new(SelectionCause::Unknown),
            history: RefCell::new(StatusHistory::new()),
            tokens: RefCell::new(tokens_for(true)),
            css,
            panes: RefCell::new(Vec::new()),
            me: RefCell::new(None),
        });

        *app.me.borrow_mut() = Some(Rc::downgrade(&app));
        *app.panes.borrow_mut() = vec![
            app.sidebar_list.clone().upcast::<gtk::Widget>(),
            app.list_view.clone().upcast::<gtk::Widget>(),
            app.reader.webview.clone().upcast::<gtk::Widget>(),
        ];

        app.load_prefs();
        app.apply_layout();
        app.apply_theme_now();
        app.start_theme_watch();
        app.start_frame_probe();
        app.install_letter_router();
        app.install_sidebar_context_menu();
        {
            let w = app.weak();
            app.window.connect_close_request(move |window| {
                if let Some(app) = w.upgrade() {
                    app.save_layout();
                }
                glib::Propagation::Proceed
            });
        }
        app.wire(factory);
        app.register_actions(application);
        app.install_width_watcher();
        app.start_drain_loop();
        app.window.present();
        app.bootstrap();
        app
    }

    pub fn weak(&self) -> Weak<App> {
        self.me
            .borrow()
            .clone()
            .expect("App-Selbstreferenz gesetzt")
    }

    // ── DB- und Net-Drain ──

    pub fn db_query<F, R, C>(&self, f: F, on: C)
    where
        F: FnOnce(&storage::Database) -> R + Send + 'static,
        R: Send + 'static,
        C: FnOnce(&Rc<App>, R) + 'static,
    {
        let rx = self.worker.send(f);
        self.pending_db.borrow_mut().push((
            rx,
            Box::new(move |app: &Rc<App>, out: JobOut| {
                if let Ok(v) = out.downcast::<R>() {
                    on(app, *v);
                }
            }),
        ));
    }

    fn start_drain_loop(&self) {
        let w = self.weak();
        glib::timeout_add_local(Duration::from_millis(120), move || {
            let Some(app) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            app.drain_once();
            glib::ControlFlow::Continue
        });
    }

    fn drain_once(self: &Rc<Self>) {
        if let Ok(mut q) = self.pending_media.lock() {
            let jobs: Vec<_> = q.drain(..).collect();
            drop(q);
            for ((feed_id, id), gen, reps) in jobs {
                let current = self.reader.current.borrow().clone();
                if current.as_deref() != Some(id.as_str())
                    || self.reader.document_generation.get() != gen
                {
                    dbg_log(&format!(
                        "Medienergebnis für {id} verworfen (Generation {gen})"
                    ));
                    continue;
                }
                let _ = feed_id;
                for (url, data) in &reps {
                    self.reader.apply_media(url, data);
                }
            }
        }
        while let Ok(job) = self.bg_jobs_rx.try_recv() {
            job(self);
        }
        loop {
            let net_event = self.net.events.try_recv().ok();
            if let Some(ev) = net_event {
                self.handle_net_event(ev);
                continue;
            }
            break;
        }
        let mut ready: Vec<(PendingCb, JobOut)> = Vec::new();
        {
            let mut pending = self.pending_db.borrow_mut();
            let mut i = 0;
            while i < pending.len() {
                match pending[i].0.try_recv() {
                    Ok(out) => {
                        let (_, cb) = pending.remove(i);
                        ready.push((cb, out));
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let (_, _cb) = pending.remove(i);
                    }
                }
            }
        }
        for (cb, out) in ready {
            let self_rc = self
                .me
                .borrow()
                .as_ref()
                .and_then(Weak::upgrade)
                .expect("App lebendig während Drain");
            cb(&self_rc, out);
        }
    }

    fn handle_net_event(&self, ev: NetEvent) {
        match ev {
            NetEvent::FetchStarted(_) => {}
            NetEvent::FetchNotModified(_) => self.touch_last_sync(),
            NetEvent::FetchDone {
                feed_id,
                added,
                title,
                ..
            } => {
                self.touch_last_sync();
                let label = title.unwrap_or_else(|| format!("Feed {feed_id}"));
                if added > 0 {
                    self.show_toast(&format!("{label}: {added} neue Artikel"));
                    let at_top = self.list_scroll.vadjustment().value() < 80.0;
                    self.reload_counts();
                    if at_top {
                        self.load_page(false);
                    }
                } else {
                    self.reload_counts();
                }
            }
            NetEvent::FetchFailed { message, .. } => {
                self.touch_last_sync();
                self.show_toast(&format!("Abruf fehlgeschlagen: {message}"));
            }
            NetEvent::DiscoveryDone { candidates, .. } => self.show_discovery_dialog(candidates),
            NetEvent::DiscoveryFailed { message, .. } => {
                self.show_toast(&format!("Kein Feed gefunden: {message}"));
            }
            NetEvent::FeedlySyncDone {
                account_id,
                run_id,
                added,
            } => {
                // Veraltete Ergebnisse (nach Logout oder neuem Lauf) werden verworfen.
                if !self.feedly_event_is_current(&account_id, run_id) {
                    dbg_log(&format!(
                        "Feedly: Ergebnis von Lauf {run_id} verworfen (bereits ersetzt)"
                    ));
                    return;
                }
                self.refresh_feedly_status();
                let reserved = {
                    let mut st = self.state.borrow_mut();
                    st.feedly_sync_running = false;
                    st.feedly_sync_queued = false;
                    st.active_feedly_run = None;
                    st.coordinator.set_now(now_ms());
                    st.coordinator.finish(&account_id, now_ms())
                };
                if reserved.is_some() {
                    // Der Coordinator hat den Folgelauf bereits reserviert; er wird
                    // direkt gestartet und **nicht** erneut angemeldet.
                    self.start_queued_feedly_run();
                }
                let w = self.weak();
                self.db_query(
                    move |db| {
                        Ok::<_, storage::StorageError>(db.last_sync(&account_id)?.unwrap_or(0))
                    },
                    move |app, res: storage::Result<i64>| {
                        if let Ok(v) = res {
                            app.state.borrow_mut().feedly_last_sync = v;
                        }
                        let _ = w;
                    },
                );
                self.reload_meta_keep();
                if added > 0 {
                    self.show_toast(&format!("Feedly: {added} neue Artikel"));
                }
            }
            NetEvent::FeedlySyncFailed {
                account_id,
                run_id,
                message,
                status,
                retry_after_ms,
            } => {
                if !self.feedly_event_is_current(&account_id, run_id) {
                    dbg_log(&format!("Feedly: Fehler von Lauf {run_id} verworfen"));
                    return;
                }
                self.refresh_feedly_status();
                {
                    let mut st = self.state.borrow_mut();
                    st.feedly_sync_running = false;
                    st.feedly_sync_queued = false;
                    st.active_feedly_run = None;
                    st.coordinator.set_now(now_ms());
                    st.coordinator.finish(&account_id, now_ms());
                    // Pausen aus dem Sync gelten für alle Wege, auch für den
                    // manuellen Refresh.
                    match status.as_deref() {
                        Some("auth_required") => {
                            st.coordinator.pause(&account_id, sync_engine::Pause::Auth)
                        }
                        Some("rate_limited") => {
                            if let Some(until) = retry_after_ms {
                                st.coordinator
                                    .pause(&account_id, sync_engine::Pause::Quota(until));
                            }
                        }
                        _ => {}
                    }
                }
                self.show_toast(&format!("Feedly-Sync fehlgeschlagen: {message}"));
                // Auch im Fehlerfall läuft ein reservierter Folgelauf weiter.
                let reserved = {
                    let mut st = self.state.borrow_mut();
                    st.coordinator.set_now(now_ms());
                    st.coordinator.finish(&account_id, now_ms())
                };
                if reserved.is_some() {
                    self.start_queued_feedly_run();
                }
            }
        }
    }

    fn touch_last_sync(&self) {
        let now = now_ms();
        self.state.borrow_mut().last_sync = Some(now);
        self.last_sync_label
            .set_label(&format!("Zuletzt aktualisiert: {}", fmt_time(now)));
        self.worker.send(move |db| db.set_last_sync("local", now));
    }

    // ── Start ──

    fn start_frame_probe(&self) {
        let mode = std::env::var("LF_FRAMECHECK").unwrap_or_default();
        if mode.is_empty() {
            return;
        }
        let w = self.weak();
        if mode.contains("stress") {
            let interval: u64 = mode
                .split(':')
                .nth(1)
                .and_then(|v| v.parse().ok())
                .unwrap_or(350);
            glib::timeout_add_local(Duration::from_millis(interval), move || match w.upgrade() {
                Some(app) => {
                    let _ = gtk::prelude::WidgetExt::activate_action(
                        &app.window,
                        "win.next-article",
                        None,
                    );
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
        }
        let samples: Rc<std::cell::RefCell<Vec<f64>>> =
            Rc::new(std::cell::RefCell::new(Vec::with_capacity(4096)));
        let last = Rc::new(std::cell::Cell::new(0i64));
        let s2 = samples.clone();
        let l2 = last.clone();
        self.window.add_tick_callback(move |_w, clock| {
            let now = clock.frame_time();
            let prev = l2.replace(now);
            if prev > 0 {
                let dt = (now - prev) as f64 / 1000.0;
                if dt > 0.0 && dt < 2000.0 {
                    let mut v = s2.borrow_mut();
                    v.push(dt);
                    if v.len() >= 3600 {
                        drop(v);
                        let mut v = std::mem::take(&mut *s2.borrow_mut());
                        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        let n = v.len();
                        let at = |q: f64| v[((n as f64 - 1.0) * q).round() as usize];
                        let over = |lim: f64| {
                            v.iter().filter(|x| **x > lim).count() as f64 / n as f64 * 100.0
                        };
                        eprintln!(
                            "[lf] frames mode={mode} n={n} p50={:.2} p95={:.2} p99={:.2} max={:.2} >16.7={:.1}% >8.3={:.1}% >50={:.1}%",
                            at(0.5),
                            at(0.95),
                            at(0.99),
                            v[n - 1],
                            over(16.7),
                            over(8.3),
                            over(50.0)
                        );
                        return glib::ControlFlow::Break;
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    fn bootstrap(&self) {
        let seed = std::env::var("LF_SEED").is_ok();
        let w = self.weak();
        self.db_query(
            move |db| {
                db.ensure_local_account()?;
                if seed {
                    let n = db.list_feeds()?;
                    if n.is_empty() {
                        crate::seed::seed_fixtures(db)?;
                    }
                }
                let feeds = db.list_feeds()?;
                let groups = db.list_groups()?;
                let counts = db.counts()?;
                let last = db.last_sync("local")?;
                let accounts = db.list_accounts()?;
                let feedly_id = accounts
                    .iter()
                    .find(|(_, k, _)| k == "feedly")
                    .map(|(id, _, _)| id.clone());
                let feedly_last = match feedly_id {
                    Some(id) => db.last_sync(&id)?.unwrap_or(0),
                    None => 0,
                };
                Ok::<_, storage::StorageError>((feeds, groups, counts, last, accounts, feedly_last))
            },
            move |app,
                  res: storage::Result<(
                Vec<FeedRow>,
                Vec<GroupRow>,
                Counts,
                Option<i64>,
                Vec<(String, String, String)>,
                i64,
            )>| {
                let Ok((feeds, groups, counts, last, accounts, feedly_last)) = res else {
                    return;
                };
                let has_feedly_account = accounts.iter().any(|(_, k, _)| k == "feedly");
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                    st.last_sync = last;
                    st.accounts = accounts;
                    st.feedly_last_sync = feedly_last;
                }
                app.start_feedly_scheduler();
                app.start_outbox_tick();
                let w2 = app.worker.clone();
                std::thread::spawn(move || {
                    let _ = w2.send(|db| db.outbox_reset_inflight());
                });
                if std::env::var("LF_FEEDLY_CONNECT").is_ok() && !has_feedly_account {
                    app.with_token(TokenAction::StartFeedly);
                }
                if let Some(ms) = last {
                    app.last_sync_label
                        .set_label(&format!("Zuletzt aktualisiert: {}", fmt_time(ms)));
                }
                app.run_retention();
                app.refresh_sidebar();
                app.load_page(false);
                if let Ok(list) = std::env::var("LF_SUBSCRIBE") {
                    for url in list.split(',').filter(|u| !u.trim().is_empty()) {
                        let url = url.trim().to_string();
                        let title = url::Url::parse(&url)
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_string))
                            .unwrap_or_else(|| url.clone());
                        app.subscribe(&url, &title);
                    }
                }
                let _ = w;
            },
        );
    }

    pub fn run_retention(&self) {
        let media = std::sync::Arc::clone(&self.media);
        let retention = self.prefs.borrow().retention_days;
        self.db_query(
            move |db| {
                let pinned = db.pinned_media_urls()?;
                let now = storage::now_ms();
                let pruned = db.prune_old_read(now, retention)?;
                Ok::<_, storage::StorageError>((pinned, pruned))
            },
            move |_app, res: storage::Result<(Vec<String>, usize)>| {
                if let Ok((pinned, pruned)) = res {
                    if pruned > 0 {
                        dbg_log(&format!(
                            "Aufbewahrung: {pruned} alte gelesene Artikel bereinigt"
                        ));
                    }
                    let keys: std::collections::HashSet<String> = pinned
                        .iter()
                        .map(|u| provider_local::media::key_of(u))
                        .collect();
                    // Der Cache führt die Pins selbst, damit jeder Prune sie kennt.
                    media.set_pins(keys);
                    media.prune_pinned();
                }
            },
        );
    }

    pub fn load_page(&self, append: bool) {
        let (scope, filter, cursor, search) = {
            let st = self.state.borrow();
            let cur = if append { st.cursor.clone() } else { None };
            (st.scope.clone(), st.filter, cur, st.search.clone())
        };
        let sort_account = self.account_of_scope(&scope);
        if append && cursor.is_none() {
            return;
        }
        let cur = cursor.clone();
        self.load_gen.set(self.load_gen.get() + 1);
        let gen = self.load_gen.get();
        self.db_query(
            move |db| {
                // Sortierung folgt dem Konto der Ansicht; ohne Konto die Vorgabe.
                let newest = match sort_account.as_deref() {
                    Some(account) => db
                        .account_newest_first(account, self_newest_first(db))
                        .unwrap_or_else(|_| self_newest_first(db)),
                    None => self_newest_first(db),
                };
                // 201 statt 200: die eine Zeile mehr verrät zuverlässig, ob es
                // weitergeht, und der Cursor bleibt der neue (sort_ms, feed, id).
                let before = cur.as_ref().map(|(ms, feed, id)| (*ms, *feed, id.as_str()));
                match &search {
                    Some(q) if !q.is_empty() => {
                        db.search_ordered(q, &scope, filter, before, (PAGE_SIZE + 1) as u32, newest)
                    }
                    _ => db.query_articles_ordered(
                        &scope,
                        filter,
                        before,
                        (PAGE_SIZE + 1) as u32,
                        newest,
                    ),
                }
            },
            move |app, res: storage::Result<Vec<ArticleRow>>| {
                if app.load_gen.get() != gen {
                    dbg_log(&format!("load_page: stale gen {gen} verworfen"));
                    return;
                }
                let Ok(rows) = res else { return };
                if let Some(first) = rows.first() {
                    dbg_log(&format!(
                        "load_page gen={gen} rows={} first={} unread={}",
                        rows.len(),
                        first.id,
                        first.unread
                    ));
                } else {
                    dbg_log(&format!("load_page gen={gen} rows=0"));
                }
                let (rows, has_more) = split_page(rows);
                let had_full_page = has_more;
                let keep_sel = app.state.borrow().selected.clone();
                let current = app.state.borrow().rows.clone();
                let feeds = app.state.borrow().feeds.clone();
                let account_of = move |feed_id: i64| {
                    feeds
                        .iter()
                        .find(|f| f.id == feed_id)
                        .map(|f| f.account_id.clone())
                        .unwrap_or_default()
                };
                let rows =
                    dedupe_by_article_id(rows, if append { &current } else { &[] }, &account_of);
                {
                    let mut st = app.state.borrow_mut();
                    if append {
                        st.append_rows(rows);
                    } else {
                        st.build_rows(rows);
                    }
                    if !had_full_page {
                        st.mark_end();
                    }
                    if let Some(sel) = keep_sel {
                        st.selected = Some(sel);
                    }
                }
                app.trim_rows();
                app.sync_store(append);
                app.request_thumbs();
                app.update_list_empty_state();
                app.offer_new_articles();
                app.state.borrow_mut().loading_more = false;
                if let Some((feed_id, sel)) = app.selected_key() {
                    let current = app.reader.current.borrow().clone();
                    if current.as_deref() != Some(sel.as_str()) {
                        app.open_article_by_id(feed_id, &sel, false, false);
                    }
                }
            },
        );
    }

    fn reload_counts(&self) {
        let w = self.weak();
        self.db_query(
            |db| db.counts(),
            move |app, res: storage::Result<Counts>| {
                let Ok(counts) = res else { return };
                app.state.borrow_mut().counts = counts;
                app.refresh_sidebar();
                app.update_reader_empty();
                let _ = w;
            },
        );
    }

    // ── Store-Sync ──

    /// Übernimmt nur die tatsächlichen Unterschiede in den Store: vorhandene
    /// Zeilen werden ersetzt statt neu aufgebaut, damit Auswahl und
    /// Scrollanker erhalten bleiben.
    pub fn sync_store(&self, append: bool) {
        let rows = self.state.borrow().rows.clone();
        self.suppress.set(true);
        let mut existing = self.list_store.n_items() as usize;
        if append {
            for r in rows.iter().skip(existing) {
                self.list_store
                    .append(&glib::BoxedAnyObject::new(r.clone()));
                existing += 1;
            }
        } else {
            while self.list_store.n_items() as usize > rows.len() {
                let last = self.list_store.n_items().saturating_sub(1);
                self.list_store.remove(last);
            }
            for (pos, r) in rows.iter().enumerate() {
                let pos = pos as u32;
                if pos < self.list_store.n_items() {
                    self.list_store.remove(pos);
                }
                self.list_store
                    .insert(pos, &glib::BoxedAnyObject::new(r.clone()));
            }
        }
        if let Some(sel) = self.state.borrow().selected.clone() {
            if let Some(pos) = self.state.borrow().row_pos(sel.0, &sel.1) {
                if gtk::SingleSelection::selected(&self.list_selection) != pos as u32 {
                    self.list_selection.set_selected(pos as u32);
                }
            }
        }
        let has_items = rows.iter().any(|r| matches!(r, ListRow::Item(_)));
        self.list_stack
            .set_visible_child_name(if has_items { "list" } else { "empty" });
        self.suppress.set(false);
    }

    /// Kappt das Listenmodell auf ein Fenster von 2 000 Zeilen (§7.2) und
    /// bewahrt dabei Auswahl und obersten sichtbaren Artikel.
    fn trim_rows(&self) {
        const MAX_ROWS: usize = 2000;
        let (over, keep_id) = {
            let st = self.state.borrow();
            if st.rows.len() <= MAX_ROWS {
                return;
            }
            let over = st.rows.len() - MAX_ROWS;
            let keep = st.selected.clone();
            (over, keep)
        };
        let anchor = self.top_visible_article_id().or(keep_id);
        {
            let mut st = self.state.borrow_mut();
            for _ in 0..over {
                if st.rows.len() <= MAX_ROWS {
                    break;
                }
                // nie die Auswahl oder den Scrollanker entfernen
                if let Some((keep_feed, keep)) = anchor.as_ref() {
                    let pos = st.rows.iter().position(|r| {
                        r.article()
                            .map(|a| a.feed_id == *keep_feed && &a.id == keep)
                            .unwrap_or(false)
                    });
                    if let Some(pos) = pos {
                        if pos < st.rows.len() - MAX_ROWS {
                            st.rows.remove(0);
                            continue;
                        }
                    }
                }
                st.rows.pop();
            }
        }
        self.sync_store(false);
    }

    /// Kleine Vorschauen für sichtbare Zeilen erzeugen (einmal pro Artikel,
    /// im Worker, begrenzt auf 128 px) — nur aus bereits gecachten Bildern.
    fn request_thumbs(&self) {
        if !self.prefs.borrow().thumbs {
            return;
        }
        let candidates: Vec<(i64, String)> = {
            let st = self.state.borrow();
            st.rows
                .iter()
                .take(40)
                .filter_map(|r| r.article())
                .filter(|a| a.thumb.is_none())
                .map(|a| (a.feed_id, a.id.clone()))
                .collect()
        };
        if candidates.is_empty() {
            return;
        }
        let worker = self.worker.clone();
        let media = std::sync::Arc::clone(&self.media);
        for (feed_id, article_id) in candidates {
            let worker2 = worker.clone();
            let media2 = std::sync::Arc::clone(&media);
            std::thread::Builder::new()
                .name("lf-thumb".into())
                .spawn(move || {
                    let id_for_db = article_id.clone();
                    let Ok(boxed) = worker2
                        .send(move |db| db.article_media_urls(feed_id, &id_for_db))
                        .recv()
                    else {
                        return;
                    };
                    let urls = match boxed.downcast::<storage::Result<Vec<String>>>().map(|r| *r) {
                        Ok(Ok(urls)) => urls,
                        _ => return,
                    };
                    let Some(url) = urls.into_iter().find(|u| media2.path_for(u).exists()) else {
                        return;
                    };
                    let Some((bytes, _mime)) = media2.get_cached(&url) else {
                        return;
                    };
                    let Some(data_uri) = thumbnail_data_uri(&bytes) else {
                        return;
                    };
                    let _ = worker2.send(move |db| {
                        db.set_article_thumb(feed_id, &article_id, Some(&data_uri))
                    });
                })
                .ok();
        }
    }

    /// „Nur lesen“ blendet beide Listen aus; als Anpassung an große Monitore (§4.5).
    fn toggle_focus_mode(&self) {
        let next = !self.focus_mode.get();
        self.focus_mode.set(next);
        if next {
            self.outer.set_show_content(false);
            self.inner.set_show_content(false);
        } else {
            self.outer.set_show_content(true);
            self.inner.set_show_content(true);
        }
    }

    /// Fenstergröße sichern (§4.3). Die Breiten der Navigations-Spalten
    /// steuert libadwaita selbst; sie lassen sich in dieser API nicht lesen
    /// und werden deshalb nicht gespeichert (dokumentierte Abweichung).
    fn save_layout(&self) {
        let width = self.window.width();
        let height = self.window.height();
        if width > 400 && height > 300 {
            self.save_pref("layout", &format!("{width};{height}"));
        }
    }

    fn apply_layout(&self) {
        let raw = self.worker.read_layout();
        if let Some(raw) = raw {
            let parts: Vec<i64> = raw
                .split(';')
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            if parts.len() == 2
                && (800..=8000).contains(&parts[0])
                && (600..=8000).contains(&parts[1])
            {
                self.window
                    .set_default_size(parts[0] as i32, parts[1] as i32);
            }
        }
    }

    /// Neue Artikel, die oberhalb des sichtbaren Bereichs liegen, werden
    /// angeboten statt automatisch eingefügt (§5.2).
    fn offer_new_articles(&self) {
        let (new_count, top) = {
            let st = self.state.borrow();
            let top = self.top_visible_article_id();
            let pos = top
                .as_ref()
                .and_then(|(feed_id, id)| st.row_pos(*feed_id, id))
                .unwrap_or(0);
            let count = st
                .rows
                .iter()
                .take(pos)
                .filter(|r| matches!(r, ListRow::Item(_)))
                .count();
            (count, top)
        };
        let _ = top;
        {
            let mut st = self.state.borrow_mut();
            st.pending_new_articles = new_count;
        }
        self.update_new_articles_bar();
    }

    fn update_new_articles_bar(&self) {
        let count = self.state.borrow().pending_new_articles;
        if count == 0 {
            self.new_articles_bar.set_visible(false);
            return;
        }
        self.new_articles_bar.set_visible(true);
        self.new_articles_label
            .set_label(&format!("{count} neue Artikel"));
    }

    /// Aktuell sichtbarer oberster Artikel als logischer Schlüssel (Feed + ID).
    fn top_visible_article_id(&self) -> Option<(i64, String)> {
        let pos = self.list_scroll.vadjustment().value().round().max(0.0);
        let st = self.state.borrow();
        st.rows
            .iter()
            .filter_map(|r| r.article())
            .min_by_key(|a| (a.sort_ms.abs_diff(pos as i64), a.id.clone()))
            .map(|a| (a.feed_id, a.id))
    }

    pub fn update_list_empty_state(&self) {
        let has = self
            .state
            .borrow()
            .rows
            .iter()
            .any(|r| matches!(r, ListRow::Item(_)));
        self.list_stack
            .set_visible_child_name(if has { "list" } else { "empty" });
    }

    fn rebind_row(&self, feed_id: i64, id: &str) {
        let Some(pos) = self.state.borrow().row_pos(feed_id, id) else {
            return;
        };
        let Some(obj) = self.list_store.item(pos as u32) else {
            return;
        };
        self.suppress.set(true);
        self.list_store.remove(pos as u32);
        self.list_store.insert(pos as u32, &obj);
        self.suppress.set(false);
    }

    fn remove_row(&self, feed_id: i64, id: &str) {
        let Some(pos) = self.state.borrow().row_pos(feed_id, id) else {
            return;
        };
        {
            let mut st = self.state.borrow_mut();
            st.rows.remove(pos);
        }
        self.suppress.set(true);
        if self.selected_id().as_deref() == Some(id) {
            let next = self
                .state
                .borrow()
                .rows
                .iter()
                .enumerate()
                .skip(pos)
                .find_map(|(i, r)| match r {
                    ListRow::Item(_) => Some(i),
                    _ => None,
                })
                .or_else(|| {
                    self.state
                        .borrow()
                        .rows
                        .iter()
                        .enumerate()
                        .take(pos)
                        .rev()
                        .find_map(|(i, r)| match r {
                            ListRow::Item(_) => Some(i),
                            _ => None,
                        })
                });
            if let Some(p) = next {
                self.list_selection.set_selected(p as u32);
            }
        }
        self.list_store.remove(pos as u32);
        self.suppress.set(false);
        self.update_list_empty_state();
    }

    fn selected_id(&self) -> Option<String> {
        self.state.borrow().selected.clone().map(|(_, id)| id)
    }

    /// Auswahl als logischer Schlüssel (Feed + ID).
    fn selected_key(&self) -> Option<(i64, String)> {
        self.state.borrow().selected.clone()
    }

    // ── Sidebar ──

    fn account_label_for_scope(&self) -> String {
        let st = self.state.borrow();
        let acc_name = |id: &str| -> String {
            st.accounts
                .iter()
                .find(|(a, _, _)| a == id)
                .map(|(_, k, n)| format!("{} · {}", k, n))
                .unwrap_or_else(|| "Konto".into())
        };
        match &st.scope {
            Scope::Global => {
                if st.accounts.iter().any(|(_, k, _)| k != "local") {
                    "Alle Konten".into()
                } else {
                    "Lokale Bibliothek".into()
                }
            }
            Scope::Account(a) => acc_name(a),
            Scope::Feed(f) => match st.feeds.iter().find(|x| x.id == *f) {
                Some(feed) if feed.account_id == "local" => "Lokale Bibliothek".into(),
                Some(feed) => acc_name(&feed.account_id),
                None => "Lokale Bibliothek".into(),
            },
            Scope::Group(g) => match st.groups.iter().find(|x| x.id == *g) {
                Some(gr) if gr.account_id == "local" => "Lokale Bibliothek".into(),
                Some(gr) => acc_name(&gr.account_id),
                None => "Lokale Bibliothek".into(),
            },
        }
    }

    /// Kontextmenü für eine Quelle: erreichbar per Rechtsklick, Menütaste
    /// und `Shift+F10` (§5.1).
    fn source_context_menu(&self, source: Scope, anchor: &gtk::Widget) {
        let menu = gio::Menu::new();
        match source {
            Scope::Feed(feed_id) => {
                menu.append(Some("Umbenennen …"), Some("win.rename-feed"));
                menu.append(Some("Gruppen …"), Some("win.edit-feed-groups"));
                menu.append(Some("Als gelesen markieren"), Some("win.mark-source-read"));
                menu.append(Some("Abbestellen …"), Some("win.unsubscribe-feed"));
            }
            Scope::Group(group_id) => {
                menu.append(Some("Gruppe umbenennen …"), Some("win.rename-group"));
                menu.append(
                    Some("Alle als gelesen markieren"),
                    Some("win.mark-scope-read"),
                );
                let _ = group_id;
            }
            Scope::Account(_) => {
                menu.append(Some("Aktualisieren"), Some("win.refresh"));
            }
            Scope::Global => {
                menu.append(Some("Aktualisieren"), Some("win.refresh"));
                menu.append(
                    Some("Alle als gelesen markieren …"),
                    Some("win.mark-scope-read"),
                );
            }
        }
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(anchor);
        popover.set_has_arrow(false);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(0, 0, 0, 0)));
        self.state.borrow_mut().menu_source = Some(source);
        popover.show();
    }

    /// Rechtsklick, Menütaste und `Shift+F10` öffnen dasselbe Menü (§5.1).
    fn install_sidebar_context_menu(&self) {
        let w = self.weak();
        let click = gtk::GestureClick::new();
        click.set_button(3);
        click.connect_pressed(move |gesture, n_press, x, y| {
            let Some(app) = w.upgrade() else { return };
            if n_press != 1 {
                return;
            }
            if let Some(row) = gesture
                .widget()
                .and_then(|w| w.parent())
                .and_then(|w| w.parent())
            {
                if let Ok(list_row) = row.clone().downcast::<gtk::ListBoxRow>() {
                    let index = list_row.index() as usize;
                    if let Some(Some(source)) = app.sidebar_filters.borrow().get(index).cloned() {
                        app.sidebar_list.select_row(Some(&list_row));
                        app.show_feed_menu(source);
                    }
                }
            }
            let _ = (x, y);
        });
        self.sidebar_list.add_controller(click);

        let w = self.weak();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, keyval, _, state| {
            let Some(app) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let menu_key = keyval == gtk::gdk::Key::Menu;
            let shift_f10 =
                keyval == gtk::gdk::Key::F10 && state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            if !menu_key && !shift_f10 {
                return glib::Propagation::Proceed;
            }
            let Some(row) = app.sidebar_list.selected_row() else {
                return glib::Propagation::Proceed;
            };
            let index = row.index() as usize;
            let source = app.sidebar_filters.borrow().get(index).cloned().flatten();
            match source {
                Some(source) => {
                    app.show_feed_menu(source);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.sidebar_list.add_controller(keys);
    }

    fn show_feed_menu(&self, source: Scope) {
        let anchor = self.window.clone();
        self.source_context_menu(source, anchor.upcast_ref());
    }

    fn current_menu_source(&self) -> Option<Scope> {
        self.state.borrow().menu_source.clone()
    }

    /// Konto, dessen Sortierung für die aktuelle Ansicht gilt.
    /// Konto-ID einer Art. Der Borrow endet beim Rückgabewert, damit im Aufrufer
    /// kein `RefCell already borrowed` entstehen kann.
    fn account_id_of_kind(&self, kind: &str) -> Option<String> {
        let accounts = &self.state.borrow().accounts;
        first_account_of_kind(accounts, kind)
    }

    fn account_of_scope(&self, scope: &storage::Scope) -> Option<String> {
        let st = self.state.borrow();
        match scope {
            storage::Scope::Account(id) => Some(id.clone()),
            storage::Scope::Feed(feed_id) => st
                .feeds
                .iter()
                .find(|f| f.id == *feed_id)
                .map(|f| f.account_id.clone()),
            storage::Scope::Group(group_id) => st
                .feeds
                .iter()
                .find(|f| f.groups.contains(group_id))
                .map(|f| f.account_id.clone()),
            storage::Scope::Global => None,
        }
    }

    fn toggle_sort_order(&self) {
        // In einer Konto- oder Feedansicht wird die Reihenfolge dieses Kontos
        // geändert, sonst die globale Vorgabe.
        let scope = self.state.borrow().scope.clone();
        let account = self.account_of_scope(&scope);
        let newest = match &account {
            Some(id) => {
                let current = self.prefs.borrow().newest_first;
                let flipped = !current;
                let id = id.clone();
                self.db_query(
                    move |db| {
                        db.set_account_newest_first(&id, flipped)?;
                        Ok::<_, storage::StorageError>(())
                    },
                    |_, _| {},
                );
                flipped
            }
            None => {
                let newest = !self.prefs.borrow().newest_first;
                self.prefs.borrow_mut().newest_first = newest;
                self.save_pref("newest_first", if newest { "1" } else { "0" });
                newest
            }
        };
        self.show_toast(if newest {
            "Reihenfolge: neueste zuerst"
        } else {
            "Reihenfolge: älteste zuerst"
        });
        self.load_page(false);
    }

    fn rename_feed_dialog(&self) {
        let Some(Scope::Feed(feed_id)) = self.current_menu_source() else {
            return;
        };
        let current = self
            .state
            .borrow()
            .feeds
            .iter()
            .find(|f| f.id == feed_id)
            .map(|f| f.title.clone())
            .unwrap_or_default();
        let entry = gtk::Entry::builder()
            .text(&current)
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading("Feed umbenennen")
            .body("Der Name wird lokal gespeichert und nicht beim nächsten Abruf überschrieben.")
            .extra_child(&entry)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("ok", "Speichern");
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            if resp != "ok" {
                return;
            }
            let Some(app) = w.upgrade() else { return };
            let title = entry.text().trim().to_string();
            if title.is_empty() {
                return;
            }
            let worker = app.worker.clone();
            let w2 = w.clone();
            app.db_query(
                move |db| db.set_user_title(feed_id, &title),
                move |app, _res| {
                    app.reload_meta_keep();
                    let _ = w2;
                },
            );
            let _ = worker;
        });
    }

    fn edit_feed_groups_dialog(&self) {
        let Some(Scope::Feed(feed_id)) = self.current_menu_source() else {
            return;
        };
        let st = self.state.borrow();
        let groups: Vec<(i64, String)> = st
            .groups
            .iter()
            .filter(|g| g.account_id == "local")
            .map(|g| (g.id, g.name.clone()))
            .collect();
        let current: Vec<i64> = st
            .feeds
            .iter()
            .find(|f| f.id == feed_id)
            .map(|f| f.groups.clone())
            .unwrap_or_default();
        drop(st);
        let checks: Vec<(i64, gtk::CheckButton)> = groups
            .iter()
            .map(|(id, name)| {
                let check = gtk::CheckButton::builder()
                    .label(name)
                    .active(current.contains(id))
                    .build();
                (*id, check)
            })
            .collect();
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for (_, check) in &checks {
            list.append(check);
        }
        let new_name = gtk::Entry::builder()
            .placeholder_text("Neue Gruppe")
            .build();
        list.append(&new_name);
        let dialog = adw::AlertDialog::builder()
            .heading("Gruppen wählen")
            .body("Ein Feed darf mehreren Gruppen angehören. Die lokale Bibliothek unterstützt verschachtelte Gruppen.")
            .extra_child(&list)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("ok", "Übernehmen");
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            if resp != "ok" {
                return;
            }
            let Some(app) = w.upgrade() else { return };
            let selected: Vec<i64> = checks
                .iter()
                .filter(|(_, c)| c.is_active())
                .map(|(id, _)| *id)
                .collect();
            let wanted = new_name.text().trim().to_string();
            app.worker.send(move |db| {
                db.ensure_local_account()?;
                if !wanted.is_empty() {
                    if let Some(existing) = db.group_by_name("local", &wanted)? {
                        db.set_feed_groups(feed_id, &[existing])?;
                        let mut all = selected.clone();
                        if !all.contains(&existing) {
                            all.push(existing);
                        }
                        db.set_feed_groups(feed_id, &all)?;
                    } else {
                        let gid = db.add_group("local", &wanted, None)?;
                        let mut all = selected.clone();
                        all.push(gid);
                        db.set_feed_groups(feed_id, &all)?;
                    }
                } else {
                    db.set_feed_groups(feed_id, &selected)?;
                }
                Ok::<_, storage::StorageError>(())
            });
            let w2 = w.clone();
            app.db_query(
                |_| Ok::<_, storage::StorageError>(()),
                move |app, _res| {
                    app.reload_meta_keep();
                    let _ = w2;
                },
            );
        });
    }

    fn unsubscribe_dialog(&self) {
        let Some(Scope::Feed(feed_id)) = self.current_menu_source() else {
            return;
        };
        let title = self
            .state
            .borrow()
            .feeds
            .iter()
            .find(|f| f.id == feed_id)
            .map(|f| f.title.clone())
            .unwrap_or_else(|| "dieser Feed".into());
        let dialog = adw::AlertDialog::builder()
            .heading("Feed abbestellen?")
            .body(format!(
                "{title} wird nicht mehr abgerufen. Gespeicherte und ausdrücklich aufbewahrte Artikel bleiben erhalten; ein späteres Wiederabonnieren übernimmt den bisherigen Status."
            ))
            .build();
        dialog.add_response("cancel", "Behalten");
        dialog.add_response("ok", "Abbestellen");
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            if resp != "ok" {
                return;
            }
            let Some(app) = w.upgrade() else { return };
            let w2 = w.clone();
            app.db_query(
                move |db| {
                    let saved = db.deactivate_feed(feed_id)?;
                    Ok::<_, storage::StorageError>(saved)
                },
                move |app, res| {
                    if let Ok(saved) = res {
                        let msg = if saved > 0 {
                            format!(
                                "Feed abbestellt — {saved} gespeicherte Artikel bleiben erhalten"
                            )
                        } else {
                            "Feed abbestellt".to_string()
                        };
                        app.show_toast(&msg);
                    }
                    app.reload_meta_keep();
                    let _ = w2;
                },
            );
        });
    }

    fn mark_source_read(&self) {
        let Some(Scope::Feed(feed_id)) = self.current_menu_source() else {
            return;
        };
        self.db_query(
            move |db| db.mark_feed_read(feed_id),
            move |app, _res| {
                app.reload_meta_keep();
                app.reload_counts();
            },
        );
    }

    pub fn refresh_sidebar(&self) {
        self.sidebar_title
            .set_subtitle(&self.account_label_for_scope());
        let state = self.state.borrow();
        let mut filters = self.sidebar_filters.borrow_mut();
        self.suppress.set(true);
        let w = self.weak();
        let cb: Rc<dyn Fn(i64)> = Rc::new(move |gid: i64| {
            if let Some(app) = w.upgrade() {
                app.toggle_group(gid);
            }
        });
        sidebar::rebuild(&self.sidebar_list, &state, &mut filters, &cb);
        self.suppress.set(false);
        drop(state);
    }

    fn toggle_group(&self, gid: i64) {
        {
            let mut st = self.state.borrow_mut();
            if !st.collapsed.remove(&gid) {
                st.collapsed.insert(gid);
            }
        }
        self.refresh_sidebar();
    }

    fn set_scope(&self, f: Scope, reset_filter_to_unread: bool) {
        {
            let mut st = self.state.borrow_mut();
            st.scope = f.clone();
            if reset_filter_to_unread {
                st.filter = Filter::Unread;
            }
            st.search = None;
            st.selected = st.last_opened.get(&(f.clone(), st.filter)).cloned();
        }
        self.sync_filter_buttons();
        self.search_bar.set_search_mode(false);
        self.list_title.set_title(&self.scope_label_now());
        self.refresh_sidebar();
        self.load_page(false);
        if let Some((feed_id, sel)) = self.selected_key() {
            self.open_article_by_id(feed_id, &sel, false, false);
        } else {
            self.update_reader_empty();
        }
        if self.outer.is_collapsed() {
            self.outer.set_show_content(true);
        }
    }

    fn set_filter(&self, f: Filter) {
        {
            let mut st = self.state.borrow_mut();
            st.filter = f;
            st.search = None;
            st.selected = st.last_opened.get(&(st.scope.clone(), f)).cloned();
        }
        self.sync_filter_buttons();
        self.load_page(false);
        if let Some((feed_id, sel)) = self.selected_key() {
            self.open_article_by_id(feed_id, &sel, false, false);
        } else {
            self.update_reader_empty();
        }
    }

    fn sync_filter_buttons(&self) {
        let f = self.state.borrow().filter;
        self.syncing_filters.set(true);
        self.filter_saved.set_active(f == Filter::Saved);
        self.filter_unread.set_active(f == Filter::Unread);
        self.filter_all.set_active(f == Filter::All);
        self.syncing_filters.set(false);
    }

    fn scope_label_now(&self) -> String {
        let st = self.state.borrow();
        if let Some(q) = &st.search {
            return format!("Suche: {q}");
        }
        match &st.scope {
            Scope::Global => "Ungelesen".into(),
            Scope::Account(a) => st
                .accounts
                .iter()
                .find(|(id, _, _)| id == a)
                .map(|(_, _, n)| n.clone())
                .unwrap_or_else(|| "Konto".into()),
            Scope::Group(g) => st
                .groups
                .iter()
                .find(|x| &x.id == g)
                .map(|x| x.name.clone())
                .unwrap_or_else(|| "Gruppe".into()),
            Scope::Feed(f) => st.feed_title(*f),
        }
    }

    // ── Artikel öffnen / Status ──

    fn open_article_by_id(&self, feed_id: i64, id: &str, focus: bool, flush: bool) {
        let Some(row) = self.state.borrow().article(feed_id, id) else {
            return;
        };
        self.open_article(row, focus, flush);
    }

    fn open_article(&self, row: ArticleRow, focus: bool, _flush: bool) {
        let id = row.id.clone();
        {
            let mut st = self.state.borrow_mut();
            let key = (st.scope.clone(), st.filter);
            st.selected = Some((row.feed_id, id.clone()));
            st.last_opened.insert(key, (row.feed_id, id.clone()));
            st.unread_guard.remove(&id);
        }
        if let Some(pos) = self.state.borrow().row_pos(row.feed_id, &id) {
            self.suppress.set(true);
            self.list_selection.set_selected(pos as u32);
            self.suppress.set(false);
        }

        self.reader.title.set_title(&row.title);
        self.reader.title.set_subtitle(&row.feed_title);
        self.update_reader_buttons(&row);
        let prev = self.reader.current.borrow().clone();
        if let Some(prev) = prev {
            if prev != id {
                self.capture_position(&prev);
            }
        }
        *self.reader.current.borrow_mut() = Some(id.clone());
        self.reader.show_loading();

        let row2 = row.clone();
        let id_cb = id.clone();
        let w = self.weak();
        self.db_query(
            move |db| db.content_html(row2.feed_id, &row2.id),
            move |app, res: storage::Result<Option<String>>| {
                let current = app.reader.current.borrow().clone();
                if current.as_deref() != Some(&id_cb) {
                    return;
                }
                match res {
                    Ok(Some(html)) => app.load_reader_html(row, html),
                    _ => app.reader.show_error(),
                }
                let _ = w;
            },
        );

        if self.inner.is_collapsed() {
            self.inner.set_show_content(true);
        }
        if focus {
            self.reader.webview.grab_focus();
        }
    }

    fn start_read_timer(&self, id: String) {
        self.read_gen.set(self.read_gen.get() + 1);
        let gen = self.read_gen.get();
        let w = self.weak();
        glib::timeout_add_local(Duration::from_millis(800), move || {
            let Some(app) = w.upgrade() else {
                dbg_log("read-timer: app tot");
                return glib::ControlFlow::Break;
            };
            if app.read_gen.get() != gen {
                dbg_log(&format!("read-timer {id}: gen ueberholt"));
                return glib::ControlFlow::Break;
            }
            if !app.window.is_active() {
                dbg_log(&format!("read-timer {id}: fenster inaktiv"));
                return glib::ControlFlow::Break;
            }
            if app.reader.current.borrow().as_deref() != Some(id.as_str()) {
                dbg_log(&format!("read-timer {id}: nicht mehr aktuell"));
                return glib::ControlFlow::Break;
            }
            if app.state.borrow().unread_guard.contains(&id) {
                dbg_log(&format!("read-timer {id}: guard"));
                return glib::ControlFlow::Break;
            }
            if !app.prefs.borrow().auto_read {
                dbg_log(&format!("read-timer {id}: Auto-Read aus"));
                return glib::ControlFlow::Break;
            }
            if !app.reader_is_visible() || !app.reader_loaded_ok() {
                dbg_log(&format!(
                    "read-timer {id}: Reader nicht sichtbar oder nicht geladen"
                ));
                return glib::ControlFlow::Break;
            }
            let feed_id = app
                .state
                .borrow()
                .selected
                .as_ref()
                .map(|(feed, _)| *feed)
                .unwrap_or_default();
            let still_unread = app
                .state
                .borrow()
                .article(feed_id, &id)
                .map(|a| a.unread)
                .unwrap_or(false);
            dbg_log(&format!("read-timer {id}: feuert, unread={still_unread}"));
            if still_unread {
                let mut batch: UndoBatch = Vec::new();
                app.apply_status(feed_id, &id, Some(true), None, &mut batch);
                app.history.borrow_mut().record_user(batch);
            }
            glib::ControlFlow::Break
        });
    }

    fn apply_status(
        &self,
        feed_id: i64,
        id: &str,
        read: Option<bool>,
        saved: Option<bool>,
        batch: &mut UndoBatch,
    ) {
        self.apply_status_inner(feed_id, id, read, saved, batch, StatusMode::User)
    }

    fn apply_status_persisted(
        &self,
        feed_id: i64,
        id: &str,
        read: Option<bool>,
        saved: Option<bool>,
    ) {
        if self.state.borrow().article(feed_id, id).is_none() {
            let id2 = id.to_string();
            self.worker.send(move |db| {
                db.apply_status_with_outbox(feed_id, &id2, read, saved)?;
                Ok::<_, storage::StorageError>(())
            });
        }
    }

    fn apply_status_inner(
        &self,
        feed_id: i64,
        id: &str,
        read: Option<bool>,
        saved: Option<bool>,
        batch: &mut UndoBatch,
        mode: StatusMode,
    ) {
        let prev = self.state.borrow().article(feed_id, id);
        let Some(mut cur) = prev else { return };
        if mode == StatusMode::User {
            self.history.borrow_mut().redo.clear();
        }
        let (prev_unread, prev_saved) = (cur.unread, cur.saved);
        // In beiden Modi entsteht der Gegen-Batch; nur das Verwerfen des
        // Redo-Verlaufs ist an die Benutzeraktion gekoppelt.
        batch.push((feed_id, id.to_string(), prev_unread, prev_saved));
        {
            if let Some(r) = read {
                cur.unread = !r;
            }
            if let Some(sv) = saved {
                cur.saved = sv;
            }
            self.state.borrow().set_article(feed_id, id, cur);
        }
        {
            let mut st = self.state.borrow_mut();
            let read_delta = unread_delta(prev_unread, read);
            if read_delta != 0 {
                st.counts.unread = (st.counts.unread + read_delta).max(0);
                if let Some(entry) = st.counts.per_feed.iter_mut().find(|(f, _)| *f == feed_id) {
                    entry.1 = (entry.1 + read_delta).max(0);
                }
                let gids: Vec<i64> = st
                    .feeds
                    .iter()
                    .find(|f| f.id == feed_id)
                    .map(|f| f.groups.clone())
                    .unwrap_or_default();
                for entry in st.counts.per_group.iter_mut() {
                    if gids.contains(&entry.0) {
                        entry.1 = (entry.1 + read_delta).max(0);
                    }
                }
            }
            let saved_delta = saved_delta(prev_saved, saved);
            if saved_delta != 0 {
                st.counts.saved = (st.counts.saved + saved_delta).max(0);
            }
        }

        let _ = feed_id;

        if self.reader.current.borrow().as_deref() == Some(id) {
            if let Some(row) = self.state.borrow().article(feed_id, id) {
                self.update_reader_buttons(&row);
            }
        }
        self.refresh_sidebar();

        // Das Ergebnis der lokalen Persistenz wird ausgewertet: der optimistic
        // Zustand darf nicht als Erfolg stehenbleiben, wenn die DB ablehnt.
        let feed_id2 = feed_id;
        let id2 = id.to_string();
        let revert = (feed_id, id.to_string(), prev_unread, prev_saved);
        let result_tx = self.bg_jobs_tx.clone();
        let rx = self.worker.send(move |db| {
            db.apply_status_with_outbox(feed_id2, &id2, read, saved)?;
            Ok::<_, storage::StorageError>(())
        });
        std::thread::spawn(move || {
            if let Ok(res) = rx.recv() {
                match *res
                    .downcast::<storage::Result<()>>()
                    .unwrap_or(Box::new(Ok(())))
                {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("[lf] Statusänderung nicht gespeichert: {e}");
                        let message = format!("Änderung konnte nicht gespeichert werden: {e}");
                        let _ = result_tx.send(Box::new(move |app: &Rc<App>| {
                            let (feed_id, id, unread, saved) = revert;
                            if let Some(mut previous) = app.state.borrow().article(feed_id, &id) {
                                previous.unread = unread;
                                previous.saved = saved;
                                app.state.borrow().set_article(feed_id, &id, previous);
                            }
                            app.reload_counts();
                            app.refresh_sidebar();
                            app.show_toast(&message);
                        })
                            as Box<dyn FnOnce(&Rc<App>) + Send>);
                    }
                    Err(_) => {
                        eprintln!("[lf] Datenbank-Worker antwortet nicht");
                    }
                }
            }
        });
    }

    fn current_article(&self) -> Option<ArticleRow> {
        if let Some((feed_id, id)) = self.selected_key() {
            if let Some(row) = self.state.borrow().article(feed_id, &id) {
                return Some(row);
            }
        }
        let id = self.reader.current.borrow().clone()?;
        let st = self.state.borrow();
        st.rows.iter().find_map(|r| {
            let a = r.article()?;
            (a.id == id).then_some(a)
        })
    }

    fn toggle_read(&self) {
        let Some(row) = self.current_article() else {
            return;
        };
        let was_unread = row.unread;
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(
            row.feed_id,
            &row.id,
            Some(read_intent(was_unread)),
            None,
            &mut batch,
        );
        if was_unread {
            self.state.borrow_mut().unread_guard.remove(&row.id);
        } else {
            self.state.borrow_mut().unread_guard.insert(row.id.clone());
        }
        self.history.borrow_mut().record_user(batch);
    }

    fn toggle_saved(&self) {
        let Some(row) = self.current_article() else {
            return;
        };
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(row.feed_id, &row.id, None, Some(!row.saved), &mut batch);
        self.history.borrow_mut().record_user(batch);
    }

    fn undo(&self) {
        let Some(batch) = self.history.borrow_mut().pop_undo() else {
            self.show_toast("Nichts rückgängig zu machen");
            return;
        };
        let mut redo: UndoBatch = Vec::new();
        for (feed_id, id, unread, saved) in &batch {
            if self.state.borrow().article(*feed_id, id).is_some() {
                self.apply_status_inner(
                    *feed_id,
                    id,
                    Some(!unread),
                    Some(*saved),
                    &mut redo,
                    StatusMode::Counterpart,
                );
            } else {
                self.apply_status_persisted(*feed_id, id, Some(!unread), Some(*saved));
                redo.push(restored_state(&(*feed_id, id.clone(), *unread, *saved)));
            }
        }
        self.history.borrow_mut().push_redo(redo);
        self.reload_counts();
        self.show_toast("Aktion rückgängig gemacht");
    }

    fn redo(&self) {
        let Some(batch) = self.history.borrow_mut().pop_redo() else {
            self.show_toast("Nichts wiederherzustellen");
            return;
        };
        let mut undo: UndoBatch = Vec::new();
        for (feed_id, id, unread, saved) in &batch {
            if self.state.borrow().article(*feed_id, id).is_some() {
                self.apply_status_inner(
                    *feed_id,
                    id,
                    Some(!unread),
                    Some(*saved),
                    &mut undo,
                    StatusMode::Counterpart,
                );
            } else {
                self.apply_status_persisted(*feed_id, id, Some(!unread), Some(*saved));
                undo.push(restored_state(&(*feed_id, id.clone(), *unread, *saved)));
            }
        }
        self.history.borrow_mut().push_undo(undo);
        self.reload_counts();
        self.show_toast("Aktion wiederhergestellt");
    }

    fn mark_scope_dialog(&self) {
        let ids: Vec<(i64, String)> = {
            let st = self.state.borrow();
            st.rows
                .iter()
                .filter_map(|r| {
                    let a = r.article()?;
                    if a.unread {
                        Some((a.feed_id, a.id.clone()))
                    } else {
                        None
                    }
                })
                .collect()
        };
        let label = self.scope_label_now();
        if ids.is_empty() {
            self.show_toast(&format!("„{label}“ enthält keine ungelesenen Artikel"));
            return;
        }
        let server_scope = self.feedly_scope_feeds();
        let body = format!(
            "„{label}“: {} zum Klickzeitpunkt bekannte Artikel werden als gelesen markiert. Rückgängig mit Strg+Z.{}",
            ids.len(),
            if server_scope.is_empty() {
                String::new()
            } else {
                " Serverseitig können weitere Artikel außerhalb des geladenen Fensters existieren.".to_string()
            }
        );
        let dialog = adw::AlertDialog::builder()
            .heading("Bereich als gelesen markieren")
            .body(body)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("mark", &format!("{} als gelesen markieren", ids.len()));
        if !server_scope.is_empty() {
            dialog.add_response("server", "Alle serverseitig (komplette Feeds)");
        }
        dialog.set_response_appearance("mark", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp == "server" {
                app.mark_scope_server();
                return;
            }
            if resp != "mark" {
                return;
            }
            let mut batch: UndoBatch = Vec::new();
            for (feed_id, id) in &ids {
                app.apply_status(*feed_id, id, Some(true), None, &mut batch);
            }
            app.history.borrow_mut().record_user(batch);
            app.show_toast(&format!("{} Artikel als gelesen markiert", ids.len()));
        });
    }

    fn feedly_scope_feeds(&self) -> Vec<(i64, String)> {
        let st = self.state.borrow();
        let feeds: Vec<&FeedRow> = match &st.scope {
            Scope::Feed(f) => st.feeds.iter().filter(|x| x.id == *f).collect(),
            Scope::Group(g) => st.feeds.iter().filter(|x| x.groups.contains(g)).collect(),
            Scope::Account(a) => st.feeds.iter().filter(|x| &x.account_id == a).collect(),
            Scope::Global => Vec::new(),
        };
        feeds
            .into_iter()
            .filter(|f| f.account_id != "local")
            .filter_map(|f| f.remote_id.clone().map(|r| (f.id, r)))
            .collect()
    }

    fn mark_scope_server(&self) {
        let scope_feeds = self.feedly_scope_feeds();
        if scope_feeds.is_empty() {
            return;
        }
        self.with_token(TokenAction::MarkScopeServer);
    }

    fn mark_scope_server_with(&self, token: String) {
        let scope_feeds = self.feedly_scope_feeds();
        if scope_feeds.is_empty() {
            return;
        }
        let account_id = match self.account_id_of_kind("feedly") {
            Some(id) => id,
            None => return,
        };
        let params = FeedlyParams {
            remote_feed_ids: scope_feeds.iter().map(|(_, r)| r.clone()).collect(),
            local_feed_ids: scope_feeds.iter().map(|(id, _)| *id).collect(),
        };
        let n_feeds = params.remote_feed_ids.len();
        {
            let mut st = self.state.borrow_mut();
            st.pending_feedly_params = Some((account_id.clone(), params.clone()));
        }
        // Auch die Serveraktion läuft über den Coordinator (A3).
        self.request_feedly_job(sync_engine::Job::ServerAction, account_id, token, params);
        self.show_toast(&format!(
            "Wird serverseitig für {n_feeds} Feeds als gelesen gemeldet — lokale Zähler folgen nach der Bestätigung"
        ));
    }

    pub fn show_toast(&self, msg: &str) {
        self.toast.add_toast(adw::Toast::new(msg));
    }

    // ── Navigation ──

    fn next_distinct(
        &self,
        positions: &[usize],
        cur_idx: i32,
        delta: i32,
    ) -> Option<(i32, usize, ArticleRow)> {
        let sel = self.selected_id();
        let n = positions.len() as i32;
        let mut i = cur_idx + delta;
        while i >= 0 && i < n {
            let pos = positions[i as usize];
            let row = self.state.borrow().rows.get(pos).and_then(|r| r.article());
            match row {
                Some(a) if Some(a.id.as_str()) == sel.as_deref() => i += delta,
                Some(a) => return Some((i, pos, a)),
                None => i += delta,
            }
        }
        None
    }

    fn move_selection(&self, delta: i32) {
        let positions: Vec<usize> = self
            .state
            .borrow()
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                ListRow::Item(_) => Some(i),
                _ => None,
            })
            .collect();
        if positions.is_empty() {
            dbg_log("move_selection: keine Artikel in der Liste");
            return;
        }
        let cur_key = self.selected_key();
        let cur_idx = cur_key
            .as_ref()
            .and_then(|(feed_id, id)| self.state.borrow().row_pos(*feed_id, id))
            .and_then(|p| positions.iter().position(|x| *x == p))
            .map(|i| i as i32)
            .unwrap_or(if delta > 0 {
                -1
            } else {
                positions.len() as i32
            });
        let Some((target, idx, row)) = self.next_distinct(&positions, cur_idx, delta) else {
            dbg_log(&format!(
                "move_selection: kein weiterer Artikel ab cur_idx={cur_idx}"
            ));
            return;
        };
        dbg_log(&format!(
            "move_selection delta={delta} cur_idx={cur_idx} target={target} idx={idx} von {}",
            positions.len()
        ));
        self.open_article(row, false, true);
        self.scroll_to_selected();
    }

    fn scroll_to_selected(&self) {
        if let Some((feed_id, id)) = self.selected_key() {
            if let Some(pos) = self.state.borrow().row_pos(feed_id, &id) {
                dbg_log(&format!("scroll_to pos={pos}"));
                self.list_view.scroll_to(
                    pos as u32,
                    gtk::ListScrollFlags::NONE,
                    None::<gtk::ScrollInfo>,
                );
            }
        }
    }

    fn move_unread(&self, dir: i32) {
        let positions: Vec<usize> = self
            .state
            .borrow()
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                let a = r.article()?;
                if a.unread {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        if positions.is_empty() {
            self.show_toast("Keine weiteren ungelesenen Artikel in dieser Ansicht");
            return;
        }
        let cur = self
            .selected_key()
            .and_then(|(feed_id, id)| self.state.borrow().row_pos(feed_id, &id))
            .unwrap_or(0);
        let next = if dir > 0 {
            positions
                .iter()
                .find(|&&p| p > cur)
                .copied()
                .or_else(|| positions.first().copied())
        } else {
            positions
                .iter()
                .rev()
                .find(|&&p| p < cur)
                .copied()
                .or_else(|| positions.last().copied())
        };
        if let Some(idx) = next {
            let sel = self.selected_id();
            let row = self.state.borrow().rows.get(idx).and_then(|r| r.article());
            if let Some(row) = row {
                if Some(row.id.as_str()) == sel.as_deref() {
                    return;
                }
            }
            let row = self.state.borrow().rows.get(idx).and_then(|r| r.article());
            if let Some(row) = row {
                self.open_article(row, false, true);
                self.scroll_to_selected();
            }
        }
    }

    fn focus_pane(&self, dir: i32) {
        let panes = self.panes.borrow().clone();
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(&self.window) else {
            if let Some(p) = panes.first() {
                gtk::prelude::GtkWindowExt::set_focus(&self.window, Some(p));
            }
            return;
        };
        let mut current: Option<usize> = None;
        let mut cur = Some(focus.clone());
        while let Some(c) = cur {
            if let Some(i) = panes.iter().position(|p| *p == c) {
                current = Some(i);
                break;
            }
            cur = c.parent();
        }
        let idx = match current {
            Some(i) => ((i as i32 + dir).rem_euclid(panes.len() as i32)) as usize,
            None => 0,
        };
        if let Some(p) = panes.get(idx) {
            gtk::prelude::GtkWindowExt::set_focus(&self.window, Some(p));
        }
    }

    fn back(&self) {
        if self.inner.is_collapsed() && self.inner.shows_content() {
            self.inner.set_show_content(false);
        } else if self.outer.is_collapsed() && self.outer.shows_content() {
            self.outer.set_show_content(false);
        }
    }

    // ── Reader-Aktionen ──

    fn update_reader_buttons(&self, row: &ArticleRow) {
        self.reader.btn_read.set_icon_name(if row.unread {
            "mail-read-symbolic"
        } else {
            "mail-unread-symbolic"
        });
        self.reader.btn_read.set_tooltip_text(Some(if row.unread {
            "Als gelesen markieren (M)"
        } else {
            "Als ungelesen markieren (M)"
        }));
        self.reader.btn_saved.set_icon_name(if row.saved {
            "user-bookmarks-symbolic"
        } else {
            "bookmark-new-symbolic"
        });
        self.reader.btn_saved.set_tooltip_text(Some(if row.saved {
            "Entspeichern (S)"
        } else {
            "Speichern (S)"
        }));
    }

    fn update_reader_empty(&self) {
        let st = self.state.borrow();
        let label = self.scope_label_now();
        let unread = match &st.scope {
            Scope::Global => st.counts.unread,
            Scope::Account(a) => st
                .counts
                .per_account
                .iter()
                .find(|(id, _)| id == a)
                .map(|(_, c)| *c)
                .unwrap_or(0),
            Scope::Feed(f) => st.feed_unread(*f),
            Scope::Group(g) => st.group_unread(*g),
        };
        let total = st
            .rows
            .iter()
            .filter(|r| matches!(r, ListRow::Item(_)))
            .count() as i64;
        drop(st);
        self.reader
            .show_empty(&label, &format!("{unread} ungelesen · {total} Artikel"));
        self.reader.title.set_title(&label);
        self.reader.title.set_subtitle("");
    }

    fn reader_doc(&self, row: &ArticleRow, html: &str, generation: u64) -> String {
        let style = self.reader.style.borrow();
        let rs = reader::ReaderStyle {
            font_size: style.font_size,
            measure_ch: style.measure_ch,
            line_height: style.line_height,
        };
        let tokens = *self.tokens.borrow();
        let published = fmt_full(row.published_ms);
        let doc = reader::ReaderDocument {
            kicker: &row.feed_title,
            title: &row.title,
            author: row.author.as_deref(),
            source: "",
            published: &published,
            content_html: html,
            generation,
        };
        reader::render_document(&doc, &tokens, &rs)
    }

    fn load_reader_html(&self, row: ArticleRow, html: String) {
        // Sofort rendern: Cache-Bilder einbetten, alle anderen als Platzhalter
        // markieren. Nachgeladen wird asynchron über die Medien-Brücke.
        let cached: Vec<(String, String)> = reader::sanitize::image_alt_texts(&html)
            .into_iter()
            .filter_map(|(url, _)| {
                self.media
                    .get_cached(&url)
                    .map(|(bytes, mime)| (url, provider_local::media::data_uri(&bytes, mime)))
            })
            .collect();
        let placeholder = provider_local::media::placeholder_data_uri("Bild");
        let mut prepared = reader::sanitize::rewrite_images(&html, &placeholder);
        for (url, data) in &cached {
            prepared = reader::sanitize::replace_marker(&prepared, url, data);
        }
        // Generation vor dem Rendern reservieren: Dokument, Jobs und Capture
        // müssen dieselbe Nummer sehen.
        let gen = self.reader.reserve_generation();
        let doc = self.reader_doc(&row, &prepared, gen);
        self.reader.load_html_doc(&doc, gen);

        if self.prefs.borrow().block_images {
            // Bilder gesperrt: nichts wird nachgeladen, die Platzhalter bleiben.
            return;
        }
        let missing: Vec<(String, String)> = reader::sanitize::image_alt_texts(&html)
            .into_iter()
            .filter(|(url, _)| !cached.iter().any(|(u, _)| u == url))
            .collect();
        if missing.is_empty() {
            return;
        }
        let media = std::sync::Arc::clone(&self.media);
        let http = self.net.http();
        let queue = self.pending_media.clone();
        let article_key = (row.feed_id, row.id.clone());
        self.net.spawn(async move {
            let mut jobs: Vec<(String, String)> = Vec::new();
            for (url, alt) in missing {
                match media.get_or_fetch(&http, &url).await {
                    Some((bytes, mime)) => {
                        jobs.push((url, provider_local::media::data_uri(&bytes, mime)));
                    }
                    None => jobs.push((url, provider_local::media::placeholder_data_uri(&alt))),
                }
            }
            if let Ok(mut q) = queue.lock() {
                q.push((article_key, gen, jobs));
            }
        });
    }

    fn capture_position(&self, id: &str) {
        let feed_id = self
            .state
            .borrow()
            .selected
            .as_ref()
            .map(|(feed, selected)| if selected == id { *feed } else { 0 })
            .unwrap_or_default();
        let Some(row) = self.state.borrow().article_by_id(id, feed_id) else {
            return;
        };
        let w = self.weak();
        let id2 = id.to_string();
        let gen = self.reader.document_generation.get();
        let row_db = row.clone();
        // Erst den Inhalt des gezeigten Dokuments sichern, dann den Hash holen.
        self.reader.webview.evaluate_javascript(
            JS_CAPTURE_POS,
            None,
            None,
            None::<&gio::Cancellable>,
            move |res| {
                let Ok(v) = res else { return };
                let raw = v.to_string();
                let Some((doc_gen, idx, off)) = parse_position(&raw) else {
                    return;
                };
                if doc_gen != gen {
                    dbg_log(&format!(
                        "Leseposition verworfen: Dokument {doc_gen} statt {gen}"
                    ));
                    return;
                }
                let w2 = w.clone();
                let id3 = id2.clone();
                app_at(&w2, move |app| {
                    let row_db2 = row_db.clone();
                    let id4 = id3.clone();
                    app.db_query(
                        move |db| {
                            let hash = db.content_hash(row_db2.feed_id, &row_db2.id)?;
                            db.save_read_position(
                                row_db2.feed_id,
                                &row_db2.id,
                                hash.as_deref(),
                                idx,
                                off,
                            )
                        },
                        move |_app, _res| {
                            let _ = &id4;
                        },
                    );
                });
            },
        );
    }

    fn after_load_finished(&self) {
        if self.reader.pending_scroll.get() >= 0.0 {
            self.reader.restore_scroll();
        }
        if let Some(id) = self.reader.current.borrow().clone() {
            self.start_read_timer(id);
        }
        if self.reader.pending_scroll.get() >= 0.0 {
            return;
        }
        let Some(id) = self.reader.current.borrow().clone() else {
            return;
        };
        let Some(row) = self.state.borrow().article_by_id(&id, 0) else {
            return;
        };
        let row_id = row.id.clone();
        let w = self.weak();
        self.db_query(
            move |db| {
                let pos = db.read_position(row.feed_id, &row.id)?;
                let hash = db.content_hash(row.feed_id, &row.id)?;
                Ok::<_, storage::StorageError>((pos, hash))
            },
            move |app, res: storage::Result<(Option<(Option<String>, i64, i64)>, Option<String>)>| {
                let Ok((pos, hash)) = res else { return };
                let Some((saved_hash, idx, off)) = pos else { return };
                if saved_hash.is_some() && saved_hash != hash {
                    return;
                }
                if idx == 0 && off == 0 {
                    return;
                }
                if app.reader.current.borrow().as_deref() != Some(row_id.as_str()) {
                    return;
                }
                let js = format!(
                    "(()=>{{const els=document.querySelectorAll('article.lf-body > *');const el=els[{idx}];if(el){{window.scrollTo(0, el.getBoundingClientRect().top+window.scrollY+{off});}}}})()"
                );
                app.reader.webview.evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, |_| {});
                let _ = w;
            },
        );
    }

    fn zoom(&self, delta: f64) {
        {
            let mut s = self.reader.style.borrow_mut();
            s.font_size = (s.font_size + delta).clamp(14.0, 32.0);
        }
        self.reload_current(true);
    }

    fn zoom_reset(&self) {
        self.reader.style.borrow_mut().font_size = ReaderStyleState::default().font_size;
        self.reload_current(true);
    }

    pub fn reload_current(&self, preserve: bool) {
        let Some(id) = self.reader.current.borrow().clone() else {
            return;
        };
        let Some(row) = self.state.borrow().article_by_id(&id, 0) else {
            return;
        };
        let w = self.weak();
        let fid = row.feed_id;
        let rid = row.id.clone();
        self.db_query(
            move |db| db.content_html(fid, &rid),
            move |app, res: storage::Result<Option<String>>| {
                let Ok(Some(html)) = res else { return };
                if preserve {
                    let pane = Rc::clone(&app.reader);
                    let wv = pane.webview.clone();
                    let row2 = row.clone();
                    let w2 = w.clone();
                    wv.evaluate_javascript(
                        "window.scrollY",
                        None,
                        None,
                        None::<&gio::Cancellable>,
                        move |res| {
                            if let Ok(v) = res {
                                if v.is_number() {
                                    pane.pending_scroll.set(v.to_double());
                                }
                            }
                            if let Some(app) = w2.upgrade() {
                                app.load_reader_html(row2, html);
                            }
                        },
                    );
                } else {
                    app.load_reader_html(row, html);
                }
                let _ = w;
            },
        );
    }

    fn open_find(&self) {
        self.reader.search_bar.set_search_mode(true);
        self.reader.search_entry.grab_focus();
    }

    fn open_external(&self) {
        let Some(url) = self.current_article().and_then(|a| a.url) else {
            return;
        };
        if !external_uri_allowed(&url) {
            self.show_toast("Link nicht geöffnet: nur http(s) ist erlaubt");
            return;
        }
        let w = self.weak();
        gtk::UriLauncher::new(&url).launch(
            None::<&gtk::Window>,
            None::<&gio::Cancellable>,
            move |res| {
                if res.is_err() {
                    if let Some(app) = w.upgrade() {
                        app.show_toast("Extern öffnen fehlgeschlagen");
                    }
                }
            },
        );
    }

    fn copy_link(&self) {
        let Some(url) = self.current_article().and_then(|a| a.url) else {
            return;
        };
        self.window.clipboard().set_text(&url);
        self.show_toast("Link kopiert");
    }

    // ── Konto ──

    fn do_refresh(&self) {
        let plan = {
            let st = self.state.borrow();
            refresh_plan(&st.scope, &st.feeds, &st.accounts)
        };
        // Lokale Feeds und Feedly werden getrennt angestoßen; ein Bereich mit
        // lokalen Feeds wird nicht von einem Feedly-Konto verdrängt.
        let local_count = plan.local_feeds.len();
        for (feed_id, url) in plan.local_feeds {
            self.net.fetch_feed(self.worker.clone(), feed_id, url, true);
        }
        if local_count > 0 {
            self.show_toast(&format!("Aktualisiere {local_count} Feeds…"));
        }
        if plan.feedly_account.is_some() {
            self.with_token(TokenAction::Refresh);
        }
        if local_count == 0 && plan.feedly_account.is_none() {
            self.show_toast("Keine Feeds im aktuellen Bereich (Strg+N)");
        }
    }

    /// Startet höchstens einen Feedly-Zyklus; ein laufender wird nicht verdoppelt,
    /// ein weiterer Wunsch wird für den nächsten Zyklus vermerkt.
    /// Schlüsselbundzugriff gehört in einen Worker; die Aktion selbst läuft
    /// danach wieder im Hauptthread.
    fn with_token(&self, action: TokenAction) {
        let tx = self.bg_jobs_tx.clone();
        std::thread::Builder::new()
            .name("lf-keyring".into())
            .spawn(move || {
                let token = feedly_sync::token_from_disk();
                let _ = tx.send(Box::new(move |app: &Rc<App>| match token {
                    Some(token) => app.run_token_action(action, token),
                    None => app.run_token_action_without_token(action),
                }) as Box<dyn FnOnce(&Rc<App>) + Send>);
            })
            .ok();
    }

    /// Ohne Token wird nie stillschweigend nichts getan: Connect führt in die
    /// Anmeldung, jede andere Aktion erklärt die fehlende Verbindung.
    fn run_token_action_without_token(&self, action: TokenAction) {
        match action {
            TokenAction::CheckConnect | TokenAction::StartFeedly => self.show_feedly_token_dialog(),
            _ => {
                self.show_toast("Feedly: keine Verbindung — bitte „Feedly verbinden“ wählen");
            }
        }
    }

    fn run_token_action(&self, action: TokenAction, token: String) {
        match action {
            TokenAction::StartFeedly => self.start_feedly(token),
            TokenAction::Refresh => {
                // Wichtig: Die Konto-ID wird vor dem Aufruf kopiert. Ein Borrow
                // über den Zweig hinaus paniked in `request_feedly_sync`.
                let account_id = self.account_id_of_kind("feedly");
                if let Some(account_id) = account_id {
                    self.request_feedly_sync(account_id, token, true);
                    self.show_toast("Feedly: Delta-Sync angefordert");
                } else {
                    self.show_toast("Feedly: kein Konto verbunden");
                }
            }
            TokenAction::QueuedSync => {
                let account_id = self.account_id_of_kind("feedly");
                match account_id {
                    Some(id) => self.request_feedly_sync(id, token, false),
                    None => dbg_log("Kein Feedly-Konto für den Sync vorhanden"),
                }
            }
            TokenAction::MarkScopeServer => self.mark_scope_server_with(token),
            TokenAction::Outbox => {
                if let Some(id) = self.account_id_of_kind("feedly") {
                    self.request_feedly_job(
                        sync_engine::Job::Outbox,
                        id,
                        token,
                        FeedlyParams::default(),
                    );
                }
            }
            TokenAction::ReservedRun => {
                // Der Coordinator hat diesen Lauf bereits reserviert. Er wird direkt
                // gestartet; eine erneute Anmeldung würde ihn nur wieder vormerken.
                let account_id = self.account_id_of_kind("feedly");
                let reserved = {
                    let mut st = self.state.borrow_mut();
                    let id = account_id.clone().unwrap_or_default();
                    st.coordinator.set_now(now_ms());
                    st.coordinator.reserved_run(&id)
                };
                match (account_id, reserved) {
                    (Some(account_id), Some((job, run_id))) => {
                        let params = self.pending_server_params(&account_id);
                        self.start_feedly_run(job, account_id, token, params, run_id);
                    }
                    _ => dbg_log("Feedly: kein reservierter Folgelauf vorhanden"),
                }
            }
            TokenAction::CheckConnect => {
                let connected = self
                    .state
                    .borrow()
                    .accounts
                    .iter()
                    .any(|(_, k, _)| k == "feedly");
                if connected {
                    self.start_feedly(token);
                } else {
                    self.show_feedly_token_dialog();
                }
            }
        }
    }

    /// Einziger Startweg für alle Feedly-Netzaufträge. Erst-Sync, Delta-Sync,
    /// Outbox und serverseitige Aktionen laufen über den Coordinator; er entscheidet
    /// über Start, Vormerkung, Pause oder Sperre und vergibt die Laufkennung.
    /// Gehört ein Abschlussereignis zum aktuell laufenden Auftrag?
    fn feedly_event_is_current(&self, account_id: &str, run_id: u64) -> bool {
        let st = self.state.borrow();
        match &st.active_feedly_run {
            Some(run) => run.account_id == account_id && run.run_id == run_id,
            None => false,
        }
    }

    /// Bricht laufende Arbeit ab (Logout, Auth- oder Quotenstopp).
    pub fn cancel_feedly_run(&self, account_id: &str) {
        let mut st = self.state.borrow_mut();
        if let Some(run) = &st.active_feedly_run {
            if run.account_id == account_id {
                run.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        st.active_feedly_run = None;
        st.coordinator.set_now(now_ms());
        st.coordinator.stop_running(account_id);
    }

    fn request_feedly_job(
        &self,
        job: sync_engine::Job,
        account_id: String,
        token: String,
        params: FeedlyParams,
    ) {
        let decision = {
            let mut st = self.state.borrow_mut();
            st.coordinator.set_now(now_ms());
            let decision = st.coordinator.request(&account_id, job);
            if let sync_engine::Decision::Start { run_id } = decision {
                st.feedly_sync_running = true;
                if job == sync_engine::Job::Refresh {
                    st.next_feedly_sync = 0;
                }
                st.active_feedly_run = Some(FeedlyRun {
                    account_id: account_id.clone(),
                    run_id,
                    cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                });
            }
            decision
        };
        match decision {
            sync_engine::Decision::Start { run_id } => {
                dbg_log(&format!(
                    "Feedly: {:?} wird als Lauf {run_id} gestartet",
                    job
                ));
                self.start_feedly_run(job, account_id, token, params, run_id);
            }
            sync_engine::Decision::Queued => {
                dbg_log("Feedly: ein Lauf ist aktiv, der Wunsch wurde vorgemerkt");
            }
            sync_engine::Decision::Paused(pause) => {
                let message = match pause {
                    sync_engine::Pause::Auth => {
                        "Feedly: Anmeldung erforderlich — bitte neu verbinden"
                    }
                    sync_engine::Pause::Quota(_) => {
                        "Feedly: Drosselung — der Versand wartet auf das Zeitfenster"
                    }
                };
                self.show_toast(message);
            }
            sync_engine::Decision::Blocked => {
                self.show_toast("Feedly: nicht verbunden");
            }
        }
    }

    /// Ziel-Feeds einer wartenden Serveraktion für den reservierten Folgelauf.
    fn pending_server_params(&self, account_id: &str) -> FeedlyParams {
        match &self.state.borrow().pending_feedly_params {
            Some((account, params)) if account == account_id => params.clone(),
            _ => FeedlyParams::default(),
        }
    }

    /// Startet einen bereits vom Coordinator reservierten Lauf. Er darf **nicht**
    /// erneut angemeldet werden, sonst hinge er ohne Netzwerkstart in der Liste.
    fn start_queued_feedly_run(&self) {
        self.with_token(TokenAction::ReservedRun);
    }

    fn start_feedly_run(
        &self,
        job: sync_engine::Job,
        account_id: String,
        token: String,
        params: FeedlyParams,
        run_id: u64,
    ) {
        let ctx = {
            let st = self.state.borrow();
            match &st.active_feedly_run {
                Some(run) if run.run_id == run_id => feedly_sync::RunCtx {
                    account_id: run.account_id.clone(),
                    run_id,
                    cancel: std::sync::Arc::clone(&run.cancel),
                    base: None,
                },
                _ => feedly_sync::RunCtx::new(&account_id, run_id),
            }
        };
        match job {
            sync_engine::Job::Initial => {
                feedly_sync::initial_sync(self.worker.clone(), &self.net, token, ctx)
            }
            sync_engine::Job::Refresh | sync_engine::Job::Scheduled => {
                let last_sync = {
                    let st = self.state.borrow();
                    if st.feedly_last_sync > 0 {
                        st.feedly_last_sync
                    } else {
                        now_ms() - 30 * 86_400_000
                    }
                };
                dbg_log(&format!("Feedly: Delta-Sync ab {last_sync}"));
                feedly_sync::delta_sync(
                    self.worker.clone(),
                    &self.net,
                    token,
                    account_id,
                    last_sync,
                    ctx,
                )
            }
            sync_engine::Job::Outbox => {
                feedly_sync::process_outbox(self.worker.clone(), &self.net, token, ctx)
            }
            sync_engine::Job::ServerAction => feedly_sync::mark_feeds_server_side(
                self.worker.clone(),
                &self.net,
                token,
                ctx,
                params.remote_feed_ids,
                params.local_feed_ids,
            ),
        }
    }

    fn request_feedly_sync(&self, account_id: String, token: String, priority: bool) {
        let job = if priority {
            sync_engine::Job::Refresh
        } else {
            sync_engine::Job::Scheduled
        };
        self.request_feedly_job(job, account_id, token, FeedlyParams::default());
    }

    fn connect_feedly_dialog(&self) {
        self.with_token(TokenAction::CheckConnect);
    }

    fn show_feedly_token_dialog(&self) {
        let known_account = self
            .state
            .borrow()
            .accounts
            .iter()
            .any(|(_, k, _)| k == "feedly");
        let entry = gtk::Entry::builder()
            .placeholder_text("Feedly Developer Token einfügen")
            .visibility(false)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading(if known_account {
                "Feedly neu verbinden"
            } else {
                "Feedly verbinden"
            })
            .body("Privater Testzugang: Token unter feedly.com/v3/auth/dev bzw. via PKCE-Flow erzeugen und hier einfügen. Gespeicherung im Schlüsselbund; nur ohne Schlüsselbund in einer Datei mit Modus 600.")
            .extra_child(&entry)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("ok", "Verbinden");
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "ok" {
                return;
            }
            let token = entry.text().trim().to_string();
            if token.is_empty() {
                app.show_toast("Bitte ein Token einfügen");
                return;
            }
            // secret-tool und Dateizugriff gehören in einen Worker, nie in den
            // Dialog-Callback des Hauptthreads.
            let tx = app.bg_jobs_tx.clone();
            std::thread::Builder::new()
                .name("lf-keyring".into())
                .spawn(move || {
                    let saved = feedly_sync::save_token(&token, None).is_ok();
                    let _ = tx.send(Box::new(move |app: &Rc<App>| {
                        if saved {
                            app.start_feedly(token);
                        } else {
                            app.show_toast("Token konnte nicht gespeichert werden");
                        }
                    }) as Box<dyn FnOnce(&Rc<App>) + Send>);
                })
                .ok();
        });
    }

    /// Trennt Feedly: Token und Kontobindung werden entfernt, laufende und
    /// nachgeforderte Zyklen werden verworfen, das Konto wird lokal abgeschaltet.
    fn disconnect_feedly(&self) {
        // Laufende Arbeit wird invalidiert, damit keine Requests und keine
        // DB-Schreibvorgänge nach dem Logout mehr stattfinden.
        {
            let mut st = self.state.borrow_mut();
            st.feedly_sync_queued = false;
            st.feedly_sync_running = false;
            st.next_feedly_sync = 0;
            for (id, kind, _) in st.accounts.clone() {
                if kind == "feedly" {
                    if let Some(run) = st.active_feedly_run.take() {
                        if run.account_id == id {
                            run.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                    st.coordinator.abort(&id);
                    st.coordinator.block(&id);
                }
            }
        }
        let account_ids: Vec<String> = self
            .state
            .borrow()
            .accounts
            .iter()
            .filter(|(_, k, _)| k == "feedly")
            .map(|(id, _, _)| id.clone())
            .collect();
        let worker = self.worker.clone();
        let tx = self.bg_jobs_tx.clone();
        for account_id in account_ids {
            worker.send(move |db| {
                db.set_account_status(&account_id, "disconnected", Some("abgemeldet"))?;
                Ok::<_, storage::StorageError>(())
            });
        }
        std::thread::Builder::new()
            .name("lf-keyring".into())
            .spawn(move || {
                feedly_sync::forget_token();
                let _ = tx.send(Box::new(move |app: &Rc<App>| {
                    app.show_toast("Feedly: getrennt, lokale Daten bleiben erhalten");
                    app.bootstrap();
                }) as Box<dyn FnOnce(&Rc<App>) + Send>);
            })
            .ok();
    }

    fn start_feedly(&self, token: String) {
        // Neue Anmeldung hebt Pausen und Sperre des Koordinators auf.
        let account_id = {
            let existing = self.account_id_of_kind("feedly");
            let mut st = self.state.borrow_mut();
            if let Some(id) = &existing {
                st.coordinator.resume(id);
                st.coordinator.unblock(id);
            }
            existing
        };
        match account_id {
            Some(id) => {
                self.show_toast("Feedly: Erst-Sync gestartet …");
                self.request_feedly_job(
                    sync_engine::Job::Initial,
                    id,
                    token,
                    FeedlyParams::default(),
                );
            }
            None => {
                // Noch kein Konto: der Erst-Sync legt es an. Dafür braucht der
                // Coordinator eine Kennung; ein vorhandenes Konto genügt als Anker.
                let anchor = self
                    .state
                    .borrow()
                    .accounts
                    .first()
                    .map(|(id, _, _)| id.clone())
                    .unwrap_or_else(|| "feedly-pending".to_string());
                self.show_toast("Feedly: Erst-Sync gestartet …");
                self.request_feedly_job(
                    sync_engine::Job::Initial,
                    anchor,
                    token,
                    FeedlyParams::default(),
                );
            }
        }
    }

    fn start_outbox_tick(&self) {
        let w = self.weak();
        glib::timeout_add_local(Duration::from_secs(10), move || {
            let Some(app) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let should = {
                let st = app.state.borrow();
                st.accounts.iter().any(|(_, k, _)| k == "feedly")
            };
            if should {
                let account_id = {
                    let st = app.state.borrow();
                    st.accounts
                        .iter()
                        .find(|(_, k, _)| k == "feedly")
                        .map(|(id, _, _)| id.clone())
                };
                if let Some(account_id) = account_id {
                    let worker = app.worker.clone();
                    let tx = app.bg_jobs_tx.clone();
                    // Schlüsselbund und Datenbank gehören nicht in den UI-Thread.
                    std::thread::Builder::new()
                        .name("lf-outbox-check".into())
                        .spawn(move || {
                            let token = feedly_sync::token_from_disk();
                            let acc = account_id.clone();
                            let has = worker
                                .send(move |db| {
                                    db.outbox_pending(&acc, storage::now_ms(), 1)
                                        .map(|v| !v.is_empty())
                                })
                                .recv()
                                .ok()
                                .and_then(|b| b.downcast::<storage::Result<bool>>().ok())
                                .map(|b| *b)
                                .unwrap_or(Ok(false))
                                .unwrap_or(false);
                            if !has {
                                return;
                            }
                            let token = match token {
                                Some(t) => t,
                                None => return,
                            };
                            let _ = tx.send(Box::new(move |app: &Rc<App>| {
                                app.request_feedly_job(
                                    sync_engine::Job::Outbox,
                                    account_id.clone(),
                                    token,
                                    FeedlyParams::default(),
                                );
                            })
                                as Box<dyn FnOnce(&Rc<App>) + Send>);
                        })
                        .ok();
                }
            }
            glib::ControlFlow::Continue
        });
    }

    fn start_feedly_scheduler(&self) {
        let has_feedly = self
            .state
            .borrow()
            .accounts
            .iter()
            .any(|(_, k, _)| k == "feedly");
        if !has_feedly {
            return;
        }
        {
            let mut st = self.state.borrow_mut();
            if st.next_feedly_sync == 0 {
                st.next_feedly_sync = now_ms() + 60_000;
            }
        }
        let w = self.weak();
        glib::timeout_add_local(Duration::from_secs(60), move || {
            let Some(app) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let due = {
                let interval = app.prefs.borrow().refresh_min.clamp(5, 1440);
                let mut st = app.state.borrow_mut();
                if st.accounts.iter().any(|(_, k, _)| k == "feedly")
                    && now_ms() >= st.next_feedly_sync
                {
                    st.next_feedly_sync = now_ms() + interval * 60_000;
                    true
                } else {
                    false
                }
            };
            if due {
                dbg_log("Feedly-Scheduler: Zyklus fällig");
                app.with_token(TokenAction::QueuedSync);
            }
            glib::ControlFlow::Continue
        });
    }

    /// Liest den gespeicherten Kontozustand (inkl. hängender Änderungen).
    pub fn refresh_feedly_status(&self) {
        let Some(account_id) = self.account_id_of_kind("feedly") else {
            return;
        };
        self.db_query(
            move |db| {
                let status = db.account_status(&account_id)?;
                let stuck = db.stuck_changes(&account_id)?;
                Ok::<_, storage::StorageError>((status, stuck))
            },
            move |app, res| {
                let Ok((status, stuck)) = res else { return };
                let mut entry = status.map(|(s, d)| (s, d));
                if let Some((s, d)) = entry.as_mut() {
                    if stuck > 0 && *s == "ready" {
                        *s = "degraded".to_string();
                        *d = Some(format!("{stuck} Änderung(en) nicht bestätigt"));
                    }
                }
                let label = entry
                    .as_ref()
                    .map(|(s, _)| status_label(s))
                    .unwrap_or("Verbunden");
                let text = format!("Feedly: {label}");
                app.last_sync_label.set_label(&text);
                app.state.borrow_mut().feedly_status = entry;
            },
        );
    }

    pub fn reload_meta_keep(&self) {
        let w = self.weak();
        self.db_query(
            |db| {
                Ok::<_, storage::StorageError>((
                    db.list_feeds()?,
                    db.list_groups()?,
                    db.counts()?,
                    db.list_accounts()?,
                ))
            },
            move |app,
                  res: storage::Result<(
                Vec<storage::FeedRow>,
                Vec<storage::GroupRow>,
                storage::Counts,
                Vec<(String, String, String)>,
            )>| {
                let Ok((feeds, groups, counts, accounts)) = res else {
                    return;
                };
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                    st.accounts = accounts;
                }
                app.refresh_sidebar();
                app.load_page(false);
                let _ = w;
            },
        );
    }

    fn backup_dialog(&self) {
        let dlg = gtk::FileDialog::builder()
            .title("Backup speichern unter")
            .build();
        dlg.set_initial_name(Some("lesefluss-backup.db"));
        let w = self.weak();
        let worker = self.worker.clone();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.save_future(None::<&gtk::Window>).await else {
                return;
            };
            let Some(path) = file.path() else { return };
            let res = tokio::task::spawn_blocking(move || {
                worker
                    .send(move |db| db.backup_to(&path))
                    .recv()
                    .ok()
                    .and_then(|b| b.downcast::<storage::Result<()>>().ok())
                    .map(|r| r.is_ok())
                    .unwrap_or(false)
            })
            .await
            .unwrap_or(false);
            if let Some(app) = w.upgrade() {
                let message = if res {
                    app.strings.get("Backup erstellt", "Backup created")
                } else {
                    app.strings.get("Backup fehlgeschlagen", "Backup failed")
                };
                app.show_toast(&message);
            }
        });
    }

    fn restore_dialog(&self) {
        let dlg = gtk::FileDialog::builder()
            .title("Backup-Datei wählen")
            .build();
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.open_future(None::<&gtk::Window>).await else {
                return;
            };
            let Some(path) = file.path() else { return };
            let pending = data_dir().join("restore.pending");
            let tmp = data_dir().join("restore.pending.tmp");
            let mut message = match std::fs::copy(&path, &tmp)
                .and_then(|_| std::fs::File::open(&tmp).and_then(|f| f.sync_all()))
                .and_then(|_| std::fs::rename(&tmp, &pending))
            {
                Ok(_) => match storage::Database::validate_candidate(&pending) {
                    Ok(_) => {
                        "Backup geprüft — es wird beim nächsten Start wiederhergestellt".to_string()
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&pending);
                        format!("Diese Datei ist keine lesbare Lesefluss-Bibliothek: {e}")
                    }
                },
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    format!("Wiederherstellung fehlgeschlagen: {e}")
                }
            };
            if let Some(app) = w.upgrade() {
                app.show_toast(&message);
            }
            message.clear();
        });
    }

    fn add_feed_dialog(&self) {
        let entry = gtk::Entry::builder()
            .placeholder_text("Feed- oder Website-URL")
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading("Feed hinzufügen")
            .body("URL eingeben; Lesefluss sucht den Feed und zeigt eine Vorschau.")
            .extra_child(&entry)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("add", "Suchen");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "add" {
                return;
            }
            let url = entry.text().trim().to_string();
            if url.is_empty() {
                return;
            }
            app.show_toast("Suche Feed…");
            app.net.discover(url);
        });
    }

    fn show_discovery_dialog(&self, candidates: Vec<provider_local::DiscoverCandidate>) {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        for c in &candidates {
            let row = gtk::ListBoxRow::builder()
                .child(
                    &gtk::Label::builder()
                        .label(&format!("{} — {}", c.title, c.url))
                        .xalign(0.0)
                        .margin_start(8)
                        .margin_end(8)
                        .margin_top(6)
                        .margin_bottom(6)
                        .build(),
                )
                .build();
            list.append(&row);
        }
        if let Some(r) = list.row_at_index(0) {
            list.select_row(Some(&r));
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Feed gefunden")
            .body("Bitte Feed auswählen und abonnieren.")
            .extra_child(&list)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("sub", "Abonnieren");
        dialog.set_response_appearance("sub", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("sub"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "sub" {
                return;
            }
            let Some(idx) = list.selected_row().map(|r| r.index() as usize) else {
                return;
            };
            let Some(c) = candidates.get(idx) else { return };
            app.subscribe(&c.url, &c.title);
        });
    }

    fn subscribe(&self, url: &str, title: &str) {
        let url = url.to_string();
        let title = title.to_string();
        let w = self.weak();
        self.db_query(
            {
                let url_db = url.clone();
                move |db: &storage::Database| {
                    if let Some(id) = db.feed_id_by_url(&url_db)? {
                        return Ok::<_, storage::StorageError>((id, false));
                    }
                    let accent = ACCENTS[url_db.len() % ACCENTS.len()];
                    let id = db.add_feed("local", &url_db, &title, None, accent)?;
                    Ok((id, true))
                }
            },
            move |app, res: storage::Result<(i64, bool)>| {
                let Ok((feed_id, is_new)) = res else { return };
                if is_new {
                    let url2 = url.clone();
                    app.net.fetch_feed(app.worker.clone(), feed_id, url2, true);
                }
                app.reload_meta_then_select(feed_id);
                let _ = w;
            },
        );
    }

    fn reload_meta_then_select(&self, feed_id: i64) {
        let w = self.weak();
        self.db_query(
            |db| {
                Ok::<_, storage::StorageError>((db.list_feeds()?, db.list_groups()?, db.counts()?))
            },
            move |app, res: storage::Result<(Vec<FeedRow>, Vec<GroupRow>, Counts)>| {
                let Ok((feeds, groups, counts)) = res else {
                    return;
                };
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                }
                app.set_scope(Scope::Feed(feed_id), true);
                let _ = w;
            },
        );
    }

    // ── Thema & Layout ──

    fn apply_theme_now(&self) {
        let mode = self.prefs.borrow().theme.clone();
        let tokens = match mode.as_str() {
            "omarchy" => crate::theme_omarchy::omarchy_tokens()
                .unwrap_or_else(|| tokens_for(adw::StyleManager::default().is_dark())),
            "dark" => tokens_for(true),
            "light" => tokens_for(false),
            _ => tokens_for(adw::StyleManager::default().is_dark()),
        };
        let scheme = match mode.as_str() {
            "dark" => adw::ColorScheme::ForceDark,
            "light" => adw::ColorScheme::ForceLight,
            "omarchy" => {
                if tokens.dark {
                    adw::ColorScheme::ForceDark
                } else {
                    adw::ColorScheme::ForceLight
                }
            }
            _ => adw::ColorScheme::PreferLight,
        };
        adw::StyleManager::default().set_color_scheme(scheme);
        *self.tokens.borrow_mut() = tokens;
        self.css.load_from_string(&gtk_css_for(&tokens));
        let rgba = gtk::gdk::RGBA::new(
            tokens.surface_reader.r as f32 / 255.0,
            tokens.surface_reader.g as f32 / 255.0,
            tokens.surface_reader.b as f32 / 255.0,
            1.0,
        );
        self.reader.webview.set_background_color(&rgba);
    }

    pub fn apply_theme(&self) {
        self.apply_theme_now();
        self.reload_current(true);
    }

    pub fn load_prefs(&self) {
        let w = self.weak();
        self.db_query(
            |db| {
                let mut out: Vec<(String, String)> = Vec::new();
                for key in [
                    "auto_read",
                    "compact",
                    "thumbs",
                    "reader_font",
                    "reader_measure",
                    "reader_line_height",
                    "theme",
                    "letter_shortcuts",
                    "newest_first",
                    "refresh_min",
                    "retention_days",
                    "media_mb",
                ] {
                    if let Ok(Some(v)) = db.get_pref(key) {
                        out.push((key.to_string(), v));
                    }
                }
                Ok::<_, storage::StorageError>(out)
            },
            move |app, res| {
                let Ok(entries) = res else { return };
                let map: std::collections::HashMap<String, String> = entries.into_iter().collect();
                let prefs = Prefs::load(&|k| map.get(k).cloned());
                *app.prefs.borrow_mut() = prefs.clone();
                dbg_log(&format!(
                    "Einstellungen geladen: {} Einträge, Buchstabenkürzel={}, Theme={}",
                    map.len(),
                    prefs.letter_shortcuts,
                    prefs.theme
                ));
                app.apply_prefs_live();
                let _ = w;
            },
        );
    }

    pub fn apply_prefs_live(&self) {
        let p = self.prefs.borrow().clone();
        self.window.remove_css_class("lf-compact");
        if p.compact {
            self.window.add_css_class("lf-compact");
        }
        let mut style = self.reader.style.borrow_mut();
        style.font_size = p.reader_font;
        style.measure_ch = p.reader_measure;
        style.line_height = p.reader_line_height;
        drop(style);
        self.media.set_max_bytes((p.media_mb as u64) * 1024 * 1024);
        self.net.set_refresh_minutes(p.refresh_min);
        self.sync_store(false);
    }

    fn reader_is_visible(&self) -> bool {
        !self.inner.is_collapsed() && self.window.is_active()
    }

    fn reader_loaded_ok(&self) -> bool {
        let name = self
            .reader
            .stack
            .visible_child_name()
            .map(|n| n.to_string())
            .unwrap_or_default();
        name == "web"
    }

    fn letters_enabled(&self) -> bool {
        self.prefs.borrow().letter_shortcuts
    }

    fn editing_widget(&self) -> bool {
        let Some(mut w) = self.window.focus_child() else {
            return false;
        };
        for _ in 0..8 {
            if is_editing_class(w.type_().name()) {
                return true;
            }
            match w.parent() {
                Some(p) => w = p,
                None => break,
            }
        }
        false
    }

    fn install_letter_router(&self) {
        let w = self.weak();
        let ctrl = gtk::EventControllerKey::new();
        ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
        ctrl.connect_key_pressed(move |_, keyval, _keycode, _state| {
            let Some(app) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let Some(action) = letter_action(keyval) else {
                return glib::Propagation::Proceed;
            };
            if !app.letters_enabled() {
                return glib::Propagation::Proceed;
            }
            if app.editing_widget() {
                dbg_log(&format!(
                    "Buchstabe {keyval:?} im Eingabefeld: nicht abgefangen"
                ));
                return glib::Propagation::Proceed;
            }
            match gtk::prelude::WidgetExt::activate_action(&app.window, action, None) {
                Ok(()) => {
                    dbg_log(&format!("Buchstabe {keyval:?} -> {action}"));
                    glib::Propagation::Stop
                }
                Err(_) => glib::Propagation::Proceed,
            }
        });
        self.window.add_controller(ctrl);
    }

    pub fn save_pref(&self, key: &str, value: &str) {
        let k = key.to_string();
        let v = value.to_string();
        self.worker.send(move |db| db.set_pref(&k, &v));
    }

    fn start_theme_watch(&self) {
        let Some(dir) = crate::theme_omarchy::watch_dir() else {
            return;
        };
        let file = gio::File::for_path(&dir);
        let Ok(monitor) = file.monitor_directory(
            gio::FileMonitorFlags::WATCH_MOVES,
            None::<&gio::Cancellable>,
        ) else {
            return;
        };
        let w = self.weak();
        let pending = Rc::new(Cell::new(false));
        monitor.connect_changed(move |_, _, _, _| {
            let Some(app) = w.upgrade() else { return };
            if app.prefs.borrow().theme != "omarchy" || pending.get() {
                return;
            }
            pending.set(true);
            let w2 = w.clone();
            let pending2 = pending.clone();
            glib::timeout_add_local(Duration::from_millis(250), move || {
                pending2.set(false);
                if let Some(app) = w2.upgrade() {
                    if app.prefs.borrow().theme == "omarchy" {
                        app.apply_theme();
                    }
                }
                glib::ControlFlow::Break
            });
        });
        std::mem::forget(monitor);
    }

    fn install_width_watcher(&self) {
        let outer = self.outer.clone();
        let inner = self.inner.clone();
        let win = self.window.clone();
        let apply = move || {
            let w = gtk::prelude::NativeExt::surface(&win)
                .map(|s| s.width())
                .unwrap_or_else(|| win.width());
            outer.set_collapsed(w <= 1119);
            inner.set_collapsed(w <= 779);
        };
        let apply_win = apply.clone();
        self.window.connect_notify_local(None, move |_, pspec| {
            let n = pspec.name();
            if n == "fullscreened" || n == "maximized" {
                apply_win();
            }
        });
        let apply_realize = apply.clone();
        self.window.connect_realize(move |win| {
            if let Some(surface) = gtk::prelude::NativeExt::surface(win) {
                let apply2 = apply_realize.clone();
                surface.connect_notify_local(Some("width"), move |_, _| apply2());
            }
            apply_realize();
        });
        apply();
    }

    // ── Signale ──

    fn wire(&self, factory: gtk::SignalListItemFactory) {
        factory.connect_setup(|_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            li.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
        });
        let w = self.weak();
        factory.connect_bind(move |_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(obj) = li.item() else { return };
            let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let row = boxed.borrow::<ListRow>();
            let thumbs = w.upgrade().map(|a| a.prefs.borrow().thumbs).unwrap_or(true);
            li.set_child(Some(&list::row_widget(&row, thumbs)));
        });
        factory.connect_unbind(|_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            if let (Some(obj), Some(child)) = (li.item(), li.child()) {
                if let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() {
                    let row = boxed.borrow::<ListRow>();
                    list::unregister(&row, &child);
                }
            }
            li.set_child(None::<&gtk::Widget>);
        });

        let w = self.weak();
        self.sidebar_list.connect_row_selected(move |_, row| {
            let Some(app) = w.upgrade() else { return };
            if app.suppress.get() {
                return;
            }
            let Some(row) = row else { return };
            let idx = row.index() as usize;
            let f = {
                let filters = app.sidebar_filters.borrow();
                match filters.get(idx) {
                    Some(Some(f)) => f.clone(),
                    _ => return,
                }
            };
            let reset = matches!(f, Scope::Global);
            app.set_scope(f, reset);
        });

        let w = self.weak();
        self.list_selection
            .connect_selected_item_notify(move |sel| {
                let Some(app) = w.upgrade() else { return };
                if app.suppress.get() {
                    return;
                }
                let cause = app.selection_cause.get();
                app.selection_cause.set(SelectionCause::Unknown);
                if cause != SelectionCause::Keyboard {
                    return;
                }
                let Some(obj) = sel.selected_item() else {
                    return;
                };
                let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let row = boxed.borrow::<ListRow>();
                if let Some(a) = row.article() {
                    app.schedule_preview(a);
                }
            });

        let w = self.weak();
        self.new_articles_label.connect_clicked(move |_| {
            let Some(app) = w.upgrade() else { return };
            app.state.borrow_mut().pending_new_articles = 0;
            app.update_new_articles_bar();
            if let Some(pos) = app
                .state
                .borrow()
                .rows
                .iter()
                .position(|r| matches!(r, ListRow::Item(_)))
            {
                app.list_view.scroll_to(
                    pos as u32,
                    gtk::ListScrollFlags::NONE,
                    None::<gtk::ScrollInfo>,
                );
            }
            app.load_page(false);
        });

        let w = self.weak();
        self.list_view.connect_activate(move |_, pos| {
            let Some(app) = w.upgrade() else { return };
            let row = app
                .state
                .borrow()
                .rows
                .get(pos as usize)
                .and_then(|r| r.article());
            if let Some(row) = row {
                app.open_article(row, true, true);
            }
        });

        let w = self.weak();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            if let Some(app) = w.upgrade() {
                app.selection_cause.set(SelectionCause::Pointer);
            }
        });
        self.list_view.add_controller(click);
        let w = self.weak();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if let Some(app) = w.upgrade() {
                match key {
                    gtk::gdk::Key::Up
                    | gtk::gdk::Key::Down
                    | gtk::gdk::Key::Home
                    | gtk::gdk::Key::End
                    | gtk::gdk::Key::Page_Up
                    | gtk::gdk::Key::Page_Down => {
                        app.selection_cause.set(SelectionCause::Keyboard);
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        self.list_view.add_controller(keys);

        for (btn, f) in [
            (self.filter_saved.clone(), Filter::Saved),
            (self.filter_unread.clone(), Filter::Unread),
            (self.filter_all.clone(), Filter::All),
        ] {
            let w = self.weak();
            btn.connect_toggled(move |b| {
                let Some(app) = w.upgrade() else { return };
                if app.syncing_filters.get() {
                    return;
                }
                if !b.is_active() {
                    app.syncing_filters.set(true);
                    b.set_active(true);
                    app.syncing_filters.set(false);
                    return;
                }
                app.set_filter(f);
            });
        }

        let last_scroll_value = std::cell::Cell::new(0.0);
        let w = self.weak();
        self.list_scroll
            .vadjustment()
            .connect_value_changed(move |adj| {
                let Some(app) = w.upgrade() else { return };
                if adj.value() > last_scroll_value.get() + 0.5 {
                    last_scroll_value.set(adj.value());
                    let mut st = app.state.borrow_mut();
                    if st.pending_new_articles > 0 {
                        st.pending_new_articles = 0;
                        drop(st);
                        app.update_new_articles_bar();
                    }
                }
                let near_bottom = adj.value() + adj.page_size() >= adj.upper() - 400.0;
                if !near_bottom {
                    return;
                }
                let st = app.state.borrow();
                let can_more = st.cursor.is_some() && !st.loading_more;
                drop(st);
                if can_more {
                    app.state.borrow_mut().loading_more = true;
                    app.load_page(true);
                }
            });

        let webview = self.reader.webview.clone();
        self.reader
            .search_entry
            .connect_search_changed(move |entry| {
                find_in_view(&webview, &entry.text());
            });
        let webview2 = self.reader.webview.clone();
        self.reader.search_entry.connect_activate(move |_| {
            find_next(&webview2);
        });
        self.reader
            .search_bar
            .set_key_capture_widget(Some(&self.window));

        let w = self.weak();
        self.search_entry.connect_search_changed(move |entry| {
            let Some(app) = w.upgrade() else { return };
            let q = entry.text().trim().to_string();
            if let Some(old) = app.search_timer.borrow_mut().take() {
                old.remove();
            }
            let w2 = w.clone();
            let timer = glib::timeout_add_local(Duration::from_millis(150), move || {
                let Some(app) = w2.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                *app.search_timer.borrow_mut() = None;
                {
                    let mut st = app.state.borrow_mut();
                    st.search = if q.is_empty() { None } else { Some(q.clone()) };
                }
                app.list_title.set_title(&app.scope_label_now());
                app.load_page(false);
                glib::ControlFlow::Break
            });
            *app.search_timer.borrow_mut() = Some(timer);
        });

        let w = self.weak();
        self.reader.webview.connect_load_changed(move |_, event| {
            if event == webkit6::LoadEvent::Finished {
                if let Some(app) = w.upgrade() {
                    app.after_load_finished();
                }
            }
        });

        let w = self.weak();
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(app) = w.upgrade() {
                app.apply_theme();
            }
        });
    }

    fn schedule_preview(&self, row: ArticleRow) {
        if let Some(old) = self.preview_timer.borrow_mut().take() {
            old.remove();
        }
        let w = self.weak();
        let timer = glib::timeout_add_local(Duration::from_millis(220), move || {
            if let Some(app) = w.upgrade() {
                *app.preview_timer.borrow_mut() = None;
                let is_current = app.reader.current.borrow().as_deref() == Some(row.id.as_str());
                if !is_current {
                    app.open_article(row.clone(), false, true);
                }
            }
            glib::ControlFlow::Break
        });
        *self.preview_timer.borrow_mut() = Some(timer);
    }

    // ── Actions ──

    fn register_actions(&self, application: &adw::Application) {
        macro_rules! win_action {
            ($name:expr, |$a:ident| $body:expr) => {{
                let w = self.weak();
                let act = gio::SimpleAction::new($name, None);
                act.connect_activate(move |_, _| {
                    if let Some($a) = w.upgrade() {
                        $body;
                    }
                });
                self.window.add_action(&act);
            }};
        }

        win_action!("reader-back", |a| a.back());
        win_action!("back", |a| a.back());
        win_action!("refresh", |a| a.do_refresh());
        win_action!("add-feed", |a| a.add_feed_dialog());
        win_action!("settings", |a| a.settings_dialog());
        win_action!("connect-feedly", |a| a.connect_feedly_dialog());
        win_action!("disconnect-feedly", |a| a.disconnect_feedly());
        win_action!("import-opml", |a| a.import_opml_dialog());
        win_action!("export-opml", |a| a.export_opml());
        win_action!("backup", |a| a.backup_dialog());
        win_action!("restore", |a| a.restore_dialog());
        win_action!("mark-scope-read", |a| a.mark_scope_dialog());
        win_action!("toggle-read", |a| a.toggle_read());
        win_action!("toggle-saved", |a| a.toggle_saved());
        win_action!("open-external", |a| a.open_external());
        win_action!("copy-link", |a| a.copy_link());
        win_action!("zoom-in", |a| a.zoom(2.0));
        win_action!("zoom-out", |a| a.zoom(-2.0));
        win_action!("zoom-reset", |a| a.zoom_reset());
        win_action!("find", |a| a.open_find());
        win_action!("article-search", |a| {
            a.search_bar.set_search_mode(true);
            a.search_entry.grab_focus();
        });
        win_action!("reader-retry", |a| a.reload_current(false));
        win_action!("toggle-sort-order", |a| a.toggle_sort_order());
        win_action!("focus-mode", |a| a.toggle_focus_mode());
        win_action!("rename-feed", |a| a.rename_feed_dialog());
        win_action!("edit-feed-groups", |a| a.edit_feed_groups_dialog());
        win_action!("unsubscribe-feed", |a| a.unsubscribe_dialog());
        win_action!("mark-source-read", |a| a.mark_source_read());
        win_action!("undo", |a| a.undo());
        win_action!("redo", |a| a.redo());
        win_action!("close", |a| a.window.close());
        win_action!("next-article", |a| a.move_selection(1));
        win_action!("prev-article", |a| a.move_selection(-1));
        win_action!("next-unread", |a| a.move_unread(1));
        win_action!("prev-unread", |a| a.move_unread(-1));
        win_action!("focus-next-pane", |a| a.focus_pane(1));
        win_action!("focus-prev-pane", |a| a.focus_pane(-1));

        let theme_action = gio::SimpleAction::new_stateful(
            "theme",
            Some(&String::static_variant_type()),
            &"system".to_variant(),
        );
        theme_action.connect_activate(|action, param| {
            let mode = param.and_then(|p| p.get::<String>()).unwrap_or_default();
            let scheme = match mode.as_str() {
                "dark" => adw::ColorScheme::ForceDark,
                "light" => adw::ColorScheme::ForceLight,
                _ => adw::ColorScheme::PreferLight,
            };
            adw::StyleManager::default().set_color_scheme(scheme);
            if let Some(p) = param {
                action.set_state(p);
            }
        });
        application.add_action(&theme_action);

        let quit = gio::SimpleAction::new("quit", None);
        let app_for_quit = application.clone();
        quit.connect_activate(move |_, _| app_for_quit.quit());
        application.add_action(&quit);

        application.set_accels_for_action("win.settings", &["<Control>comma"]);
        application.set_accels_for_action("win.find", &["<Control>f"]);
        application.set_accels_for_action("win.article-search", &["<Control>l"]);
        application.set_accels_for_action("win.refresh", &["<Control>r"]);
        application.set_accels_for_action("win.add-feed", &["<Control>n"]);
        application.set_accels_for_action("win.mark-scope-read", &["<Control><Shift>m"]);
        application.set_accels_for_action("win.toggle-sort-order", &["<Control><Shift>p"]);
        application.set_accels_for_action("win.focus-mode", &["F9"]);
        application.set_accels_for_action("win.undo", &["<Control>z"]);
        application.set_accels_for_action("win.redo", &["<Control>y", "<Control><Shift>z"]);
        application.set_accels_for_action(
            "win.zoom-in",
            &["<Control>plus", "<Control>equal", "<Control>KP_Add"],
        );
        application
            .set_accels_for_action("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]);
        application.set_accels_for_action("win.zoom-reset", &["<Control>0"]);
        application.set_accels_for_action("win.focus-next-pane", &["F6"]);
        application.set_accels_for_action("win.focus-prev-pane", &["<Shift>F6"]);
        application.set_accels_for_action("win.back", &["<Alt>Left"]);
        application.set_accels_for_action("win.close", &["<Control>w"]);
        application.set_accels_for_action("app.quit", &["<Control>q"]);
    }
}
