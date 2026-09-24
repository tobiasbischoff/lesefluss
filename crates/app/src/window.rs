use crate::dbworker::{DbWorker, JobOut};
use crate::list;
use crate::model::{ListRow, UiState};
use crate::net::{Net, NetEvent};
use crate::reader::{find_in_view, find_next, ReaderPane};
use crate::sidebar;
use crate::state::*;
use crate::style::{gtk_css_for, tokens_for, ReaderStyleState};
use adw::prelude::*;
use webkit6::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::mpsc::Receiver;
use std::time::Duration;
use storage::{ArticleRow, Counts, FeedRow, GroupRow, Filter, Scope};

pub fn dbg_log(msg: &str) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var("LF_DEBUG").is_ok()) {
        eprintln!("[lf] {msg}");
    }
}

pub fn now_ms() -> i64 {
    storage::now_ms()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectionCause {
    Unknown,
    Pointer,
    Keyboard,
}

type UndoBatch = Vec<(i64, String, bool, bool)>;
type PendingCb = Box<dyn FnOnce(&Rc<App>, JobOut)>;

const ACCENTS: &[&str] = &["#B94A2E", "#2E5FA3", "#3F8F5F", "#AC3A50", "#7A5CA8", "#B08A2E", "#2E8B8B"];

pub struct App {
    pub window: adw::ApplicationWindow,
    pub toast: adw::ToastOverlay,
    pub outer: adw::NavigationSplitView,
    pub inner: adw::NavigationSplitView,
    pub sidebar_list: gtk::ListBox,
    pub sidebar_filters: RefCell<Vec<Option<Scope>>>,
    pub last_sync_label: gtk::Label,
    pub list_title: adw::WindowTitle,
    pub list_stack: gtk::Stack,
    pub list_store: gio::ListStore,
    pub list_selection: gtk::SingleSelection,
    pub list_view: gtk::ListView,
    pub list_scroll: gtk::ScrolledWindow,
    pub list_empty: adw::StatusPage,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub filter_saved: gtk::ToggleButton,
    pub filter_unread: gtk::ToggleButton,
    pub filter_all: gtk::ToggleButton,
    pub reader: Rc<ReaderPane>,
    pub state: RefCell<UiState>,
    pub worker: DbWorker,
    pub net: Rc<Net>,
    pub pending_db: RefCell<Vec<(Receiver<JobOut>, PendingCb)>>,
    pub drain_active: Cell<bool>,
    pub preview_timer: RefCell<Option<glib::SourceId>>,
    pub search_timer: RefCell<Option<glib::SourceId>>,
    pub read_gen: Cell<u64>,
    pub suppress: Cell<bool>,
    pub selection_cause: Cell<SelectionCause>,
    pub list_dirty: RefCell<Vec<String>>,
    pub undo_stack: RefCell<Vec<UndoBatch>>,
    pub tokens: RefCell<reader::tokens::Tokens>,
    pub css: gtk::CssProvider,
    pub panes: RefCell<Vec<gtk::Widget>>,
    me: RefCell<Option<Weak<App>>>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        application: &adw::Application,
        worker: DbWorker,
        net: Rc<Net>,
    ) -> Rc<Self> {
        let reader = Rc::new(ReaderPane::new());

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
        let list_selection = gtk::SingleSelection::builder().model(&list_store).autoselect(false).build();
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

        let search_entry = gtk::SearchEntry::builder().placeholder_text("Artikel durchsuchen (Strg+L)").build();
        let search_bar = gtk::SearchBar::builder().child(&search_entry).show_close_button(true).build();

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
        let filter_saved = gtk::ToggleButton::builder()
            .icon_name("user-bookmarks-symbolic")
            .tooltip_text("Gespeicherte Artikel anzeigen")
            .build();
        let filter_unread = gtk::ToggleButton::builder()
            .icon_name("mail-unread-symbolic")
            .tooltip_text("Ungelesene Artikel anzeigen")
            .active(true)
            .build();
        let filter_all = gtk::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text("Alle Artikel anzeigen")
            .build();
        let filter_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        filter_box.add_css_class("linked");
        filter_box.append(&filter_saved);
        filter_box.append(&filter_unread);
        filter_box.append(&filter_all);
        let filter_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        filter_wrap.set_halign(gtk::Align::Center);
        filter_wrap.set_margin_bottom(8);
        filter_wrap.append(&filter_box);
        let list_toolbar = adw::ToolbarView::builder().content(&list_stack).build();
        list_toolbar.add_top_bar(&list_header);
        list_toolbar.add_top_bar(&search_bar);
        list_toolbar.add_bottom_bar(&filter_wrap);
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
        gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let app = Rc::new(Self {
            window,
            toast,
            outer,
            inner,
            sidebar_list,
            sidebar_filters: RefCell::new(Vec::new()),
            last_sync_label,
            list_title,
            list_stack,
            list_store,
            list_selection,
            list_view,
            list_scroll,
            list_empty,
            search_bar,
            search_entry,
            filter_saved,
            filter_unread,
            filter_all,
            reader,
            state: RefCell::new(UiState::default()),
            worker,
            net,
            pending_db: RefCell::new(Vec::new()),
            drain_active: Cell::new(false),
            preview_timer: RefCell::new(None),
            search_timer: RefCell::new(None),
            read_gen: Cell::new(0),
            suppress: Cell::new(false),
            selection_cause: Cell::new(SelectionCause::Unknown),
            list_dirty: RefCell::new(Vec::new()),
            undo_stack: RefCell::new(Vec::new()),
            tokens: RefCell::new(tokens_for(true)),
            css,
            panes: RefCell::new(Vec::new()),
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
        app.install_width_watcher();
        app.start_drain_loop();
        app.window.present();
        app.bootstrap();
        app
    }

    fn weak(&self) -> Weak<App> {
        self.me.borrow().clone().expect("App-Selbstreferenz gesetzt")
    }

    // ── DB- und Net-Drain ──

    pub fn db_query<F, R, C>(&self, f: F, on: C)
    where
        F: FnOnce(&storage::Database) -> R + Send + 'static,
        R: Send + 'static,
        C: FnOnce(&Rc<App>, R) + 'static,
    {
        let rx = self.worker.send(f);
        self.pending_db.borrow_mut().push((
            rx,
            Box::new(move |app: &Rc<App>, out: JobOut| {
                if let Ok(v) = out.downcast::<R>() {
                    on(app, *v);
                }
            }),
        ));
    }

    fn start_drain_loop(&self) {
        let w = self.weak();
        glib::timeout_add_local(Duration::from_millis(120), move || {
            let Some(app) = w.upgrade() else { return glib::ControlFlow::Break };
            app.drain_once();
            glib::ControlFlow::Continue
        });
    }

    fn drain_once(&self) {
        loop {
            let net_event = self.net.events.try_recv().ok();
            if let Some(ev) = net_event {
                self.handle_net_event(ev);
                continue;
            }
            break;
        }
        let mut ready: Vec<(PendingCb, JobOut)> = Vec::new();
        {
            let mut pending = self.pending_db.borrow_mut();
            let mut i = 0;
            while i < pending.len() {
                match pending[i].0.try_recv() {
                    Ok(out) => {
                        let (_, cb) = pending.remove(i);
                        ready.push((cb, out));
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => i += 1,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        let (_, _cb) = pending.remove(i);
                    }
                }
            }
        }
        for (cb, out) in ready {
            let self_rc = self
                .me
                .borrow()
                .as_ref()
                .and_then(Weak::upgrade)
                .expect("App lebendig während Drain");
            cb(&self_rc, out);
        }
    }

    fn handle_net_event(&self, ev: NetEvent) {
        match ev {
            NetEvent::FetchStarted(_) => {}
            NetEvent::FetchNotModified(_) => self.touch_last_sync(),
            NetEvent::FetchDone { feed_id, added, title, .. } => {
                self.touch_last_sync();
                let label = title.unwrap_or_else(|| format!("Feed {feed_id}"));
                if added > 0 {
                    self.show_toast(&format!("{label}: {added} neue Artikel"));
                    let at_top = self.list_scroll.vadjustment().value() < 80.0;
                    self.reload_counts();
                    if at_top {
                        self.load_page(false);
                    }
                } else {
                    self.reload_counts();
                }
            }
            NetEvent::FetchFailed { message, .. } => {
                self.touch_last_sync();
                self.show_toast(&format!("Abruf fehlgeschlagen: {message}"));
            }
            NetEvent::DiscoveryDone { candidates, .. } => self.show_discovery_dialog(candidates),
            NetEvent::DiscoveryFailed { message, .. } => {
                self.show_toast(&format!("Kein Feed gefunden: {message}"));
            }
        }
    }

    fn touch_last_sync(&self) {
        let now = now_ms();
        self.state.borrow_mut().last_sync = Some(now);
        self.last_sync_label.set_label(&format!("Zuletzt aktualisiert: {}", fmt_time(now)));
        self.worker.send(move |db| db.set_last_sync("local", now));
    }

    // ── Start ──

    fn bootstrap(&self) {
        let seed = std::env::var("LF_SEED").is_ok();
        let w = self.weak();
        self.db_query(
            move |db| {
                db.ensure_local_account()?;
                if seed {
                    let n = db.list_feeds()?;
                    if n.is_empty() {
                        crate::seed::seed_fixtures(db)?;
                    }
                }
                let feeds = db.list_feeds()?;
                let groups = db.list_groups()?;
                let counts = db.counts()?;
                let last = db.last_sync("local")?;
                Ok::<_, storage::StorageError>((feeds, groups, counts, last))
            },
            move |app, res: storage::Result<(Vec<FeedRow>, Vec<GroupRow>, Counts, Option<i64>)>| {
                let Ok((feeds, groups, counts, last)) = res else { return };
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                    st.last_sync = last;
                }
                if let Some(ms) = last {
                    app.last_sync_label.set_label(&format!("Zuletzt aktualisiert: {}", fmt_time(ms)));
                }
                app.refresh_sidebar();
                app.load_page(false);
                if let Ok(list) = std::env::var("LF_SUBSCRIBE") {
                    for url in list.split(',').filter(|u| !u.trim().is_empty()) {
                        let url = url.trim().to_string();
                        let title = url::Url::parse(&url)
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_string))
                            .unwrap_or_else(|| url.clone());
                        app.subscribe(&url, &title);
                    }
                }
                let _ = w;
            },
        );
    }

    fn load_page(&self, append: bool) {
        let (scope, filter, cursor, search) = {
            let st = self.state.borrow();
            let cur = if append { st.cursor.clone() } else { None };
            (st.scope.clone(), st.filter, cur, st.search.clone())
        };
        if append && cursor.is_none() {
            return;
        }
        let cur = cursor.clone();
        self.db_query(
            move |db| match &search {
                Some(q) if !q.is_empty() => db.search(q, 200),
                _ => db.query_articles(&scope, filter, cur.as_ref().map(|(ms, id)| (*ms, id.as_str())), 200),
            },
            move |app, res: storage::Result<Vec<ArticleRow>>| {
                let Ok(rows) = res else { return };
                let had_full_page = rows.len() >= 200;
                let keep_sel = app.state.borrow().selected.clone();
                {
                    let mut st = app.state.borrow_mut();
                    if append {
                        st.append_rows(rows);
                    } else {
                        st.build_rows(rows);
                    }
                    if !had_full_page {
                        st.cursor = st.last_item().map(|a| (a.published_ms, a.id.clone()));
                    }
                    if let Some(sel) = keep_sel {
                        st.selected = Some(sel);
                    }
                }
                app.sync_store(append);
                app.update_list_empty_state();
            },
        );
    }

    fn reload_counts(&self) {
        let w = self.weak();
        self.db_query(
            |db| db.counts(),
            move |app, res: storage::Result<Counts>| {
                let Ok(counts) = res else { return };
                app.state.borrow_mut().counts = counts;
                app.refresh_sidebar();
                app.update_reader_empty();
                let _ = w;
            },
        );
    }

    // ── Store-Sync ──

    fn sync_store(&self, append: bool) {
        let rows = self.state.borrow().rows.clone();
        self.suppress.set(true);
        if append {
            let existing = self.list_store.n_items() as usize;
            for r in rows.iter().skip(existing) {
                self.list_store.append(&glib::BoxedAnyObject::new(r.clone()));
            }
        } else {
            self.list_store.remove_all();
            for r in &rows {
                self.list_store.append(&glib::BoxedAnyObject::new(r.clone()));
            }
            if let Some(sel) = self.state.borrow().selected.clone() {
                if let Some(pos) = self.state.borrow().row_pos(&sel.1) {
                    self.list_selection.set_selected(pos as u32);
                }
            }
        }
        let has_items = rows.iter().any(|r| matches!(r, ListRow::Item(_)));
        self.list_stack.set_visible_child_name(if has_items { "list" } else { "empty" });
        self.suppress.set(false);
    }

    fn update_list_empty_state(&self) {
        let has = self.state.borrow().rows.iter().any(|r| matches!(r, ListRow::Item(_)));
        self.list_stack.set_visible_child_name(if has { "list" } else { "empty" });
    }

    fn rebind_row(&self, id: &str) {
        let Some(pos) = self.state.borrow().row_pos(id) else { return };
        let Some(obj) = self.list_store.item(pos as u32) else { return };
        self.suppress.set(true);
        self.list_store.remove(pos as u32);
        self.list_store.insert(pos as u32, &obj);
        self.suppress.set(false);
    }

    fn update_row(&self, id: &str) {
        if !self.list_view.is_mapped() {
            let mut dirty = self.list_dirty.borrow_mut();
            if !dirty.iter().any(|x| x == id) {
                dirty.push(id.to_string());
            }
            return;
        }
        self.rebind_row(id);
    }

    fn flush_dirty_rows(&self) {
        let ids = std::mem::take(&mut *self.list_dirty.borrow_mut());
        for id in ids {
            self.rebind_row(&id);
        }
    }

    fn remove_row(&self, id: &str) {
        let Some(pos) = self.state.borrow().row_pos(id) else { return };
        {
            let mut st = self.state.borrow_mut();
            st.rows.remove(pos);
        }
        self.suppress.set(true);
        if self.selected_id().as_deref() == Some(id) {
            let next = self
                .state
                .borrow()
                .rows
                .iter()
                .enumerate()
                .skip(pos)
                .find_map(|(i, r)| match r {
                    ListRow::Item(_) => Some(i),
                    _ => None,
                })
                .or_else(|| {
                    self.state.borrow().rows.iter().enumerate().take(pos).rev().find_map(|(i, r)| match r {
                        ListRow::Item(_) => Some(i),
                        _ => None,
                    })
                });
            if let Some(p) = next {
                self.list_selection.set_selected(p as u32);
            }
        }
        self.list_store.remove(pos as u32);
        self.suppress.set(false);
        self.update_list_empty_state();
    }

    fn selected_id(&self) -> Option<String> {
        self.state.borrow().selected.clone().map(|(_, id)| id)
    }

    // ── Sidebar ──

    fn refresh_sidebar(&self) {
        let state = self.state.borrow();
        let mut filters = self.sidebar_filters.borrow_mut();
        self.suppress.set(true);
        let w = self.weak();
        let cb: Rc<dyn Fn(i64)> = Rc::new(move |gid: i64| {
            if let Some(app) = w.upgrade() {
                app.toggle_group(gid);
            }
        });
        sidebar::rebuild(&self.sidebar_list, &state, &mut filters, &cb);
        self.suppress.set(false);
    }

    fn toggle_group(&self, gid: i64) {
        {
            let mut st = self.state.borrow_mut();
            if !st.collapsed.remove(&gid) {
                st.collapsed.insert(gid);
            }
        }
        self.refresh_sidebar();
    }

    fn set_scope(&self, f: Scope, reset_filter_to_unread: bool) {
        {
            let mut st = self.state.borrow_mut();
            st.scope = f.clone();
            if reset_filter_to_unread {
                st.filter = Filter::Unread;
            }
            st.search = None;
            st.selected = st.last_opened.get(&(f.clone(), st.filter)).cloned();
        }
        self.sync_filter_buttons();
        self.search_bar.set_search_mode(false);
        self.list_title.set_title(&self.scope_label_now());
        self.refresh_sidebar();
        self.load_page(false);
        if let Some(sel) = self.selected_id() {
            self.open_article_by_id(&sel, false, false);
        } else {
            self.update_reader_empty();
        }
        if self.outer.is_collapsed() {
            self.outer.set_show_content(true);
        }
    }

    fn set_filter(&self, f: Filter) {
        {
            let mut st = self.state.borrow_mut();
            st.filter = f;
            st.search = None;
            st.selected = st.last_opened.get(&(st.scope.clone(), f)).cloned();
        }
        self.sync_filter_buttons();
        self.load_page(false);
        if let Some(sel) = self.selected_id() {
            self.open_article_by_id(&sel, false, false);
        } else {
            self.update_reader_empty();
        }
    }

    fn sync_filter_buttons(&self) {
        let f = self.state.borrow().filter;
        self.filter_saved.set_active(f == Filter::Saved);
        self.filter_unread.set_active(f == Filter::Unread);
        self.filter_all.set_active(f == Filter::All);
    }

    fn scope_label_now(&self) -> String {
        let st = self.state.borrow();
        if let Some(q) = &st.search {
            return format!("Suche: {q}");
        }
        match &st.scope {
            Scope::Global => "Ungelesen".into(),
            Scope::Group(g) => st.groups.iter().find(|x| &x.id == g).map(|x| x.name.clone()).unwrap_or_else(|| "Gruppe".into()),
            Scope::Feed(f) => st.feed_title(*f),
        }
    }

    // ── Artikel öffnen / Status ──

    fn open_article_by_id(&self, id: &str, focus: bool, flush: bool) {
        let Some(row) = self.state.borrow().article(id).cloned() else { return };
        self.open_article(row, focus, flush);
    }

    fn open_article(&self, row: ArticleRow, focus: bool, _flush: bool) {
        let id = row.id.clone();
        {
            let mut st = self.state.borrow_mut();
            let key = (st.scope.clone(), st.filter);
            st.selected = Some((row.feed_id, id.clone()));
            st.last_opened.insert(key, (row.feed_id, id.clone()));
            st.unread_guard.remove(&id);
        }
        if let Some(pos) = self.state.borrow().row_pos(&id) {
            self.suppress.set(true);
            self.list_selection.set_selected(pos as u32);
            self.suppress.set(false);
        }

        self.reader.title.set_title(&row.title);
        self.reader.title.set_subtitle(&row.feed_title);
        self.update_reader_buttons(&row);
        *self.reader.current.borrow_mut() = Some(id.clone());
        self.reader.show_loading();

        let row2 = row.clone();
        let id_cb = id.clone();
        let w = self.weak();
        self.db_query(
            move |db| db.content_html(row2.feed_id, &row2.id),
            move |app, res: storage::Result<Option<String>>| {
                let current = app.reader.current.borrow().clone();
                if current.as_deref() != Some(&id_cb) {
                    return;
                }
                match res {
                    Ok(Some(html)) => {
                        let style = app.reader.style.borrow();
                        let rs = reader::ReaderStyle {
                            font_size: style.font_size,
                            measure_ch: style.measure_ch,
                            line_height: style.line_height,
                        };
                        let tokens = *app.tokens.borrow();
                        let doc = reader::ReaderDocument {
                            kicker: &row.feed_title,
                            title: &row.title,
                            author: row.author.as_deref(),
                            source: "",
                            published: &fmt_full(row.published_ms),
                            content_html: &html,
                        };
                        let html_doc = reader::render_document(&doc, &tokens, &rs);
                        app.reader.load_html_doc(&html_doc);
                    }
                    _ => app.reader.show_error(),
                }
                let _ = w;
            },
        );

        self.start_read_timer(id.clone());

        if self.inner.is_collapsed() {
            self.inner.set_show_content(true);
        }
        if focus {
            self.reader.webview.grab_focus();
        }
    }

    fn start_read_timer(&self, id: String) {
        self.read_gen.set(self.read_gen.get() + 1);
        let gen = self.read_gen.get();
        let w = self.weak();
        glib::timeout_add_local(Duration::from_millis(800), move || {
            let Some(app) = w.upgrade() else { return glib::ControlFlow::Break };
            if app.read_gen.get() != gen || !app.window.is_active() {
                return glib::ControlFlow::Break;
            }
            if app.reader.current.borrow().as_deref() != Some(id.as_str()) {
                return glib::ControlFlow::Break;
            }
            if app.state.borrow().unread_guard.contains(&id) {
                return glib::ControlFlow::Break;
            }
            let still_unread = app.state.borrow().article(&id).map(|a| a.unread).unwrap_or(false);
            if still_unread {
                let mut batch: UndoBatch = Vec::new();
                app.apply_status(&id, Some(true), None, &mut batch);
                app.undo_stack.borrow_mut().push(batch);
            }
            glib::ControlFlow::Break
        });
    }

    fn apply_status(&self, id: &str, read: Option<bool>, saved: Option<bool>, batch: &mut UndoBatch) {
        let prev = {
            let st = self.state.borrow();
            st.article(id).map(|a| (a.feed_id, a.unread, a.saved))
        };
        let Some((feed_id, prev_unread, prev_saved)) = prev else { return };
        batch.push((feed_id, id.to_string(), prev_unread, prev_saved));
        {
            let mut st = self.state.borrow_mut();
            if let Some(a) = st.article_mut(id) {
                if let Some(r) = read {
                    a.unread = !r;
                }
                if let Some(s) = saved {
                    a.saved = s;
                }
            }
            if let Some(r) = read {
                let delta = if r { -1 } else { 1 };
                st.counts.unread = (st.counts.unread + delta).max(0);
                if let Some(entry) = st.counts.per_feed.iter_mut().find(|(f, _)| *f == feed_id) {
                    entry.1 = (entry.1 + delta).max(0);
                }
                let gids: Vec<i64> = st
                    .feeds
                    .iter()
                    .find(|f| f.id == feed_id)
                    .map(|f| f.groups.clone())
                    .unwrap_or_default();
                for entry in st.counts.per_group.iter_mut() {
                    if gids.contains(&entry.0) {
                        entry.1 = (entry.1 + delta).max(0);
                    }
                }
            }
            if let Some(s) = saved {
                st.counts.saved = (st.counts.saved + if s { 1 } else { -1 }).max(0);
            }
        }

        let _ = feed_id;
        self.update_row(id);

        if self.reader.current.borrow().as_deref() == Some(id) {
            if let Some(row) = self.state.borrow().article(id).cloned() {
                self.update_reader_buttons(&row);
            }
        }
        self.refresh_sidebar();

        let feed_id2 = feed_id;
        let id2 = id.to_string();
        self.worker.send(move |db| db.set_status(feed_id2, &id2, read, saved));
    }

    fn current_article(&self) -> Option<ArticleRow> {
        let id = self.selected_id().or_else(|| self.reader.current.borrow().clone())?;
        self.state.borrow().article(&id).cloned()
    }

    fn toggle_read(&self) {
        let Some(row) = self.current_article() else { return };
        let was_unread = row.unread;
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(&row.id, Some(!was_unread), None, &mut batch);
        if was_unread {
            self.state.borrow_mut().unread_guard.remove(&row.id);
        } else {
            self.state.borrow_mut().unread_guard.insert(row.id.clone());
        }
        self.undo_stack.borrow_mut().push(batch);
    }

    fn toggle_saved(&self) {
        let Some(row) = self.current_article() else { return };
        let mut batch: UndoBatch = Vec::new();
        self.apply_status(&row.id, None, Some(!row.saved), &mut batch);
        self.undo_stack.borrow_mut().push(batch);
    }

    fn undo(&self) {
        let Some(batch) = self.undo_stack.borrow_mut().pop() else {
            self.show_toast("Nichts rückgängig zu machen");
            return;
        };
        {
            let mut st = self.state.borrow_mut();
            for (feed_id, id, unread, saved) in &batch {
                if let Some(a) = st.article_mut(id) {
                    a.unread = *unread;
                    a.saved = *saved;
                }
                let _ = feed_id;
            }
        }
        for (_, id, _, _) in &batch {
            self.update_row(id);
        }
        let batch2 = batch.clone();
        self.worker.send(move |db| {
            for (feed_id, id, unread, saved) in &batch2 {
                db.set_status(*feed_id, id, Some(!unread), Some(*saved))?;
            }
            Ok::<_, storage::StorageError>(())
        });
        self.reload_counts();
        self.show_toast("Aktion rückgängig gemacht");
    }

    fn mark_scope_dialog(&self) {
        let ids: Vec<(i64, String)> = {
            let st = self.state.borrow();
            st.rows
                .iter()
                .filter_map(|r| match r {
                    ListRow::Item(a) if a.unread => Some((a.feed_id, a.id.clone())),
                    _ => None,
                })
                .collect()
        };
        let label = self.scope_label_now();
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
            for (feed_id, id) in &ids {
                let _ = feed_id;
                app.apply_status(id, Some(true), None, &mut batch);
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
        let positions: Vec<usize> = self
            .state
            .borrow()
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                ListRow::Item(_) => Some(i),
                _ => None,
            })
            .collect();
        if positions.is_empty() {
            return;
        }
        let cur_id = self.selected_id();
        let cur_idx = cur_id
            .as_ref()
            .and_then(|id| self.state.borrow().row_pos(id))
            .and_then(|p| positions.iter().position(|x| *x == p))
            .map(|i| i as i32)
            .unwrap_or(if delta > 0 { -1 } else { positions.len() as i32 });
        let target = (cur_idx + delta).clamp(0, positions.len() as i32 - 1);
        let idx = positions[target as usize];
        let row = {
            let st = self.state.borrow();
            match st.rows.get(idx) {
                Some(ListRow::Item(a)) => Some(a.clone()),
                _ => None,
            }
        };
        if let Some(row) = row {
            self.open_article(row, false, true);
            self.scroll_to_selected();
        }
    }

    fn scroll_to_selected(&self) {
        if let Some(id) = self.selected_id() {
            if let Some(pos) = self.state.borrow().row_pos(&id) {
                self.list_view.scroll_to(pos as u32, gtk::ListScrollFlags::NONE, None::<gtk::ScrollInfo>);
            }
        }
    }

    fn move_unread(&self, dir: i32) {
        let positions: Vec<usize> = self
            .state
            .borrow()
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match r {
                ListRow::Item(a) if a.unread => Some(i),
                _ => None,
            })
            .collect();
        if positions.is_empty() {
            self.show_toast("Keine weiteren ungelesenen Artikel in dieser Ansicht");
            return;
        }
        let cur = self.selected_id().and_then(|id| self.state.borrow().row_pos(&id)).unwrap_or(0);
        let next = if dir > 0 {
            positions.iter().find(|&&p| p > cur).copied().or_else(|| positions.first().copied())
        } else {
            positions.iter().rev().find(|&&p| p < cur).copied().or_else(|| positions.last().copied())
        };
        if let Some(idx) = next {
            let row = {
                let st = self.state.borrow();
                match st.rows.get(idx) {
                    Some(ListRow::Item(a)) => Some(a.clone()),
                    _ => None,
                }
            };
            if let Some(row) = row {
                self.open_article(row, false, true);
                self.scroll_to_selected();
            }
        }
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

    fn update_reader_buttons(&self, row: &ArticleRow) {
        self.reader.btn_read.set_icon_name(if row.unread { "mail-read-symbolic" } else { "mail-unread-symbolic" });
        self.reader.btn_read.set_tooltip_text(Some(if row.unread {
            "Als gelesen markieren (M)"
        } else {
            "Als ungelesen markieren (M)"
        }));
        self.reader.btn_saved.set_icon_name(if row.saved { "user-bookmarks-symbolic" } else { "bookmark-new-symbolic" });
        self.reader.btn_saved.set_tooltip_text(Some(if row.saved { "Entspeichern (S)" } else { "Speichern (S)" }));
    }

    fn update_reader_empty(&self) {
        let st = self.state.borrow();
        let label = self.scope_label_now();
        let unread = match &st.scope {
            Scope::Global => st.counts.unread,
            Scope::Feed(f) => st.feed_unread(*f),
            Scope::Group(g) => st.group_unread(*g),
        };
        let total = st.rows.iter().filter(|r| matches!(r, ListRow::Item(_))).count() as i64;
        drop(st);
        self.reader.show_empty(&label, &format!("{unread} ungelesen · {total} Artikel"));
        self.reader.title.set_title(&label);
        self.reader.title.set_subtitle("");
    }

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
        let row = self.state.borrow().article(&id).cloned();
        let Some(row) = row else { return };
        let w = self.weak();
        self.db_query(
            move |db| db.content_html(row.feed_id, &row.id),
            move |app, res: storage::Result<Option<String>>| {
                let Ok(Some(html)) = res else { return };
                let style = app.reader.style.borrow();
                let rs = reader::ReaderStyle {
                    font_size: style.font_size,
                    measure_ch: style.measure_ch,
                    line_height: style.line_height,
                };
                let tokens = *app.tokens.borrow();
                let published = fmt_full(row.published_ms);
                let doc = reader::ReaderDocument {
                    kicker: &row.feed_title,
                    title: &row.title,
                    author: row.author.as_deref(),
                    source: "",
                    published: &published,
                    content_html: &html,
                };
                if preserve {
                    let pane = Rc::clone(&app.reader);
                    let wv = pane.webview.clone();
                    let html_doc = reader::render_document(&doc, &tokens, &rs);
                    wv.evaluate_javascript(
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
                            pane.load_html_doc(&html_doc);
                        },
                    );
                } else {
                    let html_doc = reader::render_document(&doc, &tokens, &rs);
                    app.reader.load_html_doc(&html_doc);
                }
                let _ = w;
            },
        );
    }

    fn open_find(&self) {
        self.reader.search_bar.set_search_mode(true);
        self.reader.search_entry.grab_focus();
    }

    fn open_external(&self) {
        let Some(url) = self.current_article().and_then(|a| a.url) else { return };
        gtk::UriLauncher::new(&url).launch(None::<&gtk::Window>, None::<&gio::Cancellable>, |res| {
            if let Err(e) = res {
                eprintln!("Extern öffnen fehlgeschlagen: {e}");
            }
        });
    }

    fn copy_link(&self) {
        let Some(url) = self.current_article().and_then(|a| a.url) else { return };
        self.window.clipboard().set_text(&url);
        self.show_toast("Link kopiert");
    }

    // ── Konto ──

    fn do_refresh(&self) {
        let feeds: Vec<(i64, String)> = self.state.borrow().feeds.iter().map(|f| (f.id, f.feed_url.clone())).collect();
        if feeds.is_empty() {
            self.show_toast("Noch keine Feeds abonniert (Strg+N)");
            return;
        }
        for (feed_id, url) in feeds {
            self.net.fetch_feed(self.worker.clone(), feed_id, url, true);
        }
        self.show_toast(&format!("Aktualisiere {} Feeds…", self.state.borrow().feeds.len()));
    }

    fn add_feed_dialog(&self) {
        let entry = gtk::Entry::builder().placeholder_text("Feed- oder Website-URL").activates_default(true).build();
        let dialog = adw::AlertDialog::builder()
            .heading("Feed hinzufügen")
            .body("URL eingeben; Lesefluss sucht den Feed und zeigt eine Vorschau.")
            .extra_child(&entry)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("add", "Suchen");
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
            app.show_toast("Suche Feed…");
            app.net.discover(url);
        });
    }

    fn show_discovery_dialog(&self, candidates: Vec<provider_local::DiscoverCandidate>) {
        let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
        for c in &candidates {
            let row = gtk::ListBoxRow::builder()
                .child(&gtk::Label::builder().label(&format!("{} — {}", c.title, c.url)).xalign(0.0).margin_start(8).margin_end(8).margin_top(6).margin_bottom(6).build())
                .build();
            list.append(&row);
        }
        if let Some(r) = list.row_at_index(0) {
            list.select_row(Some(&r));
        }
        let dialog = adw::AlertDialog::builder()
            .heading("Feed gefunden")
            .body("Bitte Feed auswählen und abonnieren.")
            .extra_child(&list)
            .build();
        dialog.add_response("cancel", "Abbrechen");
        dialog.add_response("sub", "Abonnieren");
        dialog.set_response_appearance("sub", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("sub"));
        dialog.set_close_response("cancel");
        let w = self.weak();
        dialog.choose(Some(&self.window), None::<&gio::Cancellable>, move |resp| {
            let Some(app) = w.upgrade() else { return };
            if resp != "sub" {
                return;
            }
            let Some(idx) = list.selected_row().map(|r| r.index() as usize) else { return };
            let Some(c) = candidates.get(idx) else { return };
            app.subscribe(&c.url, &c.title);
        });
    }

    fn subscribe(&self, url: &str, title: &str) {
        let url = url.to_string();
        let title = title.to_string();
        let w = self.weak();
        self.db_query(
            {
                let url_db = url.clone();
                move |db: &storage::Database| {
                    if let Some(id) = db.feed_id_by_url(&url_db)? {
                        return Ok::<_, storage::StorageError>((id, false));
                    }
                    let accent = ACCENTS[url_db.len() % ACCENTS.len()];
                    let id = db.add_feed("local", &url_db, &title, None, accent)?;
                    Ok((id, true))
                }
            },
            move |app, res: storage::Result<(i64, bool)>| {
                let Ok((feed_id, is_new)) = res else { return };
                if is_new {
                    let url2 = url.clone();
                    app.net.fetch_feed(app.worker.clone(), feed_id, url2, true);
                }
                app.reload_meta_then_select(feed_id);
                let _ = w;
            },
        );
    }

    fn reload_meta_then_select(&self, feed_id: i64) {
        let w = self.weak();
        self.db_query(
            |db| Ok::<_, storage::StorageError>((db.list_feeds()?, db.list_groups()?, db.counts()?)),
            move |app, res: storage::Result<(Vec<FeedRow>, Vec<GroupRow>, Counts)>| {
                let Ok((feeds, groups, counts)) = res else { return };
                {
                    let mut st = app.state.borrow_mut();
                    st.feeds = feeds;
                    st.groups = groups;
                    st.counts = counts;
                }
                app.set_scope(Scope::Feed(feed_id), true);
                let _ = w;
            },
        );
    }

    // ── Thema & Layout ──

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

    fn install_width_watcher(&self) {
        let outer = self.outer.clone();
        let inner = self.inner.clone();
        let win = self.window.clone();
        let apply = move || {
            let w = gtk::prelude::NativeExt::surface(&win).map(|s| s.width()).unwrap_or_else(|| win.width());
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

    // ── Signale ──

    fn wire(&self, factory: gtk::SignalListItemFactory) {
        factory.connect_setup(|_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            li.set_child(Some(&gtk::Box::new(gtk::Orientation::Vertical, 0)));
        });
        factory.connect_bind(move |_, list_item| {
            let li = list_item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(obj) = li.item() else { return };
            let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else { return };
            let row = boxed.borrow::<ListRow>();
            li.set_child(Some(&list::row_widget(&row)));
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
            let reset = matches!(f, Scope::Global);
            app.set_scope(f, reset);
        });

        let w = self.weak();
        self.list_selection.connect_selected_item_notify(move |sel| {
            let Some(app) = w.upgrade() else { return };
            if app.suppress.get() {
                return;
            }
            let cause = app.selection_cause.get();
            app.selection_cause.set(SelectionCause::Unknown);
            if cause != SelectionCause::Keyboard {
                return;
            }
            let Some(obj) = sel.selected_item() else { return };
            let Ok(boxed) = obj.downcast::<glib::BoxedAnyObject>() else { return };
            let row = boxed.borrow::<ListRow>();
            if let ListRow::Item(a) = row.clone() {
                app.schedule_preview(a);
            }
        });

        let w = self.weak();
        self.list_view.connect_activate(move |_, pos| {
            let Some(app) = w.upgrade() else { return };
            let row = {
                let st = app.state.borrow();
                match st.rows.get(pos as usize) {
                    Some(ListRow::Item(a)) => Some(a.clone()),
                    _ => None,
                }
            };
            if let Some(row) = row {
                app.open_article(row, true, true);
            }
        });

        let w = self.weak();
        self.list_view.connect_map(move |_| {
            let w2 = w.clone();
            glib::idle_add_local(move || {
                if let Some(app) = w2.upgrade() {
                    app.flush_dirty_rows();
                }
                glib::ControlFlow::Break
            });
        });

        let w = self.weak();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            if let Some(app) = w.upgrade() {
                app.selection_cause.set(SelectionCause::Pointer);
            }
        });
        self.list_view.add_controller(click);
        let w = self.weak();
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, key, _, _| {
            if let Some(app) = w.upgrade() {
                match key {
                    gtk::gdk::Key::Up | gtk::gdk::Key::Down | gtk::gdk::Key::Home | gtk::gdk::Key::End
                    | gtk::gdk::Key::Page_Up | gtk::gdk::Key::Page_Down => {
                        app.selection_cause.set(SelectionCause::Keyboard);
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        self.list_view.add_controller(keys);

        for (btn, f) in [
            (self.filter_saved.clone(), Filter::Saved),
            (self.filter_unread.clone(), Filter::Unread),
            (self.filter_all.clone(), Filter::All),
        ] {
            let w = self.weak();
            btn.connect_toggled(move |b| {
                let Some(app) = w.upgrade() else { return };
                if !b.is_active() {
                    b.set_active(true);
                    return;
                }
                app.set_filter(f);
            });
        }

        let w = self.weak();
        self.list_scroll.vadjustment().connect_value_changed(move |adj| {
            let Some(app) = w.upgrade() else { return };
            let near_bottom = adj.value() + adj.page_size() >= adj.upper() - 400.0;
            if !near_bottom {
                return;
            }
            let st = app.state.borrow();
            let can_more = st.cursor.is_some() && !st.loading_more;
            drop(st);
            if can_more {
                app.state.borrow_mut().loading_more = true;
                app.load_page(true);
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
        self.search_entry.connect_search_changed(move |entry| {
            let Some(app) = w.upgrade() else { return };
            let q = entry.text().trim().to_string();
            if let Some(old) = app.search_timer.borrow_mut().take() {
                old.remove();
            }
            let w2 = w.clone();
            let timer = glib::timeout_add_local(Duration::from_millis(150), move || {
                let Some(app) = w2.upgrade() else { return glib::ControlFlow::Break };
                *app.search_timer.borrow_mut() = None;
                {
                    let mut st = app.state.borrow_mut();
                    st.search = if q.is_empty() { None } else { Some(q.clone()) };
                }
                app.list_title.set_title(&app.scope_label_now());
                app.load_page(false);
                glib::ControlFlow::Break
            });
            *app.search_timer.borrow_mut() = Some(timer);
        });

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

    fn schedule_preview(&self, row: ArticleRow) {
        if let Some(old) = self.preview_timer.borrow_mut().take() {
            old.remove();
        }
        let w = self.weak();
        let timer = glib::timeout_add_local(Duration::from_millis(220), move || {
            if let Some(app) = w.upgrade() {
                *app.preview_timer.borrow_mut() = None;
                let is_current = app.reader.current.borrow().as_deref() == Some(row.id.as_str());
                if !is_current {
                    app.open_article(row.clone(), false, true);
                }
            }
            glib::ControlFlow::Break
        });
        *self.preview_timer.borrow_mut() = Some(timer);
    }

    // ── Actions ──

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
        win_action!("article-search", |a| {
            a.search_bar.set_search_mode(true);
            a.search_entry.grab_focus();
        });
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
        application.set_accels_for_action("win.article-search", &["<Control>l"]);
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
}
