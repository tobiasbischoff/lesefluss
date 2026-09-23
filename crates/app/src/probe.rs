use adw::prelude::*;
use gtk::glib;
use webkit6::prelude::*;

const APP_ID: &str = "io.github.PROJEKTINHABER.Lesefluss.Probe";

fn long_document() -> String {
    let mut body = String::new();
    body.push_str(
        "<!doctype html><html><head><meta charset='utf-8'>\
         <meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; style-src 'unsafe-inline'\">\
         <style>body{background:#1C1D20;color:#F0F0F2;font-family:sans-serif;\
         max-width:760px;margin:0 auto;padding:48px;line-height:1.6;font-size:18px}\
         h1{font-size:34px;line-height:1.15}a{color:#B9AAFF}</style></head><body>\
         <h1>M0-Probe: Langes Dokument</h1>",
    );
    for i in 1..=200 {
        body.push_str(&format!(
            "<p>Absatz {i}: Dies ist ein Testabsatz für Scrollqualität. \
             Er enthält genug Text, um mehrere Zeilen umzubrechen, sowie einen \
             <a href='https://example.com/{i}'>Link</a> und <b>fetten</b> sowie <i>kursiven</i> Text.</p>"
        ));
    }
    body.push_str("</body></html>");
    body
}

fn build_sidebar(webview: &webkit6::WebView) -> gtk::Widget {
    let webview = webview.clone();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(vec!["navigation-sidebar".to_string()])
        .build();
    for label in ["Ungelesen", "Alle Artikel", "Gespeichert", "Beispielfeed"] {
        list.append(&gtk::ListBoxRow::builder().child(&gtk::Label::new(Some(label))).build());
    }
    list.connect_row_activated(move |_, row| {
        webview.load_html(&format!("<html><body style='background:#1C1D20;color:#F0F0F2;font:18px sans-serif'><h1>{}</h1><p>Native Auswahl wirkt auf WebView.</p></body></html>", row.child().and_then(|c| c.downcast::<gtk::Label>().ok()).map(|l| l.text()).unwrap_or_default()), None);
    });

    let scrolled = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .width_request(248)
        .build();
    scrolled.upcast()
}

fn activate(app: &adw::Application) {
    let session = webkit6::NetworkSession::new_ephemeral();
    let webview = webkit6::WebView::builder()
        .network_session(&session)
        .build();
    webview.load_html(&long_document(), None);

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&build_sidebar(&webview))
        .end_child(&webview)
        .position(248)
        .resize_start_child(false)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .build();

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::builder().content(&paned).build();
    toolbar.add_top_bar(&header);

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1440)
        .default_height(900)
        .title("Lesefluss — M0-Probe")
        .content(&toolbar)
        .build();

    // F6/Shift+F6: Fokus zwischen Sidebar und WebView wechseln.
    let list_focus = webview.clone();
    let controller = gtk::EventControllerKey::new();
    controller.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::F6 {
            if let Some(w) = list_focus.ancestor(gtk::Window::static_type()) {
                gtk::prelude::GtkWindowExt::set_focus(w.downcast_ref::<gtk::Window>().unwrap(), list_focus.parent().as_ref());
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    webview.add_controller(controller);

    window.present();

    // Backend-Nachweis: Display-Typ und Sitzungs-Env ausgeben.
    let display = gtk::prelude::WidgetExt::display(&window);
    println!("GDK-Display-Typ: {}", display.type_().name());
    println!(
        "WAYLAND_DISPLAY={} XDG_SESSION_TYPE={:?}",
        std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "<leer>".into()),
        std::env::var("XDG_SESSION_TYPE").ok()
    );
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(activate);
    app.run_with_args::<String>(&[])
}
