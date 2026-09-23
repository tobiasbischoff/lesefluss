use crate::model::UiState;
use gtk::prelude::*;
use std::rc::Rc;
use storage::Source;

pub fn rebuild(
    list: &gtk::ListBox,
    state: &UiState,
    filters_out: &mut Vec<Option<Source>>,
    on_toggle_group: &Rc<dyn Fn(i64)>,
) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    filters_out.clear();

    add_section(list, filters_out, "Bibliothek");
    add_smart_row(list, filters_out, "mail-unread-symbolic", "Ungelesen", state.counts.unread, true, Source::Unread);
    add_smart_row(list, filters_out, "view-list-symbolic", "Alle Artikel", state.counts.total, false, Source::All);
    add_smart_row(list, filters_out, "user-bookmarks-symbolic", "Gespeichert", state.counts.saved, false, Source::Saved);

    add_section(list, filters_out, "Abonnements");
    let mut grouped: Vec<i64> = Vec::new();
    for g in &state.groups {
        let is_collapsed = state.collapsed.contains(&g.id);
        let count = state.group_unread(g.id);
        let arrow = if is_collapsed { "pan-end-symbolic" } else { "pan-down-symbolic" };
        let widget = icon_text_badge(Some(arrow), &g.name, count, false);
        let toggle = gtk::Button::builder()
            .icon_name(arrow)
            .css_classes(vec!["flat".to_string(), "lf-status-icon".to_string()])
            .tooltip_text(if is_collapsed { "Gruppe ausklappen" } else { "Gruppe einklappen" })
            .build();
        let cb = Rc::clone(on_toggle_group);
        let gid = g.id;
        toggle.connect_clicked(move |_| cb(gid));
        let box_row = widget.downcast::<gtk::Box>().expect("Gruppenzeile");
        box_row.append(&toggle);
        let row = gtk::ListBoxRow::builder()
            .child(&box_row)
            .css_classes(vec!["lf-sidebar-row".to_string()])
            .build();
        list.append(&row);
        filters_out.push(Some(Source::Group(g.id)));

        if !is_collapsed {
            for f in state.feeds.iter().filter(|f| f.groups.contains(&g.id)) {
                grouped.push(f.id);
                add_feed_row(list, filters_out, state, f.id, &f.title, &f.accent);
            }
        }
    }
    for f in state.feeds.iter().filter(|f| !grouped.contains(&f.id)) {
        add_feed_row(list, filters_out, state, f.id, &f.title, &f.accent);
    }

    if state.search.is_none() {
        for (i, f) in filters_out.iter().enumerate() {
            if f.as_ref() == Some(&state.source) {
                if let Some(row) = list.row_at_index(i as i32) {
                    list.select_row(Some(&row));
                }
                return;
            }
        }
    }
}

fn add_feed_row(
    list: &gtk::ListBox,
    filters: &mut Vec<Option<Source>>,
    state: &UiState,
    feed_id: i64,
    title: &str,
    accent: &str,
) {
    let count = state.feed_unread(feed_id);
    let dot = format!("<span foreground=\"{accent}\">●</span>");
    let w = markup_text_badge(&dot, title, count);
    let row = gtk::ListBoxRow::builder()
        .child(&w)
        .css_classes(vec!["lf-sidebar-row".to_string(), "lf-sidebar-feed".to_string()])
        .build();
    list.append(&row);
    filters.push(Some(Source::Feed(feed_id)));
}

fn add_section(list: &gtk::ListBox, filters: &mut Vec<Option<Source>>, label: &str) {
    let l = gtk::Label::builder()
        .label(label)
        .xalign(0.0)
        .css_classes(vec!["lf-sidebar-section".to_string()])
        .build();
    let row = gtk::ListBoxRow::builder()
        .child(&l)
        .selectable(false)
        .activatable(false)
        .focusable(false)
        .build();
    list.append(&row);
    filters.push(None);
}

fn add_smart_row(
    list: &gtk::ListBox,
    filters: &mut Vec<Option<Source>>,
    icon: &str,
    label: &str,
    count: i64,
    bold_count: bool,
    f: Source,
) {
    let w = icon_text_badge(Some(icon), label, count, bold_count);
    let row = gtk::ListBoxRow::builder().child(&w).css_classes(vec!["lf-sidebar-row".to_string()]).build();
    list.append(&row);
    filters.push(Some(f));
}

fn icon_text_badge(icon: Option<&str>, text: &str, count: i64, bold_count: bool) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if let Some(i) = icon {
        b.append(&gtk::Image::builder().icon_name(i).pixel_size(16).build());
    }
    b.append(&gtk::Label::builder()
        .label(text)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .hexpand(true)
        .build());
    b.append(&badge(count, bold_count));
    b.upcast()
}

fn markup_text_badge(markup_prefix: &str, text: &str, count: i64) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.append(&gtk::Label::builder().use_markup(true).label(markup_prefix).build());
    b.append(&gtk::Label::builder()
        .label(glib::markup_escape_text(text).to_string())
        .use_markup(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .hexpand(true)
        .build());
    b.append(&badge(count, false));
    b.upcast()
}

fn badge(count: i64, bold: bool) -> gtk::Widget {
    let cls = if bold && count > 0 { "lf-badge-unread" } else { "lf-badge" };
    let text = if count > 0 { count.to_string() } else { String::new() };
    gtk::Label::builder().label(&text).css_classes(vec![cls.to_string()]).build().upcast()
}
