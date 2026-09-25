//! OPML-Benutzerpfade: Dateiauswahl, Vorschau, Import und Export (§11).

use crate::window::App;
use adw::prelude::*;
use gtk::gio;

impl App {
    pub fn import_opml_dialog(&self) {
        let dlg = gtk::FileDialog::builder()
            .title("OPML-Datei wählen")
            .build();
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.open_future(None::<&gtk::Window>).await else {
                return;
            };
            let Some(path) = file.path() else { return };
            let bytes = match std::fs::read(&path) {
                Ok(b) if b.len() <= crate::opml::MAX_OPML_BYTES => b,
                Ok(_) => {
                    if let Some(app) = w.upgrade() {
                        app.show_toast("OPML-Datei zu groß (Limit 20 MiB)");
                    }
                    return;
                }
                Err(e) => {
                    if let Some(app) = w.upgrade() {
                        app.show_toast(&format!("Lesefehler: {e}"));
                    }
                    return;
                }
            };
            let Ok(xml) = String::from_utf8(bytes) else {
                if let Some(app) = w.upgrade() {
                    app.show_toast("OPML-Datei ist kein UTF-8");
                }
                return;
            };
            match crate::opml::parse_opml(&xml) {
                Ok(draft) => {
                    if let Some(app) = w.upgrade() {
                        app.show_opml_preview(draft);
                    }
                }
                Err(e) => {
                    if let Some(app) = w.upgrade() {
                        app.show_toast(&format!("OPML-Fehler: {e}"));
                    }
                }
            }
        });
    }

    pub fn show_opml_preview(&self, draft: crate::opml::OpmlDraft) {
        let known: std::collections::HashSet<String> = self
            .state
            .borrow()
            .feeds
            .iter()
            .map(|f| f.feed_url.clone())
            .collect();
        let new: Vec<&crate::opml::OpmlFeed> = draft
            .feeds
            .iter()
            .filter(|f| !known.contains(&f.xml_url))
            .collect();
        let existing = draft.feeds.len() - new.len();
        let listing: String = new
            .iter()
            .take(40)
            .map(|f| format!("• {} — {}\n", f.title, f.xml_url))
            .collect();
        let body = format!(
            "{} neue Feeds, {} bestehende (bleiben erhalten, Gruppen werden zusammengeführt).{}",
            new.len(),
            existing,
            if draft.errors.is_empty() {
                String::new()
            } else {
                format!(" {} ungültige Einträge übersprungen.", draft.errors.len())
            }
        );
        let label = gtk::Label::builder()
            .label(&listing)
            .xalign(0.0)
            .wrap(true)
            .margin_start(12)
            .margin_end(12)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .child(&label)
            .max_content_height(280)
            .min_content_height(80)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading("OPML-Import")
            .body(body)
            .extra_child(&scroll)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("import", &format!("{} Feeds importieren", new.len()));
        dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("import"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp == "import" {
                app.import_opml(draft.clone());
            }
        });
    }

    pub fn import_opml(&self, draft: crate::opml::OpmlDraft) {
        let w = self.weak();
        let entries: Vec<(String, String, Option<String>, Vec<String>)> = draft
            .feeds
            .iter()
            .map(|f| {
                (
                    f.title.clone(),
                    f.xml_url.clone(),
                    f.html_url.clone(),
                    f.groups.clone(),
                )
            })
            .collect();
        let errors = draft.errors.len();
        self.db_query(
            move |db| {
                db.ensure_local_account()?;
                db.import_opml_entries("local", &entries)
            },
            move |app, res: storage::Result<(usize, usize)>| {
                let Ok((new_feeds, merged)) = res else {
                    if let Some(app) = w.upgrade() {
                        app.show_toast("Import abgebrochen — es wurde nichts verändert");
                    }
                    return;
                };
                app.reload_meta_keep();
                app.show_toast(&format!(
                    "{new_feeds} Feeds importiert, {merged} zusammengeführt{}",
                    if errors > 0 {
                        format!(", {errors} Hinweise im Bericht")
                    } else {
                        String::new()
                    }
                ));
            },
        );
    }

    pub fn export_opml(&self) {
        let feeds: Vec<crate::opml::OpmlFeed> = {
            let st = self.state.borrow();
            st.feeds
                .iter()
                .map(|f| crate::opml::OpmlFeed {
                    title: f.title.clone(),
                    xml_url: f.feed_url.clone(),
                    html_url: None,
                    groups: f
                        .groups
                        .iter()
                        .filter_map(|g| {
                            st.groups
                                .iter()
                                .find(|x| x.id == *g)
                                .map(|x| x.name.clone())
                        })
                        .collect(),
                })
                .collect()
        };
        let dlg = gtk::FileDialog::builder()
            .title("OPML-Export speichern unter")
            .build();
        dlg.set_initial_name(Some("lesefluss-abonnements.opml"));
        let w = self.weak();
        glib::MainContext::default().spawn_local(async move {
            let Ok(file) = dlg.save_future(None::<&gtk::Window>).await else {
                return;
            };
            let Some(path) = file.path() else { return };
            let xml = crate::opml::build_opml(&feeds);
            let tmp = path.with_extension("opml.tmp");
            let ok = std::fs::write(&tmp, xml.as_bytes()).is_ok()
                && std::fs::rename(&tmp, &path).is_ok();
            if let Some(app) = w.upgrade() {
                app.show_toast(if ok {
                    "OPML exportiert"
                } else {
                    "Export fehlgeschlagen"
                });
            }
        });
    }
}
