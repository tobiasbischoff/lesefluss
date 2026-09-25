//! Einstellungen als eigenes Modul: Seiten, Bedienelemente und Persistenz (§16).

use crate::window::{data_dir, status_label, App};
use adw::prelude::*;
use std::time::Duration;

impl App {
    pub fn settings_dialog(&self) {
        let win = adw::PreferencesWindow::builder()
            .modal(true)
            .transient_for(&self.window)
            .title(crate::tr!("Einstellungen", "Settings"))
            .build();
        let p = self.prefs.borrow().clone();

        let language_page = adw::PreferencesPage::builder()
            .title(crate::tr!("Sprache", "Language"))
            .icon_name("preferences-desktop-locale-symbolic")
            .build();
        let language_group = adw::PreferencesGroup::builder()
            .title(crate::tr!("Oberflächensprache", "Interface language"))
            .build();
        let language = adw::ComboRow::builder()
            .title(crate::tr!("Sprache", "Language"))
            .subtitle(crate::tr!(
                "Wird nach einem Neustart angewendet",
                "Applies after restarting Lesefluss"
            ))
            .model(&gtk::StringList::new(&[
                "English",
                "Deutsch",
                crate::tr!("Systemsprache", "System language"),
            ]))
            .selected(match p.language {
                crate::strings::LanguageChoice::English => 0,
                crate::strings::LanguageChoice::German => 1,
                crate::strings::LanguageChoice::System => 2,
            })
            .build();
        language_group.add(&language);
        if std::env::var("LF_LANG").ok().is_some_and(|v| !v.is_empty()) {
            language_group.set_description(Some(crate::tr!(
                "LF_LANG überschreibt die Auswahl beim Start. Zum Verwenden dieser Einstellung LF_LANG entfernen.",
                "LF_LANG overrides this choice at startup. Unset LF_LANG to use this setting."
            )));
        }
        language_page.add(&language_group);
        win.add(&language_page);
        let w = self.weak();
        language.connect_selected_item_notify(move |row| {
            let Some(app) = w.upgrade() else { return };
            let choice = match row.selected() {
                1 => crate::strings::LanguageChoice::German,
                2 => crate::strings::LanguageChoice::System,
                _ => crate::strings::LanguageChoice::English,
            };
            app.db_query(
                move |db| db.set_pref("language", choice.key()),
                move |app, result| {
                    app.show_toast(if result.is_ok() {
                        app.prefs.borrow_mut().language = choice;
                        crate::tr!(
                            "Sprache gespeichert — bitte Lesefluss neu starten",
                            "Language saved — please restart Lesefluss"
                        )
                    } else {
                        crate::tr!(
                            "Sprache konnte nicht gespeichert werden",
                            "Could not save language"
                        )
                    });
                },
            );
        });

        let page_read = adw::PreferencesPage::builder()
            .title(crate::tr!("Lesen", "Reading"))
            .icon_name("text-x-generic-symbolic")
            .build();
        let grp = adw::PreferencesGroup::builder()
            .title(crate::tr!("Leseverhalten", "Reading behavior"))
            .build();
        let auto = adw::SwitchRow::builder()
            .title(crate::tr!(
                "Automatisch als gelesen markieren",
                "Automatically mark as read"
            ))
            .subtitle(crate::tr!(
                "Nach 0,8 s sichtbarem Artikel",
                "After the article has been visible for 0.8 seconds"
            ))
            .active(p.auto_read)
            .build();
        grp.add(&auto);
        let font = adw::SpinRow::builder()
            .title(crate::tr!("Schriftgröße Reader", "Reader font size"))
            .adjustment(&gtk::Adjustment::new(
                p.reader_font,
                14.0,
                28.0,
                1.0,
                2.0,
                0.0,
            ))
            .build();
        grp.add(&font);
        let measure = adw::SpinRow::builder()
            .title(crate::tr!(
                "Zeilenbreite (Zeichen)",
                "Line width (characters)"
            ))
            .adjustment(&gtk::Adjustment::new(
                p.reader_measure as f64,
                55.0,
                85.0,
                1.0,
                5.0,
                0.0,
            ))
            .build();
        grp.add(&measure);
        let lh = adw::SpinRow::builder()
            .title(crate::tr!("Zeilenhöhe", "Line height"))
            .adjustment(&gtk::Adjustment::new(
                p.reader_line_height,
                1.4,
                2.0,
                0.05,
                0.1,
                0.0,
            ))
            .build();
        grp.add(&lh);
        page_read.add(&grp);
        win.add(&page_read);

        let page_view = adw::PreferencesPage::builder()
            .title(crate::tr!("Darstellung", "Appearance"))
            .icon_name("preferences-desktop-appearance-symbolic")
            .build();
        let grp = adw::PreferencesGroup::builder()
            .title(crate::tr!("Erscheinungsbild", "Appearance"))
            .build();
        let theme = adw::ComboRow::builder()
            .title("Theme")
            .model(&gtk::StringList::new(&[
                crate::tr!("System", "System"),
                crate::tr!("Dunkel", "Dark"),
                crate::tr!("Hell", "Light"),
                "Omarchy",
            ]))
            .build();
        let idx = match p.theme.as_str() {
            "dark" => 1,
            "light" => 2,
            "omarchy" => 3,
            _ => 0,
        };
        theme.set_selected(idx);
        grp.add(&theme);
        let compact = adw::SwitchRow::builder()
            .title(crate::tr!("Kompakte Liste", "Compact list"))
            .active(p.compact)
            .build();
        grp.add(&compact);
        let block_images = adw::SwitchRow::builder()
            .title(crate::tr!(
                "Externe Bilder blockieren",
                "Block external images"
            ))
            .subtitle(crate::tr!(
                "Keine Bilder nachladen; nur Platzhalter mit Bildbeschreibung",
                "Show placeholders with image descriptions instead of loading images"
            ))
            .active(p.block_images)
            .build();
        grp.add(&block_images);
        let thumbs = adw::SwitchRow::builder()
            .title(crate::tr!(
                "Bildvorschauen in der Liste",
                "Image previews in the list"
            ))
            .active(p.thumbs)
            .build();
        grp.add(&thumbs);
        let letters = adw::SwitchRow::builder()
            .title(crate::tr!(
                "Buchstabenkürzel (j/k/n/p/m/s/o)",
                "Letter shortcuts (j/k/n/p/m/s/o)"
            ))
            .active(p.letter_shortcuts)
            .build();
        grp.add(&letters);
        let order = adw::ComboRow::builder()
            .title(crate::tr!("Reihenfolge", "Sort order"))
            .subtitle(crate::tr!(
                "Gilt für alle Ansichten und Konten",
                "Applies to all views and accounts"
            ))
            .model(&gtk::StringList::new(&[
                crate::tr!("Neueste zuerst", "Newest first"),
                crate::tr!("Älteste zuerst", "Oldest first"),
            ]))
            .selected(if p.newest_first { 0 } else { 1 })
            .build();
        grp.add(&order);
        page_view.add(&grp);
        win.add(&page_view);

        let page_sync = adw::PreferencesPage::builder()
            .title(crate::tr!("Aktualisierung", "Refresh"))
            .icon_name("view-refresh-symbolic")
            .build();
        let grp = adw::PreferencesGroup::builder()
            .title(crate::tr!("Abruf", "Fetching"))
            .build();
        let refresh = adw::SpinRow::builder()
            .title(crate::tr!("Intervall (Minuten)", "Interval (minutes)"))
            .adjustment(&gtk::Adjustment::new(
                p.refresh_min as f64,
                5.0,
                1440.0,
                5.0,
                30.0,
                0.0,
            ))
            .build();
        grp.add(&refresh);
        page_sync.add(&grp);
        win.add(&page_sync);

        let page_store = adw::PreferencesPage::builder()
            .title(crate::tr!("Speicher & Datenschutz", "Storage & privacy"))
            .icon_name("drive-harddisk-symbolic")
            .build();
        let grp = adw::PreferencesGroup::builder()
            .title(crate::tr!("Aufbewahrung", "Retention"))
            .build();
        let retention = adw::SpinRow::builder()
            .title(crate::tr!(
                "Gelesene Inhalte behalten (Tage)",
                "Keep read articles (days)"
            ))
            .subtitle(crate::tr!(
                "Danach Bereinigung; Gespeicherte bleiben",
                "Older read articles are removed; saved articles are kept"
            ))
            .adjustment(&gtk::Adjustment::new(
                p.retention_days as f64,
                7.0,
                3650.0,
                1.0,
                30.0,
                0.0,
            ))
            .build();
        grp.add(&retention);
        let media = adw::SpinRow::builder()
            .title(crate::tr!("Bildcache (MiB)", "Image cache (MiB)"))
            .adjustment(&gtk::Adjustment::new(
                p.media_mb as f64,
                64.0,
                4096.0,
                64.0,
                256.0,
                0.0,
            ))
            .build();
        grp.add(&media);

        // Speicherübersicht: Datenbank, Bildcache und gepinnte Dateien getrennt.
        let db_row = adw::ActionRow::builder()
            .title(crate::tr!("Datenbank", "Database"))
            .subtitle(crate::tr!("wird berechnet …", "Calculating…"))
            .build();
        let cache_row = adw::ActionRow::builder()
            .title(crate::tr!("Bildcache", "Image cache"))
            .subtitle(crate::tr!("wird berechnet …", "Calculating…"))
            .build();
        let pinned_row = adw::ActionRow::builder()
            .title(crate::tr!(
                "Gepinnte Bilder (gespeicherte Artikel)",
                "Pinned images (saved articles)"
            ))
            .subtitle(crate::tr!(
                "werden nie automatisch gelöscht",
                "Never deleted automatically"
            ))
            .build();
        let grp_usage = adw::PreferencesGroup::builder()
            .title(crate::tr!("Belegung", "Storage usage"))
            .description(crate::tr!(
                "Getrennt nach Datenbank, Cache und geschützten Dateien",
                "Database, cache and protected files shown separately"
            ))
            .build();
        grp_usage.add(&db_row);
        grp_usage.add(&cache_row);
        grp_usage.add(&pinned_row);
        page_store.add(&grp_usage);
        {
            let pinned_row = pinned_row.clone();
            let cache = std::sync::Arc::clone(&self.media);
            self.db_query(
                move |db| {
                    let pinned = db.pinned_media_urls()?;
                    let pinned_bytes: i64 = pinned
                        .iter()
                        .filter_map(|u| {
                            std::fs::metadata(cache.path_for(u))
                                .ok()
                                .and_then(|m| i64::try_from(m.len()).ok())
                        })
                        .sum();
                    Ok::<_, storage::StorageError>((pinned.len() as i64, pinned_bytes))
                },
                move |_app, res| {
                    if let Ok((count, bytes)) = res {
                        pinned_row.set_subtitle(&crate::tr_plural!(
                            count,
                            "{count} Datei · {} KiB",
                            "{count} Dateien · {} KiB",
                            "{count} file · {} KiB",
                            "{count} files · {} KiB",
                            bytes / 1024
                        ));
                    }
                },
            );
        }
        glib::timeout_add_local_once(Duration::from_millis(50), {
            let db_row = db_row.clone();
            let cache_row = cache_row.clone();
            let cache = std::sync::Arc::clone(&self.media);
            move || {
                let db_bytes = std::fs::metadata(data_dir().join("library.db"))
                    .map(|m| m.len())
                    .unwrap_or(0);
                let db_wal = std::fs::metadata(data_dir().join("library.db-wal"))
                    .map(|m| m.len())
                    .unwrap_or(0);
                db_row.set_subtitle(&format!(
                    "{} MiB (+ {} KiB WAL)",
                    (db_bytes + db_wal) / (1024 * 1024),
                    db_wal / 1024
                ));
                cache_row.set_subtitle(&format!("{} MiB", cache.total_bytes() / (1024 * 1024)));
            }
        });
        page_store.add(&grp);
        win.add(&page_store);

        let page_acc = adw::PreferencesPage::builder()
            .title(crate::tr!("Konten", "Accounts"))
            .icon_name("system-users-symbolic")
            .build();
        let grp = adw::PreferencesGroup::builder()
            .title(crate::tr!("Konten", "Accounts"))
            .build();
        let local = adw::ActionRow::builder()
            .title(crate::tr!("Lokale Bibliothek", "Local library"))
            .subtitle(crate::tr!(
                "Aktiv — Feeds, OPML, Suche, Offline",
                "Active — feeds, OPML, search, offline access"
            ))
            .build();
        grp.add(&local);
        let feedly_state = {
            let st = self.state.borrow();
            let connected = st.accounts.iter().any(|(_, k, _)| k == "feedly");
            if !connected {
                crate::tr!(
                    "Nicht verbunden (Menü → Feedly verbinden …)",
                    "Not connected (Menu → Connect to Feedly…)"
                )
                .to_string()
            } else {
                let acc = st
                    .accounts
                    .iter()
                    .find(|(_, k, _)| k == "feedly")
                    .map(|(id, _, _)| id.clone())
                    .unwrap_or_default();
                match &st.feedly_status {
                    Some((status, detail)) => {
                        let base = status_label(status);
                        match detail {
                            Some(d) => format!("{base} — {d}"),
                            None => base.to_string(),
                        }
                    }
                    None => {
                        let _ = acc;
                        crate::tr_format!(
                            "Verbunden · Delta-Sync alle {} min",
                            "Connected · incremental sync every {} min",
                            self.prefs.borrow().refresh_min
                        )
                    }
                }
            }
        };
        let feedly = adw::ActionRow::builder()
            .title("Feedly")
            .subtitle(feedly_state)
            .build();
        grp.add(&feedly);
        page_acc.add(&grp);
        win.add(&page_acc);

        let w = self.weak();
        auto.connect_active_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().auto_read = row.is_active();
                let v = if row.is_active() { "1" } else { "0" };
                app.save_pref("auto_read", v);
            }
        });
        let w = self.weak();
        font.connect_value_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().reader_font = row.value();
                app.save_pref("reader_font", &row.value().to_string());
                app.reload_current(true);
            }
        });
        let w = self.weak();
        measure.connect_value_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().reader_measure = row.value() as u32;
                app.save_pref("reader_measure", &(row.value() as u32).to_string());
                app.reload_current(true);
            }
        });
        let w = self.weak();
        lh.connect_value_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().reader_line_height = row.value();
                app.save_pref("reader_line_height", &row.value().to_string());
                app.reload_current(true);
            }
        });
        let w = self.weak();
        theme.connect_selected_item_notify(move |row| {
            if let Some(app) = w.upgrade() {
                let mode = match row.selected() {
                    1 => "dark",
                    2 => "light",
                    3 => "omarchy",
                    _ => "system",
                };
                app.prefs.borrow_mut().theme = mode.to_string();
                app.save_pref("theme", mode);
                app.apply_theme();
            }
        });
        let w = self.weak();
        compact.connect_active_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().compact = row.is_active();
                app.save_pref("compact", if row.is_active() { "1" } else { "0" });
                app.apply_prefs_live();
            }
        });
        let w = self.weak();
        block_images.connect_active_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().block_images = row.is_active();
                app.save_pref("block_images", if row.is_active() { "1" } else { "0" });
                app.reload_current(false);
            }
        });
        let w = self.weak();
        thumbs.connect_active_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().thumbs = row.is_active();
                app.save_pref("thumbs", if row.is_active() { "1" } else { "0" });
                app.apply_prefs_live();
            }
        });
        let w = self.weak();
        order.connect_selected_item_notify(move |row| {
            let Some(app) = w.upgrade() else { return };
            let newest = row.selected() == 0;
            app.prefs.borrow_mut().newest_first = newest;
            app.save_pref("newest_first", if newest { "1" } else { "0" });
            app.load_page(false);
        });

        let w = self.weak();
        letters.connect_active_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().letter_shortcuts = row.is_active();
                app.save_pref("letter_shortcuts", if row.is_active() { "1" } else { "0" });
            }
        });
        let w = self.weak();
        refresh.connect_value_notify(move |row| {
            if let Some(app) = w.upgrade() {
                let minutes = row.value() as i64;
                app.prefs.borrow_mut().refresh_min = minutes;
                app.save_pref("refresh_min", &minutes.to_string());
                app.net.set_refresh_minutes(minutes);
                app.state.borrow_mut().next_feedly_sync = 0;
            }
        });
        let w = self.weak();
        retention.connect_value_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().retention_days = row.value() as i64;
                app.save_pref("retention_days", &(row.value() as i64).to_string());
                app.run_retention();
            }
        });
        let w = self.weak();
        media.connect_value_notify(move |row| {
            if let Some(app) = w.upgrade() {
                app.prefs.borrow_mut().media_mb = row.value() as i64;
                app.save_pref("media_mb", &(row.value() as i64).to_string());
                app.media.set_max_bytes(row.value() as u64 * 1024 * 1024);
            }
        });

        win.present();
    }
}
