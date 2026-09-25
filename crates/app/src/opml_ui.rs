//! OPML-Benutzerpfade: Dateiauswahl, Vorschau, Import und Export (§11).

use crate::window::App;
use adw::prelude::*;
use gtk::gio;

impl App {
    pub fn import_opml_dialog(&self) {
        let dlg = gtk::FileDialog::builder()
            .title(crate::tr!("OPML-Datei wählen", "Choose OPML file"))
            .accept_label(crate::tr!("Öffnen", "Open"))
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
                        app.show_toast(crate::tr!(
                            "OPML-Datei zu groß (Limit 20 MiB)",
                            "OPML file too large (20 MiB limit)"
                        ));
                    }
                    return;
                }
                Err(e) => {
                    if let Some(app) = w.upgrade() {
                        app.show_toast(&crate::tr_format!(
                            "Lesefehler: {e}",
                            "Could not read file: {e}"
                        ));
                    }
                    return;
                }
            };
            let Ok(xml) = String::from_utf8(bytes) else {
                if let Some(app) = w.upgrade() {
                    app.show_toast(crate::tr!(
                        "OPML-Datei ist kein UTF-8",
                        "OPML file is not UTF-8"
                    ));
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
                        app.show_toast(&crate::tr_format!("OPML-Fehler: {e}", "OPML error: {e}"));
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
        let body = crate::tr_format!(
            "{} neue Feeds, {} bestehende (bleiben erhalten, Gruppen werden zusammengeführt).{}",
            "New feeds: {}; existing feeds: {} (kept, groups will be merged).{}",
            new.len(),
            existing,
            if draft.errors.is_empty() {
                String::new()
            } else {
                crate::tr_format!(
                    " {} ungültige Einträge übersprungen.",
                    " {} invalid entries skipped.",
                    draft.errors.len()
                )
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
            .heading(crate::tr!("OPML-Import", "Import OPML"))
            .body(body)
            .extra_child(&scroll)
            .build();
        dialog.add_response("cancel", crate::tr!("Abbrechen", "Cancel"));
        dialog.add_response(
            "import",
            &crate::tr_format!("Feeds importieren: {}", "Import feeds: {}", new.len()),
        );
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
                        app.show_toast(crate::tr!(
                            "Import abgebrochen — es wurde nichts verändert",
                            "Import cancelled — no changes made"
                        ));
                    }
                    return;
                };
                app.reload_meta_keep();
                app.show_toast(&crate::tr_format!(
                    "{new_feeds} Feeds importiert, {merged} zusammengeführt{}",
                    "Feeds imported: {new_feeds}; merged: {merged}{}",
                    if errors > 0 {
                        crate::tr_format!(
                            ", {errors} Hinweise im Bericht",
                            ", {errors} notices in the report"
                        )
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
            .title(crate::tr!(
                "OPML-Export speichern unter",
                "Save OPML export as"
            ))
            .accept_label(crate::tr!("Speichern", "Save"))
            .build();
        dlg.set_initial_name(Some(crate::tr!(
            "lesefluss-abonnements.opml",
            "lesefluss-subscriptions.opml"
        )));
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
                    crate::tr!("OPML exportiert", "OPML exported")
                } else {
                    crate::tr!("Export fehlgeschlagen", "Export failed")
                });
            }
        });
    }
}
