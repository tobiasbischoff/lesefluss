use crate::fixtures::Library;
use crate::list;
use crate::reader::{find_in_view, find_next, ReaderPane};
use crate::sidebar;
use crate::state::*;
use crate::style::{gtk_css_for, tokens_for, ReaderStyleState};
use domain::ArticleId;
use adw::prelude::*;
use webkit6::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct App {
    pub window: adw::ApplicationWindow,
    pub toast: adw::ToastOverlay,
    pub outer: adw::NavigationSplitView,
    pub inner: adw::NavigationSplitView,
    pub sidebar_list: gtk::ListBox,
    pub sidebar_filters: RefCell<Vec<Option<SourceFilter>>>,
    pub collapsed_groups: RefCell<HashSet<String>>,
    pub last_sync_label: gtk::Label,
    pub list_title: adw::WindowTitle,
    pub list_stack: gtk::Stack,
    pub list_store: gio::ListStore,
    pub list_selection: gtk::SingleSelection,
    pub list_view: gtk::ListView,
    pub list_empty: adw::StatusPage,
    pub reader: Rc<ReaderPane>,
    pub lib: Rc<RefCell<Library>>,
    pub source: RefCell<SourceFilter>,
    pub selected: RefCell<Option<ArticleId>>,
    pub keep_visible: RefCell<Option<ArticleId>>,
    pub unread_guard: RefCell<HashSet<ArticleId>>,
    pub last_opened: RefCell<HashMap<SourceFilter, ArticleId>>,
    pub undo_stack: RefCell<Vec<UndoBatch>>,
    pub preview_timer: RefCell<Option<glib::SourceId>>,
    pub read_gen: Cell<u64>,
    pub suppress: Cell<bool>,
    pub tokens: RefCell<reader::tokens::Tokens>,
    pub css: gtk::CssProvider,
    pub panes: RefCell<Vec<gtk::Widget>>,
    pub custom_feed_counter: Cell<u32>,
    me: RefCell<Option<Weak<App>>>,
}

type UndoBatch = Vec<(ArticleId, bool, bool)>;

impl App {
    pub fn new(application: &adw::Application) -> Rc<Self> {
        let lib = Rc::new(RefCell::new(crate::fixtures::build()));
        let session = webkit6::NetworkSession::new_ephemeral();
        let reader = Rc::new(ReaderPane::new(&session));

        let sidebar_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(vec!["lf-sidebar".to_string(), "navigation-sidebar".to_string()])
            .build();
        let sidebar_scroll = gtk::ScrolledWindow::builder()
            .child(&sidebar_list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let last_sync_label = gtk::Label::builder()
            .label("Noch nicht aktualisiert")
            .xalign(0.0)
            .margin_start(16)
            .margin_end(16)
            .margin_top(6)
            .margin_bottom(10)
            .css_classes(vec!["lf-article-meta".to_string()])
            .build();
        let sidebar_footer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_footer.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        sidebar_footer.append(&last_sync_label);

        let btn_hamburger = gtk::Button::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Quellen einblenden")
            .action_name("win.toggle-sources")
            .build();
        let btn_refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Aktualisieren (Strg+R)")
            .action_name("win.refresh")
            .build();
        let btn_add = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Feed hinzufügen (Strg+N)")
            .action_name("win.add-feed")
            .build();
        let sidebar_header = adw::HeaderBar::builder()
            .title_widget(&adw::WindowTitle::new("Lesefluss", "Lokale Bibliothek"))
            .build();
        sidebar_header.pack_start(&btn_hamburger);
        sidebar_header.pack_end(&btn_refresh);
        sidebar_header.pack_end(&btn_add);

        let sidebar_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_body.append(&sidebar_scroll);
        sidebar_body.append(&sidebar_footer);
        sidebar_body.add_css_class("lf-sidebar");
        let sidebar_toolbar = adw::ToolbarView::builder().content(&sidebar_body).build();
        sidebar_toolbar.add_top_bar(&sidebar_header);
        let sources_page = adw::NavigationPage::new(&sidebar_toolbar, "Quellen");

        let list_store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let list_selection = gtk::SingleSelection::builder()
            .model(&list_store)
            .autoselect(false)
            .build();
        let factory = gtk::SignalListItemFactory::new();
        let list_view = gtk::ListView::builder()
            .model(&list_selection)
            .factory(&factory)
            .single_click_activate(true)
            .css_classes(vec!["lf-articles".to_string(), "lf-list".to_string()])
            .build();
        let list_scroll = gtk::ScrolledWindow::builder()
            .css_classes(vec!["lf-list-bg".to_string()])
            .child(&list_view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let list_empty = adw::StatusPage::builder()
            .icon_name("mailbox-symbolic")
            .title("Keine Artikel")
            .description("In dieser Ansicht ist gerade nichts los.")
            .vexpand(true)
            .build();
        let list_stack = gtk::Stack::builder().css_classes(vec!["lf-list-bg".to_string()]).build();
        list_stack.add_named(&list_scroll, Some("list"));
        list_stack.add_named(&list_empty, Some("empty"));
        list_stack.set_visible_child_name("list");

        let list_title = adw::WindowTitle::new("Ungelesen", "");
        let list_header = adw::HeaderBar::builder().title_widget(&list_title).build();
        let list_menu = gio::Menu::new();
        list_menu.append(Some("Bereich als gelesen markieren…"), Some("win.mark-scope-read"));
        let list_more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&list_menu)
            .tooltip_text("Listenaktionen")
            .build();
        list_header.pack_end(&list_more);
        let list_toolbar = adw::ToolbarView::builder().content(&list_stack).build();
        list_toolbar.add_top_bar(&list_header);
        let list_page = adw::NavigationPage::new(&list_toolbar, "Artikel");

        let btn_back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Zurück zur Artikelliste")
            .action_name("win.reader-back")
            .build();
        reader.header.pack_start(&btn_back);

        let inner = adw::NavigationSplitView::builder()
            .min_sidebar_width(280.0)
            .max_sidebar_width(460.0)
            .sidebar(&list_page)
            .content(&adw::NavigationPage::new(&reader.toolbar, "Lesen"))
            .build();

        let outer = adw::NavigationSplitView::builder()
            .min_sidebar_width(208.0)
            .max_sidebar_width(320.0)
            .sidebar(&sources_page)
            .content(&adw::NavigationPage::new(&inner, "Artikel"))
            .build();

        btn_back.bind_property("visible", &inner, "collapsed").sync_create().build();
        btn_hamburger.bind_property("visible", &outer, "collapsed").sync_create().build();

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&outer));
        let window = adw::ApplicationWindow::builder()
            .application(application)
            .default_width(1440)
            .default_height(900)
            .title("Lesefluss")
            .content(&toast)
            .build();
        window.add_css_class("lf-window");

