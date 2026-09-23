use crate::fixtures::Library;
use crate::state::{fmt_time, article_of, feed_of, ListRow};
use gtk::pango;
use gtk::prelude::*;

pub fn row_widget(row: &ListRow, lib: &Library) -> gtk::Widget {
    match row {
        ListRow::Header { label, .. } => {
            let l = gtk::Label::builder()
                .label(label)
                .xalign(0.0)
                .css_classes(vec!["lf-day-header".to_string()])
                .build();
            l.upcast()
        }
        ListRow::Item { id } => article_row(id, lib).upcast(),
    }
}

fn article_row(id: &str, lib: &Library) -> gtk::Box {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(vec!["lf-article-row".to_string()])
        .build();

    let text_col = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text_col.set_hexpand(true);
    text_col.set_valign(gtk::Align::Start);

    let Some(a) = article_of(lib, id) else {
        root.append(&text_col);
        return root;
    };
    let feed_title = feed_of(lib, &a.feed_id).map(|f| f.title.as_str()).unwrap_or("Unbekannt");
    let accent = feed_of(lib, &a.feed_id).map(|f| f.accent.as_str()).unwrap_or("#888888");

    let meta_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let meta = gtk::Label::builder()
        .use_markup(true)
        .label(&format!(
            "<span weight=\"bold\" foreground=\"{}\">{}</span><span> · {}</span>",
            accent,
            glib::markup_escape_text(feed_title),
            fmt_time(a.published_at)
        ))
        .css_classes(vec!["lf-article-meta".to_string()])
        .ellipsize(pango::EllipsizeMode::End)
        .xalign(0.0)
        .hexpand(true)
        .build();
    meta_box.append(&meta);
    if a.saved {
        let saved_icon = gtk::Image::builder()
            .icon_name("user-bookmarks-symbolic")
            .pixel_size(12)
            .tooltip_text("Gespeichert")
            .css_classes(vec!["lf-status-icon".to_string()])
            .build();
        meta_box.append(&saved_icon);
    }
    if !a.unread {
        let read_icon = gtk::Image::builder()
            .icon_name("object-select-symbolic")
            .pixel_size(12)
            .tooltip_text("Gelesen")
            .css_classes(vec!["lf-status-icon".to_string()])
            .build();
        meta_box.append(&read_icon);
    }
    text_col.append(&meta_box);

    let title_cls = if a.unread { "lf-article-title-unread" } else { "lf-article-title" };
    let title = gtk::Label::builder()
        .label(&a.title)
        .css_classes(vec![title_cls.to_string()])
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(pango::EllipsizeMode::End)
        .max_width_chars(38)
        .build();
    text_col.append(&title);

    let excerpt = gtk::Label::builder()
        .label(&a.excerpt)
        .css_classes(vec!["lf-article-excerpt".to_string()])
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(pango::EllipsizeMode::End)
        .max_width_chars(38)
        .build();
    text_col.append(&excerpt);

    root.append(&text_col);

    let thumb = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .width_request(64)
        .height_request(64)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .css_classes(vec!["lf-thumb".to_string()])
        .build();
    let initial = feed_title.chars().next().unwrap_or('?').to_string();
    let thumb_label = gtk::Label::builder()
        .use_markup(true)
        .label(&format!("<span size=\"18000\" weight=\"bold\" foreground=\"{accent}\">{initial}</span>"))
        .vexpand(true)
        .hexpand(true)
        .build();
    thumb.append(&thumb_label);
    root.append(&thumb);

    root
}
