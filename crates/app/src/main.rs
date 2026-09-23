mod fixtures;
mod list;
mod reader;
mod sidebar;
mod state;
mod style;
mod window;

use gtk::prelude::*;

fn main() -> gtk::glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("io.github.PROJEKTINHABER.Lesefluss")
        .build();
    app.connect_activate(|app| {
        window::run_and_keep(app);
    });
    app.run()
}