        let css = gtk::CssProvider::new();
        let display = gtk::prelude::WidgetExt::display(&window);
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let app = Rc::new(Self {
            window,
            toast,
            outer,
            inner,
            sidebar_list,
            sidebar_filters: RefCell::new(Vec::new()),
            collapsed_groups: RefCell::new(HashSet::new()),
            last_sync_label,
            list_title,
            list_stack,
            list_store,
            list_selection,
            list_view,
            list_empty,
            reader,
            lib,
            source: RefCell::new(SourceFilter::Unread),
            selected: RefCell::new(None),
            keep_visible: RefCell::new(None),
            unread_guard: RefCell::new(HashSet::new()),
            last_opened: RefCell::new(HashMap::new()),
            undo_stack: RefCell::new(Vec::new()),
            preview_timer: RefCell::new(None),
            read_gen: Cell::new(0),
            suppress: Cell::new(false),
            tokens: RefCell::new(tokens_for(true)),
            css,
            panes: RefCell::new(Vec::new()),
            custom_feed_counter: Cell::new(0),
            me: RefCell::new(None),
        });

        *app.me.borrow_mut() = Some(Rc::downgrade(&app));
        *app.panes.borrow_mut() = vec![
            app.sidebar_list.clone().upcast::<gtk::Widget>(),
            app.list_view.clone().upcast::<gtk::Widget>(),
            app.reader.webview.clone().upcast::<gtk::Widget>(),
        ];

        app.apply_theme_now();
        app.wire(factory);
        app.register_actions(application);
        app.install_breakpoints();
        app.refresh_sidebar();
        app.refresh_list();
        app.update_reader_empty();
        app.window.present();

