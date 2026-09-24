//! Einstellungen als eigenes Modul: Seiten, Bedienelemente und Persistenz (§16).

use crate::window::{status_label, App};
use adw::prelude::*;
use gtk::prelude::*;
use std::rc::Rc;

impl App {
    pub fn settings_dialog(&self) {
        let win = adw::PreferencesWindow::builder().modal(true).transient_for(&self.window).build();
        let p = self.prefs.borrow().clone();

        let page_read = adw::PreferencesPage::builder().title("Lesen").icon_name("text-x-generic-symbolic").build();
        let grp = adw::PreferencesGroup::builder().title("Leseverhalten").build();
        let auto = adw::SwitchRow::builder().title("Automatisch als gelesen markieren").subtitle("Nach 0,8 s sichtbarem Artikel").active(p.auto_read).build();
        grp.add(&auto);
        let font = adw::SpinRow::builder().title("Schriftgröße Reader").adjustment(&gtk::Adjustment::new(p.reader_font, 14.0, 28.0, 1.0, 2.0, 0.0)).build();
        grp.add(&font);
        let measure = adw::SpinRow::builder().title("Zeilenbreite (Zeichen)").adjustment(&gtk::Adjustment::new(p.reader_measure as f64, 55.0, 85.0, 1.0, 5.0, 0.0)).build();
        grp.add(&measure);
        let lh = adw::SpinRow::builder().title("Zeilenhöhe").adjustment(&gtk::Adjustment::new(p.reader_line_height, 1.4, 2.0, 0.05, 0.1, 0.0)).build();
        grp.add(&lh);
        page_read.add(&grp);
        win.add(&page_read);

        let page_view = adw::PreferencesPage::builder().title("Darstellung").icon_name("preferences-desktop-appearance-symbolic").build();
        let grp = adw::PreferencesGroup::builder().title("Erscheinungsbild").build();
        let theme = adw::ComboRow::builder().title("Theme").model(&gtk::StringList::new(&["system", "dark", "light", "omarchy"])).build();
        let idx = match p.theme.as_str() {
            "dark" => 1,
            "light" => 2,
            "omarchy" => 3,
            _ => 0,
        };
        theme.set_selected(idx);
        grp.add(&theme);
        let compact = adw::SwitchRow::builder().title("Kompakte Liste").active(p.compact).build();
        grp.add(&compact);
        let thumbs = adw::SwitchRow::builder().title("Bildvorschauen in der Liste").active(p.thumbs).build();
        grp.add(&thumbs);
        let letters = adw::SwitchRow::builder().title("Buchstabenkürzel (j/k/n/p/m/s/o)").active(p.letter_shortcuts).build();
        grp.add(&letters);
        let order = adw::ComboRow::builder()
            .title("Reihenfolge")
            .subtitle("Gilt für alle Ansichten und Konten")
            .model(&gtk::StringList::new(&["Neueste zuerst", "Älteste zuerst"]))
            .selected(if p.newest_first { 0 } else { 1 })
            .build();
        grp.add(&order);
        page_view.add(&grp);
        win.add(&page_view);

        let page_sync = adw::PreferencesPage::builder().title("Aktualisierung").icon_name("view-refresh-symbolic").build();
        let grp = adw::PreferencesGroup::builder().title("Abruf").build();
        let refresh = adw::SpinRow::builder().title("Intervall (Minuten)").adjustment(&gtk::Adjustment::new(p.refresh_min as f64, 5.0, 1440.0, 5.0, 30.0, 0.0)).build();
        grp.add(&refresh);
        page_sync.add(&grp);
        win.add(&page_sync);

        let page_store = adw::PreferencesPage::builder().title("Speicher & Datenschutz").icon_name("drive-harddisk-symbolic").build();
        let grp = adw::PreferencesGroup::builder().title("Aufbewahrung").build();
        let retention = adw::SpinRow::builder().title("Gelesene Inhalte behalten (Tage)").subtitle("Danach Bereinigung; Gespeicherte bleiben").adjustment(&gtk::Adjustment::new(p.retention_days as f64, 7.0, 3650.0, 1.0, 30.0, 0.0)).build();
        grp.add(&retention);
        let media = adw::SpinRow::builder().title("Bildcache (MiB)").adjustment(&gtk::Adjustment::new(p.media_mb as f64, 64.0, 4096.0, 64.0, 256.0, 0.0)).build();
        grp.add(&media);
        page_store.add(&grp);
        win.add(&page_store);

        let page_acc = adw::PreferencesPage::builder().title("Konten").icon_name("system-users-symbolic").build();
        let grp = adw::PreferencesGroup::builder().title("Konten").build();
        let local = adw::ActionRow::builder().title("Lokale Bibliothek").subtitle("Aktiv — Feeds, OPML, Suche, Offline").build();
        grp.add(&local);
        let feedly_state = {
            let st = self.state.borrow();
            let connected = st.accounts.iter().any(|(_, k, _)| k == "feedly");
            if !connected {
                "Nicht verbunden (Menü → Feedly verbinden …)".to_string()
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
                        format!(
                            "Verbunden · Delta-Sync alle {} min",
                            self.prefs.borrow().refresh_min
                        )
                    }
                }
            }
        };
        let feedly = adw::ActionRow::builder().title("Feedly").subtitle(feedly_state).build();
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
