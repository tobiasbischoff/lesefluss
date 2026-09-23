use crate::fixtures::Library;
use crate::state::{
    feeds_in_group, saved_total, unread_in_feed, unread_in_group, unread_total, SourceFilter,
};
use gtk::prelude::*;
use std::collections::HashSet;
use std::rc::Rc;

pub fn rebuild(
    list: &gtk::ListBox,
    lib: &Library,
    source: &SourceFilter,
    collapsed: &mut HashSet<String>,
    filters_out: &mut Vec<Option<SourceFilter>>,
    on_toggle_group: &Rc<dyn Fn(String)>,
) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    filters_out.clear();

    add_section(list, filters_out, "Bibliothek");
    add_smart_row(list, filters_out, "mail-unread-symbolic", "Ungelesen", unread_total(lib), true, SourceFilter::Unread);
    add_smart_row(list, filters_out, "view-list-symbolic", "Alle Artikel", lib.articles.len(), false, SourceFilter::All);
    add_smart_row(list, filters_out, "user-bookmarks-symbolic", "Gespeichert", saved_total(lib), false, SourceFilter::Saved);

    add_section(list, filters_out, "Abonnements");
    let mut grouped_feeds: HashSet<&str> = HashSet::new();
    for g in &lib.groups {
        let feeds = feeds_in_group(lib, &g.id);
        let is_collapsed = collapsed.contains(&g.id);
        let count = unread_in_group(lib, &g.id);
        let arrow = if is_collapsed { "pan-end-symbolic" } else { "pan-down-symbolic" };

        let widget = icon_text_badge(Some(arrow), &g.name, None, count, false);
        let toggle = gtk::Button::builder()
            .icon_name(if is_collapsed { "pan-end-symbolic" } else { "pan-down-symbolic" })
            .css_classes(vec!["flat".to_string()])
            .tooltip_text(if is_collapsed { "Gruppe ausklappen" } else { "Gruppe einklappen" })
            .css_classes(vec!["flat".into(), "lf-status-icon".to_string()])
            .build();
        let cb = Rc::clone(on_toggle_group);
        let cb_id = g.id.clone();
        toggle.connect_clicked(move |_| cb(cb_id.clone()));

        let box_row = widget.downcast::<gtk::Box>().expect("Gruppenzeile");
        if let Some(last) = box_row.last_child() {
            box_row.insert_child_after(&toggle, Some(&last));
        } else {
            box_row.append(&toggle);
        }

        let row = gtk::ListBoxRow::builder()
            .child(&box_row)
            .css_classes(vec!["lf-sidebar-row".to_string()])
            .build();
        list.append(&row);
        filters_out.push(Some(SourceFilter::Group(g.id.clone())));

        if !is_collapsed {
            for f in feeds {
                grouped_feeds.insert(f.id.as_str());
                let count = unread_in_feed(lib, &f.id);
                let dot = format!("<span foreground=\"{}\">●</span>", f.accent);
                let w = markup_text_badge(&dot, &f.title, count);
                let frow = gtk::ListBoxRow::builder()
                    .child(&w)
                    .css_classes(vec!["lf-sidebar-row".into(), "lf-sidebar-feed".to_string()])
                    .build();
                list.append(&frow);
                filters_out.push(Some(SourceFilter::Feed(f.id.clone())));
            }
        }
    }

    for f in lib.feeds.iter().filter(|f| !grouped_feeds.contains(f.id.as_str())) {
        let count = unread_in_feed(lib, &f.id);
        let dot = format!("<span foreground=\"{}\">●</span>", f.accent);
        let w = markup_text_badge(&dot, &f.title, count);
        let frow = gtk::ListBoxRow::builder()
            .child(&w)
            .css_classes(vec!["lf-sidebar-row".into(), "lf-sidebar-feed".to_string()])
            .build();
        list.append(&frow);
        filters_out.push(Some(SourceFilter::Feed(f.id.clone())));
    }

    select_current(list, filters_out, source);
}

fn select_current(list: &gtk::ListBox, filters: &[Option<SourceFilter>], source: &SourceFilter) {
    for (i, f) in filters.iter().enumerate() {
        if f.as_ref() == Some(source) {
            if let Some(row) = list.row_at_index(i as i32) {
                list.select_row(Some(&row));
            }
            return;
        }
    }
}

fn add_section(list: &gtk::ListBox, filters: &mut Vec<Option<SourceFilter>>, label: &str) {
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

#[allow(clippy::too_many_arguments)]
fn add_smart_row(
    list: &gtk::ListBox,
    filters: &mut Vec<Option<SourceFilter>>,
    icon: &str,
    label: &str,
    count: usize,
    bold_count: bool,
    f: SourceFilter,
) {
    let w = icon_text_badge(Some(icon), label, None, count, bold_count);
    let row = gtk::ListBoxRow::builder().child(&w).css_classes(vec!["lf-sidebar-row".to_string()]).build();
    list.append(&row);
    filters.push(Some(f));
}

fn icon_text_badge(icon: Option<&str>, text: &str, markup_prefix: Option<&str>, count: usize, bold_count: bool) -> gtk::Widget {
    let _ = markup_prefix;
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

fn markup_text_badge(markup_prefix: &str, text: &str, count: usize) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.append(&gtk::Label::builder().use_markup(true).label(markup_prefix).build());
    b.append(&gtk::Label::builder()
        .label(glib::markup_escape_text(text).as_str())
        .use_markup(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .xalign(0.0)
        .hexpand(true)
        .build());
    b.append(&badge(count, false));
    b.upcast()
}

fn badge(count: usize, bold: bool) -> gtk::Widget {
    let cls = if bold && count > 0 { "lf-badge-unread" } else { "lf-badge" };
    let text = if count > 0 { count.to_string() } else { String::new() };
    gtk::Label::builder().label(&text).css_classes(vec![cls.to_string()]).build().upcast()
}
