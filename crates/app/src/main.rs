mod dbworker;
mod feedly_sync;
mod fixtures;
mod list;
mod model;
mod net;
mod opml;
mod opml_ui;
mod prefs;
mod reader;
mod seed;
mod settings;
mod sidebar;
mod state;
mod strings;
mod style;
mod theme_omarchy;
mod window;

use gtk::prelude::*;
use std::rc::Rc;

thread_local! {
    static INSTANCE: std::cell::RefCell<Option<Rc<window::App>>> = const { std::cell::RefCell::new(None) };
}

/// Exklusive Sperre auf die Bibliothek. Sie wird gehalten, bevor irgendein Code die
/// Datenbank öffnet oder ersetzt, und verschwindet automatisch mit dem Prozess.
pub struct LibraryLock {
    file: std::fs::File,
}

impl LibraryLock {
    pub fn acquire(dir: &std::path::Path) -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;
        let path = dir.join("library.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| format!("Sperrdatei {} nicht zu öffnen: {e}", path.display()))?;
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            return Err(
                "Die Bibliothek ist bereits von einer anderen Lesefluss-Instanz geöffnet \
                 (wiederhergestellt wird nur mit exklusivem Zugriff)."
                    .to_string(),
            );
        }
        Ok(Self { file })
    }
}

impl Drop for LibraryLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn fsync_file(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

fn fsync_dir(dir: &std::path::Path) {
    if let Ok(handle) = std::fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

/// consistency copy of the current library including the WAL, with a plain file copy
/// as a fallback for a database that can no longer be opened.
fn consistent_backup(
    dir: &std::path::Path,
    db_path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let stamp = storage::now_ms();
    let safety = dir.join(format!("library.pre-restore-{stamp}.db"));
    match storage::Database::open(db_path) {
        Ok(db) => {
            db.wal_checkpoint()
                .map_err(|e| format!("Sicherung des alten Bestands fehlgeschlagen (WAL): {e}"))?;
            db.backup_to(&safety)
                .map_err(|e| format!("Sicherung des alten Bestands fehlgeschlagen: {e}"))?;
        }
        Err(_) => {
            std::fs::copy(db_path, &safety)
                .map_err(|e| format!("Sicherung des alten Bestands fehlgeschlagen: {e}"))?;
        }
    }
    fsync_file(&safety).map_err(|e| format!("Sicherung nicht dauerhaft schreibbar: {e}"))?;
    Ok(safety)
}

/// Sets the activation of a prepared candidate complete after a crash. Everything that
/// is found here was validated before the crash and is rechecked here before use.
fn finish_interrupted_activation(
    dir: &std::path::Path,
    new_db: &std::path::Path,
) -> Result<bool, String> {
    if !new_db.exists() {
        return Ok(false);
    }
    if storage::Database::validate_candidate(new_db).is_err() {
        let _ = std::fs::remove_file(new_db);
        return Ok(false);
    }
    let db_path = dir.join("library.db");
    let _ = std::fs::remove_file(dir.join("library.db-wal"));
    let _ = std::fs::remove_file(dir.join("library.db-shm"));
    std::fs::rename(new_db, &db_path)
        .map_err(|e| format!("Wiederherstellung nicht aktiviert, alter Bestand erhalten: {e}"))?;
    fsync_dir(dir);
    let _ = std::fs::remove_file(dir.join("restore.pending"));
    let _ = std::fs::remove_file(dir.join("library.restore-staged"));
    Ok(true)
}

/// Restore runs exclusively under the library lock. The old library is secured as a
/// consistent copy including the WAL, the candidate is fully checked and migrated on a
/// staging file, and only then swapped atomically. No sidecar of a running database is
/// ever deleted.
pub fn apply_pending_restore(dir: &std::path::Path) -> Result<Option<String>, String> {
    let pending = dir.join("restore.pending");
    let staged = dir.join("library.restore-staged");
    let new_db = dir.join("library.db.new");
    let db_path = dir.join("library.db");

    if finish_interrupted_activation(dir, &new_db)? {
        return Ok(Some(
            "Wiederherstellung wurde nach einem Abbruch abgeschlossen".to_string(),
        ));
    }
    if !pending.exists() {
        return Ok(None);
    }
    if let Err(e) = storage::Database::validate_candidate(&pending) {
        let _ = std::fs::remove_file(&pending);
        return Err(format!(
            "Wiederherstellung abgebrochen, die vorhandene Bibliothek bleibt unverändert: {e}"
        ));
    }
    if db_path.is_file() {
        consistent_backup(dir, &db_path)?;
    }
    let _ = std::fs::remove_file(&staged);
    if let Err(e) = std::fs::copy(&pending, &staged) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("Kopie in die Staging-Datei fehlgeschlagen: {e}"));
    }
    if let Err(e) = storage::Database::prepare_restore_candidate(&staged) {
        let _ = std::fs::remove_file(&staged);
        return Err(format!(
            "Wiederherstellung abgebrochen, die vorhandene Bibliothek bleibt unverändert: {e}"
        ));
    }
    let _ = std::fs::remove_file(&new_db);
    if let Err(e) = std::fs::copy(&staged, &new_db) {
        let _ = std::fs::remove_file(&new_db);
        let _ = std::fs::remove_file(&staged);
        return Err(format!("Aktivierungsdatei nicht schreibbar: {e}"));
    }
    if let Err(e) = fsync_file(&new_db) {
        let _ = std::fs::remove_file(&new_db);
        let _ = std::fs::remove_file(&staged);
        return Err(format!("Aktivierungsdatei nicht dauerhaft schreibbar: {e}"));
    }
    // Sidecars belong to the old file and were secured together with it. The lock
    // guarantees that no other process has the database open.
    let _ = std::fs::remove_file(dir.join("library.db-wal"));
    let _ = std::fs::remove_file(dir.join("library.db-shm"));
    if let Err(e) = std::fs::rename(&new_db, &db_path) {
        let _ = std::fs::remove_file(&new_db);
        return Err(format!(
            "Wiederherstellung nicht aktiviert, alter Bestand erhalten: {e}"
        ));
    }
    fsync_dir(dir);
    let _ = std::fs::remove_file(&pending);
    let _ = std::fs::remove_file(&staged);
    Ok(Some(
        "Backup wurde beim Start wiederhergestellt".to_string(),
    ))
}

