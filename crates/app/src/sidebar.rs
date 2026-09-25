use crate::model::UiState;
use gtk::prelude::*;
use std::rc::Rc;
use storage::{FeedRow, GroupRow, Scope as Source};

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

    add_section(list, filters_out, crate::tr!("Bibliothek", "Library"));
    add_smart_row(
        list,
        filters_out,
        "mail-unread-symbolic",
        crate::tr!("Ungelesen", "Unread"),
        state.counts.unread,
        true,
        Source::Global,
    );

    add_section(
        list,
        filters_out,
        crate::tr!("Lokale Bibliothek", "Local library"),
    );
    add_account_block(list, state, filters_out, "local", on_toggle_group);

    for (id, kind, name) in &state.accounts {
        if kind == "local" {
            continue;
        }
        let label = format!("{} · {}", kind_label(kind), name);
        add_section(list, filters_out, &label);
        let unread = state
            .counts
            .per_account
            .iter()
            .find(|(a, _)| a == id)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        let w = icon_text_badge(Some("cloud-fill-symbolic"), name, unread, true);
        let row = gtk::ListBoxRow::builder()
            .child(&w)
            .css_classes(vec!["lf-sidebar-row".to_string()])
            .build();
        list.append(&row);
        filters_out.push(Some(Source::Account(id.clone())));
        add_account_block(list, state, filters_out, id, on_toggle_group);
    }

    if state.search.is_none() {
        for (i, f) in filters_out.iter().enumerate() {
            if f.as_ref() == Some(&state.scope) {
                if let Some(row) = list.row_at_index(i as i32) {
                    list.select_row(Some(&row));
                }
                return;
            }
        }
    }
}

fn kind_label(kind: &str) -> String {
    match kind {
        "feedly" => "Feedly".to_string(),
        other => other.to_string(),
    }
}

/// Ordnet Feeds den sichtbaren Gruppen zu. Eingeklappte Gruppen behalten ihre
/// Feeds; nur wirklich ungruprierte Feeds landen unten.
pub fn partition_feeds(
    feeds: &[FeedRow],
    groups: &[GroupRow],
    account_id: &str,
) -> (Vec<i64>, Vec<i64>) {
    let mut grouped: Vec<i64> = Vec::new();
    for g in groups.iter().filter(|g| g.account_id == account_id) {
        for f in feeds
            .iter()
            .filter(|f| f.groups.contains(&g.id) && f.account_id == account_id)
        {
            if !grouped.contains(&f.id) {
                grouped.push(f.id);
            }
        }
    }
    let rest = feeds
        .iter()
        .filter(|f| f.account_id == account_id && !grouped.contains(&f.id))
        .map(|f| f.id)
        .collect();
    (grouped, rest)
}

fn add_account_block(
    list: &gtk::ListBox,
    state: &UiState,
    filters: &mut Vec<Option<Source>>,
    account_id: &str,
    on_toggle_group: &Rc<dyn Fn(i64)>,
) {
    let (grouped, rest) = partition_feeds(&state.feeds, &state.groups, account_id);
    let mut grouped: Vec<i64> = grouped;
    for g in state.groups.iter().filter(|g| g.account_id == account_id) {
        let is_collapsed = state.collapsed.contains(&g.id);
        let count = state.group_unread(g.id);
        let arrow = if is_collapsed {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        };
        let widget = icon_text_badge(Some(arrow), &g.name, count, false);
        let toggle = gtk::Button::builder()
            .icon_name(arrow)
            .css_classes(vec!["flat".to_string(), "lf-status-icon".to_string()])
            .tooltip_text(if is_collapsed {
                crate::tr!("Gruppe ausklappen", "Expand group")
            } else {
                crate::tr!("Gruppe einklappen", "Collapse group")
            })
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
        filters.push(Some(Source::Group(g.id)));

        for f in state
            .feeds
            .iter()
            .filter(|f| f.groups.contains(&g.id) && f.account_id == account_id)
        {
            grouped.push(f.id);
            if !is_collapsed {
                add_feed_row(list, filters, state, f.id, &f.title, &f.accent);
            }
        }
    }
    for feed_id in rest {
        if let Some(f) = state.feeds.iter().find(|f| f.id == feed_id) {
            add_feed_row(list, filters, state, f.id, &f.title, &f.accent);
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
        .css_classes(vec![
            "lf-sidebar-row".to_string(),
            "lf-sidebar-feed".to_string(),
        ])
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
    let row = gtk::ListBoxRow::builder()
        .child(&w)
        .css_classes(vec!["lf-sidebar-row".to_string()])
        .build();
    list.append(&row);
    filters.push(Some(f));
}

fn icon_text_badge(icon: Option<&str>, text: &str, count: i64, bold_count: bool) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if let Some(i) = icon {
        b.append(&gtk::Image::builder().icon_name(i).pixel_size(16).build());
    }
    b.append(
        &gtk::Label::builder()
            .label(text)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .hexpand(true)
            .build(),
    );
    b.append(&badge(count, bold_count));
    b.upcast()
}

fn markup_text_badge(markup_prefix: &str, text: &str, count: i64) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.append(
        &gtk::Label::builder()
            .use_markup(true)
            .label(markup_prefix)
            .build(),
    );
    b.append(
        &gtk::Label::builder()
            .label(glib::markup_escape_text(text).to_string())
            .use_markup(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .hexpand(true)
            .build(),
    );
    b.append(&badge(count, false));
    b.upcast()
}

fn badge(count: i64, bold: bool) -> gtk::Widget {
    let cls = if bold && count > 0 {
        "lf-badge-unread"
    } else {
        "lf-badge"
    };
    let text = if count > 0 {
        count.to_string()
    } else {
        String::new()
    };
    gtk::Label::builder()
        .label(&text)
        .css_classes(vec![cls.to_string()])
        .build()
        .upcast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::{FeedRow, GroupRow};

    fn feed(id: i64, account: &str, groups: Vec<i64>) -> FeedRow {
        FeedRow {
            id,
            account_id: account.to_string(),
            remote_id: None,
            feed_url: format!("https://example.com/{id}.xml"),
            title: format!("Feed {id}"),
            website: None,
            accent: "#111111".to_string(),
            groups,
        }
    }

    fn group(id: i64, account: &str) -> GroupRow {
        GroupRow {
            id,
            name: format!("Gruppe {id}"),
            parent_id: None,
            remote_id: None,
            account_id: account.to_string(),
        }
    }

    #[test]
    fn eingeklappte_gruppen_behalten_ihre_feeds() {
        let feeds = vec![
            feed(1, "feedly-1", vec![10]),
            feed(2, "feedly-1", vec![10]),
            feed(3, "feedly-1", vec![]),
            feed(4, "feedly-2", vec![10]),
        ];
        let groups = vec![group(10, "feedly-1"), group(10, "feedly-2")];
        let (grouped, rest) = partition_feeds(&feeds, &groups, "feedly-1");
        assert_eq!(
            grouped,
            vec![1, 2],
            "Feeds eingeklappter Gruppen bleiben dort"
        );
        assert_eq!(
            rest,
            vec![3],
            "nur wirklich ungruprierte Feeds kommen nach unten"
        );
    }
}
