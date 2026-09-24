mod dbworker;
mod fixtures;
mod list;
mod model;
mod feedly_sync;
mod net;
mod opml;
mod opml_ui;
mod prefs;
mod theme_omarchy;
mod reader;
mod seed;
mod settings;
mod sidebar;
mod strings;
mod state;
mod style;
mod window;

use gtk::prelude::*;
use std::rc::Rc;

thread_local! {
    static INSTANCE: std::cell::RefCell<Option<Rc<window::App>>> = const { std::cell::RefCell::new(None) };
}

pub fn apply_pending_restore(dir: &std::path::Path) -> Result<Option<String>, String> {
    let pending = dir.join("restore.pending");
    if !pending.exists() {
        return Ok(None);
    }
    let db_path = dir.join("library.db");
    if let Err(e) = storage::Database::validate_candidate(&pending) {
        let _ = std::fs::remove_file(&pending);
        return Err(format!(
            "Wiederherstellung abgebrochen, die vorhandene Bibliothek bleibt unverändert: {e}"
        ));
    }
    if db_path.exists() {
        let stamp = storage::now_ms();
        let safety = dir.join(format!("library.pre-restore-{stamp}.db"));
        std::fs::copy(&db_path, &safety)
            .map_err(|e| format!("Sicherung des alten Bestands fehlgeschlagen: {e}"))?;
    }
    let staged = dir.join("library.restore-staged");
    let _ = std::fs::remove_file(&staged);
    std::fs::copy(&pending, &staged)
        .map_err(|e| format!("Kopie in die Staging-Datei fehlgeschlagen: {e}"))?;
    if let Err(e) = std::fs::File::open(&staged).and_then(|f| f.sync_all()) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("Staging-Datei nicht dauerhaft schreibbar: {e}"));
    }
    let _ = std::fs::remove_file(dir.join("library.db-wal"));
    let _ = std::fs::remove_file(dir.join("library.db-shm"));
    if let Err(e) = std::fs::rename(&staged, &db_path) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("Wiederherstellung nicht aktiviert, alter Bestand erhalten: {e}"));
    }
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
    let _ = std::fs::remove_file(&pending);
    Ok(Some("Backup wurde beim Start wiederhergestellt".to_string()))
}

#[cfg(test)]
mod restore_tests {
    use super::*;

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lesefluss-restore-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_db(path: &std::path::Path) {
        let db = storage::Database::open(path).unwrap();
        db.ensure_local_account().unwrap();
        let feed = db.add_feed("local", "https://example.com/feed.xml", "Beispiel", None, "#123456").unwrap();
        db.upsert_article(feed, "a1", "Artikel", None, None, storage::now_ms(), "e", Some("<p>x</p>"), storage::now_ms())
            .unwrap();
    }

    #[test]
    fn valid_candidate_replaces_library_and_keeps_safety_copy() {
        let dir = tempdir("valid");
        let live = dir.join("library.db");
        sample_db(&live);
        let candidate = dir.join("backup.db");
        sample_db(&candidate);
        std::fs::write(dir.join("restore.pending"), std::fs::read(&candidate).unwrap()).unwrap();

        let notice = apply_pending_restore(&dir).unwrap().expect("Hinweis");
        assert!(notice.contains("wiederhergestellt"));
        assert!(!dir.join("restore.pending").exists());
        assert!(!dir.join("library.restore-staged").exists());
        let db = storage::Database::open(&live).unwrap();
        assert_eq!(db.list_feeds().unwrap().len(), 1);
        let safety: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("library.pre-restore-"))
            .collect();
        assert_eq!(safety.len(), 1, "alter Bestand wurde gesichert");
    }

    #[test]
    fn broken_candidate_keeps_existing_library() {
        let dir = tempdir("broken");
        let live = dir.join("library.db");
        sample_db(&live);
        let before = std::fs::read(&live).unwrap();
        std::fs::write(dir.join("restore.pending"), vec![7u8; 8192]).unwrap();

        let err = apply_pending_restore(&dir).unwrap_err();
        assert!(err.contains("abgebrochen"), "{err}");
        assert_eq!(std::fs::read(&live).unwrap(), before, "Bibliothek unverändert");
        assert!(!dir.join("restore.pending").exists(), "ungültige Datei wird entfernt");
    }

    #[test]
    fn no_pending_file_is_a_no_op() {
        let dir = tempdir("none");
        assert!(apply_pending_restore(&dir).unwrap().is_none());
    }
}

fn main() -> gtk::glib::ExitCode {
    let t0 = std::time::Instant::now();
    let dir = window::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let db_path = dir.join("library.db");
    let restore_notice: Option<String> = match apply_pending_restore(&dir) {
        Ok(Some(msg)) => Some(msg),
        Ok(None) => None,
        Err(msg) => {
            eprintln!("[lf] {msg}");
            Some(msg)
        }
    };
    let worker = dbworker::DbWorker::start(db_path);
    let net = Rc::new(net::Net::start());
    net.spawn_scheduler(worker.clone());

    let app = adw::Application::builder()
        .application_id("io.github.PROJEKTINHABER.Lesefluss")
        .build();
    let worker2 = worker.clone();
    let net2 = Rc::clone(&net);
    app.connect_activate(move |a| {
        let t0 = t0;
        let restore_notice = restore_notice.clone();
        INSTANCE.with(|slot| {
            if slot.borrow().is_none() {
                let instance = window::App::new(a, worker2.clone(), Rc::clone(&net2));
                if let Some(msg) = restore_notice.clone() {
                    instance.show_toast(&msg);
                }
                if let Some(err) = worker2.failure.lock().ok().and_then(|f| f.clone()) {
                    instance.show_toast(&format!("Datenbank nicht geöffnet: {err}"));
                }
                glib::idle_add_local(move || {
                    window::dbg_log(&format!(
                        "startup-ready {} ms",
                        t0.elapsed().as_millis()
                    ));
                    glib::ControlFlow::Break
                });
                *slot.borrow_mut() = Some(instance);
            } else if let Some(existing) = slot.borrow().as_ref() {
                existing.window.present();
            }
        });
    });
    app.run()
}
