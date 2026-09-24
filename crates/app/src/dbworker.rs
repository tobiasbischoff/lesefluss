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
        Self { tx: self.tx.clone(), failure: self.failure.clone() }
    }
}

impl DbWorker {
    pub fn start(path: PathBuf) -> Self {
        let (tx, rx): (std::sync::mpsc::Sender<Envelope>, Receiver<Envelope>) = std::sync::mpsc::channel();
        let failure: std::sync::Arc<std::sync::Mutex<Option<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let failure_thread = failure.clone();
        std::thread::Builder::new()
            .name("lf-db".into())
            .spawn(move || {
                let db = match Database::open(&path) {
                    Ok(db) => db,
                    Err(e) => {
                        eprintln!("Datenbank konnte nicht geöffnet werden: {e}");
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
