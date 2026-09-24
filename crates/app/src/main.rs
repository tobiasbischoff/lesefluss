mod dbworker;
mod fixtures;
mod list;
mod model;
mod net;
mod opml;
mod reader;
mod seed;
mod sidebar;
mod state;
mod style;
mod window;

use gtk::prelude::*;
use std::rc::Rc;

thread_local! {
    static INSTANCE: std::cell::RefCell<Option<Rc<window::App>>> = const { std::cell::RefCell::new(None) };
}

fn main() -> gtk::glib::ExitCode {
    let dir = window::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let db_path = dir.join("library.db");
    let pending = dir.join("restore.pending");
    if pending.exists() {
        let _ = std::fs::remove_file(dir.join("library.db-wal"));
        let _ = std::fs::remove_file(dir.join("library.db-shm"));
        let _ = std::fs::copy(&pending, &db_path);
        let _ = std::fs::remove_file(&pending);
    }
    let worker = dbworker::DbWorker::start(db_path);
    let net = Rc::new(net::Net::start());
    net.spawn_scheduler(worker.clone());

    let app = adw::Application::builder()
        .application_id("io.github.PROJEKTINHABER.Lesefluss")
        .build();
    let worker2 = worker.clone();
    let net2 = Rc::clone(&net);
    app.connect_activate(move |a| {
        INSTANCE.with(|slot| {
            if slot.borrow().is_none() {
                let instance = window::App::new(a, worker2.clone(), Rc::clone(&net2));
                *slot.borrow_mut() = Some(instance);
            } else if let Some(existing) = slot.borrow().as_ref() {
                existing.window.present();
            }
        });
    });
    app.run()
}
