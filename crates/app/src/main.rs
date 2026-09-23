mod dbworker;
mod fixtures;
mod list;
mod model;
mod net;
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

fn data_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home).join(".local/share")
        });
    base.join("lesefluss")
}

fn main() -> gtk::glib::ExitCode {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let db_path = dir.join("library.db");
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
