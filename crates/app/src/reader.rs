use gtk::prelude::*;
use gtk::gio;
use std::cell::{Cell, RefCell};
use webkit6::prelude::*;

pub struct ReaderPane {
    pub loading: adw::StatusPage,
    pub toolbar: adw::ToolbarView,
    pub header: adw::HeaderBar,
    pub title: adw::WindowTitle,
    pub stack: gtk::Stack,
    pub webview: webkit6::WebView,
    pub empty: adw::StatusPage,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub btn_read: gtk::Button,
    pub btn_saved: gtk::Button,
    pub current: RefCell<Option<String>>,
    pub pending_scroll: Cell<f64>,
    pub style: RefCell<crate::style::ReaderStyleState>,
}

impl ReaderPane {
    pub fn new() -> Self {
        let session = webkit6::NetworkSession::new_ephemeral();
        let webview = webkit6::WebView::builder().network_session(&session).build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_enable_back_forward_navigation_gestures(false);
        }

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .css_classes(vec!["lf-pane-bg".to_string()])
            .build();
        let loading = adw::StatusPage::builder()
            .icon_name("content-loading-symbolic")
            .title("Artikel wird geladen …")
            .vexpand(true)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("applications-library-symbolic")
            .title("Kein Artikel geöffnet")
            .description("Wähle links einen Artikel aus.")
            .vexpand(true)
            .build();
        let error = adw::StatusPage::builder()
            .icon_name("dialog-error-symbolic")
            .title("Der Artikel konnte nicht dargestellt werden")
            .description("Der Web-Prozess wurde beendet. Der Inhalt ist weiterhin lokal verfügbar.")
            .vexpand(true)
            .build();
        let retry = gtk::Button::builder()
            .label("Erneut versuchen")
            .css_classes(vec!["pill".to_string(), "suggested-action".to_string()])
            .action_name("win.reader-retry")
            .build();
        error.set_child(Some(&retry));

        stack.add_named(&loading, Some("loading"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&webview, Some("web"));
        stack.add_named(&error, Some("error"));
        stack.set_visible_child_name("empty");

        let search_entry = gtk::SearchEntry::builder().placeholder_text("Im Artikel suchen").build();
        let search_bar = gtk::SearchBar::builder().child(&search_entry).show_close_button(false).build();

        let btn_read = gtk::Button::builder()
            .icon_name("mail-unread-symbolic")
            .tooltip_text("Gelesen/ungelesen umschalten (M)")
            .action_name("win.toggle-read")
            .build();
        let btn_saved = gtk::Button::builder()
            .icon_name("bookmark-new-symbolic")
            .tooltip_text("Speichern/Entspeichern (S)")
            .action_name("win.toggle-saved")
            .build();
        let btn_external = gtk::Button::builder()
            .icon_name("external-link-symbolic")
            .tooltip_text("Im Browser öffnen (O)")
            .action_name("win.open-external")
            .build();

        let zoom_menu = gio::Menu::new();
        zoom_menu.append(Some("Größer"), Some("win.zoom-in"));
        zoom_menu.append(Some("Kleiner"), Some("win.zoom-out"));
        zoom_menu.append(Some("Zurücksetzen"), Some("win.zoom-reset"));
        let btn_zoom = gtk::MenuButton::builder()
            .icon_name("font-x-large-symbolic")
            .tooltip_text("Typografie")
            .menu_model(&zoom_menu)
            .build();

        let more_menu = gio::Menu::new();
        more_menu.append(Some("Link kopieren"), Some("win.copy-link"));
        more_menu.append(Some("Im Artikel suchen"), Some("win.find"));
        let btn_more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Weitere Aktionen")
            .menu_model(&more_menu)
            .build();

        let title = adw::WindowTitle::new("Lesefluss", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        header.pack_start(&btn_read);
        header.pack_start(&btn_saved);
        header.pack_end(&btn_external);
        header.pack_end(&btn_zoom);
        header.pack_end(&btn_more);

        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_box.append(&search_bar);
        content_box.append(&stack);
        let toolbar = adw::ToolbarView::builder().content(&content_box).build();
        toolbar.add_top_bar(&header);

        let stack_weak = stack.clone();
        webview.connect_web_process_terminated(move |_, _reason| {
            stack_weak.set_visible_child_name("error");
        });

        webview.connect_decide_policy(|_, decision, decision_type| {
            if decision_type == webkit6::PolicyDecisionType::NavigationAction {
                if let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() {
                    if let Some(uri) = nav.navigation_action().and_then(|na| na.request()).and_then(|r| r.uri()) {
                        if !uri.starts_with("about:") {
                            decision.ignore();
                            let launcher = gtk::UriLauncher::new(&uri);
                            launcher.launch(None::<&gtk::Window>, None::<&gio::Cancellable>, |res| {
                                if let Err(e) = res {
                                    eprintln!("Extern öffnen fehlgeschlagen: {e}");
                                }
                            });
                            return true;
                        }
                    }
                }
            }
            if decision_type == webkit6::PolicyDecisionType::Response {
                decision.ignore();
                return true;
            }
            false
        });

        Self {
            toolbar,
            header,
            title,
            stack,
            webview,
            loading,
            empty,
            search_bar,
            search_entry,
            btn_read,
            btn_saved,
            current: RefCell::new(None),
            pending_scroll: Cell::new(-1.0),
            style: RefCell::new(crate::style::ReaderStyleState::default()),
        }
    }

    pub fn show_loading(&self) {
        self.stack.set_visible_child_name("loading");
    }

    pub fn load_html_doc(&self, html: &str) {
        self.stack.set_visible_child_name("web");
        self.webview.load_html(html, None);
    }

    pub fn show_error(&self) {
        self.stack.set_visible_child_name("error");
    }

    pub fn show_empty(&self, title: &str, description: &str) {
        *self.current.borrow_mut() = None;
        self.empty.set_title(title);
        self.empty.set_description(Some(description));
        self.stack.set_visible_child_name("empty");
    }

    pub fn restore_scroll(&self) {
        let y = self.pending_scroll.get();
        if y >= 0.0 {
            self.pending_scroll.set(-1.0);
            self.webview.evaluate_javascript(
                &format!("window.scrollTo(0, {y});"),
                None,
                None,
                None::<&gio::Cancellable>,
                |_| {},
            );
        }
    }
}

pub fn find_in_view(webview: &webkit6::WebView, text: &str) {
    if let Some(fc) = webview.find_controller() {
        if text.is_empty() {
            fc.search_finish();
        } else {
            fc.search(text, webkit6::FindOptions::WRAP_AROUND.bits(), 200);
        }
    }
}

pub fn find_next(webview: &webkit6::WebView) {
    if let Some(fc) = webview.find_controller() {
        fc.search_next();
    }
}
