use crate::state::fmt_time;
use gtk::pango;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use storage::ArticleRow;

#[derive(Clone)]
pub struct RowHandles {
    pub root: gtk::Box,
    pub title: gtk::Label,
    pub meta: gtk::Label,
    pub excerpt: gtk::Label,
    pub saved_icon: gtk::Image,
}

fn meta_markup(a: &ArticleRow) -> String {
    format!(
        "<span weight=\"bold\" foreground=\"{}\">{}</span><span> · {}</span>",
        if a.unread {
            a.accent.clone()
        } else {
            "#8a8d96".into()
        },
        glib::markup_escape_text(&a.feed_title),
        fmt_time(a.published_ms)
    )
}

impl RowHandles {
    fn apply(&self, a: &ArticleRow) {
        self.title.set_css_classes(&[if a.unread {
            "lf-article-title-unread"
        } else {
            "lf-article-title-read"
        }]);
        self.meta.set_markup(&meta_markup(a));
        self.meta.set_css_classes(&[if a.unread {
            "lf-article-meta"
        } else {
            "lf-article-meta-read"
        }]);
        self.excerpt.set_css_classes(&[if a.unread {
            "lf-article-excerpt"
        } else {
            "lf-article-excerpt-read"
        }]);
        self.saved_icon.set_visible(a.saved);
    }
}

pub struct RowCell {
    pub data: RefCell<ArticleRow>,
    bound: RefCell<Vec<RowHandles>>,
}

impl RowCell {
    pub fn new(a: ArticleRow) -> Self {
        Self {
            data: RefCell::new(a),
            bound: RefCell::new(Vec::new()),
        }
    }

    pub fn article(&self) -> ArticleRow {
        self.data.borrow().clone()
    }

    pub fn update(&self, a: ArticleRow) {
        *self.data.borrow_mut() = a.clone();
        let handles: Vec<RowHandles> = self.bound.borrow().clone();
        for h in handles {
            h.apply(&a);
        }
    }

    fn register(&self, h: RowHandles) {
        self.bound.borrow_mut().push(h);
    }

    fn unregister(&self, root: &gtk::Box) {
        self.bound.borrow_mut().retain(|h| &h.root != root);
    }
}

#[derive(Clone)]
pub enum ListRow {
    Header { key: String, label: String },
    Item(Rc<RowCell>),
}

impl ListRow {
    pub fn article(&self) -> Option<ArticleRow> {
        match self {
            ListRow::Item(c) => Some(c.article()),
            _ => None,
        }
    }
}

pub fn row_widget(row: &ListRow, thumbs: bool) -> gtk::Widget {
    match row {
        ListRow::Header { label, .. } => gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .css_classes(vec!["lf-day-header".to_string()])
            .build()
            .upcast(),
        ListRow::Item(cell) => {
            let a = cell.article();
            let (root, handles) = article_row(&a, thumbs);
            cell.register(handles);
            root.upcast()
        }
    }
}

pub fn unregister(row: &ListRow, widget: &gtk::Widget) {
    if let ListRow::Item(cell) = row {
        if let Some(b) = widget.downcast_ref::<gtk::Box>() {
            cell.unregister(b);
        }
    }
}

/// Wandelt eine `data:image/png;base64,…`-URI in ein Paintable.
fn data_uri_texture(data: &str) -> Option<gtk::gdk::Texture> {
    use base64::Engine;
    let payload = data.strip_prefix("data:image/png;base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(bytes)).ok()
}

fn initial_thumb(initial: &str, accent: &str) -> gtk::Widget {
    let box_row = gtk::Box::builder()
        .width_request(64)
        .height_request(64)
        .valign(gtk::Align::Start)
        .halign(gtk::Align::End)
        .css_classes(vec!["lf-thumb".to_string()])
        .build();
    let label = gtk::Label::builder()
        .use_markup(true)
        .label(format!(
            "<span size=\"18000\" weight=\"bold\" foreground=\"{accent}\">{}</span>",
            glib::markup_escape_text(initial)
        ))
        .vexpand(true)
        .hexpand(true)
        .build();
    box_row.append(&label);
    box_row.upcast()
}

fn article_row(a: &ArticleRow, thumbs: bool) -> (gtk::Box, RowHandles) {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .css_classes(vec!["lf-article-row".to_string()])
        .build();

    let text_col = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text_col.set_hexpand(true);
    text_col.set_valign(gtk::Align::Start);

    let meta_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let meta = gtk::Label::builder()
        .use_markup(true)
        .label(meta_markup(a))
        .css_classes(vec![if a.unread {
            "lf-article-meta".to_string()
        } else {
            "lf-article-meta-read".to_string()
        }])
        .ellipsize(pango::EllipsizeMode::End)
        .xalign(0.0)
        .hexpand(true)
        .build();
    meta_box.append(&meta);
    let saved_icon = gtk::Image::builder()
        .icon_name("user-bookmarks-symbolic")
        .pixel_size(12)
        .tooltip_text(crate::tr!("Gespeichert", "Saved"))
        .css_classes(vec!["lf-status-icon".to_string()])
        .build();
    saved_icon.set_visible(a.saved);
    meta_box.append(&saved_icon);
    text_col.append(&meta_box);

    let title = gtk::Label::builder()
        .label(&a.title)
        .css_classes(vec![if a.unread {
            "lf-article-title-unread".to_string()
        } else {
            "lf-article-title-read".to_string()
        }])
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
        .css_classes(vec![if a.unread {
            "lf-article-excerpt".to_string()
        } else {
            "lf-article-excerpt-read".to_string()
        }])
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(pango::EllipsizeMode::End)
        .max_width_chars(38)
        .build();
    text_col.append(&excerpt);

    root.append(&text_col);

    if thumbs {
        let thumb = gtk::Picture::builder()
            .width_request(64)
            .height_request(64)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .content_fit(gtk::ContentFit::Cover)
            .css_classes(vec!["lf-thumb".to_string()])
            .build();
        match a.thumb.as_ref() {
            Some(data) => {
                // Vorschaubild liegt als kleines PNG (Daten-URI) vor.
                if let Some(paintable) = data_uri_texture(data) {
                    thumb.set_paintable(Some(&paintable));
                    root.append(&thumb);
                } else {
                    let initial = a.feed_title.chars().next().unwrap_or('?').to_string();
                    root.append(&initial_thumb(&initial, &a.accent));
                }
            }
            None => {
                let initial = a.feed_title.chars().next().unwrap_or('?').to_string();
                root.append(&initial_thumb(&initial, &a.accent));
            }
        }
    }

    let handles = RowHandles {
        root: root.clone(),
        title: title.clone(),
        meta: meta.clone(),
        excerpt: excerpt.clone(),
        saved_icon: saved_icon.clone(),
    };
    (root, handles)
}
