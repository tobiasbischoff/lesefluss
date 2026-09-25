use gtk::gio;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use webkit6::prelude::*;

/// Läuft in einer isolierten Script-Welt und setzt nachgeladene Bilder anhand
/// von `data-lf-src`. Der WebView selbst bleibt skriptfrei (CSP `default-src 'none'`).
const MEDIA_BRIDGE_JS: &str = r#"
document.addEventListener('lf-media', function (event) {
  var detail = event.detail;
  if (!detail || !detail.url) return;
  var images = document.querySelectorAll('img[data-lf-src="' + detail.url + '"]');
  for (var i = 0; i < images.length; i++) {
    images[i].src = detail.data;
    images[i].removeAttribute('data-pending');
  }
});
"#;

pub struct ReaderPane {
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
    pub document_generation: Cell<u64>,
}

impl ReaderPane {
    pub fn new() -> Self {
        let session = webkit6::NetworkSession::new_ephemeral();
        // Mediennachlagerung läuft als User-Script in einer isolierten Welt;
        // die Seite selbst bleibt durch `default-src 'none'` skriptfrei.
        let content_manager = webkit6::UserContentManager::new();
        content_manager.add_script(&webkit6::UserScript::new(
            MEDIA_BRIDGE_JS,
            webkit6::UserContentInjectedFrames::TopFrame,
            webkit6::UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
        let webview = webkit6::WebView::builder()
            .network_session(&session)
            .user_content_manager(&content_manager)
            .build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_enable_back_forward_navigation_gestures(false);
        }

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .css_classes(vec!["lf-pane-bg".to_string()])
            .build();
        let loading = adw::StatusPage::builder()
            .icon_name("content-loading-symbolic")
            .title(crate::tr!("Artikel wird geladen …", "Loading article…"))
            .vexpand(true)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("applications-library-symbolic")
            .title(crate::tr!("Kein Artikel geöffnet", "No article open"))
            .description(crate::tr!(
                "Wähle links einen Artikel aus.",
                "Select an article on the left."
            ))
            .vexpand(true)
            .build();
        let error = adw::StatusPage::builder()
            .icon_name("dialog-error-symbolic")
            .title(crate::tr!(
                "Der Artikel konnte nicht dargestellt werden",
                "The article could not be displayed"
            ))
            .description(crate::tr!(
                "Der Web-Prozess wurde beendet. Der Inhalt ist weiterhin lokal verfügbar.",
                "The web process stopped. The content is still available offline."
            ))
            .vexpand(true)
            .build();
        let retry = gtk::Button::builder()
            .label(crate::tr!("Erneut versuchen", "Try again"))
            .css_classes(vec!["pill".to_string(), "suggested-action".to_string()])
            .action_name("win.reader-retry")
            .build();
        error.set_child(Some(&retry));

        stack.add_named(&loading, Some("loading"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&webview, Some("web"));
        stack.add_named(&error, Some("error"));
        stack.set_visible_child_name("empty");

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text(crate::tr!("Im Artikel suchen", "Find in article"))
            .build();
        let search_bar = gtk::SearchBar::builder()
            .child(&search_entry)
            .show_close_button(false)
            .build();

        let btn_read = gtk::Button::builder()
            .icon_name("mail-unread-symbolic")
            .tooltip_text(crate::tr!(
                "Gelesen/ungelesen umschalten (M)",
                "Toggle read/unread (M)"
            ))
            .action_name("win.toggle-read")
            .build();
        let btn_saved = gtk::Button::builder()
            .icon_name("bookmark-new-symbolic")
            .tooltip_text(crate::tr!("Speichern/Entspeichern (S)", "Save/unsave (S)"))
            .action_name("win.toggle-saved")
            .build();
        let btn_external = gtk::Button::builder()
            .icon_name("external-link-symbolic")
            .tooltip_text(crate::tr!("Im Browser öffnen (O)", "Open in browser (O)"))
            .action_name("win.open-external")
            .build();

        let zoom_menu = gio::Menu::new();
        zoom_menu.append(Some(crate::tr!("Größer", "Larger")), Some("win.zoom-in"));
        zoom_menu.append(Some(crate::tr!("Kleiner", "Smaller")), Some("win.zoom-out"));
        zoom_menu.append(
            Some(crate::tr!("Zurücksetzen", "Reset")),
            Some("win.zoom-reset"),
        );
        let btn_zoom = gtk::MenuButton::builder()
            .icon_name("font-x-large-symbolic")
            .tooltip_text(crate::tr!("Typografie", "Typography"))
            .menu_model(&zoom_menu)
            .build();

        let more_menu = gio::Menu::new();
        more_menu.append(
            Some(crate::tr!("Link kopieren", "Copy link")),
            Some("win.copy-link"),
        );
        more_menu.append(
            Some(crate::tr!("Im Artikel suchen", "Find in article")),
            Some("win.find"),
        );
        let btn_more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text(crate::tr!("Weitere Aktionen", "More actions"))
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
                    if let Some(uri) = nav
                        .navigation_action()
                        .and_then(|na| na.request())
                        .and_then(|r| r.uri())
                    {
                        if !uri.starts_with("about:") {
                            decision.ignore();
                            let launcher = gtk::UriLauncher::new(&uri);
                            launcher.launch(
                                None::<&gtk::Window>,
                                None::<&gio::Cancellable>,
                                |res| {
                                    if let Err(e) = res {
                                        eprintln!(
                                            "{}",
                                            crate::tr_format!(
                                                "Extern öffnen fehlgeschlagen: {e}",
                                                "Could not open in browser: {e}"
                                            )
                                        );
                                    }
                                },
                            );
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
            empty,
            search_bar,
            search_entry,
            btn_read,
            btn_saved,
            current: RefCell::new(None),
            pending_scroll: Cell::new(-1.0),
            style: RefCell::new(crate::style::ReaderStyleState::default()),
            document_generation: Cell::new(0),
        }
    }

    pub fn show_loading(&self) {
        self.stack.set_visible_child_name("loading");
    }

    /// Reserviert die nächste Dokumentgeneration **bevor** das HTML entsteht.
    /// HTML, Medienjobs und Positionscapture verwenden danach dieselbe Nummer.
    pub fn reserve_generation(&self) -> u64 {
        let next = self.document_generation.get() + 1;
        self.document_generation.set(next);
        next
    }

    pub fn load_html_doc(&self, html: &str, generation: u64) {
        self.stack.set_visible_child_name("web");
        if self.document_generation.get() < generation {
            self.document_generation.set(generation);
        }
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

    /// Weist einem Bild anhand seiner `data-lf-src`-Adresse ein Bild zu.
    /// Die Brücke läuft im User-Script, nicht in der Seite.
    pub fn apply_media(&self, url: &str, data_uri: &str) {
        self.evaluate_media_js(&media_event_js(url, data_uri));
    }

    pub fn evaluate_media_js(&self, js: &str) {
        self.webview
            .evaluate_javascript(js, None, None, None::<&gio::Cancellable>, |result| {
                if let Err(e) = result {
                    eprintln!(
                        "[lf] Reader-JS abgelehnt: {}",
                        crate::window::redact(&e.to_string())
                    );
                }
            });
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

/// Der Payload entsteht mit dem JSON-Serializer; von Hand escape erzeugte Strings
/// waren ungültiges JavaScript und erreichten den Reader nicht.
pub fn media_event_js(url: &str, data_uri: &str) -> String {
    let payload = serde_json::json!({ "url": url, "data": data_uri }).to_string();
    format!("document.dispatchEvent(new CustomEvent('lf-media', {{ detail: {payload} }}));")
}

#[cfg(test)]
mod media_js_tests {
    use super::*;

    fn media_js(url: &str, data_uri: &str) -> String {
        media_event_js(url, data_uri)
    }

    #[test]
    fn media_js_ist_gueltiges_javascript() {
        for (url, data) in [
            ("https://example.com/a.png", "data:image/png;base64,AAAA"),
            (
                "https://example.com/b.png?x=1&y=2",
                "data:image/png;base64,AAA'BBB",
            ),
            (
                "https://example.com/c.png",
                "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=",
            ),
        ] {
            let js = media_js(url, data);
            let wrapped = format!(
                "let document = {{ dispatchEvent: function(e) {{ if (!e.detail.url || !e.detail.data) throw new Error('unvollständig'); globalThis.__ok = e.detail.url + '|' + e.detail.data; }} }};\n{js}"
            );
            let path = std::env::temp_dir().join(format!(
                "lesefluss-media-js-{}-{}.js",
                std::process::id(),
                url.len()
            ));
            std::fs::write(&path, &wrapped).unwrap();
            let output = std::process::Command::new("node")
                .arg("--check")
                .arg(&path)
                .output();
            let _ = std::fs::remove_file(&path);
            match output {
                Ok(out) => assert!(
                    out.status.success(),
                    "Syntaxfehler für {url}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ),
                Err(_) => {
                    // Ohne Node bleibt mindestens die strukturelle Prüfung.
                    assert!(js.starts_with("document.dispatchEvent(new CustomEvent("));
                    assert!(!js.contains("\\\"url\\\":'"), "kein handescapter Payload");
                }
            }
        }
    }

    #[test]
    fn media_js_ist_auch_als_ausdruck_auswertbar() {
        let js = media_js("https://example.com/a.png", "data:image/png;base64,AAAA");
        let program = format!(
            "let captured = null; let document = {{ dispatchEvent: (e) => {{ captured = e.detail; }} }};\n{js}\nif (captured.url !== \"https://example.com/a.png\") throw new Error('url');\nif (captured.data !== \"data:image/png;base64,AAAA\") throw new Error('data');\n"
        );
        let path =
            std::env::temp_dir().join(format!("lesefluss-media-run-{}.js", std::process::id()));
        std::fs::write(&path, program).unwrap();
        if let Ok(out) = std::process::Command::new("node").arg(&path).output() {
            let _ = std::fs::remove_file(&path);
            assert!(
                out.status.success(),
                "Auswertung fehlgeschlagen: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}