        app
    }

    fn weak(&self) -> Weak<App> {
        self.me.borrow().clone().expect("App-Selbstreferenz gesetzt")
    }

    // ── Verdrahtung ──

    fn wire(&self, factory: gtk::SignalListItemFactory) {
        let lib = Rc::clone(&self.lib);
        factory.connect_setup(|_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            li.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
        });
        factory.connect_bind(move |_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(obj) = li.item() else { return };
            let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else { return };
            let row = boxed.borrow::<ListRow>();
            let widget = list::row_widget(&row, &lib.borrow());
            li.set_child(Some(&widget));
        });
        factory.connect_unbind(|_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            li.set_child(None::<&gtk::Widget>);
        });

        let w = self.weak();
        self.sidebar_list.connect_row_selected(move |_, row| {
            let Some(app) = w.upgrade() else { return };
            if app.suppress.get() {
                return;
            }
            let Some(row) = row else { return };
            let idx = row.index() as usize;
            let f = {
                let filters = app.sidebar_filters.borrow();
                match filters.get(idx) {
                    Some(Some(f)) => f.clone(),
                    _ => return,
                }
            };
            app.set_source(f);
        });

        let w = self.weak();
        self.list_selection.connect_selected_item_notify(move |sel| {
            let Some(app) = w.upgrade() else { return };
            if app.suppress.get() {
                return;
            }
            let Some(obj) = sel.selected_item() else { return };
            let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else { return };
            let row = boxed.borrow::<ListRow>();
            if let ListRow::Item { id } = row.clone() {
                app.schedule_preview(id);
            }
        });

        let w = self.weak();
        self.list_view.connect_activate(move |_, pos| {
            let Some(app) = w.upgrade() else { return };
            if let Some(id) = app.item_id_at(pos) {
                app.open_article(id, true, true);
            }
        });

        let webview = self.reader.webview.clone();
        self.reader.search_entry.connect_search_changed(move |entry| {
            find_in_view(&webview, &entry.text());
        });
        let webview2 = self.reader.webview.clone();
        self.reader.search_entry.connect_activate(move |_| {
            find_next(&webview2);
        });
        self.reader.search_bar.set_key_capture_widget(Some(&self.window));

        let w = self.weak();
        self.reader.webview.connect_load_changed(move |_, event| {
            if event == webkit6::LoadEvent::Finished {
                if let Some(app) = w.upgrade() {
                    app.reader.restore_scroll();
                }
            }
        });

        let w = self.weak();
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(app) = w.upgrade() {
                app.apply_theme();
            }
        });
    }

    fn install_breakpoints(&self) {
        let outer = self.outer.clone();
        let inner = self.inner.clone();
        let win = self.window.clone();
        let apply = move || {
            let w = gtk::prelude::NativeExt::surface(&win)
                .map(|s| s.width())
                .unwrap_or_else(|| win.width());
            outer.set_collapsed(w <= 1119);
            inner.set_collapsed(w <= 779);
        };
        let apply_win = apply.clone();
        self.window.connect_notify_local(None, move |_, pspec| {
            let n = pspec.name();
            if n == "fullscreened" || n == "maximized" {
                apply_win();
            }
        });
        let apply_realize = apply.clone();
        self.window.connect_realize(move |win| {
            if let Some(surface) = gtk::prelude::NativeExt::surface(win) {
                let apply2 = apply_realize.clone();
                surface.connect_notify_local(Some("width"), move |_, _| apply2());
            }
            apply_realize();
        });
        apply();
    }

    fn register_actions(&self, application: &adw::Application) {
        macro_rules! win_action {
            ($name:expr, |$a:ident| $body:expr) => {{
                let w = self.weak();
                let act = gio::SimpleAction::new($name, None);
                act.connect_activate(move |_, _| {
                    if let Some($a) = w.upgrade() {
                        $body;
                    }
                });
                self.window.add_action(&act);
            }};
        }

        win_action!("toggle-sources", |a| a.outer.set_show_content(false));
        win_action!("reader-back", |a| a.back());
        win_action!("back", |a| a.back());
        win_action!("refresh", |a| a.do_refresh());
        win_action!("add-feed", |a| a.add_feed_dialog());
        win_action!("mark-scope-read", |a| a.mark_scope_dialog());
        win_action!("toggle-read", |a| a.toggle_read());
        win_action!("toggle-saved", |a| a.toggle_saved());
        win_action!("open-external", |a| a.open_external());
        win_action!("copy-link", |a| a.copy_link());
        win_action!("zoom-in", |a| a.zoom(2.0));
        win_action!("zoom-out", |a| a.zoom(-2.0));
        win_action!("zoom-reset", |a| a.zoom_reset());
        win_action!("find", |a| a.open_find());
        win_action!("reader-retry", |a| a.reload_current(false));
        win_action!("undo", |a| a.undo());
        win_action!("close", |a| a.window.close());
        win_action!("next-article", |a| a.move_selection(1));
        win_action!("prev-article", |a| a.move_selection(-1));
        win_action!("next-unread", |a| a.move_unread(1));
        win_action!("prev-unread", |a| a.move_unread(-1));
        win_action!("focus-next-pane", |a| a.focus_pane(1));
        win_action!("focus-prev-pane", |a| a.focus_pane(-1));

        let theme_action = gio::SimpleAction::new_stateful(
            "theme",
            Some(&String::static_variant_type()),
            &"system".to_variant(),
        );
        theme_action.connect_activate(|action, param| {
            let mode = param.and_then(|p| p.get::<String>()).unwrap_or_default();
            let scheme = match mode.as_str() {
                "dark" => adw::ColorScheme::ForceDark,
                "light" => adw::ColorScheme::ForceLight,
                _ => adw::ColorScheme::PreferLight,
            };
            adw::StyleManager::default().set_color_scheme(scheme);
            if let Some(p) = param {
                action.set_state(p);
            }
        });
        application.add_action(&theme_action);

        let quit = gio::SimpleAction::new("quit", None);
        let app_for_quit = application.clone();
        quit.connect_activate(move |_, _| app_for_quit.quit());
        application.add_action(&quit);

        application.set_accels_for_action("win.next-article", &["j"]);
        application.set_accels_for_action("win.prev-article", &["k"]);
        application.set_accels_for_action("win.next-unread", &["n"]);
        application.set_accels_for_action("win.prev-unread", &["p"]);
        application.set_accels_for_action("win.toggle-read", &["m"]);
        application.set_accels_for_action("win.toggle-saved", &["s"]);
        application.set_accels_for_action("win.open-external", &["o"]);
        application.set_accels_for_action("win.find", &["<Control>f"]);
        application.set_accels_for_action("win.refresh", &["<Control>r"]);
        application.set_accels_for_action("win.add-feed", &["<Control>n"]);
        application.set_accels_for_action("win.mark-scope-read", &["<Control><Shift>m"]);
        application.set_accels_for_action("win.undo", &["<Control>z"]);
        application.set_accels_for_action("win.zoom-in", &["<Control>plus", "<Control>equal", "<Control>KP_Add"]);
        application.set_accels_for_action("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]);
        application.set_accels_for_action("win.zoom-reset", &["<Control>0"]);
        application.set_accels_for_action("win.focus-next-pane", &["F6"]);
        application.set_accels_for_action("win.focus-prev-pane", &["<Shift>F6"]);
        application.set_accels_for_action("win.back", &["<Alt>Left"]);
        application.set_accels_for_action("win.close", &["<Control>w"]);
        application.set_accels_for_action("app.quit", &["<Control>q"]);
    }

    // ── Thema ──

    fn apply_theme_now(&self) {
        let dark = adw::StyleManager::default().is_dark();
        let tokens = tokens_for(dark);
        *self.tokens.borrow_mut() = tokens;
        self.css.load_from_string(&gtk_css_for(&tokens));
        let rgba = gtk::gdk::RGBA::new(
            tokens.surface_reader.r as f32 / 255.0,
            tokens.surface_reader.g as f32 / 255.0,
            tokens.surface_reader.b as f32 / 255.0,
            1.0,
        );
        self.reader.webview.set_background_color(&rgba);
    }

    fn apply_theme(&self) {
        self.apply_theme_now();
        self.reload_current(true);
    }

    // ── Sidebar ──

    fn refresh_sidebar(&self) {
        let lib = self.lib.borrow();
        let source = self.source.borrow().clone();
        let mut collapsed = self.collapsed_groups.borrow_mut();
        let mut filters = self.sidebar_filters.borrow_mut();
        self.suppress.set(true);
        let w = self.weak();
        let cb: Rc<dyn Fn(String)> = Rc::new(move |id: String| {
            if let Some(app) = w.upgrade() {
                app.toggle_group(&id);
            }
        });
        sidebar::rebuild(&self.sidebar_list, &lib, &source, &mut collapsed, &mut filters, &cb);
        self.suppress.set(false);
    }

    fn toggle_group(&self, id: &str) {
        {
            let mut c = self.collapsed_groups.borrow_mut();
            if !c.remove(id) {
                c.insert(id.to_string());
            }
        }
        self.refresh_sidebar();
    }

    // ── Liste ──

    fn refresh_list(&self) {
        let rows = {
            let lib = self.lib.borrow();
            let source = self.source.borrow().clone();
            rows_for(&lib, &source, now_ms())
        };
        self.suppress.set(true);
        self.list_store.remove_all();
        let mut has_items = false;
        for r in rows {
            if matches!(r, ListRow::Item { .. }) {
                has_items = true;
            }
            self.list_store.append(&glib::BoxedAnyObject::new(r));
        }
        self.list_stack.set_visible_child_name(if has_items { "list" } else { "empty" });
        if let Some(sel) = self.selected.borrow().clone() {
            if let Some(pos) = self.row_pos(&sel) {
                self.list_selection.set_selected(pos);
            }
        }
        self.suppress.set(false);
    }

    fn row_pos(&self, id: &str) -> Option<u32> {
        for pos in 0..self.list_store.n_items() {
            if self.item_id_at(pos).as_deref() == Some(id) {
                return Some(pos);
            }
        }
        None
    }

    fn item_id_at(&self, pos: u32) -> Option<ArticleId> {
        let obj = self.list_store.item(pos)?;
        let boxed = obj.downcast_ref::<glib::BoxedAnyObject>()?;
        let row = boxed.borrow::<ListRow>();
        match &*row {
            ListRow::Item { id } => Some(id.clone()),
            ListRow::Header { .. } => None,
        }
    }

    fn item_positions(&self) -> Vec<u32> {
        (0..self.list_store.n_items()).filter(|p| self.item_id_at(*p).is_some()).collect()
    }

    // ── Auswahl und Öffnen ──

    fn set_source(&self, f: SourceFilter) {
        self.flush_keep_visible(None);
        *self.source.borrow_mut() = f.clone();
        self.list_title.set_title(&source_label(&self.lib.borrow(), &f));
        let restore = self
            .last_opened
            .borrow()
            .get(&f)
            .cloned()
            .filter(|id| {
                let lib = self.lib.borrow();
                article_of(&lib, id).map(|a| crate::state::matches(&lib, a, &f)).unwrap_or(false)
            });
        *self.selected.borrow_mut() = restore.clone();
        self.refresh_sidebar();
        self.refresh_list();
        if let Some(id) = restore {
            self.open_article(id, false, false);
        } else {
            self.update_reader_empty();
        }
        if self.outer.is_collapsed() {
            self.outer.set_show_content(true);
        }
    }

    fn schedule_preview(&self, id: ArticleId) {
        if let Some(old) = self.preview_timer.borrow_mut().take() {
            old.remove();
        }
        let w = self.weak();
        let timer = glib::timeout_add_local(Duration::from_millis(220), move || {
            if let Some(app) = w.upgrade() {
                *app.preview_timer.borrow_mut() = None;
                let is_current = app.reader.current.borrow().as_deref() == Some(id.as_str());
                if !is_current {
                    app.open_article(id.clone(), false, true);
                }
                *app.selected.borrow_mut() = Some(id.clone());
            }
            glib::ControlFlow::Break
        });
        *self.preview_timer.borrow_mut() = Some(timer);
    }

    fn open_article(&self, id: ArticleId, focus: bool, flush: bool) {
        if flush {
            self.flush_keep_visible(Some(&id));
        }
        let Some(html) = self.render_article(&id) else { return };
        *self.selected.borrow_mut() = Some(id.clone());
        self.last_opened
            .borrow_mut()
            .insert(self.source.borrow().clone(), id.clone());
        self.unread_guard.borrow_mut().remove(&id);

        if let Some(pos) = self.row_pos(&id) {
            self.suppress.set(true);
            self.list_selection.set_selected(pos);
            self.suppress.set(false);
        }

        self.reader.load_html_doc(&html);
        *self.reader.current.borrow_mut() = Some(id.clone());
        self.update_reader_header(&id);
        self.update_reader_buttons(&id);
        self.start_read_timer(id);

        if self.inner.is_collapsed() {
            self.inner.set_show_content(true);
        }
        if focus {
            self.reader.webview.grab_focus();
        }
    }

    fn start_read_timer(&self, id: ArticleId) {
        self.read_gen.set(self.read_gen.get() + 1);
        let gen = self.read_gen.get();
        let w = self.weak();
        glib::timeout_add_local(Duration::from_millis(800), move || {
            if let Some(app) = w.upgrade() {
                if app.read_gen.get() != gen || !app.window.is_active() {
                    return glib::ControlFlow::Break;
                }
                if app.reader.current.borrow().as_deref() != Some(id.as_str()) {
                    return glib::ControlFlow::Break;
                }
                if app.unread_guard.borrow().contains(&id) {
                    return glib::ControlFlow::Break;
                }
                let still_unread = {
                    let lib = app.lib.borrow();
                    article_of(&lib, &id).map(|a| a.unread).unwrap_or(false)
                };
                if still_unread {
                    let mut batch: UndoBatch = Vec::new();
                    app.apply_status(&id, Some(false), None, &mut batch);
                    app.undo_stack.borrow_mut().push(batch);
                }
            }
            glib::ControlFlow::Break
        });
    }

    fn render_article(&self, id: &str) -> Option<String> {
        let lib = self.lib.borrow();
        let a = article_of(&lib, id)?;
        let feed = feed_of(&lib, &a.feed_id);
        let content = lib.contents.get(id).cloned().unwrap_or_else(|| "<p>Inhalt fehlt.</p>".into());
        let style = self.reader.style.borrow();
        let tokens = *self.tokens.borrow();
        let rs = reader::ReaderStyle {
            font_size: style.font_size,
            measure_ch: style.measure_ch,
            line_height: style.line_height,
        };
        let published = fmt_full(a.published_at);
        let doc = reader::ReaderDocument {
            kicker: feed.map(|f| f.title.as_str()).unwrap_or(""),
            title: &a.title,
            author: a.author.as_deref(),
            source: "",
            published: &published,
            content_html: &content,
        };
        Some(reader::render_document(&doc, &tokens, &rs))
    }

    fn update_reader_header(&self, id: &str) {
        let lib = self.lib.borrow();
        let Some(a) = article_of(&lib, id) else { return };
        let feed_title = feed_of(&lib, &a.feed_id).map(|f| f.title.clone()).unwrap_or_default();
        self.reader.title.set_title(&a.title);
        self.reader.title.set_subtitle(&feed_title);
    }

    fn update_reader_buttons(&self, id: &str) {
        let lib = self.lib.borrow();
        let Some(a) = article_of(&lib, id) else { return };
        self.reader.btn_read.set_icon_name(if a.unread {
            "mail-read-symbolic"
        } else {
            "mail-unread-symbolic"
        });
        self.reader.btn_read.set_tooltip_text(Some(if a.unread {
            "Als gelesen markieren (M)"
        } else {
            "Als ungelesen markieren (M)"
        }));
        self.reader.btn_saved.set_icon_name(if a.saved {
            "user-bookmarks-symbolic"
        } else {
            "bookmark-new-symbolic"
        });
        self.reader.btn_saved.set_tooltip_text(Some(if a.saved {
            "Entspeichern (S)"
        } else {
            "Speichern (S)"
        }));
    }

    fn update_reader_empty(&self) {
        let lib = self.lib.borrow();
        let source = self.source.borrow().clone();
        let ids = visible_articles(&lib, &source, now_ms());
        let unread = ids
            .iter()
            .filter(|id| article_of(&lib, id).map(|a| a.unread).unwrap_or(false))
            .count();
        *self.reader.current.borrow_mut() = None;
        self.reader.show_empty(
            &source_label(&lib, &source),
            &format!("{unread} ungelesen · {} Artikel", ids.len()),
        );
        let label = source_label(&lib, &source);
        self.reader.title.set_title(&label);
        self.reader.title.set_subtitle("");
    }

    fn flush_keep_visible(&self, new_selection: Option<&str>) {
        let kv = self.keep_visible.borrow_mut().take();
        if let Some(kv) = kv {
            if Some(kv.as_str()) == new_selection {
                *self.keep_visible.borrow_mut() = Some(kv);
                return;
            }
            let still_matches = {
                let lib = self.lib.borrow();
                let source = self.source.borrow().clone();
                article_of(&lib, &kv).map(|a| crate::state::matches(&lib, a, &source)).unwrap_or(false)
            };
            if !still_matches {
                self.remove_row(&kv);
            }
        }
    }

    fn remove_row(&self, id: &str) {
        if let Some(pos) = self.row_pos(id) {
            self.suppress.set(true);
            self.list_store.remove(pos);
            if self.selected.borrow().as_deref() == Some(id) {
                let next = self
                    .item_positions()
                    .into_iter()
                    .find(|p| *p >= pos)
                    .or_else(|| self.item_positions().last().copied());
                if let Some(p) = next {
                    self.list_selection.set_selected(p);
                }
            }
            self.suppress.set(false);
        }
        if self.item_positions().is_empty() {
            self.list_stack.set_visible_child_name("empty");
        }
    }

    fn update_row_in_place(&self, id: &str) {
        if let Some(pos) = self.row_pos(id) {
            self.list_store.items_changed(pos, 1, 1);
        }
    }

    // ── Status-Mutationen ──

    fn apply_status(&self, id: &str, read: Option<bool>, saved: Option<bool>, batch: &mut UndoBatch) {
        let prev = {
            let lib = self.lib.borrow();
            article_of(&lib, id).map(|a| (a.id.clone(), a.unread, a.saved))
        };
        let Some((pid, prev_unread, prev_saved)) = prev else { return };
        batch.push((pid, prev_unread, prev_saved));
        {
            let mut lib = self.lib.borrow_mut();
            if let Some(a) = article_mut(&mut lib, id) {
                if let Some(r) = read {
                    a.unread = !r;
                }
                if let Some(s) = saved {
                    a.saved = s;
                }
            }
        }
        if read == Some(false) {
            self.unread_guard.borrow_mut().insert(id.to_string());
        } else if read.is_some() {
            self.unread_guard.borrow_mut().remove(id);
        }

        let still_matches = {
            let lib = self.lib.borrow();
            let source = self.source.borrow().clone();
            article_of(&lib, id).map(|a| crate::state::matches(&lib, a, &source)).unwrap_or(false)
        };
        if still_matches {
            self.update_row_in_place(id);
        } else if self.selected.borrow().as_deref() == Some(id)
            || self.keep_visible.borrow().as_deref() == Some(id)
        {
            *self.keep_visible.borrow_mut() = Some(id.to_string());
        } else {
            self.remove_row(id);
        }

        if self.reader.current.borrow().as_deref() == Some(id) {
            self.update_reader_buttons(id);
        }
        self.refresh_sidebar();
    }

    fn current_article_id(&self) -> Option<ArticleId> {
        self.selected
            .borrow()
            .clone()
            .or_else(|| self.reader.current.borrow().clone())
    }

    fn toggle_read(&self) {
        let Some(id) = self.current_article_id() else { return };
        let unread = {
            let lib = self.lib.borrow();
            article_of(&lib, &id).map(|a| a.unread).unwrap_or(false)
        };
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(&id, Some(!unread), None, &mut batch);
        self.undo_stack.borrow_mut().push(batch);
    }

    fn toggle_saved(&self) {
        let Some(id) = self.current_article_id() else { return };
        let saved = {
            let lib = self.lib.borrow();
            article_of(&lib, &id).map(|a| a.saved).unwrap_or(false)
        };
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(&id, None, Some(!saved), &mut batch);
        self.undo_stack.borrow_mut().push(batch);
    }

    fn undo(&self) {
        let Some(batch) = self.undo_stack.borrow_mut().pop() else {
            self.show_toast("Nichts rückgängig zu machen");
            return;
        };
        {
            let mut lib = self.lib.borrow_mut();
            for (id, unread, saved) in &batch {
                if let Some(a) = article_mut(&mut lib, id) {
                    a.unread = *unread;
                    a.saved = *saved;
                }
            }
        }
        self.refresh_list();
        self.refresh_sidebar();
        if let Some(id) = self.reader.current.borrow().clone() {
            self.update_reader_buttons(&id);
        }
        self.show_toast("Aktion rückgängig gemacht");
    }

    fn mark_scope_dialog(&self) {
        let ids: Vec<ArticleId> = {
            let lib = self.lib.borrow();
            let source = self.source.borrow().clone();
            visible_articles(&lib, &source, now_ms())
                .into_iter()
                .filter(|id| article_of(&lib, id).map(|a| a.unread).unwrap_or(false))
                .collect()
        };
        let label = source_label(&self.lib.borrow(), &self.source.borrow());
        if ids.is_empty() {
            self.show_toast(&format!("„{label}“ enthält keine ungelesenen Artikel"));
            return;
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Bereich als gelesen markieren")
            .body(format!(
                "„{label}“: {} zum Klickzeitpunkt bekannte Artikel werden als gelesen markiert. Rückgängig mit Strg+Z.",
                ids.len()
            ))
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("mark", &format!("{} als gelesen markieren", ids.len()));
        dialog.set_response_appearance("mark", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "mark" {
                return;
            }
            let mut batch: UndoBatch = Vec::new();
            for id in &ids {
                app.apply_status(id, Some(false), None, &mut batch);
            }
            app.undo_stack.borrow_mut().push(batch);
            app.show_toast(&format!("{} Artikel als gelesen markiert", ids.len()));
        });
    }

    fn show_toast(&self, msg: &str) {
        self.toast.add_toast(adw::Toast::new(msg));
    }

    // ── Navigation ──

    fn move_selection(&self, delta: i32) {
        let positions = self.item_positions();
        if positions.is_empty() {
            return;
        }
        let cur_id = self.selected.borrow().clone();
        let cur_idx = cur_id
            .as_ref()
            .and_then(|id| {
                positions
                    .iter()
                    .position(|p| self.item_id_at(*p).as_deref() == Some(id.as_str()))
            })
            .map(|i| i as i32)
            .unwrap_or(if delta > 0 { -1 } else { positions.len() as i32 });
        let target = (cur_idx + delta).clamp(0, positions.len() as i32 - 1);
        if let Some(id) = self.item_id_at(positions[target as usize]) {
            self.open_article(id, false, true);
        }
    }

    fn move_unread(&self, dir: i32) {
        let positions = self.item_positions();
        if positions.is_empty() {
            return;
        }
        let cur_id = self.selected.borrow().clone();
        let cur_idx = cur_id
            .as_ref()
            .and_then(|id| {
                positions
                    .iter()
                    .position(|p| self.item_id_at(*p).as_deref() == Some(id.as_str()))
            })
            .unwrap_or(0);
        let n = positions.len() as i32;
        for step in 1..=n {
            let idx = ((cur_idx as i32 + dir * step) % n + n) % n;
            if let Some(id) = self.item_id_at(positions[idx as usize]) {
                let unread = {
                    let lib = self.lib.borrow();
                    article_of(&lib, &id).map(|a| a.unread).unwrap_or(false)
                };
                if unread {
                    self.open_article(id, false, true);
                    return;
                }
            }
        }
        self.show_toast("Keine weiteren ungelesenen Artikel in dieser Ansicht");
    }

    fn focus_pane(&self, dir: i32) {
        let panes = self.panes.borrow().clone();
        let Some(focus) = gtk::prelude::GtkWindowExt::focus(&self.window) else {
            if let Some(p) = panes.first() {
                gtk::prelude::GtkWindowExt::set_focus(&self.window, Some(p));
            }
            return;
        };
        let mut current: Option<usize> = None;
        let mut cur = Some(focus.clone());
        while let Some(c) = cur {
            if let Some(i) = panes.iter().position(|p| *p == c) {
                current = Some(i);
                break;
            }
            cur = c.parent();
        }
        let idx = match current {
            Some(i) => ((i as i32 + dir).rem_euclid(panes.len() as i32)) as usize,
            None => 0,
        };
        if let Some(p) = panes.get(idx) {
            gtk::prelude::GtkWindowExt::set_focus(&self.window, Some(p));
        }
    }

    fn back(&self) {
        if self.inner.is_collapsed() && self.inner.shows_content() {
            self.inner.set_show_content(false);
        } else if self.outer.is_collapsed() && self.outer.shows_content() {
            self.outer.set_show_content(false);
        }
    }

    // ── Reader-Aktionen ──

    fn zoom(&self, delta: f64) {
        {
            let mut s = self.reader.style.borrow_mut();
            s.font_size = (s.font_size + delta).clamp(14.0, 32.0);
        }
        self.reload_current(true);
    }

    fn zoom_reset(&self) {
        self.reader.style.borrow_mut().font_size = ReaderStyleState::default().font_size;
        self.reload_current(true);
    }

    fn reload_current(&self, preserve: bool) {
        let Some(id) = self.reader.current.borrow().clone() else { return };
        let Some(html) = self.render_article(&id) else { return };
        if preserve {
            let webview = self.reader.webview.clone();
            let pane = Rc::clone(&self.reader);
            webview.evaluate_javascript(
                "window.scrollY",
                None,
                None,
                None::<&gio::Cancellable>,
                move |res| {
                    if let Ok(v) = res {
                        if v.is_number() {
                            pane.pending_scroll.set(v.to_double());
                        }
                    }
                    pane.load_html_doc(&html);
                },
            );
        } else {
            self.reader.pending_scroll.set(-1.0);
            self.reader.load_html_doc(&html);
        }
    }

    fn open_find(&self) {
        self.reader.search_bar.set_search_mode(true);
        self.reader.search_entry.grab_focus();
    }

    fn open_external(&self) {
        let Some(url) = self.current_url() else { return };
        gtk::UriLauncher::new(&url).launch(None::<&gtk::Window>, None::<&gio::Cancellable>, |res| {
            if let Err(e) = res {
                eprintln!("Extern öffnen fehlgeschlagen: {e}");
            }
        });
    }

    fn copy_link(&self) {
        let Some(url) = self.current_url() else { return };
        self.window.clipboard().set_text(&url);
        self.show_toast("Link kopiert");
    }

    fn current_url(&self) -> Option<String> {
        let id = self.current_article_id()?;
        let lib = self.lib.borrow();
        article_of(&lib, &id).and_then(|a| a.url.clone())
    }

    // ── Konto-Aktionen (Fixtures) ──

    fn do_refresh(&self) {
        let now = now_ms();
        self.last_sync_label.set_label(&format!("Zuletzt aktualisiert: {}", fmt_time(now)));
        self.show_toast("Aktualisiert (M1: Fixture-Bestand, echter Abruf folgt in M2)");
    }

    fn add_feed_dialog(&self) {
        let entry = gtk::Entry::builder()
            .placeholder_text("Feed- oder Website-URL")
            .activates_default(true)
            .build();
        let dialog = adw::AlertDialog::builder()
            .heading("Feed hinzufügen")
            .body("URL eingeben. Discovery und Abruf folgen in M2; der Feed wird lokal angelegt.")
            .extra_child(&entry)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("add", "Hinzufügen");
        dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("add"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "add" {
                return;
            }
            let url = entry.text().trim().to_string();
            if url.is_empty() {
                return;
            }
            let n = app.custom_feed_counter.get() + 1;
            app.custom_feed_counter.set(n);
            let host = url
                .split("://")
                .nth(1)
                .unwrap_or(url.as_str())
                .split('/')
                .next()
                .unwrap_or("Neuer Feed");
            let id = format!("f-custom-{n}");
            app.lib.borrow_mut().feeds.push(domain::Feed {
                id: id.clone(),
                title: host.to_string(),
                website: Some(url),
                accent: "#7A8B99".into(),
                groups: vec![],
            });
            app.set_source(SourceFilter::Feed(id));
            app.list_empty.set_title("Noch keine Artikel");
            app.list_empty.set_description(Some(
                "Der Feed wurde angelegt — Discovery und Abruf folgen in M2.",
            ));
            app.show_toast("Feed hinzugefügt (lokal, ohne Abruf)");
        });
    }
}

thread_local! {
    static APP: std::cell::RefCell<Option<Rc<App>>> = const { std::cell::RefCell::new(None) };
}

pub fn run_and_keep(application: &adw::Application) -> Rc<App> {
    let app = App::new(application);
    APP.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&app)));
    app
}