fn main() -> gtk::glib::ExitCode {
    let t0 = std::time::Instant::now();
    let dir = window::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let app = adw::Application::builder()
        .application_id("io.github.PROJEKTINHABER.Lesefluss")
        .build();
    // Erst die Einzelinstanz behaupten: ein Zweitstart darf die Datenbank einer
    // laufenden Instanz weder lesen noch ersetzen.
    if let Err(_already_running) = app.register(gio::Cancellable::NONE) {
        app.activate();
        return gtk::glib::ExitCode::SUCCESS;
    }
    let _library_lock = match LibraryLock::acquire(&dir) {
        Ok(lock) => lock,
        Err(msg) => {
            eprintln!("[lf] {msg}");
            return gtk::glib::ExitCode::FAILURE;
        }
    };
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
                    window::dbg_log(&format!("startup-ready {} ms", t0.elapsed().as_millis()));
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

#[cfg(test)]
mod restore_tests {
    use super::*;

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lesefluss-restore-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_db(path: &std::path::Path) {
        let db = storage::Database::open(path).unwrap();
        db.ensure_local_account().unwrap();
        let feed = db
            .add_feed(
                "local",
                "https://example.com/feed.xml",
                "Beispiel",
                None,
                "#123456",
            )
            .unwrap();
        db.upsert_article(
            feed,
            "a1",
            "Artikel",
            None,
            None,
            storage::now_ms(),
            "e",
            Some("<p>x</p>"),
            storage::now_ms(),
        )
        .unwrap();
    }

    #[test]
    fn valid_candidate_replaces_library_and_keeps_safety_copy() {
        let dir = tempdir("valid");
        let live = dir.join("library.db");
        sample_db(&live);
        let candidate = dir.join("backup.db");
        sample_db(&candidate);
        std::fs::write(
            dir.join("restore.pending"),
            std::fs::read(&candidate).unwrap(),
        )
        .unwrap();

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
        assert_eq!(
            std::fs::read(&live).unwrap(),
            before,
            "Bibliothek unverändert"
        );
        assert!(
            !dir.join("restore.pending").exists(),
            "ungültige Datei wird entfernt"
        );
    }

    #[test]
    fn no_pending_file_is_a_no_op() {
        let dir = tempdir("none");
        assert!(apply_pending_restore(&dir).unwrap().is_none());
    }

    #[test]
    fn zweite_instanz_kann_die_bibliothek_nicht_sperren() {
        let dir = tempdir("lock");
        let first = LibraryLock::acquire(&dir).expect("erste Instanz");
        let err = match LibraryLock::acquire(&dir) {
            Err(e) => e,
            Ok(_) => panic!("zweite Instanz darf die Sperre nicht erhalten"),
        };
        assert!(err.contains("bereits"), "{err}");
        drop(first);
        LibraryLock::acquire(&dir).expect("nach Freigabe wieder möglich");
    }

    #[test]
    fn widerspruechliche_versionsangabe_bricht_vor_der_aktivierung_ab() {
        let dir = tempdir("version");
        let live = dir.join("library.db");
        sample_db(&live);
        let before = std::fs::read(&live).unwrap();
        let candidate = dir.join("backup.db");
        {
            let db = storage::Database::open(&candidate).unwrap();
            db.raw()
                .execute("DELETE FROM schema_version WHERE version > 2", [])
                .unwrap();
        }
        std::fs::write(
            dir.join("restore.pending"),
            std::fs::read(&candidate).unwrap(),
        )
        .unwrap();
        let err = apply_pending_restore(&dir).unwrap_err();
        assert!(err.contains("abgebrochen"), "{err}");
        assert_eq!(
            std::fs::read(&live).unwrap(),
            before,
            "Bibliothek unverändert"
        );
        assert!(
            !dir.join("library.db.new").exists(),
            "nichts wurde aktiviert"
        );
    }

    #[test]
    fn formal_gueltige_datenbank_mit_falschem_schema_wird_abgewiesen() {
        let dir = tempdir("falschschema");
        let live = dir.join("library.db");
        sample_db(&live);
        let before = std::fs::read(&live).unwrap();
        let fake = dir.join("fake.db");
        {
            let db = storage::Database::open(&fake).unwrap();
            db.raw().execute_batch("DROP TABLE articles;").unwrap();
            db.raw()
                .execute(
                    "CREATE TABLE articles (id TEXT PRIMARY KEY, unpassend INTEGER)",
                    [],
                )
                .unwrap();
        }
        std::fs::write(dir.join("restore.pending"), std::fs::read(&fake).unwrap()).unwrap();
        let err = apply_pending_restore(&dir).unwrap_err();
        assert!(err.contains("Schem") || err.contains("Spalte"), "{err}");
        assert_eq!(
            std::fs::read(&live).unwrap(),
            before,
            "Bibliothek unverändert"
        );
    }

    #[test]
    fn renamefehler_erhaelt_den_alten_bestand() {
        let dir = tempdir("rename");
        // library.db als Verzeichnis: der Rename schlägt fehl, der alte Pfad bleibt.
        std::fs::create_dir_all(dir.join("library.db")).unwrap();
        std::fs::write(dir.join("library.db/inhalt"), "alt").unwrap();
        let candidate = dir.join("backup.db");
        sample_db(&candidate);
        std::fs::write(
            dir.join("restore.pending"),
            std::fs::read(&candidate).unwrap(),
        )
        .unwrap();
        let err = apply_pending_restore(&dir).unwrap_err();
        assert!(err.contains("alter Bestand erhalten"), "{err}");
        assert!(
            dir.join("library.db/inhalt").exists(),
            "alter Bestand unangetastet"
        );
        assert!(
            dir.join("restore.pending").exists(),
            "Wunsch bleibt für später erhalten"
        );
    }

    #[test]
    fn absturz_beim_tauschen_wird_beim_naechsten_start_vollendet() {
        let dir = tempdir("absturz");
        let live = dir.join("library.db");
        sample_db(&live);
        let candidate = dir.join("backup.db");
        {
            let db = storage::Database::open(&candidate).unwrap();
            let acc = db.ensure_local_account().unwrap();
            let feed = db
                .add_feed(
                    &acc,
                    "https://example.org/zweite.xml",
                    "Zweite",
                    None,
                    "#654321",
                )
                .unwrap();
            db.upsert_article(
                feed,
                "z1",
                "Aus dem Backup",
                None,
                None,
                storage::now_ms(),
                "e",
                None,
                storage::now_ms(),
            )
            .unwrap();
        }
        std::fs::write(
            dir.join("restore.pending"),
            std::fs::read(&candidate).unwrap(),
        )
        .unwrap();
        // Absturzbild: Kandidat ist schon fertig validiert und als library.db.new da.
        let staged = dir.join("library.restore-staged");
        std::fs::copy(dir.join("restore.pending"), &staged).unwrap();
        storage::Database::prepare_restore_candidate(&staged).unwrap();
        std::fs::copy(&staged, dir.join("library.db.new")).unwrap();

        let notice = apply_pending_restore(&dir).unwrap().expect("Hinweis");
        assert!(notice.contains("abgeschlossen"), "{notice}");
        let db = storage::Database::open(&live).unwrap();
        assert!(
            db.raw()
                .query_row("SELECT COUNT(*) FROM articles WHERE id='z1'", [], |r| r
                    .get::<_, i64>(0))
                .unwrap()
                == 1,
            "der vorbereitete Kandidat ist aktiv"
        );
        assert!(!dir.join("library.db.new").exists());
        assert!(!dir.join("restore.pending").exists());
    }

    #[test]
    fn sicherung_enthält_auch_nicht_eingecheckte_wal_daten() -> Result<(), String> {
        let dir = tempdir("wal");
        let live = dir.join("library.db");
        let db = storage::Database::open(&live).unwrap();
        let acc = db.ensure_local_account().unwrap();
        let feed = db
            .add_feed(
                &acc,
                "https://example.net/feed.xml",
                "Feed",
                None,
                "#111111",
            )
            .unwrap();
        db.raw()
            .execute_batch("PRAGMA wal_autocheckpoint=0;")
            .map_err(|e| e.to_string())?;
        db.upsert_article(
            feed,
            "wal1",
            "Nur im WAL",
            None,
            None,
            storage::now_ms(),
            "e",
            None,
            storage::now_ms(),
        )
        .unwrap();
        // Absturz simulieren: Verbindung nie schließen, damit nichts eingecheckt wird.
        std::mem::forget(db);
        let wal = live.with_extension("db-wal");
        assert!(wal.exists(), "WAL vorhanden");
        let safety = consistent_backup(&dir, &live)?;
        let backup = storage::Database::open(&safety).unwrap();
        assert_eq!(
            backup
                .raw()
                .query_row("SELECT COUNT(*) FROM articles WHERE id='wal1'", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            1,
            "Sicherung enthält die nur im WAL stehende Transaktion"
        );
        Ok(())
    }

    #[test]
    fn paralleler_restore_wird_durch_die_sperre_verhindert() {
        let dir = tempdir("parallel");
        let _lock = LibraryLock::acquire(&dir).unwrap();
        let candidate = dir.join("backup.db");
        sample_db(&candidate);
        std::fs::write(
            dir.join("restore.pending"),
            std::fs::read(&candidate).unwrap(),
        )
        .unwrap();
        // Der Produktpfad ruft apply_pending_restore ausschließlich unter der Sperre auf;
        // der Test hält sie und dokumentiert die Reihenfolge.
        let lock_blocks_second = LibraryLock::acquire(&dir).is_err();
        assert!(lock_blocks_second);
        drop(_lock);
        assert!(apply_pending_restore(&dir).unwrap().is_some());
    }
}
