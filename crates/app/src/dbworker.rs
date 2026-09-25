use std::any::Any;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, SyncSender};
use storage::Database;

pub type JobOut = Box<dyn Any + Send>;
type Job = Box<dyn FnOnce(&Database) -> JobOut + Send>;

struct Envelope {
    job: Job,
    reply: SyncSender<JobOut>,
}

pub struct DbWorker {
    tx: std::sync::mpsc::Sender<Envelope>,
    pub failure: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl Clone for DbWorker {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            failure: self.failure.clone(),
        }
    }
}

impl DbWorker {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx): (std::sync::mpsc::Sender<Envelope>, Receiver<Envelope>) =
            std::sync::mpsc::channel();
        let failure: std::sync::Arc<std::sync::Mutex<Option<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let failure_thread = failure.clone();
        std::thread::Builder::new()
            .name("lf-db".into())
            .spawn(move || {
                let db = match Database::open(&path) {
                    Ok(db) => db,
                    Err(e) => {
                        eprintln!(
                            "{}",
                            crate::tr_format!(
                                "Datenbank konnte nicht geöffnet werden: {e}",
                                "Could not open database: {e}"
                            )
                        );
                        if let Ok(mut slot) = failure_thread.lock() {
                            *slot = Some(e.to_string());
                        }
                        return;
                    }
                };
                for env in rx {
                    let out = (env.job)(&db);
                    let _ = env.reply.send(out);
                }
            })
            .expect("db worker thread");
        Self { tx, failure }
    }

    /// Layoutwiederherstellung beim Start. Läuft vor dem Ereignis-Schleifen-
    /// Anlauf und wartet daher bewusst auf den Worker; Fehler werden benannt
    /// statt als „kein Layout“ verschluckt.
    pub fn read_layout(&self) -> Result<Option<String>, String> {
        let out = self.send(|db| db.get_pref("layout")).recv().map_err(|_| {
            crate::tr!(
                "Datenbank-Worker antwortet nicht",
                "Database worker is not responding"
            )
            .to_string()
        })?;
        match out.downcast::<storage::Result<Option<String>>>() {
            Ok(value) => value.map_err(|e| e.to_string()),
            Err(_) => Err(crate::tr!(
                "unerwartetes Antwortformat des Datenbank-Workers",
                "Unexpected response format from database worker"
            )
            .to_string()),
        }
    }

    pub fn send<F, R>(&self, f: F) -> Receiver<JobOut>
    where
        F: FnOnce(&Database) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (reply, rx) = std::sync::mpsc::sync_channel(1);
        let job: Job = Box::new(move |db| Box::new(f(db)));
        let _ = self.tx.send(Envelope { job, reply });
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B7: Der Layout-Leser nennt Fehler, statt sie als „kein Layout“ zu
    /// verschlucken, und legt keinen blockierenden Hilfs-Thread an.
    #[test]
    fn layout_lesen_liefert_wert_oder_nennt_den_fehler() {
        let dir = std::env::temp_dir().join(format!("lesefluss-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let worker = DbWorker::start(dir.join("library.db"));
        assert_eq!(worker.read_layout().unwrap(), None);
        worker
            .send(|db| db.set_pref("layout", "1280;800"))
            .recv()
            .unwrap()
            .downcast::<storage::Result<()>>()
            .unwrap()
            .unwrap();
        assert_eq!(
            worker.read_layout().unwrap(),
            Some("1280;800".to_string()),
            "der gespeicherte Wert kommt zurück"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// B7: Ein nicht erreichbarer Worker endet als benannter Fehler.
    #[test]
    fn read_layout_ohne_worker_meldet_fehler() {
        let path = std::env::temp_dir().join(format!("lesefluss-fehlt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        // Als Verzeichnis kann SQLite keine Datenbank öffnen: der Worker startet nicht.
        std::fs::create_dir_all(path.join("library.db")).unwrap();
        let worker = DbWorker::start(path.join("library.db"));
        let err = worker.read_layout().unwrap_err();
        assert_eq!(err, "Database worker is not responding");
        let _ = std::fs::remove_dir_all(&path);
    }
}
